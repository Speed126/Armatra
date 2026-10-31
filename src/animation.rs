use crate::scene::{Asset, Local};
use glam::{Quat, Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Property {
    Translation,
    Rotation,
    Scale,
    Weights,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interpolation {
    Step,
    Linear,
    Cubic,
}
#[derive(Clone, Debug)]
pub struct Track {
    pub node: usize,
    pub property: Property,
    pub interpolation: Interpolation,
    pub times: Vec<f32>,
    pub values: Vec<Vec4>,
}
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub tracks: Vec<Track>,
    pub duration: f32,
    pub supported: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Cursor {
    pub left: usize,
    pub previous: Option<f32>,
}
impl Track {
    pub fn sample(&self, time: f32, cursor: &mut Cursor) -> Vec4 {
        let n = self.times.len();
        let time = if time.is_finite() { time } else { 0. };
        if cursor
            .previous
            .is_some_and(|p| time >= p && time - p < 0.25)
        {
            while cursor.left + 1 < n && self.times[cursor.left + 1] <= time {
                cursor.left += 1;
            }
        } else {
            cursor.left = self.times.partition_point(|&v| v <= time).saturating_sub(1);
        }
        cursor.previous = Some(time);
        let i = cursor.left.min(n - 1);
        let value = |k: usize| {
            self.values[if self.interpolation == Interpolation::Cubic {
                k * 3 + 1
            } else {
                k
            }]
        };
        let mut v = if time <= self.times[0] {
            value(0)
        } else if i + 1 == n || self.interpolation == Interpolation::Step {
            value(i)
        } else {
            let dt = self.times[i + 1] - self.times[i];
            let u = (time - self.times[i]) / dt;
            if self.interpolation == Interpolation::Cubic {
                let u2 = u * u;
                let u3 = u2 * u;
                value(i) * (2. * u3 - 3. * u2 + 1.)
                    + self.values[i * 3 + 2] * (dt * (u3 - 2. * u2 + u))
                    + value(i + 1) * (-2. * u3 + 3. * u2)
                    + self.values[(i + 1) * 3] * (dt * (u3 - u2))
            } else if self.property == Property::Rotation {
                Vec4::from_array(
                    Quat::from_array(value(i).to_array())
                        .slerp(Quat::from_array(value(i + 1).to_array()), u)
                        .normalize()
                        .to_array(),
                )
            } else {
                value(i).lerp(value(i + 1), u)
            }
        };
        if self.property == Property::Rotation {
            v = if v.length_squared() > 1e-12 {
                v.normalize()
            } else {
                Vec4::W
            };
        }
        v
    }
}
pub fn evaluate(
    target: &Asset,
    clip: &Clip,
    mapping: &[Option<usize>],
    time: f32,
    cursors: &mut Vec<Cursor>,
) -> Vec<Local> {
    let mut pose = target.rest_pose();
    cursors.resize_with(clip.tracks.len(), Cursor::default);
    for (track, cursor) in clip.tracks.iter().zip(cursors) {
        let Some(Some(node)) = mapping.get(track.node) else {
            continue;
        };
        let Some(Local::Trs(trs)) = pose.get_mut(*node) else {
            continue;
        };
        let v = track.sample(time, cursor);
        match track.property {
            Property::Translation => trs.translation = Vec3::new(v.x, v.y, v.z),
            Property::Scale => trs.scale = Vec3::new(v.x, v.y, v.z),
            Property::Rotation => trs.rotation = Quat::from_array(v.to_array()),
            Property::Weights => (),
        }
    }
    pose
}
#[derive(Clone, Copy, Debug)]
pub struct RootTravel {
    pub node: usize,
    pub displacement: Vec3,
}
/// Cache once per clip/mapping. Only a unique highest animated translation root
/// may drive continuous travel; unrelated independently animated nodes do not.
pub fn root_travel(target: &Asset, clip: &Clip, mapping: &[Option<usize>]) -> Option<RootTravel> {
    let mut roots: Vec<_> = clip
        .tracks
        .iter()
        .filter(|t| t.property == Property::Translation)
        .filter_map(|t| mapping.get(t.node).copied().flatten())
        .collect();
    roots.sort_unstable();
    roots.dedup();
    let highest: Vec<_> = roots
        .iter()
        .copied()
        .filter(|node| {
            let mut parent = target.nodes[*node].parent;
            while let Some(p) = parent {
                if roots.contains(&p) {
                    return false;
                }
                parent = target.nodes[p].parent;
            }
            true
        })
        .collect();
    if highest.len() != 1 {
        return None;
    }
    let node = highest[0];
    let a = target.globals(&evaluate(target, clip, mapping, 0., &mut vec![]));
    let b = target.globals(&evaluate(target, clip, mapping, clip.duration, &mut vec![]));
    let displacement = b[node].w_axis.truncate() - a[node].w_axis.truncate();
    displacement
        .is_finite()
        .then_some(RootTravel { node, displacement })
}
pub fn place_cycle(target: &Asset, pose: &mut [Local], travel: Option<RootTravel>, cycle: i64) {
    let Some(travel) = travel else {
        return;
    };
    if cycle == 0 {
        return;
    }
    let global = target.globals(pose);
    let parent = target.nodes[travel.node]
        .parent
        .map_or(glam::Mat4::IDENTITY, |p| global[p]);
    let offset = parent
        .inverse()
        .transform_vector3(travel.displacement * cycle as f32);
    if offset.is_finite()
        && let Local::Trs(root) = &mut pose[travel.node]
    {
        root.translation += offset;
    }
}
#[derive(Clone, Debug)]
pub struct Playback {
    pub cycle: i64,
    pub time: f32,
    pub speed: f32,
    pub playing: bool,
    pub looping: bool,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            cycle: 0,
            time: 0.,
            speed: 1.,
            playing: false,
            looping: true,
        }
    }
}
impl Playback {
    pub fn tick(&mut self, seconds: f32, duration: f32) {
        if !self.playing || duration <= 0. {
            return;
        }
        self.time += seconds.max(0.) * self.speed;
        if self.looping {
            self.cycle = self
                .cycle
                .saturating_add((self.time / duration).floor() as i64);
            self.time = self.time.rem_euclid(duration);
        } else {
            if self.time >= duration || self.time < 0. {
                self.playing = false;
            }
            self.time = self.time.clamp(0., duration);
        }
    }
}
