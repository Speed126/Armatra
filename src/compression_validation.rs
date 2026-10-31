//! Independent dense/key/STEP-boundary validation and mesh-informed probes.
use crate::{
    animation::{self, Clip, Interpolation},
    compression::{self, Codec, DerivedClip},
    scene::{Asset, Local, skin_position},
};
use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorReport {
    pub sample_times: usize,
    pub skeleton_probe_count: usize,
    pub vertex_probes_per_sample: usize,
    pub max_probe_error: f32,
    pub p99_probe_error: f32,
    pub max_end_effector_error: f32,
    pub max_vertex_error: f32,
    pub p99_vertex_error: f32,
    pub budget: f32,
    pub accepted: bool,
}
pub fn sample_times(clip: &Clip) -> Vec<f32> {
    let count = (clip.duration * 120.).ceil() as usize;
    let mut times: Vec<_> = (0..=count)
        .map(|i| (i as f32 / 120.).min(clip.duration))
        .collect();
    for track in &clip.tracks {
        times.extend(&track.times);
        for &t in &track.times {
            if track.interpolation == Interpolation::Step {
                times.extend([t.next_down().max(0.), t.next_up().min(clip.duration)]);
            }
        }
        for p in track.times.windows(2) {
            for i in 1..8 {
                times.push(p[0] + (p[1] - p[0]) * i as f32 / 8.);
            }
        }
    }
    times.sort_by(f32::total_cmp);
    times.dedup();
    times
}
/// Mesh radii use the 95th percentile distance in joint-local rest space for
/// vertices with influence > .05. Six axis probes plus the joint origin.
pub fn radii(asset: &Asset) -> Vec<f32> {
    let rest = asset.globals(&asset.rest_pose());
    let mut distances = vec![Vec::new(); asset.nodes.len()];
    for (node, n) in asset.nodes.iter().enumerate() {
        let (Some(mesh), Some(skin)) = (n.mesh, n.skin) else {
            continue;
        };
        for p in &asset.meshes[mesh].primitives {
            if !p.has_skin {
                continue;
            }
            for v in &p.vertices {
                let world = rest[node].transform_point3(Vec3::from(v.position));
                for (&j, &w) in v.joints.iter().zip(&v.weights) {
                    if w > 0.05 {
                        let joint = asset.skins[skin].joints[j as usize];
                        if rest[joint].determinant().abs() > 1e-10 {
                            distances[joint]
                                .push(rest[joint].inverse().transform_point3(world).length());
                        }
                    }
                }
            }
        }
    }
    distances
        .into_iter()
        .map(|mut d| {
            d.sort_by(f32::total_cmp);
            if d.is_empty() {
                0.1
            } else {
                d[((d.len() - 1) as f32 * 0.95).ceil() as usize].max(0.01)
            }
        })
        .collect()
}
fn percentile(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values
        .get(((values.len().saturating_sub(1)) as f32 * 0.99).ceil() as usize)
        .copied()
        .unwrap_or(0.)
}
pub fn validate_with(
    asset: &Asset,
    clip: &Clip,
    budget: f32,
    mut sample: impl FnMut(usize, f32) -> Vec<Local>,
) -> Result<ErrorReport> {
    ensure!(
        budget.is_finite() && budget >= 0.,
        "Invalid sampled error budget"
    );
    let times = sample_times(clip);
    ensure!(
        times.len() <= 1_000_000,
        "Validation sample budget exceeded"
    );
    let radii = radii(asset);
    let mapping: Vec<_> = (0..asset.nodes.len()).map(Some).collect();
    let mut probe_errors = Vec::new();
    let mut vertex_errors = Vec::new();
    let mut end_error = 0f32;
    let mut vertices = Vec::new();
    for node in asset.visible_nodes(asset.default_scene) {
        if let Some(mesh) = asset.nodes[node].mesh {
            for p in &asset.meshes[mesh].primitives {
                let stride = p.vertices.len().div_ceil(512).max(1);
                for v in p.vertices.iter().step_by(stride) {
                    vertices.push((node, p.has_skin, v));
                }
            }
        }
    }
    let mut max_probe = 0f32;
    let mut max_vertex = 0f32;
    for (i, &t) in times.iter().enumerate() {
        let raw = asset.globals(&animation::evaluate(asset, clip, &mapping, t, &mut vec![]));
        let decoded = sample(i, t);
        ensure!(
            decoded.len() == asset.nodes.len(),
            "Decoded pose size mismatch"
        );
        let derived = asset.globals(&decoded);
        ensure!(
            derived.iter().all(|m| m.is_finite()),
            "Nonfinite reconstructed globals"
        );
        for j in 0..asset.nodes.len() {
            for axis in [
                Vec3::ZERO,
                Vec3::X,
                -Vec3::X,
                Vec3::Y,
                -Vec3::Y,
                Vec3::Z,
                -Vec3::Z,
            ] {
                let error = raw[j]
                    .transform_point3(axis * radii[j])
                    .distance(derived[j].transform_point3(axis * radii[j]));
                ensure!(error.is_finite(), "Nonfinite probe error");
                max_probe = max_probe.max(error);
                probe_errors.push(error);
                if axis == Vec3::ZERO && asset.nodes[j].children.is_empty() {
                    end_error = end_error.max(error);
                }
            }
        }
        let mut palettes = std::collections::HashMap::new();
        for &(node, skinned, v) in &vertices {
            let (a, b) = if skinned && asset.nodes[node].skin.is_some() {
                if let std::collections::hash_map::Entry::Vacant(entry) = palettes.entry(node) {
                    entry.insert((
                        asset.palette(node, &raw).map_err(anyhow::Error::msg)?,
                        asset.palette(node, &derived).map_err(anyhow::Error::msg)?,
                    ));
                }
                let (a, b) = &palettes[&node];
                (skin_position(v, a), skin_position(v, b))
            } else {
                (Vec3::from(v.position), Vec3::from(v.position))
            };
            let error = raw[node]
                .transform_point3(a)
                .distance(derived[node].transform_point3(b));
            ensure!(error.is_finite(), "Nonfinite vertex error");
            max_vertex = max_vertex.max(error);
            vertex_errors.push(error);
        }
    }
    Ok(ErrorReport {
        sample_times: times.len(),
        skeleton_probe_count: asset.nodes.len() * 7,
        vertex_probes_per_sample: vertices.len(),
        max_probe_error: max_probe,
        p99_probe_error: percentile(&mut probe_errors),
        max_end_effector_error: end_error,
        max_vertex_error: max_vertex,
        p99_vertex_error: percentile(&mut vertex_errors),
        budget,
        accepted: max_probe <= budget && max_vertex <= budget,
    })
}
pub fn validate(
    asset: &Asset,
    clip: &Clip,
    derived: &DerivedClip,
    budget: f32,
) -> Result<ErrorReport> {
    validate_with(asset, clip, budget, |_, t| derived.evaluate(asset, t))
}

/// Every candidate is validated as a complete wire round-trip. The chosen
/// allocation is independently checked again; failure retains source playback.
pub fn select(
    asset: &Asset,
    clip: &Clip,
    hash: [u8; 32],
    budget: f32,
) -> Result<Option<(DerivedClip, ErrorReport)>> {
    let mut best: Option<(DerivedClip, usize)> = None;
    for codec in [
        Codec::ReducedF32,
        Codec::Quantized(10),
        Codec::Quantized(12),
        Codec::Quantized(16),
        Codec::SampledF32,
    ] {
        let candidate = compression::encode(asset, clip, hash, 60, codec)?;
        let bytes = candidate.to_bytes();
        let decoded = DerivedClip::from_bytes(&bytes, asset, hash)?;
        if validate(asset, clip, &decoded, budget)?.accepted
            && best.as_ref().is_none_or(|(_, n)| bytes.len() < *n)
        {
            best = Some((decoded, bytes.len()));
        }
    }
    if let Some((clip_data, _)) = best {
        let report = validate(asset, clip, &clip_data, budget)?;
        ensure!(report.accepted, "Final allocation failed validation");
        Ok(Some((clip_data, report)))
    } else {
        Ok(None)
    }
}
