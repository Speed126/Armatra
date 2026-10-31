//! Deterministic skeleton descriptors. A frame is only inferred from semantic
//! pelvis/head and bilateral anchors; unknown frames never imply a facing.
use crate::scene::Asset;
use glam::{Mat3, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anatomy {
    Unknown,
    Root,
    Pelvis,
    Spine,
    Chest,
    Neck,
    Head,
    Shoulder,
    UpperArm,
    Forearm,
    Hand,
    Thigh,
    Shin,
    Foot,
    Toe,
    Finger,
    Helper,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Unknown,
    Center,
    Left,
    Right,
}
#[derive(Clone, Debug)]
pub struct Descriptor {
    pub node: usize,
    pub tokens: Vec<String>,
    pub anatomy: Anatomy,
    pub side: Side,
    pub semantic_confidence: f32,
    pub position: Vec3,
    pub direction: Vec3,
    pub length: f32,
    pub depth: f32,
    pub children: usize,
    pub parent: Option<usize>,
    pub rest_rotation: Quat,
    pub rigid: bool,
}
#[derive(Clone, Debug)]
pub struct Rig {
    pub joints: Vec<Descriptor>,
    /// Character coordinates -> asset scene coordinates. None means unknown.
    pub frame: Option<Quat>,
    pub origin: Vec3,
    pub height: f32,
    pub warnings: Vec<String>,
}
pub fn tokens(name: &str) -> Vec<String> {
    let name = name.rsplit(':').next().unwrap_or(name);
    let mut split = String::new();
    let mut prev = ' ';
    for c in name.chars() {
        if c.is_uppercase() && prev.is_lowercase() {
            split.push(' ');
        }
        split.push(if c.is_alphabetic() {
            c.to_ascii_lowercase()
        } else {
            ' '
        });
        prev = c;
    }
    split
        .split_whitespace()
        .filter(|s| !matches!(*s, "mixamorig" | "skeleton" | "joint" | "bone" | "bip"))
        .map(str::to_owned)
        .collect()
}
pub fn semantics(name: &str) -> (Anatomy, Side, f32) {
    use Anatomy::*;
    let t = tokens(name);
    let joined = t.join("");
    let has = |words: &[&str]| words.iter().any(|w| t.iter().any(|v| v == w));
    let stem = name
        .rsplit(':')
        .next()
        .unwrap_or(name)
        .split('.')
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    let vertebra = ["th", "l", "c"].into_iter().find(|p| {
        stem.strip_prefix(p)
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
    });
    let side = if vertebra.is_some() {
        Side::Center
    } else if has(&["left", "l"]) || joined.starts_with("left") {
        Side::Left
    } else if has(&["right", "r"]) || joined.starts_with("right") {
        Side::Right
    } else {
        Side::Unknown
    };
    let anatomy = if vertebra == Some("c") {
        Neck
    } else if vertebra.is_some() {
        Spine
    } else if has(&["twist", "helper", "ctrl", "control", "end", "ik", "target"]) {
        Helper
    } else if has(&[
        "thumb", "index", "pointer", "middle", "ring", "pinky", "pinkie", "little", "finger",
    ]) {
        Finger
    } else if has(&["toe", "toes", "ball"]) {
        Toe
    } else if has(&["pelvis", "hips"]) {
        Pelvis
    } else if has(&["upperleg", "upleg", "thigh", "femur", "hip"]) || joined.contains("upleg") {
        Thigh
    } else if has(&["lowerleg", "leg", "shin", "calf", "knee", "tibia"]) {
        Shin
    } else if has(&["foot", "ankle"]) {
        Foot
    } else if has(&["forearm", "lowerarm", "elbow", "radius", "ulna"]) || joined.contains("forearm")
    {
        Forearm
    } else if has(&["upperarm", "arm", "humerus"]) || joined.contains("upperarm") {
        UpperArm
    } else if has(&["hand", "wrist"]) {
        Hand
    } else if has(&["shoulder", "clavicle", "collar"]) {
        Shoulder
    } else if has(&["head", "skull"]) {
        Head
    } else if has(&["neck", "nack"]) {
        Neck
    } else if has(&["chest", "thorax", "sternum"]) {
        Chest
    } else if has(&["spine", "spinal"])
        || (t
            .first()
            .is_some_and(|s| matches!(s.as_str(), "th" | "l" | "c"))
            && name.chars().any(|c| c.is_ascii_digit()))
    {
        Spine
    } else if has(&["root", "rig", "armature", "rootjoint"]) {
        Root
    } else {
        Unknown
    };
    let side = if side == Side::Unknown
        && matches!(anatomy, Root | Pelvis | Spine | Chest | Neck | Head)
    {
        Side::Center
    } else {
        side
    };
    (anatomy, side, if anatomy == Unknown { 0. } else { 0.8 })
}
pub fn rotation(m: Mat4) -> Quat {
    m.to_scale_rotation_translation().1.normalize()
}
pub fn rigid(m: Mat4) -> bool {
    let (s, r, _) = m.to_scale_rotation_translation();
    m.is_finite()
        && s.min_element() > 1e-6
        && (s.max_element() - s.min_element()) < s.max_element() * 1e-3
        && Mat4::from_scale_rotation_translation(s, r, m.w_axis.truncate()).abs_diff_eq(m, 1e-3)
}
impl Rig {
    pub fn analyze(asset: &Asset) -> Self {
        Self::analyze_skin(asset, None)
    }
    pub fn analyze_skin(asset: &Asset, skin: Option<usize>) -> Self {
        let globals = asset.globals(&asset.rest_pose());
        let mut relevant: BTreeSet<usize> = asset
            .skins
            .iter()
            .enumerate()
            .filter(|(i, _)| skin.is_none_or(|s| s == *i))
            .map(|(_, s)| s)
            .flat_map(|s| s.joints.iter().copied())
            .collect();
        // A sparse clip animates only part of its supplied rig. Retain the
        // complete connected hierarchy (including unanimated rest anchors),
        // without pulling in unrelated scene roots or mesh attachments.
        if asset.skins.is_empty() {
            let mut roots = BTreeSet::new();
            for track in asset.clips.iter().flat_map(|c| &c.tracks) {
                let mut root = track.node;
                while let Some(parent) = asset.nodes[root].parent {
                    root = parent;
                }
                roots.insert(root);
            }
            let mut pending: Vec<_> = roots.into_iter().collect();
            while let Some(node) = pending.pop() {
                if asset.nodes[node].mesh.is_none() {
                    relevant.insert(node);
                }
                pending.extend(asset.nodes[node].children.iter().copied());
            }
        }
        let mut semantic: Vec<_> = relevant
            .iter()
            .map(|&i| (i, semantics(&asset.nodes[i].name)))
            .collect();
        // Some rigs call a thigh "leg". Topology disambiguates leg -> knee -> foot.
        for (i, (a, _, _)) in &mut semantic {
            if *a == Anatomy::Shin
                && asset.nodes[*i]
                    .children
                    .iter()
                    .any(|&c| semantics(&asset.nodes[c].name).0 == Anatomy::Shin)
            {
                *a = Anatomy::Thigh;
            }
        }
        let anchor = |a, side| {
            semantic
                .iter()
                .filter(|(_, v)| v.0 == a && (side == Side::Unknown || v.1 == side))
                .map(|v| v.0)
                .collect::<Vec<_>>()
        };
        let pelvis = anchor(Anatomy::Pelvis, Side::Unknown);
        let heads = anchor(Anatomy::Head, Side::Unknown);
        // A head and its head-tip descendants share one anatomical frame.
        // Separate head roots remain ambiguous. Keep descriptor roles intact:
        // similarity analysis still applies its own strict uniqueness rules.
        let head_roots: Vec<_> = heads
            .iter()
            .copied()
            .filter(|&i| {
                let mut parent = asset.nodes[i].parent;
                while let Some(p) = parent {
                    if heads.contains(&p) {
                        return false;
                    }
                    parent = asset.nodes[p].parent;
                }
                true
            })
            .collect();
        let origin = pelvis
            .first()
            .map_or(Vec3::ZERO, |&i| globals[i].w_axis.truncate());
        let mut frame = None;
        if pelvis.len() == 1 && head_roots.len() == 1 {
            let up = (globals[head_roots[0]].w_axis.truncate() - origin).normalize_or_zero();
            for a in [Anatomy::Thigh, Anatomy::Shoulder, Anatomy::UpperArm] {
                let l = anchor(a, Side::Left);
                let r = anchor(a, Side::Right);
                if l.len() == 1 && r.len() == 1 {
                    let lateral = globals[l[0]].w_axis.truncate() - globals[r[0]].w_axis.truncate();
                    let x = (lateral - up * lateral.dot(up)).normalize_or_zero();
                    let z = x.cross(up).normalize_or_zero();
                    if x.length_squared() > 0.9
                        && up.length_squared() > 0.9
                        && z.length_squared() > 0.9
                    {
                        frame = Some(Quat::from_mat3(&Mat3::from_cols(x, up, z)).normalize());
                        break;
                    }
                }
            }
        }
        let inv = frame.unwrap_or(Quat::IDENTITY).inverse();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &i in &relevant {
            let p = inv * (globals[i].w_axis.truncate() - origin);
            min = min.min(p);
            max = max.max(p);
        }
        let height = if relevant.is_empty() {
            1.
        } else if frame.is_some() {
            (max.y - min.y).max(1e-4)
        } else {
            (max - min).length().max(1e-4)
        };
        let mut depths = vec![0usize; asset.nodes.len()];
        for &i in &asset.order {
            depths[i] = asset.nodes[i].parent.map_or(0, |p| depths[p] + 1);
        }
        let max_depth = relevant
            .iter()
            .map(|&i| depths[i])
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        let joints = semantic
            .into_iter()
            .map(|(i, (anatomy, side, confidence))| {
                let mut parent = asset.nodes[i].parent;
                while let Some(p) = parent {
                    if relevant.contains(&p) {
                        break;
                    }
                    parent = asset.nodes[p].parent;
                }
                let delta = parent.map_or(Vec3::ZERO, |p| {
                    globals[i].w_axis.truncate() - globals[p].w_axis.truncate()
                });
                Descriptor {
                    node: i,
                    tokens: tokens(&asset.nodes[i].name),
                    anatomy,
                    side,
                    semantic_confidence: confidence,
                    position: inv * (globals[i].w_axis.truncate() - origin) / height,
                    direction: inv * delta.normalize_or_zero(),
                    length: delta.length() / height,
                    depth: depths[i] as f32 / max_depth,
                    children: asset.nodes[i].children.len(),
                    parent,
                    rest_rotation: rotation(globals[i]),
                    rigid: rigid(globals[i]),
                }
            })
            .collect();
        let mut warnings = Vec::new();
        if frame.is_none() {
            warnings.push("Character orientation unknown: geometry costs reduced; confirm frame alignment before retargeting".into());
        }
        if relevant.len() > 1024 {
            warnings.push("Rig exceeds automatic assignment limit (1024 joints)".into());
        }
        Self {
            joints,
            frame,
            origin,
            height,
            warnings,
        }
    }
}
