//! Bounded, opt-in humanoid retrieval. Descriptor similarity is not perception.
use crate::{
    animation::{self, Clip},
    rig::{Anatomy, Rig, Side},
    scene::Asset,
};
use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};
mod preview_strip;
pub use preview_strip::{PREVIEW_FRAME_VERSION, PoseImage, preview_strip};

const FRAMES: usize = 48;
const ROLES: [(Anatomy, Side); 7] = [
    (Anatomy::Pelvis, Side::Unknown),
    (Anatomy::Head, Side::Unknown),
    (Anatomy::Spine, Side::Unknown),
    (Anatomy::Hand, Side::Left),
    (Anatomy::Hand, Side::Right),
    (Anatomy::Foot, Side::Left),
    (Anatomy::Foot, Side::Right),
];
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Medoid {
    pub time: f32,
    pub weight: f64,
    pub descriptor: Vec<f32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub version: u32,
    pub medoids: Vec<Medoid>,
    pub root_speed: f32,
    pub duration: f32,
    pub objective: f64,
}
pub struct Entry {
    pub id: String,
    pub name: String,
    pub summary: Summary,
    pub preview: image::RgbaImage,
}
pub fn distance(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
        return f64::INFINITY;
    }
    a.iter()
        .zip(b)
        .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
        .sum::<f64>()
        .sqrt()
}
/// Ordinary deterministic PAM, bounded to 64 samples and 8 best-swap passes.
pub fn medoids(samples: &[Vec<f32>], k: usize) -> Result<(Vec<usize>, Vec<usize>, f64)> {
    ensure!(
        !samples.is_empty() && samples.len() <= 64 && k > 0 && k <= samples.len(),
        "Medoid budget exceeded"
    );
    let n = samples.len();
    let len = samples[0].len();
    ensure!(
        len > 0
            && samples
                .iter()
                .all(|s| s.len() == len && s.iter().all(|v| v.is_finite())),
        "Invalid descriptors"
    );
    let distances: Vec<Vec<_>> = samples
        .iter()
        .map(|a| samples.iter().map(|b| distance(a, b)).collect())
        .collect();
    let cost = |m: &[usize]| -> f64 {
        (0..n)
            .map(|i| {
                m.iter()
                    .map(|&j| distances[i][j])
                    .min_by(f64::total_cmp)
                    .unwrap()
            })
            .sum()
    };
    let mut selected = vec![0];
    while selected.len() < k {
        let next = (0..n)
            .filter(|i| !selected.contains(i))
            .max_by(|&a, &b| {
                let near = |i: usize| {
                    selected
                        .iter()
                        .map(|&j| distances[i][j])
                        .min_by(f64::total_cmp)
                        .unwrap()
                };
                near(a).total_cmp(&near(b)).then_with(|| b.cmp(&a))
            })
            .unwrap();
        selected.push(next);
    }
    for _ in 0..8 {
        let old = cost(&selected);
        let mut best = old;
        let mut change = None;
        for slot in 0..k {
            for candidate in 0..n {
                if selected.contains(&candidate) {
                    continue;
                }
                let mut trial = selected.clone();
                trial[slot] = candidate;
                let c = cost(&trial);
                if c < best - 1e-9 {
                    best = c;
                    change = Some(trial);
                }
            }
        }
        if let Some(next) = change {
            selected = next;
        } else {
            break;
        }
    }
    let mut counts = vec![0; k];
    for row in &distances {
        let group = (0..k)
            .min_by(|&a, &b| row[selected[a]].total_cmp(&row[selected[b]]))
            .unwrap();
        counts[group] += 1;
    }
    let objective = cost(&selected);
    Ok((selected, counts, objective))
}
pub fn summarize(asset: &Asset, clip: &Clip) -> Result<Summary> {
    ensure!(
        clip.supported
            && asset.nodes.len() <= 2048
            && clip.tracks.len() <= 2048
            && clip.duration.is_finite(),
        "Motion index budget/format unsupported"
    );
    let rig = Rig::analyze(asset);
    let frame = rig
        .frame
        .ok_or_else(|| anyhow::anyhow!("Motion search needs an identifiable humanoid frame"))?;
    let mut nodes = Vec::new();
    for (anatomy, side) in ROLES {
        let found: Vec<_> = rig
            .joints
            .iter()
            .filter(|j| j.anatomy == anatomy && (side == Side::Unknown || side == j.side))
            .collect();
        ensure!(
            found.len() == 1,
            "Motion search needs unique major humanoid roles"
        );
        nodes.push(found[0].node);
    }
    let rest = asset.globals(&asset.rest_pose());
    let (_, root_rest, _) = rest[nodes[0]].to_scale_rotation_translation();
    let mapping: Vec<_> = (0..asset.nodes.len()).map(Some).collect();
    let mut descriptors = Vec::new();
    let mut speed_sum = 0.;
    let global_at = |t| asset.globals(&animation::evaluate(asset, clip, &mapping, t, &mut vec![]));
    for i in 0..FRAMES {
        let t = clip.duration * i as f32 / FRAMES as f32;
        let dt = (clip.duration / FRAMES as f32 * 0.25).clamp(1e-4, 0.01);
        let before = (t - dt).max(0.);
        let after = (t + dt).min(clip.duration);
        let g = global_at(t);
        let a = global_at(before);
        let b = global_at(after);
        let delta = (after - before).max(1e-5);
        let origin = g[nodes[0]].w_axis.truncate();
        let (_, r, _) = g[nodes[0]].to_scale_rotation_translation();
        let orientation = (r * root_rest.inverse() * frame).normalize();
        let inverse = orientation.inverse();
        let root_velocity =
            (b[nodes[0]].w_axis - a[nodes[0]].w_axis).truncate() / delta / rig.height;
        speed_sum += root_velocity.length();
        let mut descriptor = Vec::new();
        for &node in &nodes {
            descriptor
                .extend((inverse * (g[node].w_axis.truncate() - origin) / rig.height).to_array());
            let velocity =
                (b[node].w_axis - a[node].w_axis).truncate() / delta / rig.height - root_velocity;
            descriptor.extend((inverse * velocity * 0.15).to_array());
        }
        descriptor.extend((inverse * root_velocity * 0.3).to_array());
        descriptor.extend((frame.inverse() * (orientation * Vec3::Z) * 0.1).to_array());
        for &foot in &nodes[5..] {
            let height =
                (frame.inverse() * (g[foot].w_axis - rest[foot].w_axis).truncate()).y / rig.height;
            let speed = (b[foot].w_axis - a[foot].w_axis).truncate().length() / delta / rig.height;
            descriptor.push(if height.abs() < 0.04 && speed < 0.15 {
                0.1
            } else {
                0.
            });
        }
        ensure!(
            descriptor.iter().all(|v| v.is_finite()),
            "Nonfinite motion descriptor"
        );
        descriptors.push(descriptor);
    }
    let (indices, counts, objective) = medoids(&descriptors, 4)?;
    Ok(Summary {
        version: 1,
        medoids: indices
            .into_iter()
            .zip(counts)
            .filter(|(_, n)| *n > 0)
            .map(|(i, n)| Medoid {
                time: clip.duration * i as f32 / FRAMES as f32,
                weight: n as f64 / FRAMES as f64,
                descriptor: descriptors[i].clone(),
            })
            .collect(),
        root_speed: speed_sum / FRAMES as f32,
        duration: clip.duration,
        objective,
    })
}

