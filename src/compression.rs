//! Opt-in, sampled reconstruction research. Canonical GLBs are never modified.
use crate::{
    animation::{Clip, Cursor, Interpolation, Property, Track},
    scene::{Asset, Local},
};
use anyhow::{Result, bail, ensure};
use glam::{Quat, Vec3, Vec4};

pub const FORMAT_VERSION: u32 = 1;
const MAX_SAMPLES: usize = 1_000_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const SEGMENT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    SampledF32,
    ReducedF32,
    Quantized(u8),
}
#[derive(Clone, Debug)]
pub struct DerivedClip {
    pub source_hash: [u8; 32],
    pub duration: f32,
    pub sample_rate: u32,
    pub codec: Codec,
    pub tracks: Vec<DerivedTrack>,
}
#[derive(Clone, Debug)]
pub struct DerivedTrack {
    pub node: usize,
    pub property: Property,
    pub data: TrackData,
}
#[derive(Clone, Debug)]
pub enum TrackData {
    Default,
    Constant(Vec4),
    Animated { step: bool, segments: Vec<Segment> },
}
#[derive(Clone, Debug)]
pub struct Segment {
    pub times: Vec<f32>,
    pub values: Values,
}
#[derive(Clone, Debug)]
pub enum Values {
    Float(Vec<Vec4>),
    Vector {
        min: Vec3,
        max: Vec3,
        bits: u8,
        data: Vec<[u16; 3]>,
    },
    Rotation {
        bits: u8,
        data: Vec<(u8, [u16; 3])>,
    },
}

fn rest_value(asset: &Asset, node: usize, property: Property) -> Result<Vec4> {
    let Some(Local::Trs(t)) = asset.nodes.get(node).map(|n| n.rest) else {
        bail!("Derived track requires a TRS node");
    };
    Ok(match property {
        Property::Translation => t.translation.extend(0.),
        Property::Rotation => Vec4::from_array(t.rotation.to_array()),
        Property::Scale => t.scale.extend(0.),
        Property::Weights => bail!("Morph compression is unsupported"),
    })
}
fn distance(a: Vec4, b: Vec4, rotation: bool) -> f32 {
    if rotation {
        (a - b).length().min((a + b).length())
    } else {
        (a - b).length()
    }
}
fn interpolate(a: Vec4, b: Vec4, u: f32, rotation: bool) -> Vec4 {
    if rotation {
        Vec4::from_array(
            Quat::from_array(a.to_array())
                .slerp(Quat::from_array(b.to_array()), u)
                .normalize()
                .to_array(),
        )
    } else {
        a.lerp(b, u)
    }
}

/// Keys, uniform samples and adaptive cubic midpoint samples. This is not a
/// continuous-time bound. Validation uses a separate, denser reference grid.
fn track_times(track: &Track, duration: f32, rate: u32) -> Vec<f32> {
    if track.interpolation == Interpolation::Step {
        return track.times.clone();
    }
    let mut times = track.times.clone();
    let count = (duration * rate as f32).ceil() as usize;
    times.extend((0..=count).map(|i| (i as f32 / rate as f32).min(duration)));
    if track.interpolation == Interpolation::Cubic {
        fn subdivide(t: &Track, a: f32, b: f32, depth: u8, out: &mut Vec<f32>) {
            let mid = (a + b) * 0.5;
            let sample = |v| t.sample(v, &mut Cursor::default());
            let linear = interpolate(sample(a), sample(b), 0.5, t.property == Property::Rotation);
            // Always inspect two levels: a symmetric cubic may have zero midpoint error.
            if depth < 2
                || (depth < 8
                    && distance(sample(mid), linear, t.property == Property::Rotation) > 1e-4)
            {
                out.push(mid);
                subdivide(t, a, mid, depth + 1, out);
                subdivide(t, mid, b, depth + 1, out);
            }
        }
        for pair in track.times.windows(2) {
            subdivide(track, pair[0], pair[1], 0, &mut times);
        }
    }
    times.sort_by(f32::total_cmp);
    times.dedup();
    times
}

