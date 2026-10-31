//! BVH is a motion hierarchy with ordered transform channels, not a mesh format.
//! Values keep their source units. Rotations are composed in declared order.
use super::*;
use glam::{Quat, Vec3};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Channel {
    Position(usize),
    Rotation(usize),
}
struct Joint {
    name: String,
    offset: Vec3,
    channels: Vec<Channel>,
    children: Vec<usize>,
}
#[derive(Clone, Copy)]
struct Tokens<'a> {
    text: &'a str,
}
impl<'a> Tokens<'a> {
    fn next(&mut self) -> Result<&'a str> {
        self.text = self.text.trim_start();
        ensure!(!self.text.is_empty(), "Unexpected end of BVH");
        let count = if self.text.starts_with(['{', '}']) {
            1
        } else {
            self.text
                .find(|c: char| c.is_whitespace() || c == '{' || c == '}')
                .unwrap_or(self.text.len())
        };
        let (token, rest) = self.text.split_at(count);
        self.text = rest;
        Ok(token)
    }
    fn expect(&mut self, expected: &str) -> Result<()> {
        let token = self.next()?;
        ensure!(token == expected, "Expected {expected}, found {token}");
        Ok(())
    }
    fn number(&mut self) -> Result<f32> {
        let token = self.next()?;
        let n: f32 = token
            .parse()
            .with_context(|| format!("Invalid BVH number: {token}"))?;
        ensure!(n.is_finite(), "Non-finite BVH value");
        Ok(n)
    }
    fn count(&mut self) -> Result<usize> {
        self.next()?.parse().context("Invalid BVH count")
    }
}
fn joint(
    tokens: &mut Tokens<'_>,
    nodes: &mut Vec<Joint>,
    limits: &Limits,
    depth: usize,
    end: bool,
) -> Result<usize> {
    ensure!(
        depth <= 128 && nodes.len() < limits.nodes,
        "BVH hierarchy/depth budget exceeded"
    );
    let name = if end {
        tokens.expect("Site")?;
        format!("End helper {}", nodes.len())
    } else {
        tokens.next()?.to_owned()
    };
    tokens.expect("{")?;
    tokens.expect("OFFSET")?;
    let offset = Vec3::new(tokens.number()?, tokens.number()?, tokens.number()?);
    let id = nodes.len();
    nodes.push(Joint {
        name,
        offset,
        channels: vec![],
        children: vec![],
    });
    let mut channels_declared = false;
    loop {
        match tokens.next()? {
            "}" => break,
            "CHANNELS" if !end => {
                ensure!(!channels_declared, "Repeated CHANNELS declaration");
                channels_declared = true;
                let count = tokens.count()?;
                ensure!(count <= 6, "BVH joint has more than six channels");
                for _ in 0..count {
                    let token = tokens.next()?;
                    let channel = match token {
                        "Xposition" => Channel::Position(0),
                        "Yposition" => Channel::Position(1),
                        "Zposition" => Channel::Position(2),
                        "Xrotation" => Channel::Rotation(0),
                        "Yrotation" => Channel::Rotation(1),
                        "Zrotation" => Channel::Rotation(2),
                        _ => bail!("Unsupported BVH channel {token}"),
                    };
                    ensure!(
                        !nodes[id].channels.contains(&channel),
                        "Duplicate BVH channel"
                    );
                    nodes[id].channels.push(channel);
                }
            }
            "JOINT" if !end => {
                let child = joint(tokens, nodes, limits, depth + 1, false)?;
                nodes[id].children.push(child);
            }
            "End" if !end => {
                let child = joint(tokens, nodes, limits, depth + 1, true)?;
                nodes[id].children.push(child);
            }
            token => bail!("Unexpected BVH hierarchy token {token}"),
        }
    }
    Ok(id)
}
fn accessor(
    doc: &mut Value,
    bin: &mut Vec<u8>,
    data: &[f32],
    kind: &str,
    width: usize,
    limits: &Limits,
) -> Result<usize> {
    let raw: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
    let offset = append(bin, &raw, limits)?;
    let views = doc["bufferViews"].as_array_mut().unwrap();
    let view = views.len();
    views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":raw.len()}));
    let accessors = doc["accessors"].as_array_mut().unwrap();
    let id = accessors.len();
    accessors
        .push(json!({"bufferView":view,"componentType":5126,"count":data.len()/width,"type":kind}));
    Ok(id)
}
pub(super) fn normalize(bytes: &[u8], limits: &Limits, name: &str) -> Result<Vec<u8>> {
    ensure!(bytes.len() <= limits.file_bytes, "BVH file budget exceeded");
    let text = std::str::from_utf8(bytes).context("BVH must be UTF-8/ASCII text")?;
    let mut tokens = Tokens { text };
    let mut nodes = vec![];
    tokens.expect("HIERARCHY")?;
    tokens.expect("ROOT")?;
    joint(&mut tokens, &mut nodes, limits, 0, false)?;
    tokens.expect("MOTION")?;
    tokens.expect("Frames:")?;
    let frames = tokens.count()?;
    ensure!(
        (1..=120_000).contains(&frames),
        "BVH frame count must be 1..120000"
    );
    tokens.expect("Frame")?;
    tokens.expect("Time:")?;
    let dt = tokens.number()?;
    ensure!(dt > 0., "BVH frame time must be positive");
    let animated = nodes.iter().filter(|n| !n.channels.is_empty()).count();
    ensure!(animated > 0, "BVH has no animation channels");
    let expanded = frames
        .checked_mul(animated)
        .and_then(|n| n.checked_mul(36))
        .context("BVH sample budget overflow")?;
    ensure!(
        expanded <= limits.decoded_bytes && expanded / 4 <= limits.elements,
        "BVH decoded sample budget exceeded"
    );
    let mut translations: Vec<_> = nodes
        .iter()
        .map(|n| Vec::with_capacity(if n.channels.is_empty() { 0 } else { frames * 3 }))
        .collect();
    let mut rotations: Vec<_> = nodes
        .iter()
        .map(|n| Vec::with_capacity(if n.channels.is_empty() { 0 } else { frames * 4 }))
        .collect();
    for _ in 0..frames {
        for (index, node) in nodes.iter().enumerate() {
            if node.channels.is_empty() {
                continue;
            }
            let mut translation = node.offset;
            let mut rotation = Quat::IDENTITY;
            for channel in &node.channels {
                let value = tokens.number()?;
                let axis = match channel {
                    Channel::Position(a) | Channel::Rotation(a) => [Vec3::X, Vec3::Y, Vec3::Z][*a],
                };
                match channel {
                    Channel::Position(_) => translation += rotation * (axis * value),
                    Channel::Rotation(_) => {
                        rotation *= Quat::from_axis_angle(axis, value.to_radians())
                    }
                }
            }
            rotation = rotation.normalize();
            ensure!(
                translation.is_finite() && rotation.is_finite(),
                "Non-finite composed BVH transform"
            );
            translations[index].extend(translation.to_array());
            rotations[index].extend(rotation.to_array());
        }
    }
    ensure!(
        tokens.text.trim().is_empty(),
        "Trailing BVH samples or unsupported extra hierarchy"
    );
    let times: Vec<_> = (0..frames).map(|i| i as f32 * dt).collect();
    ensure!(
        times.iter().all(|t| t.is_finite()) && times.windows(2).all(|t| t[1] > t[0]),
        "Invalid BVH sample times"
    );
    let mut doc = json!({"asset":{"version":"2.0","generator":"Armatra BVH motion import"},"buffers":[{"byteLength":0}],"bufferViews":[],"accessors":[],
        "nodes":nodes.iter().map(|n|json!({"name":n.name,"translation":n.offset.to_array(),"children":n.children})).collect::<Vec<_>>(),"scenes":[{"nodes":[0]}],"scene":0});
    let mut bin = vec![];
    let input = accessor(&mut doc, &mut bin, &times, "SCALAR", 1, limits)?;
    let mut samplers = vec![];
    let mut channels = vec![];
    for (node, joint) in nodes.iter().enumerate() {
        if joint.channels.is_empty() {
            continue;
        }
        for (property, values, kind, width) in [
            ("translation", &translations[node], "VEC3", 3),
            ("rotation", &rotations[node], "VEC4", 4),
        ] {
            let output = accessor(&mut doc, &mut bin, values, kind, width, limits)?;
            let sampler = samplers.len();
            samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
            channels.push(json!({"sampler":sampler,"target":{"node":node,"path":property}}));
        }
    }
    doc["animations"] = json!([{"name":name,"samplers":samplers,"channels":channels}]);
    doc["buffers"][0]["byteLength"] = json!(bin.len());
    encode(&doc, &bin, limits)
}
