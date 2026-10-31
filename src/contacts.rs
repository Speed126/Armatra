//! Offline contacts and target-world onset anchors. Evaluation has no mutable
//! playback state: (clip time, cycle, plan, placement) fully determines contacts.
use crate::{
    animation::{self, Clip},
    retarget::{self, Plan},
    rig::{Anatomy, Rig, Side},
    scene::{Asset, Local},
};
use anyhow::{Result, bail};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::path::Path;
pub const VERSION: u32 = 1;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Ground {
    Disabled,
    /// Explicit policy: plane through the lowest rest foot-joint, normal from
    /// the validated character frame. This measures ankle contact, not mesh soles.
    RestFeet,
    Plane {
        source_normal: [f32; 3],
        source_height: f32,
        target_height: f32,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LoopPolicy {
    Repeat,
    Accumulate,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub ground: Ground,
    pub looping: LoopPolicy,
    pub sample_rate: f32,
    pub enter_speed: f32,
    pub exit_speed: f32,
    pub enter_height: f32,
    pub exit_height: f32,
    pub minimum_duration: f32,
    pub transition: f32,
    pub project_ground: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            ground: Ground::RestFeet,
            looping: LoopPolicy::Repeat,
            sample_rate: 60.,
            enter_speed: 0.08,
            exit_speed: 0.16,
            enter_height: 0.035,
            exit_height: 0.06,
            minimum_duration: 0.08,
            transition: 0.12,
            project_ground: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Interval {
    pub source: usize,
    pub target: usize,
    pub start: f32,
    pub end: f32,
    pub source_point: [f32; 3],
    pub anchor: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contacts {
    pub key: String,
    pub version: u32,
    pub settings: Settings,
    pub duration: f32,
    pub intervals: Vec<Interval>,
    pub cycle_displacement: [f32; 3],
    pub placement: [f32; 16],
    pub target_normal: [f32; 3],
    pub target_height: f32,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Active {
    pub target: usize,
    pub anchor: Vec3,
    pub weight: f32,
    pub interval: usize,
    pub cycle: i64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Time {
    pub local: f32,
    pub cycle: i64,
}
pub fn time(unwrapped: f64, duration: f32, looping: bool) -> Time {
    if !unwrapped.is_finite() || !duration.is_finite() || duration <= 0. {
        return Time {
            local: 0.,
            cycle: 0,
        };
    }
    if !looping {
        return Time {
            local: unwrapped.clamp(0., duration as f64) as f32,
            cycle: 0,
        };
    }
    let cycles = (unwrapped / duration as f64).floor();
    Time {
        local: unwrapped.rem_euclid(duration as f64) as f32,
        cycle: cycles.clamp(-1e9, 1e9) as i64,
    }
}
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    x * x * (3. - 2. * x)
}
impl Contacts {
    pub fn cpu_bytes(&self) -> usize {
        size_of::<Self>()
            + self.intervals.capacity() * size_of::<Interval>()
            + self.warnings.iter().map(String::capacity).sum::<usize>()
    }
    pub fn active(&self, unwrapped: f64, looping: bool) -> Vec<Active> {
        let t = time(unwrapped, self.duration, looping);
        self.intervals
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if t.local < c.start || t.local > c.end {
                    return None;
                }
                let window = self
                    .settings
                    .transition
                    .min((c.end - c.start) * 0.5)
                    .max(1e-5);
                let weight =
                    smooth((t.local - c.start) / window).min(smooth((c.end - t.local) / window));
                Some(Active {
                    target: c.target,
                    anchor: Vec3::from_array(c.anchor)
                        + Vec3::from_array(self.cycle_displacement) * t.cycle as f32,
                    weight,
                    interval: i,
                    cycle: t.cycle,
                })
            })
            .collect()
    }
    pub fn raw_pose_checked(
        &self,
        source: &Asset,
        target: &Asset,
        clip: &Clip,
        plan: &Plan,
        unwrapped: f64,
        looping: bool,
    ) -> Result<Vec<Local>> {
        let t = time(unwrapped, self.duration, looping);
        let mut pose = retarget::sample_checked(source, target, clip, plan, t.local)?;
        let global = target.globals(&pose);
        let parent = target.nodes[plan.motion_target]
            .parent
            .map_or(Mat4::IDENTITY, |p| global[p]);
        let scene_displacement = Mat4::from_cols_array(&self.placement)
            .inverse()
            .transform_vector3(Vec3::from_array(self.cycle_displacement) * t.cycle as f32);
        if let Local::Trs(root) = &mut pose[plan.motion_target] {
            root.translation += parent.inverse().transform_vector3(scene_displacement);
        }
        Ok(pose)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn raw_pose(
        &self,
        source: &Asset,
        target: &Asset,
        clip: &Clip,
        plan: &Plan,
        unwrapped: f64,
        looping: bool,
    ) -> Vec<Local> {
        self.raw_pose_checked(source, target, clip, plan, unwrapped, looping)
            .unwrap_or_else(|_| target.rest_pose())
    }
}
pub fn identity(clip: &Clip, plan: &Plan, placement: Mat4) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&VERSION.to_le_bytes());
    h.update(&serde_json::to_vec(plan).expect("plan"));
    h.update(&clip.duration.to_le_bytes());
    for track in &clip.tracks {
        h.update(&(track.node as u64).to_le_bytes());
        h.update(format!("{:?}{:?}", track.property, track.interpolation).as_bytes());
        for v in &track.times {
            h.update(&v.to_le_bytes());
        }
        for v in &track.values {
            for x in v.to_array() {
                h.update(&x.to_le_bytes());
            }
        }
    }
    for x in placement.to_cols_array() {
        h.update(&x.to_le_bytes());
    }
    h.finalize().to_hex().to_string()
}
pub fn compile(
    source: &Asset,
    target: &Asset,
    clip: &Clip,
    plan: &Plan,
    placement: Mat4,
) -> Result<Contacts> {
    let settings = &plan.settings.contacts;
    if !clip.supported
        || !clip.duration.is_finite()
        || clip.duration <= 0.
        || !crate::rig::rigid(placement)
    {
        bail!("Contacts require a supported positive-duration clip and rigid placement")
    }
    if !settings.sample_rate.is_finite()
        || !(10.0..=240.).contains(&settings.sample_rate)
        || [
            settings.enter_speed,
            settings.exit_speed,
            settings.enter_height,
            settings.exit_height,
            settings.minimum_duration,
            settings.transition,
        ]
        .iter()
        .any(|x| !x.is_finite() || *x < 0.)
        || settings.enter_speed > settings.exit_speed
        || settings.enter_height > settings.exit_height
        || settings.transition == 0.
    {
        bail!("Invalid contact thresholds or sampling rate")
    }
    let count = (clip.duration * settings.sample_rate).ceil() as usize;
    if count > 120_000 {
        bail!("Contact preprocessing exceeds 120,000 sample limit")
    }
    let source_rig = Rig::analyze_skin(source, plan.settings.source_skin);
    let target_rig = Rig::analyze_skin(target, plan.settings.target_skin);
    let mut feet = Vec::new();
    for side in [Side::Left, Side::Right] {
        if let Some(d) = source_rig
            .joints
            .iter()
            .filter(|d| d.anatomy == Anatomy::Foot && d.side == side)
            .min_by(|a, b| a.depth.total_cmp(&b.depth))
            && let Some(j) = plan
                .rows
                .iter()
                .find(|r| r.source == d.node)
                .and_then(|r| r.target)
        {
            feet.push((d.node, j));
        }
    }
    let mut data = Contacts {
        key: identity(clip, plan, placement),
        version: VERSION,
        settings: settings.clone(),
        duration: clip.duration,
        intervals: Vec::new(),
        cycle_displacement: [0.; 3],
        placement: placement.to_cols_array(),
        target_normal: Vec3::Y.to_array(),
        target_height: 0.,
        warnings: Vec::new(),
    };
    if settings.looping == LoopPolicy::Accumulate {
        let first = target.globals(&retarget::sample(source, target, clip, plan, 0.));
        let last = target.globals(&retarget::sample(source, target, clip, plan, clip.duration));
        data.cycle_displacement = placement
            .transform_vector3(
                last[plan.motion_target].w_axis.truncate()
                    - first[plan.motion_target].w_axis.truncate(),
            )
            .to_array();
    }
    if settings.ground == Ground::Disabled {
        data.warnings
            .push("Contact locking disabled: no ground policy selected".into());
        return Ok(data);
    }
    if feet.is_empty() {
        data.warnings
            .push("No mapped semantic feet: automatic contact locking disabled".into());
        return Ok(data);
    }
    let sg = source.globals(&source.rest_pose());
    let tg = target.globals(&target.rest_pose());
    let (normal, source_height, target_normal, target_height) = match settings.ground {
        Ground::Disabled => unreachable!(),
        Ground::RestFeet => {
            let (Some(sf), Some(tf)) = (source_rig.frame, target_rig.frame) else {
                data.warnings.push("Contact locking unavailable: character frame unknown. Loop placement still applies; inspect orientation or choose an explicit plane.".into());
                return Ok(data);
            };
            let sn = sf * Vec3::Y;
            let tn = tf * Vec3::Y;
            (
                sn,
                feet.iter()
                    .map(|&(i, _)| sn.dot(sg[i].w_axis.truncate()))
                    .fold(f32::INFINITY, f32::min),
                tn,
                feet.iter()
                    .map(|&(_, j)| tn.dot(tg[j].w_axis.truncate()))
                    .fold(f32::INFINITY, f32::min),
            )
        }
        Ground::Plane {
            source_normal,
            source_height,
            target_height,
        } => {
            let sn = Vec3::from_array(source_normal);
            if !sn.is_finite()
                || (sn.length_squared() - 1.).abs() > 1e-3
                || !source_height.is_finite()
                || !target_height.is_finite()
            {
                bail!("Ground plane must have a unit normal and finite offsets")
            }
            (
                sn,
                source_height,
                glam::Quat::from_array(plan.frame) * sn,
                target_height,
            )
        }
    };
    let world_normal = placement.transform_vector3(target_normal).normalize();
    data.target_normal = world_normal.to_array();
    data.target_height =
        world_normal.dot(placement.transform_point3(target_normal * target_height));
    let dt = clip.duration / count.max(1) as f32;
    let identity: Vec<_> = (0..source.nodes.len()).map(Some).collect();
    let mut cursors = Vec::new();
    let mut positions = vec![Vec::with_capacity(count + 1); feet.len()];
    for k in 0..=count {
        let pose = animation::evaluate(source, clip, &identity, k as f32 * dt, &mut cursors);
        let global = source.globals(&pose);
        anyhow::ensure!(
            plan.transfers
                .iter()
                .all(|t| crate::rig::rigid(global[t.source])),
            "Unsupported animated source frame; contact retargeting needs rigid positive uniform transforms"
        );
        for (f, &(node, _)) in feet.iter().enumerate() {
            positions[f].push(global[node].w_axis.truncate());
        }
    }
    for (f, &(node, target_node)) in feet.iter().enumerate() {
        let p = &positions[f];
        let mut start = None;
        for k in 0..=count {
            let a = k.saturating_sub(1);
            let b = (k + 1).min(count);
            let speed = p[b].distance(p[a]) / ((b - a).max(1) as f32 * dt) / source_rig.height;
            let height = (normal.dot(p[k]) - source_height) / source_rig.height;
            let contact = if start.is_some() {
                speed <= settings.exit_speed && height <= settings.exit_height
            } else {
                speed <= settings.enter_speed && height <= settings.enter_height
            };
            if contact && start.is_none() {
                start = Some(k);
            }
            if (!contact || k == count)
                && let Some(begin) = start.take()
            {
                let end = if contact { k } else { k.saturating_sub(1) };
                let t0 = begin as f32 * dt;
                let t1 = end as f32 * dt;
                if t1 - t0 < settings.minimum_duration {
                    continue;
                }
                let onset = retarget::sample(source, target, clip, plan, t0);
                let global = target.globals(&onset);
                let mut anchor = placement.transform_point3(global[target_node].w_axis.truncate());
                if settings.project_ground {
                    anchor -= world_normal * (world_normal.dot(anchor) - data.target_height);
                }
                data.intervals.push(Interval {
                    source: node,
                    target: target_node,
                    start: t0,
                    end: t1,
                    source_point: p[begin].to_array(),
                    anchor: anchor.to_array(),
                });
            }
        }
    }
    data.intervals
        .sort_by(|a, b| a.start.total_cmp(&b.start).then(a.source.cmp(&b.source)));
    Ok(data)
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    checksum: String,
    payload: String,
}
pub fn cached(
    root: &Path,
    source: &Asset,
    target: &Asset,
    clip: &Clip,
    plan: &Plan,
    placement: Mat4,
) -> Result<Contacts> {
    let key = identity(clip, plan, placement);
    let path = root.join("derived/contacts").join(format!("{key}.json"));
    if let Ok(bytes) = std::fs::read(&path)
        && bytes.len() < 16 * 1024 * 1024
        && let Ok(env) = serde_json::from_slice::<Envelope>(&bytes)
        && blake3::hash(env.payload.as_bytes()).to_hex().as_str() == env.checksum
        && let Ok(c) = serde_json::from_str::<Contacts>(&env.payload)
        && c.key == key
        && c.version == VERSION
        && c.intervals.iter().all(|i| {
            i.source < source.nodes.len()
                && i.target < target.nodes.len()
                && i.start >= 0.
                && i.end <= clip.duration
                && i.end >= i.start
                && Vec3::from_array(i.anchor).is_finite()
        })
    {
        return Ok(c);
    }
    let c = compile(source, target, clip, plan, placement)?;
    let payload = serde_json::to_string(&c)?;
    let env = Envelope {
        checksum: blake3::hash(payload.as_bytes()).to_hex().to_string(),
        payload,
    };
    // A corrupt immutable cache is ignored and rebuilt in memory. Correspondence
    // profiles have a separate generation-based recovery contract.
    if path.exists() {
        return Ok(c);
    }
    crate::library::atomic_publish(&path, &serde_json::to_vec(&env)?)?;
    Ok(c)
}