#[derive(Clone)]
struct Edge {
    to: usize,
    reverse: usize,
    capacity: f64,
    cost: f64,
}
/// Successive shortest augmenting paths with residual reverse edges. This is
/// balanced transport, not nearest-neighbor or greedy pairing.
pub fn transport(a: &[Medoid], b: &[Medoid]) -> Result<f64> {
    ensure!(
        !a.is_empty() && !b.is_empty() && a.len() <= 8 && b.len() <= 8,
        "Transport budget"
    );
    for group in [a, b] {
        ensure!(
            group.iter().all(|m| m.weight.is_finite() && m.weight > 0.)
                && (group.iter().map(|m| m.weight).sum::<f64>() - 1.).abs() < 1e-6,
            "Unbalanced medoid weights"
        );
    }
    let n = a.len() + b.len() + 2;
    let sink = n - 1;
    let mut graph = vec![Vec::<Edge>::new(); n];
    fn edge(g: &mut [Vec<Edge>], u: usize, v: usize, capacity: f64, cost: f64) {
        let ru = g[v].len();
        let rv = g[u].len();
        g[u].push(Edge {
            to: v,
            reverse: ru,
            capacity,
            cost,
        });
        g[v].push(Edge {
            to: u,
            reverse: rv,
            capacity: 0.,
            cost: -cost,
        });
    }
    for (i, m) in a.iter().enumerate() {
        edge(&mut graph, 0, 1 + i, m.weight, 0.);
    }
    for (j, m) in b.iter().enumerate() {
        edge(&mut graph, 1 + a.len() + j, sink, m.weight, 0.);
    }
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            let d = distance(&x.descriptor, &y.descriptor);
            ensure!(d.is_finite(), "Incompatible descriptors");
            edge(&mut graph, 1 + i, 1 + a.len() + j, 1., d);
        }
    }
    let mut flow: f64 = 0.;
    let mut cost: f64 = 0.;
    for _ in 0..256 {
        if flow >= 1. - 1e-9 {
            return Ok(cost.max(0.));
        }
        let mut distances = vec![f64::INFINITY; n];
        let mut previous = vec![None; n];
        distances[0] = 0.;
        for _ in 0..n - 1 {
            let mut changed = false;
            for u in 0..n {
                for (e, edge) in graph[u].iter().enumerate() {
                    if edge.capacity > 1e-9 && distances[u] + edge.cost < distances[edge.to] - 1e-12
                    {
                        distances[edge.to] = distances[u] + edge.cost;
                        previous[edge.to] = Some((u, e));
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        ensure!(previous[sink].is_some(), "Transport has no augmenting path");
        let mut amount = 1. - flow;
        let mut v = sink;
        while v != 0 {
            let (u, e) = previous[v].unwrap();
            amount = amount.min(graph[u][e].capacity);
            v = u;
        }
        v = sink;
        while v != 0 {
            let (u, e) = previous[v].unwrap();
            let reverse = graph[u][e].reverse;
            graph[u][e].capacity -= amount;
            graph[v][reverse].capacity += amount;
            v = u;
        }
        flow += amount;
        cost += amount * distances[sink];
    }
    anyhow::bail!("Transport iteration budget")
}
pub fn similarity(a: &Summary, b: &Summary) -> Result<f64> {
    ensure!(a.version == b.version, "Summary version mismatch");
    Ok(transport(&a.medoids, &b.medoids)? + (a.root_speed - b.root_speed).abs() as f64 * 0.3)
}

pub fn preview(asset: &Asset, clip: &Clip, time: f32) -> image::RgbaImage {
    let pose = animation::evaluate(
        asset,
        clip,
        &(0..asset.nodes.len()).map(Some).collect::<Vec<_>>(),
        time,
        &mut vec![],
    );
    let g = asset.globals(&pose);
    let rig = Rig::analyze(asset);
    let inverse = rig.frame.unwrap_or_default().inverse();
    let positions: Vec<_> = g
        .iter()
        .map(|m| inverse * (m.w_axis.truncate() - rig.origin) / rig.height)
        .collect();
    let min = positions
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |a, &p| a.min(p));
    let max = positions
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |a, &p| a.max(p));
    let scale = 104. / (max - min).x.max((max - min).y).max(0.1);
    let point = |i: usize| {
        let p = positions[i] - (min + max) * 0.5;
        [64. + p.x * scale, 64. - p.y * scale]
    };
    let mut image = image::RgbaImage::from_pixel(
        128,
        128,
        image::Rgba(crate::ui::theme::BACKGROUND.to_array()),
    );
    for (i, n) in asset.nodes.iter().enumerate() {
        if let Some(parent) = n.parent {
            let a = point(i);
            let b = point(parent);
            for k in 0..=100 {
                let u = k as f32 / 100.;
                let x = (a[0] * (1. - u) + b[0] * u).round() as i32;
                let y = (a[1] * (1. - u) + b[1] * u).round() as i32;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if (0..128).contains(&(x + dx)) && (0..128).contains(&(y + dy)) {
                            image.put_pixel(
                                (x + dx) as u32,
                                (y + dy) as u32,
                                image::Rgba(crate::ui::theme::SECONDARY.to_array()),
                            );
                        }
                    }
                }
            }
        }
    }
    image
}
