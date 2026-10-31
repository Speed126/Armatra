//! Rest-relative transfer using column-vector matrices and glam Hamilton
//! quaternions: target_rest * A * inverse(source_rest) * source_pose * inverse(A).
//! Unmapped source ancestors are composed before the delta is formed.
use crate::{
    animation::{self, Clip},
    correspondence,
    rig::{self, Anatomy, Rig},
    scene::{Asset, Local},
};
use anyhow::{Result, bail};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
pub const ALGORITHM_VERSION: u32 = 7;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum RootPolicy {
    #[default]
    ScaleHeight,
    Unscaled,
    InPlace,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Settings {
    pub ik: crate::ik::Settings,
    pub contacts: crate::contacts::Settings,
    pub source_skin: Option<usize>,
    pub target_skin: Option<usize>,
    /// Source scene world axes -> target scene world axes, explicitly supplied.
    pub alignment_override: Option<[f32; 4]>,
    pub root: RootPolicy,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Override {
    pub source: usize,
    pub target: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Confidence {
    High,
    Ambiguous,
    Manual,
    Unmatched,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub source: usize,
    pub target: Option<usize>,
    pub confidence: Confidence,
    pub reason: String,
    pub margin: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transfer {
    pub source: usize,
    pub target: usize,
    pub source_ancestor: Option<usize>,
    pub target_ancestor: Option<usize>,
    pub source_rest_relative: [f32; 4],
    pub target_rest_relative: [f32; 4],
    pub alignment: [f32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TwistChain {
    pub nodes: Vec<usize>,
    pub weights: Vec<f32>,
    pub axes: Vec<[f32; 3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanKey {
    pub source: String,
    pub target: String,
    pub settings: String,
    pub algorithm: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub target_height: f32,
    pub key: PlanKey,
    pub settings: Settings,
    pub overrides: Vec<Override>,
    pub rows: Vec<Row>,
    pub transfers: Vec<Transfer>,
    pub chains: Vec<TwistChain>,
    pub frame: [f32; 4],
    pub scale: f32,
    pub motion_source: usize,
    pub motion_target: usize,
    pub warnings: Vec<String>,
}
pub fn signature(asset: &Asset) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&(asset.nodes.len() as u64).to_le_bytes());
    for node in &asset.nodes {
        h.update(&(node.name.len() as u64).to_le_bytes());
        h.update(node.name.as_bytes());
        h.update(&(node.parent.map_or(u64::MAX, |i| i as u64)).to_le_bytes());
        for x in node.rest.matrix().to_cols_array() {
            h.update(&x.to_le_bytes());
        }
    }
    for skin in &asset.skins {
        h.update(&(skin.joints.len() as u64).to_le_bytes());
        for &j in &skin.joints {
            h.update(&(j as u64).to_le_bytes());
        }
    }
    if asset.skins.is_empty() {
        let animated: std::collections::BTreeSet<_> = asset
            .clips
            .iter()
            .flat_map(|c| c.tracks.iter().map(|t| t.node))
            .collect();
        for node in animated {
            h.update(&(node as u64).to_le_bytes());
        }
    }
    h.finalize().to_hex().to_string()
}
pub fn key(source: &Asset, target: &Asset, settings: &Settings) -> PlanKey {
    PlanKey {
        source: signature(source),
        target: signature(target),
        settings: blake3::hash(&serde_json::to_vec(settings).expect("finite settings"))
            .to_hex()
            .to_string(),
        algorithm: ALGORITHM_VERSION,
    }
}
pub fn compile(
    source: &Asset,
    target: &Asset,
    settings: &Settings,
    overrides: &[Override],
) -> Result<Plan> {
    for (side, asset, selected) in [
        ("Source", source, settings.source_skin),
        ("Target", target, settings.target_skin),
    ] {
        if let Some(index) = selected
            && index >= asset.skins.len()
        {
            bail!(
                "{side} skin {index} does not exist ({} skins available)",
                asset.skins.len()
            );
        }
        if asset.skins.len() > 1 && selected.is_none() {
            bail!(
                "Multiple skins: {side} has {} skins. Select a {side} skin before applying.",
                asset.skins.len()
            );
        }
    }
    let s = Rig::analyze_skin(source, settings.source_skin);
    let t = Rig::analyze_skin(target, settings.target_skin);
    if s.joints.is_empty() || t.joints.is_empty() {
        bail!(
            "No identifiable source/target rig; associate a source rig or supply a rigged animation"
        )
    }
    if s.joints.len() > 1024 || t.joints.len() > 1024 {
        bail!("Automatic correspondence is limited to 1024 joints per selected rig")
    }
    if s.joints.iter().chain(&t.joints).any(|d| !d.rigid) {
        bail!(
            "Retargeting needs rigid rest frames with positive uniform scale; affine/nonuniform or reflected rig transforms are unsupported"
        )
    }
    let frame = if let Some(q) = settings.alignment_override {
        let q = Quat::from_array(q);
        if !q.is_finite() || (q.length_squared() - 1.).abs() > 1e-3 {
            bail!("Frame alignment must be a finite unit quaternion")
        }
        q
    } else {
        match (s.frame, t.frame) {
            (Some(a), Some(b)) => b * a.inverse(),
            _ => bail!(
                "Character orientation unknown; confirm source-to-target frame alignment in the mapping editor"
            ),
        }
    };
    let result = correspondence::refine(&s, &t)?;
    let mut nodes: Vec<Option<usize>> = vec![None; source.nodes.len()];
    for (i, j) in result.assignment.targets.iter().enumerate() {
        nodes[s.joints[i].node] = j.map(|j| t.joints[j].node);
    }
    let mut manual = HashSet::new();
    let mut manual_targets = HashSet::new();
    for o in overrides {
        if !s.joints.iter().any(|d| d.node == o.source)
            || o.target
                .is_some_and(|j| !t.joints.iter().any(|d| d.node == j))
            || !manual.insert(o.source)
            || o.target.is_some_and(|j| !manual_targets.insert(j))
        {
            bail!("Manual overrides need unique valid source and target joints")
        }
    }
    for o in overrides {
        if let Some(j) = o.target {
            for (i, n) in nodes.iter_mut().enumerate() {
                if i != o.source && *n == Some(j) {
                    *n = None;
                }
            }
        }
        nodes[o.source] = o.target;
    }
    let sg = source.globals(&source.rest_pose());
    let tg = target.globals(&target.rest_pose());
    let mut transfers = Vec::new();
    let mut rows = Vec::new();
    let mut warnings = s.warnings.clone();
    warnings.extend(t.warnings.clone());
    if target.skins.len() > 1 {
        warnings.push("Only the selected target skin's joints participate in retargeting. Other skins remain visible; independent rigs are not automatically attached or mapped.".into());
    }
    let p = crate::assignment::problem(&s, &t);
    for (i, d) in s.joints.iter().enumerate() {
        let j = nodes[d.node];
        let mut margin = 0.;
        let confidence = if manual.contains(&d.node) {
            Confidence::Manual
        } else if let Some(j) = j {
            let ti = t.joints.iter().position(|d| d.node == j).unwrap();
            // A global exclusion margin is a score difference, not a probability.
            if s.joints.len() <= 128 {
                let mut alternate = p.clone();
                alternate.costs[i][ti] = None;
                use crate::assignment::AssignmentSolver;
                margin = crate::assignment::ShortestPath.solve(&alternate)?.cost
                    - crate::assignment::ShortestPath.solve(&p)?.cost;
            }
            if margin > 0.2
                && d.anatomy != Anatomy::Unknown
                && d.side != rig::Side::Unknown
                && t.joints[ti].side != rig::Side::Unknown
                && t.joints[ti].anatomy != Anatomy::Unknown
            {
                Confidence::High
            } else {
                Confidence::Ambiguous
            }
        } else {
            Confidence::Unmatched
        };
        let reason = match confidence {
            Confidence::Manual => "Manual override",
            Confidence::High => "Anatomy, side, geometry and global assignment margin",
            Confidence::Ambiguous => {
                "Review: weak semantics, unknown side or close global alternative"
            }
            Confidence::Unmatched => "No compatible target; explicit unmatched penalty",
        }
        .to_string();
        rows.push(Row {
            source: d.node,
            target: j,
            confidence,
            reason,
            margin,
        });
        let Some(j) = j else { continue };
        if !matches!(source.nodes[d.node].rest, Local::Trs(_))
            || !matches!(target.nodes[j].rest, Local::Trs(_))
        {
            bail!("Matrix-authored mapped joints need explicit TRS conversion")
        }
        let mut ancestor = source.nodes[d.node].parent;
        while let Some(a) = ancestor {
            if nodes[a].is_some() {
                break;
            }
            ancestor = source.nodes[a].parent;
        }
        let ta = ancestor.and_then(|a| nodes[a]);
        if let Some(a) = ta {
            let mut parent = target.nodes[j].parent;
            while let Some(p) = parent {
                if p == a {
                    break;
                }
                parent = target.nodes[p].parent;
            }
            if parent.is_none() {
                bail!(
                    "Mapping hierarchy inconsistent at source joint {}: review parent/chain assignments",
                    d.node
                )
            }
        }
        let sr = ancestor.map_or(rig::rotation(sg[d.node]), |a| {
            rig::rotation(sg[a]).inverse() * rig::rotation(sg[d.node])
        });
        let tr = ta.map_or(rig::rotation(tg[j]), |a| {
            rig::rotation(tg[a]).inverse() * rig::rotation(tg[j])
        });
        let alignment = rig::rotation(tg[j]).inverse() * frame * rig::rotation(sg[d.node]);
        transfers.push(Transfer {
            source: d.node,
            target: j,
            source_ancestor: ancestor,
            target_ancestor: ta,
            source_rest_relative: sr.to_array(),
            target_rest_relative: tr.to_array(),
            alignment: alignment.normalize().to_array(),
        });
    }
    transfers.sort_by_key(|x| {
        target
            .order
            .iter()
            .position(|&i| i == x.target)
            .unwrap_or(usize::MAX)
    });
    let root = s
        .joints
        .iter()
        .find(|d| d.anatomy == Anatomy::Pelvis && nodes[d.node].is_some())
        .or_else(|| {
            s.joints
                .iter()
                .find(|d| d.anatomy == Anatomy::Root && nodes[d.node].is_some())
        })
        .ok_or_else(|| anyhow::anyhow!("Map a pelvis or locomotion root before playback"))?
        .node;
    let motion_target = nodes[root].unwrap();
    let mapped_targets: HashSet<_> = nodes.iter().flatten().copied().collect();
    let mut chains = Vec::new();
    for transfer in &transfers {
        let start = transfer.target;
        let mut current = start;
        let mut chain = vec![start];
        while target.nodes[current].children.len() == 1 {
            let next = target.nodes[current].children[0];
            if mapped_targets.contains(&next) {
                break;
            }
            if rig::semantics(&target.nodes[next].name).0 != Anatomy::Helper {
                break;
            }
            chain.push(next);
            current = next;
        }
        if chain.len() < 2 {
            continue;
        }
        let end = target.nodes[current]
            .children
            .first()
            .copied()
            .unwrap_or(current);
        let axis = (rig::rotation(tg[start]).inverse()
            * (tg[end].w_axis.truncate() - tg[start].w_axis.truncate()))
        .normalize_or_zero();
        if axis.length_squared() < 0.5 {
            continue;
        }
        let mut weights = Vec::new();
        let mut axes = Vec::new();
        for (k, &node) in chain.iter().enumerate() {
            let next = chain.get(k + 1).copied().unwrap_or(end);
            weights.push(
                tg[node]
                    .w_axis
                    .truncate()
                    .distance(tg[next].w_axis.truncate())
                    .max(1e-5),
            );
            axes.push(
                (rig::rotation(tg[node]).inverse() * rig::rotation(tg[start]) * axis)
                    .normalize()
                    .to_array(),
            );
        }
        let sum: f32 = weights.iter().sum();
        for w in &mut weights {
            *w /= sum;
        }
        chains.push(TwistChain {
            nodes: chain,
            weights,
            axes,
        });
    }
    if rows.iter().any(|r| r.confidence == Confidence::Ambiguous) {
        warnings.push("Ambiguous mappings are candidates: inspect and correct them before relying on playback".into());
    }
    Ok(Plan {
        target_height: t.height,
        key: key(source, target, settings),
        settings: settings.clone(),
        overrides: overrides.to_vec(),
        rows,
        transfers,
        chains,
        frame: frame.to_array(),
        scale: t.height / s.height,
        motion_source: root,
        motion_target,
        warnings,
    })
}
pub fn swing_twist(q: Quat, axis: Vec3) -> (Quat, Quat) {
    let q = if q.w < 0. { -q } else { q };
    let a = axis.normalize_or_zero();
    let v = Vec3::new(q.x, q.y, q.z);
    let p = a * v.dot(a);
    let raw = Quat::from_xyzw(p.x, p.y, p.z, q.w);
    let twist = if raw.length_squared() < 1e-12 {
        Quat::IDENTITY
    } else {
        raw.normalize()
    };
    ((q * twist.inverse()).normalize(), twist)
}
pub fn transfer_checked(
    source: &Asset,
    target: &Asset,
    source_pose: &[Local],
    plan: &Plan,
) -> Result<Vec<Local>> {
    anyhow::ensure!(
        source_pose.len() == source.nodes.len(),
        "Source pose size mismatch"
    );
    let sg = source.globals(source_pose);
    anyhow::ensure!(
        plan.transfers.iter().all(|t| rig::rigid(sg[t.source])),
        "Unsupported animated source frame: singular, reflected, nonuniform or non-finite rig transform"
    );
    let sr = source.globals(&source.rest_pose());
    let tr = target.globals(&target.rest_pose());
    let mut pose = target.rest_pose();
    let mut global = vec![Mat4::IDENTITY; target.nodes.len()];
    let transfers: HashMap<_, _> = plan.transfers.iter().map(|t| (t.target, t)).collect();
    for &i in &target.order {
        let parent = target.nodes[i].parent.map_or(Mat4::IDENTITY, |p| global[p]);
        if let Some(t) = transfers.get(&i)
            && let Local::Trs(local) = &mut pose[i]
        {
            let source_relative = t.source_ancestor.map_or(rig::rotation(sg[t.source]), |a| {
                rig::rotation(sg[a]).inverse() * rig::rotation(sg[t.source])
            });
            let delta = Quat::from_array(t.source_rest_relative).inverse() * source_relative;
            let a = Quat::from_array(t.alignment);
            let desired = t
                .target_ancestor
                .map_or(Quat::IDENTITY, |p| rig::rotation(global[p]))
                * Quat::from_array(t.target_rest_relative)
                * a
                * delta
                * a.inverse();
            local.rotation = (rig::rotation(parent).inverse() * desired).normalize();
        }
        if i == plan.motion_target
            && let Local::Trs(local) = &mut pose[i]
        {
            let factor = match plan.settings.root {
                RootPolicy::ScaleHeight => plan.scale,
                RootPolicy::Unscaled => 1.,
                RootPolicy::InPlace => 0.,
            };
            let displacement = Quat::from_array(plan.frame)
                * (sg[plan.motion_source].w_axis.truncate()
                    - sr[plan.motion_source].w_axis.truncate())
                * factor;
            local.translation = parent
                .inverse()
                .transform_point3(tr[i].w_axis.truncate() + displacement);
        }
        global[i] = parent * pose[i].matrix();
    }
    for chain in &plan.chains {
        let Local::Trs(rest) = target.nodes[chain.nodes[0]].rest else {
            continue;
        };
        let Local::Trs(local) = pose[chain.nodes[0]] else {
            continue;
        };
        let delta = rest.rotation.inverse() * local.rotation;
        let (swing, twist) = swing_twist(delta, Vec3::from_array(chain.axes[0]));
        let angle = 2. * twist.w.clamp(-1., 1.).acos();
        let sign = Vec3::new(twist.x, twist.y, twist.z)
            .dot(Vec3::from_array(chain.axes[0]))
            .signum();
        for (k, &node) in chain.nodes.iter().enumerate() {
            if let (Local::Trs(r), Local::Trs(p)) = (target.nodes[node].rest, &mut pose[node]) {
                let part = Quat::from_axis_angle(
                    Vec3::from_array(chain.axes[k]),
                    angle * sign * chain.weights[k],
                );
                p.rotation = (r.rotation * if k == 0 { swing * part } else { part }).normalize();
            }
        }
    }
    anyhow::ensure!(
        pose.iter().all(|p| p.matrix().is_finite()),
        "Non-finite retarget pose; rest fallback used"
    );
    Ok(pose)
}
pub fn transfer(source: &Asset, target: &Asset, source_pose: &[Local], plan: &Plan) -> Vec<Local> {
    transfer_checked(source, target, source_pose, plan).unwrap_or_else(|_| target.rest_pose())
}
pub fn sample(source: &Asset, target: &Asset, clip: &Clip, plan: &Plan, time: f32) -> Vec<Local> {
    sample_checked(source, target, clip, plan, time).unwrap_or_else(|_| target.rest_pose())
}
pub fn sample_checked(
    source: &Asset,
    target: &Asset,
    clip: &Clip,
    plan: &Plan,
    time: f32,
) -> Result<Vec<Local>> {
    let identity: Vec<_> = (0..source.nodes.len()).map(Some).collect();
    let source_pose = animation::evaluate(source, clip, &identity, time, &mut Vec::new());
    transfer_checked(source, target, &source_pose, plan)
}