pub fn encode(
    asset: &Asset,
    clip: &Clip,
    source_hash: [u8; 32],
    rate: u32,
    codec: Codec,
) -> Result<DerivedClip> {
    ensure!(
        clip.supported && clip.duration.is_finite() && clip.duration >= 0.,
        "Unsupported clip"
    );
    ensure!(
        (1..=240).contains(&rate) && clip.duration * rate as f32 <= 100_000.,
        "Sampling budget exceeded"
    );
    if let Codec::Quantized(bits) = codec {
        ensure!((8..=16).contains(&bits), "Invalid quantization bits");
    }
    ensure!(clip.tracks.len() <= 16384, "Too many tracks");
    let mut tracks = Vec::new();
    let mut sample_count = 0;
    for track in &clip.tracks {
        ensure!(
            !track.times.is_empty()
                && track
                    .times
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0. && *v <= clip.duration)
                && track.times.windows(2).all(|p| p[0] < p[1]),
            "Invalid source times"
        );
        ensure!(
            track.values.len()
                == track.times.len()
                    * if track.interpolation == Interpolation::Cubic {
                        3
                    } else {
                        1
                    }
                && track.values.iter().all(|v| v.is_finite()),
            "Invalid source values"
        );
        let rest = rest_value(asset, track.node, track.property)?;
        let mut times = track_times(track, clip.duration, rate);
        sample_count += times.len();
        ensure!(
            sample_count <= MAX_SAMPLES,
            "Derived sample budget exceeded"
        );
        let mut values: Vec<_> = times
            .iter()
            .map(|&t| track.sample(t, &mut Cursor::default()))
            .collect();
        let rotation = track.property == Property::Rotation;
        // Quaternion interpolation/normalization can perturb identical keys by
        // an ulp. Classification is provisional until the final error pass.
        let constant = values
            .iter()
            .all(|&v| distance(values[0], v, rotation) <= 1e-7);
        let data = if constant && distance(values[0], rest, rotation) == 0. {
            TrackData::Default
        } else if constant {
            TrackData::Constant(values[0])
        } else {
            if codec == Codec::ReducedF32 && track.interpolation != Interpolation::Step {
                let mut keep = vec![false; times.len()];
                keep[0] = true;
                keep[times.len() - 1] = true;
                let mut work = vec![(0, times.len() - 1)];
                while let Some((a, b)) = work.pop() {
                    let worst = (a + 1..b)
                        .map(|i| {
                            (
                                i,
                                distance(
                                    values[i],
                                    interpolate(
                                        values[a],
                                        values[b],
                                        (times[i] - times[a]) / (times[b] - times[a]),
                                        rotation,
                                    ),
                                    rotation,
                                ),
                            )
                        })
                        .max_by(|a, b| a.1.total_cmp(&b.1));
                    if let Some((i, error)) = worst
                        && error > 1e-4
                    {
                        keep[i] = true;
                        work.push((a, i));
                        work.push((i, b));
                    }
                }
                times = times
                    .into_iter()
                    .zip(&keep)
                    .filter_map(|(t, &k)| k.then_some(t))
                    .collect();
                values = values
                    .into_iter()
                    .zip(&keep)
                    .filter_map(|(v, &k)| k.then_some(v))
                    .collect();
            }
            let mut segments = Vec::new();
            let mut start = 0;
            loop {
                let end = (start + SEGMENT).min(times.len());
                let slice = &values[start..end];
                let encoded = if let Codec::Quantized(bits) = codec {
                    if rotation {
                        Values::Rotation {
                            bits,
                            data: slice.iter().map(|v| pack_quat(*v, bits)).collect(),
                        }
                    } else {
                        let min = slice
                            .iter()
                            .fold(Vec3::splat(f32::INFINITY), |a, v| a.min(v.truncate()));
                        let max = slice
                            .iter()
                            .fold(Vec3::splat(f32::NEG_INFINITY), |a, v| a.max(v.truncate()));
                        Values::Vector {
                            min,
                            max,
                            bits,
                            data: slice
                                .iter()
                                .map(|v| {
                                    std::array::from_fn(|i| quantize(v[i], min[i], max[i], bits))
                                })
                                .collect(),
                        }
                    }
                } else {
                    Values::Float(slice.to_vec())
                };
                segments.push(Segment {
                    times: times[start..end].to_vec(),
                    values: encoded,
                });
                if end == times.len() {
                    break;
                }
                start = end - 1;
            }
            TrackData::Animated {
                step: track.interpolation == Interpolation::Step,
                segments,
            }
        };
        tracks.push(DerivedTrack {
            node: track.node,
            property: track.property,
            data,
        });
    }
    Ok(DerivedClip {
        source_hash,
        duration: clip.duration,
        sample_rate: rate,
        codec,
        tracks,
    })
}
fn quantize(v: f32, min: f32, max: f32, bits: u8) -> u16 {
    if max - min <= 1e-20 {
        0
    } else {
        (((v - min) / (max - min)).clamp(0., 1.) * ((1u32 << bits) - 1) as f32).round() as u16
    }
}
fn expand(v: u16, min: f32, max: f32, bits: u8) -> f32 {
    min + (max - min) * v as f32 / ((1u32 << bits) - 1) as f32
}
fn pack_quat(mut q: Vec4, bits: u8) -> (u8, [u16; 3]) {
    let omitted = (0..4)
        .max_by(|&i, &j| q[i].abs().total_cmp(&q[j].abs()))
        .unwrap();
    if q[omitted] < 0. {
        q = -q;
    }
    let mut components = [0; 3];
    let mut j = 0;
    for i in 0..4 {
        if i != omitted {
            components[j] = quantize(
                q[i],
                -std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
                bits,
            );
            j += 1;
        }
    }
    (omitted as u8, components)
}
impl Values {
    fn get(&self, i: usize) -> Vec4 {
        match self {
            Self::Float(v) => v[i],
            Self::Vector {
                min,
                max,
                bits,
                data,
            } => Vec3::from_array(std::array::from_fn(|j| {
                expand(data[i][j], min[j], max[j], *bits)
            }))
            .extend(0.),
            Self::Rotation { bits, data } => {
                let (omitted, encoded) = data[i];
                let mut q = Vec4::ZERO;
                let mut j = 0;
                for k in 0..4 {
                    if k != omitted as usize {
                        q[k] = expand(
                            encoded[j],
                            -std::f32::consts::FRAC_1_SQRT_2,
                            std::f32::consts::FRAC_1_SQRT_2,
                            *bits,
                        );
                        j += 1;
                    }
                }
                q[omitted as usize] = (1. - q.length_squared()).max(0.).sqrt();
                q.normalize()
            }
        }
    }
}
impl DerivedClip {
    pub fn evaluate(&self, asset: &Asset, time: f32) -> Vec<Local> {
        let mut pose = asset.rest_pose();
        let time = if time.is_finite() {
            time.clamp(0., self.duration)
        } else {
            0.
        };
        for track in &self.tracks {
            let v = match &track.data {
                TrackData::Default => continue,
                TrackData::Constant(v) => *v,
                TrackData::Animated { step, segments } => {
                    let s = &segments[segments
                        .partition_point(|s| s.times[0] <= time)
                        .saturating_sub(1)];
                    let i = s.times.partition_point(|&t| t <= time).saturating_sub(1);
                    if *step || i + 1 == s.times.len() || time <= s.times[0] {
                        s.values.get(i)
                    } else {
                        interpolate(
                            s.values.get(i),
                            s.values.get(i + 1),
                            (time - s.times[i]) / (s.times[i + 1] - s.times[i]),
                            track.property == Property::Rotation,
                        )
                    }
                }
            };
            if let Some(Local::Trs(t)) = pose.get_mut(track.node) {
                match track.property {
                    Property::Translation => t.translation = v.truncate(),
                    Property::Scale => t.scale = v.truncate(),
                    Property::Rotation => t.rotation = Quat::from_array(v.to_array()),
                    Property::Weights => (),
                }
            }
        }
        pose
    }
    /// Stable little-endian wire format. The identity includes every setting and
    /// the source GLB hash. Sub-16-bit candidates use u16 lanes (no fictitious bit packing).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.0.extend(b"ARMC");
        w.u32(FORMAT_VERSION);
        w.0.extend(self.source_hash);
        w.f32(self.duration);
        w.u32(self.sample_rate);
        w.u8(match self.codec {
            Codec::SampledF32 => 0,
            Codec::ReducedF32 => 1,
            Codec::Quantized(b) => b,
        });
        w.u32(self.tracks.len() as u32);
        for t in &self.tracks {
            w.u32(t.node as u32);
            w.u8(match t.property {
                Property::Translation => 0,
                Property::Rotation => 1,
                Property::Scale => 2,
                Property::Weights => 3,
            });
            match &t.data {
                TrackData::Default => w.u8(0),
                TrackData::Constant(v) => {
                    w.u8(1);
                    w.vec4(*v);
                }
                TrackData::Animated { step, segments } => {
                    w.u8(if *step { 3 } else { 2 });
                    w.u32(segments.len() as u32);
                    for s in segments {
                        w.u32(s.times.len() as u32);
                        for &v in &s.times {
                            w.f32(v);
                        }
                        match &s.values {
                            Values::Float(v) => {
                                w.u8(0);
                                for &v in v {
                                    w.vec4(v);
                                }
                            }
                            Values::Vector {
                                min,
                                max,
                                bits,
                                data,
                            } => {
                                w.u8(1);
                                w.u8(*bits);
                                for v in min.to_array().into_iter().chain(max.to_array()) {
                                    w.f32(v);
                                }
                                for row in data {
                                    for &v in row {
                                        w.u16(v);
                                    }
                                }
                            }
                            Values::Rotation { bits, data } => {
                                w.u8(2);
                                w.u8(*bits);
                                for (i, row) in data {
                                    w.u8(*i);
                                    for &v in row {
                                        w.u16(v);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        w.0
    }
    pub fn from_bytes(bytes: &[u8], asset: &Asset, source_hash: [u8; 32]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BYTES, "Derived clip exceeds byte budget");
        let mut r = Reader { bytes, offset: 0 };
        ensure!(
            r.take(4)? == b"ARMC" && r.u32()? == FORMAT_VERSION,
            "Unknown derived format/version"
        );
        ensure!(r.take(32)? == source_hash, "Stale derived source hash");
        let duration = r.f32()?;
        ensure!(duration >= 0., "Invalid duration");
        let sample_rate = r.u32()?;
        ensure!((1..=240).contains(&sample_rate), "Invalid sample rate");
        let codec = match r.u8()? {
            0 => Codec::SampledF32,
            1 => Codec::ReducedF32,
            b @ 8..=16 => Codec::Quantized(b),
            _ => bail!("Unknown codec"),
        };
        let count = r.u32()? as usize;
        ensure!(count <= 16384, "Track budget exceeded");
        let mut tracks = Vec::new();
        let mut samples = 0usize;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..count {
            let node = r.u32()? as usize;
            let property = match r.u8()? {
                0 => Property::Translation,
                1 => Property::Rotation,
                2 => Property::Scale,
                _ => bail!("Invalid property"),
            };
            rest_value(asset, node, property)?;
            ensure!(seen.insert((node, property)), "Duplicate track");
            let mode = r.u8()?;
            let data = match mode {
                0 => TrackData::Default,
                1 => {
                    let v = r.vec4()?;
                    ensure!(
                        property != Property::Rotation || (v.length_squared() - 1.).abs() < 1e-3,
                        "Invalid quaternion"
                    );
                    TrackData::Constant(v)
                }
                2 | 3 => {
                    let n = r.u32()? as usize;
                    ensure!((1..=MAX_SAMPLES).contains(&n), "Invalid segment count");
                    let mut segments: Vec<Segment> = Vec::new();
                    for _ in 0..n {
                        let k = r.u32()? as usize;
                        samples = samples
                            .checked_add(k)
                            .ok_or_else(|| anyhow::anyhow!("Sample overflow"))?;
                        ensure!(
                            (1..=SEGMENT).contains(&k) && samples <= MAX_SAMPLES,
                            "Sample budget exceeded"
                        );
                        let times = (0..k).map(|_| r.f32()).collect::<Result<Vec<_>>>()?;
                        ensure!(
                            times[0] >= 0.
                                && times[k - 1] <= duration
                                && times.windows(2).all(|p| p[0] < p[1]),
                            "Invalid derived times"
                        );
                        if let Some(previous) = segments.last() {
                            ensure!(
                                times[0] == *previous.times.last().unwrap(),
                                "Noncontiguous segments"
                            );
                        }
                        let mode = r.u8()?;
                        let values = if mode == 0 {
                            let values = (0..k).map(|_| r.vec4()).collect::<Result<Vec<_>>>()?;
                            ensure!(
                                property != Property::Rotation
                                    || values
                                        .iter()
                                        .all(|v| (v.length_squared() - 1.).abs() < 1e-3),
                                "Invalid quaternion"
                            );
                            Values::Float(values)
                        } else {
                            ensure!(mode == 1 || mode == 2, "Unknown value encoding");
                            let bits = r.u8()?;
                            ensure!((8..=16).contains(&bits), "Invalid bits");
                            let min = if mode == 1 {
                                r.vec4_three()?
                            } else {
                                Vec3::ZERO
                            };
                            let max = if mode == 1 {
                                r.vec4_three()?
                            } else {
                                Vec3::ZERO
                            };
                            ensure!(
                                min.cmple(max).all() && (max - min).is_finite(),
                                "Invalid quantization range"
                            );
                            ensure!(
                                (mode == 2) == (property == Property::Rotation),
                                "Encoding/property mismatch"
                            );
                            let mut vectors = Vec::new();
                            let mut rotations = Vec::new();
                            for _ in 0..k {
                                let i = if mode == 2 { r.u8()? } else { 0 };
                                ensure!(i < 4, "Invalid omitted index");
                                let row = [r.u16()?, r.u16()?, r.u16()?];
                                ensure!(
                                    row.iter().all(|&v| (v as u32) < (1u32 << bits)),
                                    "Quantized component out of range"
                                );
                                if mode == 1 {
                                    vectors.push(row);
                                } else {
                                    rotations.push((i, row));
                                }
                            }
                            if mode == 1 {
                                Values::Vector {
                                    min,
                                    max,
                                    bits,
                                    data: vectors,
                                }
                            } else {
                                Values::Rotation {
                                    bits,
                                    data: rotations,
                                }
                            }
                        };
                        segments.push(Segment { times, values });
                    }
                    TrackData::Animated {
                        step: mode == 3,
                        segments,
                    }
                }
                _ => bail!("Invalid track mode"),
            };
            tracks.push(DerivedTrack {
                node,
                property,
                data,
            });
        }
        ensure!(r.offset == bytes.len(), "Trailing derived data");
        Ok(Self {
            source_hash,
            duration,
            sample_rate,
            codec,
            tracks,
        })
    }
}
struct Writer(Vec<u8>);
impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v)
    }
    fn u16(&mut self, v: u16) {
        self.0.extend(v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes())
    }
    fn f32(&mut self, v: f32) {
        self.0.extend(v.to_le_bytes())
    }
    fn vec4(&mut self, v: Vec4) {
        for v in v.to_array() {
            self.f32(v)
        }
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .offset
            .checked_add(n)
            .ok_or_else(|| anyhow::anyhow!("Offset overflow"))?;
        ensure!(end <= self.bytes.len(), "Truncated derived clip");
        let v = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn f32(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        ensure!(v.is_finite(), "Nonfinite derived value");
        Ok(v)
    }
    fn vec4(&mut self) -> Result<Vec4> {
        Ok(Vec4::new(
            self.f32()?,
            self.f32()?,
            self.f32()?,
            self.f32()?,
        ))
    }
    fn vec4_three(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
}
