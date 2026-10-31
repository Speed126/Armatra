//! Original procedural fixtures, distributed under the project MIT license.
use glam::{Mat4, Quat, Vec3};
use serde_json::{Value, json};
pub struct Builder {
    pub json: Value,
    pub bin: Vec<u8>,
}
impl Default for Builder {
    fn default() -> Self {
        Self {
            json: json!({"asset":{"version":"2.0","generator":"Armatra fixture generator"},"buffers":[{"byteLength":0}],"bufferViews":[],"accessors":[],"nodes":[],"scenes":[{"nodes":[]}],"scene":0}),
            bin: Vec::new(),
        }
    }
}
impl Builder {
    pub fn view(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let views = self.json["bufferViews"].as_array_mut().expect("array");
        let id = views.len();
        views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len()}));
        id
    }
    pub fn accessor(&mut self, data: &[f32], dimensions: &str, width: usize) -> usize {
        let bytes: Vec<_> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes);
        let accessors = self.json["accessors"].as_array_mut().expect("array");
        let id = accessors.len();
        accessors.push(json!({"bufferView":view,"componentType":5126,"type":dimensions,"count":data.len()/width}));
        id
    }
    pub fn finish(mut self) -> Vec<u8> {
        self.json["buffers"][0]["byteLength"] = json!(self.bin.len());
        encode(&self.json, &self.bin)
    }
    pub fn positions(&mut self, data: &[f32]) -> usize {
        let accessor = self.accessor(data, "VEC3", 3);
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for xyz in data.as_chunks::<3>().0 {
            let position = Vec3::new(xyz[0], xyz[1], xyz[2]);
            min = min.min(position);
            max = max.max(position);
        }
        self.json["accessors"][accessor]["min"] = json!(min.to_array());
        self.json["accessors"][accessor]["max"] = json!(max.to_array());
        accessor
    }
}
pub fn encode(json: &Value, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(json).expect("fixture JSON");
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let len = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = b"glTF".to_vec();
    out.extend(2u32.to_le_bytes());
    out.extend((len as u32).to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(0x4e4f534au32.to_le_bytes());
    out.extend(json);
    if !bin.is_empty() {
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(0x004e4942u32.to_le_bytes());
        out.extend(bin);
    }
    out
}
/// Original two independent skinned robots for skin-selection regressions.
pub fn multi_skin_wave_target() -> Builder {
    let mut b = robot(false, false);
    // Deliberately differs from Wave's rest transform, requiring retargeting
    // rather than the exact same-rig playback shortcut.
    b.json["nodes"][0]["translation"] = json!([0.1, 0., 0.]);
    b.json["nodes"][12]["translation"] = json!([0.1, 0., 0.]);
    let original = b.json["nodes"].as_array().unwrap().clone();
    let offset = original.len();
    let mut second = original.clone();
    for node in &mut second {
        if let Some(children) = node.get_mut("children").and_then(|v| v.as_array_mut()) {
            for child in children {
                *child = json!(child.as_u64().unwrap() as usize + offset);
            }
        }
        if node.get("skin").is_some() {
            node["skin"] = json!(1);
        }
    }
    second[0]["translation"] = json!([2.8, 0., 0.]);
    second[12]["translation"] = json!([2.8, 0., 0.]);
    b.json["nodes"].as_array_mut().unwrap().extend(second);
    let mut skin = b.json["skins"][0].clone();
    for joint in skin["joints"].as_array_mut().unwrap() {
        *joint = json!(joint.as_u64().unwrap() as usize + offset);
    }
    skin["skeleton"] = json!(offset);
    b.json["skins"].as_array_mut().unwrap().push(skin);
    b.json["scenes"][0]["nodes"] = json!([0, 12, offset, offset + 12]);
    b.json["animations"] = json!([]);
    b
}
pub fn robot(animation_only: bool, permuted: bool) -> Builder {
    let mut b = Builder::default();
    // Rig and mesh attachments are independent; dedicated tests add non-joint channels.
    let definitions = [
        ("Rig", None, [0., 0., 0.]),
        ("Pelvis", Some(0), [0., 1., 0.]),
        ("Chest", Some(1), [0., 0.55, 0.]),
        ("Head", Some(2), [0., 0.65, 0.]),
        ("UpperArm.L", Some(2), [0.5, 0.35, 0.]),
        ("Forearm.L", Some(4), [0.48, 0., 0.]),
        ("UpperArm.R", Some(2), [-0.5, 0.35, 0.]),
        ("Forearm.R", Some(6), [-0.48, 0., 0.]),
        ("Thigh.L", Some(1), [0.23, -0.12, 0.]),
        ("Shin.L", Some(8), [0., -0.43, 0.]),
        ("Thigh.R", Some(1), [-0.23, -0.12, 0.]),
        ("Shin.R", Some(10), [0., -0.43, 0.]),
    ];
    let mut globals = Vec::new();
    let mut nodes: Vec<Value> = definitions
        .iter()
        .map(|(name, _, t)| json!({"name":name,"translation":t,"children":[]}))
        .collect();
    for (i, (_, p, t)) in definitions.iter().enumerate() {
        let local = Mat4::from_translation(Vec3::from(*t));
        globals.push(p.map_or(local, |p| globals[p] * local));
        if let Some(p) = p {
            nodes[*p]["children"]
                .as_array_mut()
                .expect("children")
                .push(json!(i));
        }
    }
    if !animation_only {
        let shapes = [
            (1, [0., 0.1, 0.], [0.64, 0.35, 0.4]),
            (2, [0., 0.22, 0.], [0.8, 0.6, 0.4]),
            (3, [0., 0.12, 0.], [0.48, 0.4, 0.4]),
            (4, [0.22, 0., 0.], [0.43, 0.22, 0.24]),
            (5, [0.18, 0., 0.], [0.38, 0.18, 0.2]),
            (6, [-0.22, 0., 0.], [0.43, 0.22, 0.24]),
            (7, [-0.18, 0., 0.], [0.38, 0.18, 0.2]),
            (8, [0., -0.19, 0.], [0.25, 0.42, 0.28]),
            (9, [0., -0.2, 0.06], [0.23, 0.42, 0.38]),
            (10, [0., -0.19, 0.], [0.25, 0.42, 0.28]),
            (11, [0., -0.2, 0.06], [0.23, 0.42, 0.38]),
        ];
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uv = Vec::new();
        let mut joints = Vec::<u16>::new();
        let mut weights = Vec::new();
        let faces = [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (-Vec3::X, -Vec3::Y, Vec3::Z),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (-Vec3::Y, -Vec3::Z, Vec3::X),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (-Vec3::Z, -Vec3::X, Vec3::Y),
        ];
        for (bone, center, size) in shapes {
            let center = globals[bone].transform_point3(center.into());
            let half = Vec3::from(size) * 0.5;
            for (normal, u, v) in faces {
                for (x, y) in [
                    (-1., -1.),
                    (1., -1.),
                    (1., 1.),
                    (-1., -1.),
                    (1., 1.),
                    (-1., 1.),
                ] {
                    positions
                        .extend_from_slice((center + (normal + u * x + v * y) * half).as_ref());
                    normals.extend_from_slice(normal.as_ref());
                    uv.extend_from_slice(&[(x + 1.) * 0.5, (y + 1.) * 0.5]);
                    joints.extend([bone as u16; 4]);
                    weights.extend([0.25f32; 4]);
                }
            }
        }
        let count = positions.len() / 3;
        let p = b.positions(&positions);
        let n = b.accessor(&normals, "VEC3", 3);
        let tex = b.accessor(&uv, "VEC2", 2);
        let joint_bytes: Vec<_> = joints.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = b.view(&joint_bytes);
        let j = b.json["accessors"].as_array().expect("array").len();
        b.json["accessors"]
            .as_array_mut()
            .expect("array")
            .push(json!({"bufferView":view,"componentType":5123,"count":count,"type":"VEC4"}));
        let w = b.accessor(&weights, "VEC4", 4);
        let ibm: Vec<_> = globals
            .iter()
            .flat_map(|g| g.inverse().to_cols_array())
            .collect();
        let inverse = b.accessor(&ibm, "MAT4", 16);
        let pixels = image::RgbaImage::from_fn(8, 8, |x, y| {
            if (x / 2 + y / 2) % 2 == 0 {
                image::Rgba([100, 220, 185, 255])
            } else {
                image::Rgba([50, 150, 140, 255])
            }
        });
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("PNG");
        let image_view = b.view(encoded.get_ref());
        b.json["images"] = json!([{"bufferView":image_view,"mimeType":"image/png"}]);
        b.json["textures"] = json!([{"source":0}]);
        b.json["materials"] = json!([{"name":"Jade checker","pbrMetallicRoughness":{"baseColorTexture":{"index":0},"metallicFactor":0,"roughnessFactor":0.8}}]);
        b.json["meshes"] = json!([{"name":"Armatra robot","primitives":[{"attributes":{"POSITION":p,"NORMAL":n,"TEXCOORD_0":tex,"JOINTS_0":j,"WEIGHTS_0":w},"material":0}]}]);
        b.json["skins"] = json!([{"joints":(0..nodes.len()).collect::<Vec<_>>(),"inverseBindMatrices":inverse,"skeleton":0}]);
        nodes.push(json!({"name":"Robot mesh","mesh":0,"skin":0}));
    }
    let times = b.accessor(&[0., 0.75, 1.5, 2.25, 3.], "SCALAR", 1);
    b.json["accessors"][times]["min"] = json!([0]);
    b.json["accessors"][times]["max"] = json!([3]);
    let rotations: Vec<_> = [0., 1.2, -0.3, 1.2, 0.]
        .iter()
        .flat_map(|&angle| Quat::from_rotation_z(angle).to_array())
        .collect();
    let rotation = b.accessor(&rotations, "VEC4", 4);
    let root = b.accessor(
        &[
            0., 0., 0., 0., 0.08, 0., 0., 0., 0., 0., 0.08, 0., 0., 0., 0.,
        ],
        "VEC3",
        3,
    );
    b.json["animations"] = json!([{"name":"Wave","samplers":[{"input":times,"output":rotation,"interpolation":"LINEAR"},{"input":times,"output":root,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":5,"path":"rotation"}},{"sampler":1,"target":{"node":0,"path":"translation"}}]}]);
    b.json["scenes"][0]["nodes"] = if animation_only {
        json!([0])
    } else {
        json!([0, nodes.len() - 1])
    };
    if permuted {
        let count = nodes.len();
        let map = |i: usize| count - 1 - i;
        nodes.reverse();
        for node in &mut nodes {
            if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
                for c in children {
                    *c = json!(map(c.as_u64().expect("id") as usize));
                }
            }
        }
        for channel in b.json["animations"][0]["channels"]
            .as_array_mut()
            .expect("array")
        {
            let old = channel["target"]["node"].as_u64().expect("id") as usize;
            channel["target"]["node"] = json!(map(old));
        }
        for root in b.json["scenes"][0]["nodes"].as_array_mut().expect("roots") {
            *root = json!(map(root.as_u64().expect("id") as usize));
        }
        if let Some(joints) = b
            .json
            .get_mut("skins")
            .and_then(|s| s[0].get_mut("joints"))
            .and_then(Value::as_array_mut)
        {
            for j in joints {
                *j = json!(map(j.as_u64().expect("id") as usize));
            }
        }
    }
    b.json["nodes"] = json!(nodes);
    b
}
pub fn write_samples() -> anyhow::Result<()> {
    std::fs::create_dir_all("tests/fixtures")?;
    std::fs::write(
        "tests/fixtures/Armatra-demo.glb",
        robot(false, false).finish(),
    )?;
    std::fs::write(
        "tests/fixtures/Wave-permuted.glb",
        robot(true, true).finish(),
    )?;
    std::fs::write("tests/fixtures/Viewport-cube.glb", cube().finish())?;
    std::fs::create_dir_all("tests/fixtures/motions")?;
    std::fs::write(
        "tests/fixtures/motions/Layered-preview.glb",
        crate::motion_fixtures::layered_preview().finish(),
    )?;
    std::fs::write(
        "tests/fixtures/motions/Multi-clip-wave.glb",
        multi_clip_wave().finish(),
    )?;
    Ok(())
}
pub fn multi_clip_wave() -> Builder {
    let mut b = robot(true, true);
    let mut clip = b.json["animations"][0].clone();
    clip["name"] = json!("Wave alternate");
    b.json["animations"].as_array_mut().unwrap().push(clip);
    b
}
pub fn cube() -> Builder {
    let mut b = robot(false, false);
    let attributes = b.json["meshes"][0]["primitives"][0]["attributes"]
        .as_object()
        .expect("attributes")
        .clone();
    for index in attributes.values() {
        let i = index.as_u64().expect("accessor") as usize;
        b.json["accessors"][i]["count"] = json!(36);
    }
    let view = b.json["accessors"][0]["bufferView"].as_u64().expect("view") as usize;
    let start = b.json["bufferViews"][view]["byteOffset"]
        .as_u64()
        .expect("offset") as usize;
    let positions: Vec<_> = b.bin[start..start + 36 * 3 * 4]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect();
    let exact = b.positions(&positions);
    b.json["meshes"][0]["primitives"][0]["attributes"]["POSITION"] = json!(exact);
    b.json["nodes"] = json!([{"name":"Cube","mesh":0,"translation":[0.,0.5-1.1/0.35,0.],"scale":[1./0.64,1./0.35,1./0.4]}]);
    b.json["scenes"][0]["nodes"] = json!([0]);
    b.json.as_object_mut().expect("document").remove("skins");
    b.json
        .as_object_mut()
        .expect("document")
        .remove("animations");
    b
}
