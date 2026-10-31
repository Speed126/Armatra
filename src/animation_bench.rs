//! Small reproducible experiments; all timings are measured, never estimates.
use crate::{
    animation,
    compression::{self, Codec, DerivedClip, TrackData},
    import::{self, Limits},
    scene::Asset,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::{hint::black_box, path::Path, time::Instant};

pub fn environment() -> Value {
    json!({"os":std::env::var("ARMATRA_OS").unwrap_or_else(|_|std::env::consts::OS.into()),"cpu":std::env::var("ARMATRA_CPU").unwrap_or_else(|_|"unreported".into()),"version":crate::VERSION,"profile":if cfg!(debug_assertions){"debug"}else{"release"}})
}
pub fn save(name: &str, value: &Value) -> Result<()> {
    std::fs::create_dir_all("target/reports/animation")?;
    std::fs::write(
        format!("target/reports/animation/{name}.json"),
        serde_json::to_vec_pretty(value)?,
    )?;
    Ok(())
}
pub fn corpus() -> Result<Vec<(String, Vec<u8>, Asset)>> {
    let mut sources = vec![
        (
            "procedural-walk".into(),
            crate::rig_corpus::humanoid(Default::default(), true)
                .0
                .finish(),
        ),
        (
            "procedural-wave".into(),
            crate::fixtures::robot(false, false).finish(),
        ),
    ];
    let fixture = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target".into())
        .join("external-fixtures/Fox.glb");
    let fox = fixture.as_path();
    if fox.exists() {
        sources.push(("Khronos-Fox".into(), std::fs::read(fox)?));
    }
    sources
        .into_iter()
        .map(|(name, bytes)| {
            let asset = import::from_bytes(&bytes, &Limits::default())?;
            Ok((name, bytes, asset))
        })
        .collect()
}
pub fn compression_benchmark() -> Result<()> {
    let mut rows = Vec::new();
    for (name, source, asset) in corpus()? {
        for clip in &asset.clips {
            let mapping: Vec<_> = (0..asset.nodes.len()).map(Some).collect();
            let raw_bytes: usize = clip
                .tracks
                .iter()
                .map(|t| t.times.len() * 4 + t.values.len() * 16)
                .sum();
            for codec in [
                Codec::SampledF32,
                Codec::ReducedF32,
                Codec::Quantized(10),
                Codec::Quantized(16),
            ] {
                let begin = Instant::now();
                let derived = compression::encode(
                    &asset,
                    clip,
                    *blake3::hash(&source).as_bytes(),
                    60,
                    codec,
                )?;
                let bytes = derived.to_bytes();
                let encode_us = begin.elapsed().as_secs_f64() * 1e6;
                let begin = Instant::now();
                let decoded =
                    DerivedClip::from_bytes(&bytes, &asset, *blake3::hash(&source).as_bytes())?;
                let deserialize_us = begin.elapsed().as_secs_f64() * 1e6;
                let mut max_local_matrix_error = 0f32;
                for i in 0..401 {
                    let time = clip.duration * ((i * 197) % 401) as f32 / 400.;
                    let raw = animation::evaluate(&asset, clip, &mapping, time, &mut vec![]);
                    let pose = decoded.evaluate(&asset, time);
                    for (a, b) in raw.iter().zip(&pose) {
                        for (a, b) in a
                            .matrix()
                            .to_cols_array()
                            .into_iter()
                            .zip(b.matrix().to_cols_array())
                        {
                            max_local_matrix_error = max_local_matrix_error.max((a - b).abs());
                        }
                    }
                }
                let begin = Instant::now();
                for i in 0..5000 {
                    black_box(decoded.evaluate(&asset, clip.duration * (i % 401) as f32 / 400.));
                }
                let sequential_us = begin.elapsed().as_secs_f64() * 1e6 / 5000.;
                let begin = Instant::now();
                for i in 0..5000 {
                    black_box(
                        decoded.evaluate(&asset, clip.duration * ((i * 197) % 401) as f32 / 400.),
                    );
                }
                let random_seek_us = begin.elapsed().as_secs_f64() * 1e6 / 5000.;
                let mut cursors = vec![];
                let begin = Instant::now();
                for i in 0..5000 {
                    black_box(animation::evaluate(
                        &asset,
                        clip,
                        &mapping,
                        clip.duration * ((i * 197) % 401) as f32 / 400.,
                        &mut cursors,
                    ));
                }
                let raw_seek_us = begin.elapsed().as_secs_f64() * 1e6 / 5000.;
                rows.push(json!({"asset":name,"clip":clip.name,"source_blake3":blake3::hash(&source).to_hex().to_string(),"codec":format!("{codec:?}"),"raw_curve_bytes":raw_bytes,"derived_bytes":bytes.len(),"encode_us":encode_us,"deserialize_us":deserialize_us,"sequential_pose_us":sequential_us,"random_seek_pose_us":random_seek_us,"raw_random_seek_pose_us":raw_seek_us,"max_local_matrix_component_error":max_local_matrix_error,"default_tracks":derived.tracks.iter().filter(|t|matches!(t.data,TrackData::Default)).count(),"constant_tracks":derived.tracks.iter().filter(|t|matches!(t.data,TrackData::Constant(_))).count()}));
            }
        }
    }
    save(
        "compression-comparison",
        &json!({"environment":environment(),"method":"60 Hz plus source keys and adaptive cubic samples; 401 permuted seek comparisons; 5000 pose evaluations per timing; raw bytes count decoded times/Vec4 curves, derived bytes count complete wire format. Quantized lanes are u16 even for 10-bit experiments.","rows":rows}),
    )?;
    println!("Compression comparison: {} measured rows", rows.len());
    Ok(())
}

pub fn deformation_benchmark() -> Result<()> {
    use crate::compression_validation::{select, validate};
    let mut rows = Vec::new();
    for (name, source, asset) in corpus()? {
        for clip in &asset.clips {
            let hash = *blake3::hash(&source).as_bytes();
            let globals = asset.globals(&asset.rest_pose());
            let min = globals
                .iter()
                .fold(glam::Vec3::splat(f32::INFINITY), |a, g| {
                    a.min(g.w_axis.truncate())
                });
            let max = globals
                .iter()
                .fold(glam::Vec3::splat(f32::NEG_INFINITY), |a, g| {
                    a.max(g.w_axis.truncate())
                });
            let budget = (max - min).length().max(1.) * 0.001;
            for codec in [
                Codec::SampledF32,
                Codec::ReducedF32,
                Codec::Quantized(10),
                Codec::Quantized(16),
            ] {
                let start = Instant::now();
                let data = compression::encode(&asset, clip, hash, 60, codec)?;
                let data = DerivedClip::from_bytes(&data.to_bytes(), &asset, hash)?;
                let encode_us = start.elapsed().as_secs_f64() * 1e6;
                let start = Instant::now();
                let error = validate(&asset, clip, &data, budget)?;
                let validation_ms = start.elapsed().as_secs_f64() * 1e3;
                rows.push(json!({"asset":name,"clip":clip.name,"codec":format!("{codec:?}"),"bytes":data.to_bytes().len(),"encode_us":encode_us,"validation_ms":validation_ms,"sampled_error":error}));
            }
            let start = Instant::now();
            let best = select(&asset, clip, hash, budget)?;
            rows.push(json!({"asset":name,"clip":clip.name,"selection":best.as_ref().map(|(c,_)|format!("{:?}",c.codec)),"final_bytes":best.as_ref().map(|(c,_)|c.to_bytes().len()),"final_validation":best.as_ref().map(|(_,e)|e),"allocation_ms":start.elapsed().as_secs_f64()*1e3,"promoted":false}));
            export_acl(&name, &asset, clip)?;
        }
    }
    save(
        "deformation",
        &json!({"environment":environment(),"method":"Independent 120Hz grid plus keys, eighth interiors and adjacent representable STEP boundary times. Mesh-derived 95th percentile joint radii, seven probes/node, leaf origins and up to 512 stratified vertices/primitive; budget=.001*max(1,rest-node AABB diagonal). Final complete wire-format allocation revalidated. Sampled bound only.","decision":"Offline comparison only; exact source curves remain default. No continuous-time guarantee.","rows":rows}),
    )?;
    println!("Deformation validation: {} rows", rows.len());
    Ok(())
}

fn export_acl(name: &str, asset: &Asset, clip: &animation::Clip) -> Result<()> {
    use crate::{compression_validation, scene::Local};
    let stem = format!("{}-{}", name, clip.name.replace([' ', '/', '\\'], "_"));
    let dir = Path::new("target/acl-input");
    std::fs::create_dir_all(dir)?;
    let count = (clip.duration * 60.).ceil() as u32 + 1;
    let rate = if clip.duration > 0. {
        (count - 1) as f32 / clip.duration
    } else {
        60.
    };
    let mut bytes = Vec::new();
    let mut u32_write = |v: u32| bytes.extend(v.to_le_bytes());
    u32_write(asset.nodes.len() as u32);
    u32_write(count);
    bytes.extend(rate.to_le_bytes());
    for n in &asset.nodes {
        bytes.extend(n.parent.map_or(u32::MAX, |p| p as u32).to_le_bytes());
    }
    for r in compression_validation::radii(asset) {
        bytes.extend(r.to_le_bytes());
    }
    let mapping: Vec<_> = (0..asset.nodes.len()).map(Some).collect();
    for i in 0..count {
        let pose = animation::evaluate(asset, clip, &mapping, i as f32 / rate, &mut vec![]);
        for local in pose {
            let (s, r, t) = match local {
                Local::Trs(t) => (t.scale, t.rotation, t.translation),
                Local::Matrix(m) => m.to_scale_rotation_translation(),
            };
            for v in r
                .to_array()
                .into_iter()
                .chain(t.to_array())
                .chain(s.to_array())
            {
                bytes.extend(v.to_le_bytes());
            }
        }
    }
    let times = compression_validation::sample_times(clip);
    bytes.extend((times.len() as u32).to_le_bytes());
    for t in times {
        bytes.extend(t.to_le_bytes());
    }
    std::fs::write(dir.join(format!("{stem}.qvv")), bytes)?;
    Ok(())
}

pub fn acl_validation() -> Result<()> {
    use crate::{
        compression_validation,
        scene::{Local, Trs},
    };
    let mut rows = Vec::new();
    for (name, _, asset) in corpus()? {
        for clip in &asset.clips {
            let stem = format!("{}-{}", name, clip.name.replace([' ', '/', '\\'], "_"));
            let bytes = std::fs::read(format!("target/acl-input/{stem}.qvv.decoded"))?;
            anyhow::ensure!(bytes.len() >= 8, "Truncated ACL output");
            let nodes = u32::from_le_bytes(bytes[..4].try_into()?) as usize;
            let queries = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
            anyhow::ensure!(
                nodes == asset.nodes.len()
                    && queries == compression_validation::sample_times(clip).len()
                    && bytes.len() == 8 + queries * nodes * 40,
                "ACL output dimensions"
            );
            let mut poses = Vec::new();
            for row in bytes[8..].chunks_exact(nodes * 40) {
                let mut pose = Vec::new();
                for v in row.as_chunks::<40>().0 {
                    let f: Vec<_> = v
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|v| f32::from_le_bytes(*v))
                        .collect();
                    pose.push(Local::Trs(Trs {
                        rotation: glam::Quat::from_xyzw(f[0], f[1], f[2], f[3]),
                        translation: glam::Vec3::new(f[4], f[5], f[6]),
                        scale: glam::Vec3::new(f[7], f[8], f[9]),
                    }));
                }
                poses.push(pose);
            }
            let g = asset.globals(&asset.rest_pose());
            let min = g.iter().fold(glam::Vec3::splat(f32::INFINITY), |a, g| {
                a.min(g.w_axis.truncate())
            });
            let max = g.iter().fold(glam::Vec3::splat(f32::NEG_INFINITY), |a, g| {
                a.max(g.w_axis.truncate())
            });
            let report = compression_validation::validate_with(
                &asset,
                clip,
                (max - min).length().max(1.) * 0.001,
                |i, _| poses[i].clone(),
            )?;
            rows.push(json!({"asset":name,"clip":clip.name,"acl_sampled_error":report}));
        }
    }
    save(
        "acl-validation",
        &json!({"environment":environment(),"method":"ACL decoded poses compared to original glTF evaluator at exactly the custom codec validation times; includes sampling loss before ACL encoding.","rows":rows}),
    )?;
    Ok(())
}

