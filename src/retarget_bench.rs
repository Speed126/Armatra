//! Reproducible CPU mapping measurements, with annotated answers independent of
//! the algorithms. Accounted problem storage is not a process RSS measurement.
use crate::{
    assignment,
    rig::{Anatomy, Rig},
    rig_corpus,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::time::Instant;

pub fn mapping_report() -> Result<Value> {
    let mut rows = Vec::new();
    for (case, s, t) in rig_corpus::cases() {
        let sr = Rig::analyze(&s);
        let tr = Rig::analyze(&t);
        for stage in ["names-only", "weighted", "hierarchy-chain"] {
            let begin = Instant::now();
            let mut mapping = Vec::new();
            for _ in 0..10 {
                mapping = if stage == "names-only" {
                    assignment::greedy_names(&sr, &tr)
                } else if stage == "weighted" {
                    assignment::weighted(&sr, &tr)?.targets
                } else {
                    crate::correspondence::refine(&sr, &tr)?.assignment.targets
                };
            }
            let us = begin.elapsed().as_secs_f64() * 1e6 / 10.;
            let (mut tp, mut fp, mut fn_, mut unmatched_correct, mut edits) = (0, 0, 0, 0, 0);
            for (i, j) in mapping.iter().enumerate() {
                let expected = case.expected[sr.joints[i].node];
                let actual = j.map(|j| tr.joints[j].node);
                edits += usize::from(expected != actual);
                if expected.is_none() && actual.is_none() {
                    unmatched_correct += 1;
                }
                if matches!(
                    sr.joints[i].anatomy,
                    Anatomy::Unknown
                        | Anatomy::Helper
                        | Anatomy::Finger
                        | Anatomy::Toe
                        | Anatomy::Root
                ) {
                    continue;
                }
                if actual.is_some() {
                    if actual == expected {
                        tp += 1;
                    } else {
                        fp += 1;
                    }
                }
                if expected.is_some() && actual != expected {
                    fn_ += 1;
                }
            }
            let precision = tp as f64 / (tp + fp).max(1) as f64;
            let recall = tp as f64 / (tp + fn_).max(1) as f64;
            rows.push(json!({"case":case.name,"stage":stage,"hierarchy_violations":crate::correspondence::violations(&sr,&tr,&mapping),"major_tp":tp,"major_fp":fp,"major_fn":fn_,"precision":precision,"recall":recall,"f1":if precision+recall>0. {2.*precision*recall/(precision+recall)} else {0.},"unmatched_correct":unmatched_correct,"manual_edits":edits,"mean_us":us,"accounted_problem_bytes":sr.joints.len()*tr.joints.len()*size_of::<Option<f64>>()+sr.joints.len()*size_of::<f64>()}));
        }
    }
    Ok(
        json!({"corpus":"tests/corpus/rig-manifest.json","cpu":std::env::var("ARMATRA_CPU").unwrap_or_else(|_|"unspecified".into()),"os":std::env::consts::OS,"build":if cfg!(debug_assertions) {"debug"} else {"release"},"iterations":10,"rows":rows}),
    )
}

pub fn seed_library(
    root: &std::path::Path,
    source_path: &std::path::Path,
    target_path: &std::path::Path,
) -> Result<()> {
    use crate::{
        import::{self, Limits},
        library::Library,
        profile, retarget, session,
    };
    let sb = import::read_file(source_path, &Limits::default())?;
    let tb = import::read_file(target_path, &Limits::default())?;
    let source = import::from_bytes(&sb, &Limits::default())?;
    let target = import::from_bytes(&tb, &Limits::default())?;
    anyhow::ensure!(!source.clips.is_empty(), "Demo source needs a clip");
    let mut library = Library::open(root.to_path_buf())?;
    let sh = library.import(&sb, "Walk source", &source)?;
    let th = library.import(&tb, "Target rig", &target)?;
    let p = {
        retarget::compile(
            &source,
            &target,
            &retarget::Settings::default(),
            &[retarget::Override {
                source: 7,
                target: Some(7),
            }],
        )?
    };
    profile::save(root, &p, &source, &target)?;
    session::save(&library, &th, &sh, 0)?;
    Ok(())
}
pub fn run() -> Result<()> {
    std::fs::create_dir_all("target/reports")?;
    let report = mapping_report()?;
    std::fs::write(
        "target/reports/retarget-mapping.json",
        serde_json::to_vec_pretty(&report)?,
    )?;
    let mut contacts = Vec::new();
    for (case, s, t) in rig_corpus::cases()
        .into_iter()
        .filter(|x| ["proportions", "combined"].contains(&x.0.name.as_str()))
    {
        let p = crate::retarget::compile(&s, &t, &crate::retarget::Settings::default(), &[])?;
        let c = crate::contacts::compile(&s, &t, &s.clips[0], &p, glam::Mat4::IDENTITY)?;
        contacts.push(json!({"case":case.name,"contacts":c}));
    }
    std::fs::write(
        "target/reports/retarget-contacts.json",
        serde_json::to_vec_pretty(&contacts)?,
    )?;
    std::fs::write(
        "target/reports/retarget-motion.json",
        serde_json::to_vec_pretty(&motion_report()?)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub fn motion_report() -> Result<Value> {
    use crate::{
        contacts, ik,
        retarget::{self, RootPolicy, Settings},
        scene::Local,
    };
    use glam::{Mat4, Vec3};
    let mut rows = Vec::new();
    for (case, s, t) in rig_corpus::cases()
        .into_iter()
        .filter(|x| ["proportions", "combined"].contains(&x.0.name.as_str()))
    {
        let plan = retarget::compile(&s, &t, &Settings::default(), &[])?;
        let begin = Instant::now();
        let c = contacts::compile(&s, &t, &s.clips[0], &plan, Mat4::IDENTITY)?;
        let preprocessing_us = begin.elapsed().as_secs_f64() * 1e6;
        for stage in [
            "rest-fk-unscaled",
            "root-scaled",
            "detected-contacts",
            "pelvis",
            "two-bone-ik",
        ] {
            let mut p = plan.clone();
            if stage == "rest-fk-unscaled" {
                p.settings.root = RootPolicy::Unscaled;
            }
            let (
                mut core_slide,
                mut full_slide,
                mut penetration,
                mut trajectory,
                mut jump,
                mut max_angle,
                mut outside_angle,
                mut seek_error,
            ) = (0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
            let mut previous = std::collections::HashMap::new();
            let mut previous_core = std::collections::HashMap::new();
            let mut previous_correction = std::collections::HashMap::new();
            let mut count = 0;
            let mut core_count = 0;
            let begin = Instant::now();
            for k in 0usize..=480 {
                let time = k as f64 / 240.;
                let raw = c.raw_pose(&s, &t, &s.clips[0], &p, time, false);
                let pose = match stage {
                    "pelvis" => ik::correct(&s, &t, &p, &c, raw.clone(), time, false, false).pose,
                    "two-bone-ik" => ik::sample(&s, &t, &s.clips[0], &p, &c, time, false).pose,
                    _ => raw.clone(),
                };
                let rg = t.globals(&raw);
                let g = t.globals(&pose);
                let active = c.active(time, false);
                for foot in [16, 20] {
                    let expected = plan
                        .rows
                        .iter()
                        .find(|r| r.source == foot)
                        .and_then(|r| r.target)
                        .unwrap();
                    let point = g[expected].w_axis.truncate();
                    let raw_point = rg[expected].w_axis.truncate();
                    penetration = penetration.max(
                        (c.target_height - Vec3::from_array(c.target_normal).dot(point)).max(0.)
                            / plan.target_height,
                    );
                    let correction = point - raw_point;
                    if let Some(prior) = previous_correction.insert(expected, correction) {
                        jump = jump.max(correction.distance(prior) / plan.target_height);
                    }
                }
                for (raw, pose) in raw.iter().zip(&pose) {
                    if let (Local::Trs(a), Local::Trs(b)) = (raw, pose) {
                        let angle = if a.rotation.abs_diff_eq(b.rotation, 1e-6)
                            || a.rotation.abs_diff_eq(-b.rotation, 1e-6)
                        {
                            0.
                        } else {
                            2. * a
                                .rotation
                                .normalize()
                                .dot(b.rotation.normalize())
                                .abs()
                                .clamp(0., 1.)
                                .acos()
                        };
                        max_angle = max_angle.max(angle);
                        if active.iter().all(|c| c.weight < 1e-6) {
                            outside_angle = outside_angle.max(angle);
                        }
                    }
                }
                for contact in active {
                    let point = g[contact.target].w_axis.truncate();
                    let id = contact.interval;
                    if let Some(before) = previous.insert(id, point) {
                        full_slide += point.distance(before) / plan.target_height;
                    }
                    if contact.weight > 0.999 {
                        if let Some(before) = previous_core.insert(id, point) {
                            core_slide += point.distance(before) / plan.target_height;
                        }
                        core_count += 1;
                        trajectory += point.distance(contact.anchor) / plan.target_height;
                    }
                    count += 1;
                }
                if stage == "two-bone-ik" && k.is_multiple_of(17) {
                    let _ = ik::sample(&s, &t, &s.clips[0], &p, &c, 1.8 - time, false);
                    let direct = ik::sample(&s, &t, &s.clips[0], &p, &c, time, false);
                    for (a, b) in pose.iter().zip(direct.pose) {
                        for (x, y) in a
                            .matrix()
                            .to_cols_array()
                            .into_iter()
                            .zip(b.matrix().to_cols_array())
                        {
                            seek_error = seek_error.max((x - y).abs());
                        }
                    }
                }
            }
            let mean_us = begin.elapsed().as_secs_f64() * 1e6 / 481.;
            rows.push(json!({"case":case.name,"stage":stage,"preprocessing_us":preprocessing_us,"evaluation_and_metrics_mean_us":mean_us,"full_contact_slide_heights":full_slide,"core_contact_slide_heights":core_slide,"core_contact_samples":core_count,"contact_samples":count,"max_foot_joint_penetration_heights":penetration,"core_mean_anchor_error_heights":trajectory/core_count.max(1) as f32,"max_correction_step_heights_240hz":jump,"max_joint_deviation_radians":max_angle,"outside_contact_max_joint_deviation_radians":outside_angle,"random_seek_max_matrix_error":seek_error}));
        }
    }
    Ok(
        json!({"cpu":std::env::var("ARMATRA_CPU").unwrap_or_else(|_|"unspecified".into()),"os":std::env::consts::OS,"build":if cfg!(debug_assertions) {"debug"} else {"release"},"sample_rate":240,"core_contact":"weight > 0.999; excludes smooth entry/exit windows","ground":"Rest foot-joint plane; penetration is ankle-reference, not mesh soles","rows":rows}),
    )
}
pub fn preview() -> Result<()> {
    use crate::{
        import::{self, Limits},
        render::{Camera, Gpu, Renderer, Target},
        retarget,
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (gpu, _) = pollster::block_on(Gpu::new(&instance, None))?;
    let renderer = Renderer::new(&gpu);
    let load = |name: &str| -> Result<_> {
        Ok(import::from_bytes(
            &std::fs::read(format!("tests/fixtures/retarget/{name}.glb"))?,
            &Limits::default(),
        )?)
    };
    let source = load("Walk-source")?;
    let targets = [load("Tall-target")?, load("Rest-frame-target")?];
    let mut atlas = image::RgbaImage::new(1800, 700);
    for (i, asset) in std::iter::once(&source).chain(targets.iter()).enumerate() {
        let pose = if i == 0 {
            crate::animation::evaluate(
                asset,
                &source.clips[0],
                &(0..source.nodes.len()).map(Some).collect::<Vec<_>>(),
                0.55,
                &mut Vec::new(),
            )
        } else {
            let plan = retarget::compile(&source, asset, &retarget::Settings::default(), &[])?;
            retarget::sample(&source, asset, &source.clips[0], &plan, 0.55)
        };
        let resident = renderer.upload(&gpu, asset)?;
        let target = Target::new(&gpu.device, [600, 700]);
        let mut camera = Camera::default();
        camera.fit(asset, asset.default_scene);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.render(
            &gpu,
            &mut encoder,
            &target,
            &camera,
            asset,
            &resident,
            &asset.globals(&pose),
            asset.default_scene,
            true,
            true,
        );
        gpu.queue.submit([encoder.finish()]);
        let image = crate::diagnostics::read_target(&gpu, &target)?;
        image::imageops::replace(&mut atlas, &image, i as i64 * 600, 0);
    }
    std::fs::create_dir_all("target/reports")?;
    atlas.save("target/reports/retarget-transfer.png")?;
    std::fs::write(
        "target/reports/retarget-transfer.txt",
        format!(
            "GPU: {} / {:?}\nSource walk at 0.55 seconds -> tall target with extra helpers -> target with rotated local joint frames.\nRest identity, endpoint and finite output tests: tests/retarget.rs.\n",
            gpu.info.name, gpu.info.backend
        ),
    )?;
    Ok(())
}

/// Six-second GPU comparison; ffmpeg is an optional evidence-generation tool,
/// never a runtime dependency of the player.
pub fn comparison() -> Result<()> {
    use crate::{
        contacts,
        import::{self, Limits},
        render::{Camera, Gpu, Renderer, Target},
        retarget,
    };
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (gpu, _) = pollster::block_on(Gpu::new(&instance, None))?;
    let renderer = Renderer::new(&gpu);
    let load = |name: &str| -> Result<_> {
        Ok(import::from_bytes(
            &std::fs::read(format!("tests/fixtures/retarget/{name}.glb"))?,
            &Limits::default(),
        )?)
    };
    let source = load("Walk-source")?;
    let targets = [load("Tall-target")?, load("Rest-frame-target")?];
    let source_plan = retarget::compile(&source, &source, &retarget::Settings::default(), &[])?;
    let source_contacts = contacts::compile(
        &source,
        &source,
        &source.clips[0],
        &source_plan,
        glam::Mat4::IDENTITY,
    )?;
    let mut plans = Vec::new();
    let mut contact_sets = Vec::new();
    let mut residents = Vec::new();
    let mut cameras = Vec::new();
    for a in std::iter::once(&source).chain(targets.iter()) {
        residents.push(renderer.upload(&gpu, a)?);
        let mut cam = Camera::default();
        cam.fit(a, a.default_scene);
        cam.yaw = 0.95;
        cameras.push(cam);
    }
    for a in &targets {
        let p = retarget::compile(&source, a, &retarget::Settings::default(), &[])?;
        contact_sets.push(contacts::compile(
            &source,
            a,
            &source.clips[0],
            &p,
            glam::Mat4::IDENTITY,
        )?);
        plans.push(p);
    }
    std::fs::create_dir_all("target/reports")?;
    std::fs::create_dir_all("artifacts")?;
    let font = "fontfile='assets/fonts/ArmatraUI-500.ttf':";
    let filter = format!(
        "drawtext={font}text='SOURCE WALK':x=20:y=18:fontsize=22:fontcolor=white:box=1:boxcolor=black@0.6,drawtext={font}text='RAW RETARGET':x=500:y=18:fontsize=22:fontcolor=white:box=1:boxcolor=black@0.6,drawtext={font}text='CONTACT IK':x=980:y=18:fontsize=22:fontcolor=white:box=1:boxcolor=black@0.6"
    );
    let mut command = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            "1440x560",
            "-framerate",
            "30",
            "-i",
            "pipe:0",
            "-vf",
            &filter,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "22",
            "target/reports/retarget-comparison.mp4",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(
            "artifacts/retarget-encode.log",
        )?))
        .spawn()?;
    let mut stdin = child.stdin.take().expect("encoder stdin");
    let target = Target::new(&gpu.device, [480, 560]);
    for frame in 0..180 {
        let variant = usize::from(frame >= 90);
        let a = &targets[variant];
        let p = &plans[variant];
        let c = &contact_sets[variant];
        let time = (frame % 90) as f64 / 30.;
        let poses = [
            source_contacts.raw_pose(&source, &source, &source.clips[0], &source_plan, time, true),
            c.raw_pose(&source, a, &source.clips[0], p, time, true),
            crate::ik::sample(&source, a, &source.clips[0], p, c, time, true).pose,
        ];
        let mut atlas = image::RgbaImage::new(1440, 560);
        for (panel, pose) in poses.iter().enumerate() {
            let asset = if panel == 0 { &source } else { a };
            let id = if panel == 0 { 0 } else { variant + 1 };
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(
                &gpu,
                &mut encoder,
                &target,
                &cameras[id],
                asset,
                &residents[id],
                &asset.globals(pose),
                asset.default_scene,
                true,
                true,
            );
            gpu.queue.submit([encoder.finish()]);
            let image = crate::diagnostics::read_target(&gpu, &target)?;
            image::imageops::replace(&mut atlas, &image, panel as i64 * 480, 0);
        }
        if frame == 24 {
            atlas.save("target/reports/retarget-contact-comparison.png")?;
        }
        stdin.write_all(atlas.as_raw())?;
    }
    drop(stdin);
    anyhow::ensure!(
        child.wait()?.success(),
        "Video encoding failed; see artifacts/retarget-encode.log"
    );
    Ok(())
}
