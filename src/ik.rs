//! Stateless, contact-constrained pelvis and analytical two-bone IK. All goals
//! are in target world space. Segment lengths are preserved; unreachable goals
//! retain a residual instead of stretching or producing non-finite rotations.
use crate::{
    animation::Clip,
    contacts::Contacts,
    retarget::Plan,
    rig::{self, Anatomy},
    scene::{Asset, Local},
};
use anyhow::{Result, bail};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub pelvis_stiffness: f32,
    pub max_pelvis_height: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            pelvis_stiffness: 1.,
            max_pelvis_height: 0.08,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Solution {
    pub middle: Vec3,
    pub end: Vec3,
    pub unreachable: bool,
    pub residual: f32,
}
pub fn solve(a: Vec3, b: Vec3, c: Vec3, goal: Vec3, pole: Vec3) -> Result<Solution> {
    if [a, b, c, goal, pole].iter().any(|p| !p.is_finite()) {
        bail!("IK points must be finite")
    }
    let first = a.distance(b);
    let second = b.distance(c);
    if first < 1e-6 || second < 1e-6 {
        bail!("Degenerate zero-length IK segment")
    }
    let d = goal - a;
    let distance = d.length();
    let direction = if distance > 1e-7 {
        d / distance
    } else {
        (c - a).normalize_or(Vec3::Y)
    };
    let epsilon = (first + second) * 1e-6;
    let reach = distance.clamp((first - second).abs() + epsilon, first + second - epsilon);
    let along = (first * first - second * second + reach * reach) / (2. * reach);
    let height = (first * first - along * along).max(0.).sqrt();
    let mut bend = (b - a) - direction * (b - a).dot(direction);
    if bend.length_squared() < epsilon * epsilon {
        bend = (pole - a) - direction * (pole - a).dot(direction);
    }
    if bend.length_squared() < epsilon * epsilon {
        let reference = if direction.z.abs() < 0.8 {
            Vec3::Z
        } else {
            Vec3::X
        };
        bend = reference - direction * reference.dot(direction);
    }
    let middle = a + direction * along + bend.normalize() * height;
    let end = a + direction * reach;
    Ok(Solution {
        middle,
        end,
        unreachable: (distance - reach).abs() > epsilon * 2.,
        residual: end.distance(goal),
    })
}
#[derive(Clone, Debug, Serialize)]
pub struct FootResult {
    pub node: usize,
    pub weight: f32,
    pub unreachable: bool,
    pub residual: f32,
}
#[derive(Clone, Debug)]
pub struct Correction {
    pub pose: Vec<Local>,
    pub pelvis_delta: Vec3,
    pub feet: Vec<FootResult>,
    pub warnings: Vec<String>,
}
fn ancestor(asset: &Asset, a: usize, mut b: usize) -> bool {
    for _ in 0..asset.nodes.len() {
        if a == b {
            return true;
        }
        let Some(p) = asset.nodes[b].parent else {
            return false;
        };
        b = p;
    }
    false
}
fn limb(source: &Asset, target: &Asset, plan: &Plan, foot: usize) -> Option<(usize, usize)> {
    // Prefer target anatomy, falling back to mapped source anatomy for unnamed
    // target rigs. Mapped helper subdivisions may lie between these endpoints.
    let find = |asset: &Asset, node: usize, anatomy| {
        let mut parent = asset.nodes[node].parent;
        while let Some(p) = parent {
            if rig::semantics(&asset.nodes[p].name).0 == anatomy {
                return Some(p);
            }
            parent = asset.nodes[p].parent;
        }
        None
    };
    if let Some(mid) = find(target, foot, Anatomy::Shin)
        && let Some(upper) = find(target, mid, Anatomy::Thigh)
    {
        return Some((upper, mid));
    }
    let sf = plan.rows.iter().find(|r| r.target == Some(foot))?.source;
    let sm = find(source, sf, Anatomy::Shin)?;
    let su = find(source, sm, Anatomy::Thigh)?;
    let mid = plan.rows.iter().find(|r| r.source == sm)?.target?;
    let upper = plan.rows.iter().find(|r| r.source == su)?.target?;
    (ancestor(target, upper, mid) && ancestor(target, mid, foot)).then_some((upper, mid))
}
fn world_rotation(
    asset: &Asset,
    pose: &mut [Local],
    globals: &[Mat4],
    placement: Mat4,
    node: usize,
    desired: Quat,
) {
    let parent = asset.nodes[node]
        .parent
        .map_or(placement, |p| placement * globals[p]);
    if let Local::Trs(local) = &mut pose[node] {
        local.rotation = (rig::rotation(parent).inverse() * desired).normalize();
    }
}
#[allow(clippy::too_many_arguments)]
pub fn correct(
    source: &Asset,
    target: &Asset,
    plan: &Plan,
    contacts: &Contacts,
    pose: Vec<Local>,
    unwrapped: f64,
    looping: bool,
    legs: bool,
) -> Correction {
    let settings = &plan.settings.ik;
    let mut output = Correction {
        pose,
        pelvis_delta: Vec3::ZERO,
        feet: Vec::new(),
        warnings: Vec::new(),
    };
    if !settings.pelvis_stiffness.is_finite()
        || settings.pelvis_stiffness < 0.
        || !settings.max_pelvis_height.is_finite()
        || settings.max_pelvis_height < 0.
    {
        output
            .warnings
            .push("Invalid pelvis settings; correction disabled".into());
        return output;
    }
    let active = contacts.active(unwrapped, looping);
    let placement = Mat4::from_cols_array(&contacts.placement);
    let mut global = target.globals(&output.pose);
    let mut sum = Vec3::ZERO;
    let mut weight = 0.;
    for c in &active {
        if ancestor(target, plan.motion_target, c.target) {
            let p = placement.transform_point3(global[c.target].w_axis.truncate());
            sum += (c.anchor - p) * c.weight;
            weight += c.weight;
        }
    }
    if weight > 0. {
        let world_scale = placement.x_axis.truncate().length();
        let mut delta = (sum / (settings.pelvis_stiffness + weight).max(1e-6))
            .clamp_length_max(settings.max_pelvis_height * plan.target_height * world_scale);
        // Project the translation onto the ground half-space so planting one
        // foot cannot push a free foot farther below the reference plane.
        let normal = Vec3::from_array(contacts.target_normal);
        let cycle = crate::contacts::time(unwrapped, contacts.duration, looping).cycle;
        let ground_height = contacts.target_height
            + normal.dot(Vec3::from_array(contacts.cycle_displacement) * cycle as f32);
        let mut lower = f32::NEG_INFINITY;
        for row in &plan.rows {
            if rig::semantics(&source.nodes[row.source].name).0 == Anatomy::Foot
                && let Some(j) = row.target
            {
                let height = normal.dot(placement.transform_point3(global[j].w_axis.truncate()));
                lower = lower.max(ground_height.min(height) - height);
            }
        }
        if lower.is_finite() {
            delta += normal * (lower - normal.dot(delta)).max(0.);
        }
        delta =
            delta.clamp_length_max(settings.max_pelvis_height * plan.target_height * world_scale);
        let parent = target.nodes[plan.motion_target]
            .parent
            .map_or(placement, |p| placement * global[p]);
        if let Local::Trs(root) = &mut output.pose[plan.motion_target] {
            root.translation += parent.inverse().transform_vector3(delta);
            output.pelvis_delta = delta;
        }
        global = target.globals(&output.pose);
    }
    if !legs {
        return output;
    }
    for c in active.into_iter().filter(|c| c.weight > 1e-6) {
        let Some((upper, mid)) = limb(source, target, plan, c.target) else {
            output.warnings.push(format!(
                "Foot {}: no valid mapped thigh/shin chain; IK skipped",
                c.target
            ));
            continue;
        };
        let (Local::Trs(raw_upper), Local::Trs(raw_mid)) = (output.pose[upper], output.pose[mid])
        else {
            continue;
        };
        let wa = placement * global[upper];
        let wb = placement * global[mid];
        let wc = placement * global[c.target];
        let a = wa.w_axis.truncate();
        let b = wb.w_axis.truncate();
        let end = wc.w_axis.truncate();
        let solution = match solve(a, b, end, c.anchor, b) {
            Ok(s) => s,
            Err(e) => {
                output.warnings.push(format!("Foot {}: {e}", c.target));
                continue;
            }
        };
        let delta = Quat::from_rotation_arc((b - a).normalize(), (solution.middle - a).normalize());
        world_rotation(
            target,
            &mut output.pose,
            &global,
            placement,
            upper,
            delta * rig::rotation(wa),
        );
        global = target.globals(&output.pose);
        let b2 = placement.transform_point3(global[mid].w_axis.truncate());
        let c2 = placement.transform_point3(global[c.target].w_axis.truncate());
        let delta = Quat::from_rotation_arc((c2 - b2).normalize(), (solution.end - b2).normalize());
        let mid_world = placement * global[mid];
        world_rotation(
            target,
            &mut output.pose,
            &global,
            placement,
            mid,
            delta * rig::rotation(mid_world),
        );
        if let Local::Trs(local) = &mut output.pose[upper] {
            local.rotation = raw_upper
                .rotation
                .slerp(local.rotation, c.weight)
                .normalize();
        }
        if let Local::Trs(local) = &mut output.pose[mid] {
            local.rotation = raw_mid.rotation.slerp(local.rotation, c.weight).normalize();
        }
        global = target.globals(&output.pose);
        // Keep the original foot orientation while leg rotations satisfy contact.
        world_rotation(
            target,
            &mut output.pose,
            &global,
            placement,
            c.target,
            rig::rotation(wc),
        );
        global = target.globals(&output.pose);
        let residual = placement
            .transform_point3(global[c.target].w_axis.truncate())
            .distance(c.anchor);
        if solution.unreachable && solution.residual > plan.target_height * 0.05 && c.weight > 0.95
        {
            output.warnings.push(format!(
                "Foot {}: unreachable contact, residual {:.4}; limb length retained",
                c.target, residual
            ));
        }
        output.feet.push(FootResult {
            node: c.target,
            weight: c.weight,
            unreachable: solution.unreachable,
            residual,
        });
    }
    output
}
pub fn sample(
    source: &Asset,
    target: &Asset,
    clip: &Clip,
    plan: &Plan,
    contacts: &Contacts,
    unwrapped: f64,
    looping: bool,
) -> Correction {
    let pose = match contacts.raw_pose_checked(source, target, clip, plan, unwrapped, looping) {
        Ok(p) => p,
        Err(e) => {
            return Correction {
                pose: target.rest_pose(),
                pelvis_delta: Vec3::ZERO,
                feet: Vec::new(),
                warnings: vec![format!("Retarget disabled at this time: {e:#}")],
            };
        }
    };
    correct(
        source, target, plan, contacts, pose, unwrapped, looping, true,
    )
}