pub fn motion_benchmark() -> Result<()> {
    use crate::motion_search;
    let labels: Value = serde_json::from_str(include_str!("../tests/corpus/motion-labels.json"))?;
    let family = |name: &str| {
        labels["labels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == name)
            .unwrap()["family"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let mut indexed = Vec::new();
    let mut preprocess = Vec::new();
    std::fs::create_dir_all("tests/fixtures/motions")?;
    std::fs::create_dir_all("target/reports/animation/medoids")?;
    let mut atlas = image::RgbaImage::new(128 * 9, 128);
    for (i, (name, b)) in crate::motion_fixtures::motions().into_iter().enumerate() {
        let bytes = b.finish();
        std::fs::write(format!("tests/fixtures/motions/{name}.glb"), &bytes)?;
        let asset = import::from_bytes(&bytes, &Limits::default())?;
        let start = Instant::now();
        let summary = motion_search::summarize(&asset, &asset.clips[0])?;
        preprocess.push(json!({"clip":name,"summary_ms":start.elapsed().as_secs_f64()*1e3,"objective":summary.objective,"medoid_times":summary.medoids.iter().map(|m|m.time).collect::<Vec<_>>(),"weights":summary.medoids.iter().map(|m|m.weight).collect::<Vec<_>>()}));
        let preview = motion_search::preview(&asset, &asset.clips[0], summary.medoids[0].time);
        preview.save(format!("target/reports/animation/medoids/{name}.png"))?;
        image::imageops::replace(&mut atlas, &preview, i as i64 * 128, 0);
        indexed.push((name, summary));
    }
    atlas.save("target/reports/animation/motion-medoids.png")?;
    let mut queries = Vec::new();
    let mut top1 = 0;
    let mut precision = 0.;
    for (i, (name, summary)) in indexed.iter().enumerate() {
        let start = Instant::now();
        let mut ranking: Vec<_> = indexed
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, (n, s))| Ok((n.clone(), motion_search::similarity(summary, s)?)))
            .collect::<Result<_>>()?;
        ranking.sort_by(|a, b| a.1.total_cmp(&b.1));
        let query_us = start.elapsed().as_secs_f64() * 1e6;
        let correct = family(name) == family(&ranking[0].0);
        top1 += usize::from(correct);
        let relevant = ranking
            .iter()
            .take(3)
            .filter(|(n, _)| family(n) == family(name))
            .count() as f64
            / 3.;
        precision += relevant;
        queries.push(json!({"query":name,"family":family(name),"top1_correct":correct,"precision_at_3":relevant,"query_us":query_us,"ranking":ranking}));
    }
    save(
        "motion-search",
        &json!({"environment":environment(),"annotation":labels,"method":"48 phase samples; seven semantic major roles, root-facing/height normalization, relative joint velocities, root velocity, facing and heuristic foot contact. Bounded deterministic PAM k=4; balanced min-cost residual transport plus root-speed difference. Self excluded.","top1_accuracy":top1 as f64/indexed.len() as f64,"mean_precision_at_3":precision/indexed.len() as f64,"preprocess":preprocess,"queries":queries,"failures":["Pose distributions discard long-range temporal ordering.","Symmetric/ambiguous anatomy and unknown humanoid frames are skipped, including quadrupeds.","Small generated families and author labels do not establish perceptual quality on real motion libraries.","Absolute root-facing and tempo can change similarity despite a similar action."]}),
    )?;
    println!(
        "Motion retrieval: {top1}/{} top-1 author-family matches",
        indexed.len()
    );
    Ok(())
}
