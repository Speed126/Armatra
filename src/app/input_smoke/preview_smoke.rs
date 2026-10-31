//! Render the transition frame, which the earlier UI-only Library tests omitted.
use super::*;
fn render(e: &mut Editor, r: &Renderer, g: &Gpu) -> Result<String> {
    e.prepare_source_preview(r, g);
    let target = crate::render::Target::new(&g.device, [256, 256]);
    let mut encoder = g.device.create_command_encoder(&Default::default());
    if let Some(p) = &e.source_preview {
        ensure!(
            e.source
                .as_ref()
                .is_some_and(|s| p.key == format!("{}:{}", s.0, s.2)),
            "Resident is bound to the current source before rendering"
        );
        let pose = animation::evaluate(
            &p.asset,
            &p.asset.clips[p.clip],
            &(0..p.asset.nodes.len()).map(Some).collect::<Vec<_>>(),
            e.playback.time,
            &mut vec![],
        );
        let globals = p.asset.globals(&pose);
        ensure!(globals.iter().all(|m| m.is_finite()), "Finite source pose");
        r.render(
            g,
            &mut encoder,
            &target,
            &p.camera,
            &p.asset,
            &p.resident,
            &globals,
            p.asset.default_scene,
            true,
            true,
        );
    } else {
        let (_, asset, resident) = e.model.clone().context("Renderable model/source")?;
        let (pose, _) = e.target_pose();
        let globals = asset.globals(&pose);
        ensure!(globals.iter().all(|m| m.is_finite()), "Finite target pose");
        if e.retarget.is_some() {
            ensure!(
                e.active_pair
                    == Some((
                        e.source.as_ref().unwrap().0.clone(),
                        e.model.as_ref().unwrap().0.clone(),
                        e.source.as_ref().unwrap().2
                    )),
                "Plan binds the actual clip"
            );
        }
        r.render(
            g,
            &mut encoder,
            &target,
            &e.camera,
            &asset,
            &resident,
            &globals,
            e.scene,
            true,
            true,
        );
    }
    g.queue.submit([encoder.finish()]);
    let image = crate::diagnostics::read_target(g, &target)?;
    ensure!(image.dimensions() == (256, 256), "GPU frame readback");
    let hash = blake3::hash(image.as_raw()).to_hex().to_string();
    e.preview_displayed();
    Ok(hash)
}
fn show(e: &mut Editor, r: &Renderer, g: &Gpu, record: &Record) {
    e.ui_state.workspace = Workspace::Library;
    e.motion.open = true;
    e.motion.characters = false;
    e.motion.filter = record.name.clone();
    for _ in 0..3 {
        input(e, r, g, vec![]);
    }
}
fn preview(e: &mut Editor, r: &Renderer, g: &Gpu, record: &Record, medoid: bool) -> Result<String> {
    show(e, r, g, record);
    e.prepare_source_preview(r, g);
    let previous = e.motion.displayed_count;
    let at = if medoid {
        e.input_regions
            .as_ref()
            .unwrap()
            .motion_medoid_previews
            .iter()
            .find(|((id, _), _)| id == &record.id)
            .map(|(_, v)| v.0.center())
            .context("Actual medoid button")?
    } else {
        e.input_regions
            .as_ref()
            .unwrap()
            .motion_preview_buttons
            .get(&record.id)
            .context("Actual Preview button")?
            .center()
    };
    click(e, r, g, at);
    // THIS frame must reconcile and render before the next worker/UI iteration.
    let first = render(e, r, g)?;
    settle(e, r, g)?;
    let last = render(e, r, g)?;
    ensure!(
        e.actual_motion_id() == Some(record.id.as_str())
            && e.playback.playing
            && e.motion.pending_preview.is_none(),
        "Correct ready playback after Preview {}: {} {:?}",
        record.id,
        e.status,
        e.preview_identity()
    );
    ensure!(
        e.motion.displayed_count == previous + 1,
        "Exactly one displayed activation"
    );
    Ok(format!("{first}/{last}"))
}
pub(super) fn run(r: &Renderer, g: &Gpu, root: &std::path::Path) -> Result<()> {
    let location = root.join("preview-preview-managed");
    let mut l = Library::open(location.clone())?;
    for (name, bytes) in [
        (
            "Layered",
            crate::motion_fixtures::layered_preview().finish(),
        ),
        ("Default", crate::fixtures::robot(false, false).finish()),
        ("Skinless", {
            let mut b = crate::fixtures::robot(true, true);
            let mut a = b.json["animations"][0].clone();
            a["name"] = serde_json::json!("Wave alternate");
            b.json["animations"].as_array_mut().unwrap().push(a);
            b.finish()
        }),
        ("Cube", crate::fixtures::cube().finish()),
        ("Multi", crate::fixtures::multi_skin_wave_target().finish()),
    ]
    .into_iter()
    .map(|(n, b)| (n.to_string(), b))
    .chain(
        crate::motion_fixtures::motions()
            .into_iter()
            .map(|(n, b)| (n, b.finish())),
    ) {
        let asset = import::from_bytes(&bytes, &Limits::default())?;
        l.import(&bytes, &name, &asset)?;
    }
    // Also cover both bundled non-humanoid twist clips and measured walk.
    for (name, bytes) in [
        ("Twist", crate::motion_fixtures::twist(false).finish()),
        ("Affine", crate::motion_fixtures::twist(true).finish()),
        (
            "Measured",
            crate::rig_corpus::humanoid(crate::rig_corpus::Variant::default(), true)
                .0
                .finish(),
        ),
    ] {
        let a = import::from_bytes(&bytes, &Limits::default())?;
        l.import(&bytes, name, &a)?;
    }
    let mut e = Editor::new(l, context());
    e.input_regions = Some(InputRegions::default());
    e.request_motion_index();
    settle(&mut e, r, g)?;
    let records = e.records.clone();
    let find = |name: &str, kind: Kind| {
        records
            .iter()
            .find(|v| (v.name == name || v.name.ends_with(&format!(" · {name}"))) && v.kind == kind)
            .unwrap()
            .clone()
    };
    let layered = find("Layered", Kind::Clip(0));
    let skinless = find("Skinless", Kind::Clip(0));
    let cube = find("Cube", Kind::Model);
    let multi = find("Multi", Kind::Model);
    e.request(&cube, r, g);
    e.request(&skinless, r, g);
    settle(&mut e, r, g)?;
    e.request(&layered, r, g);
    settle(&mut e, r, g)?;
    show(&mut e, r, g, &skinless);
    e.prepare_source_preview(r, g);
    let old = e
        .source_preview
        .as_ref()
        .context("Old layered source resident")?;
    ensure!(
        old.asset.materials.len() == 4 && e.cpu.contains(&skinless.hash),
        "Regression requires warm source plus layered old materials"
    );
    preview(&mut e, r, g, &skinless, false)?;
    ensure!(
        e.source_preview.as_ref().unwrap().asset.materials.len() == 1,
        "Transition uses new material table and new GPU resident"
    );
    let jump = find("Jump", Kind::Clip(0));
    show(&mut e, r, g, &jump);
    settle(&mut e, r, g)?;
    let before = (
        e.actual_motion_id().map(str::to_owned),
        e.source_generation,
        e.playback.time,
    );
    let rect = e.input_regions.as_ref().unwrap().motion_results[&jump.id];
    click(&mut e, r, g, rect.left_top() + egui::vec2(190., 20.));
    ensure!(
        e.motion.selected_result.as_ref() == Some(&jump.id)
            && (
                e.actual_motion_id().map(str::to_owned),
                e.source_generation,
                e.playback.time
            ) == before,
        "Selection is independent from playback"
    );
    settle(&mut e, r, g)?;
    preview(&mut e, r, g, &jump, true)?;
    // Evict derived textures, then activate while the new strip's response is
    // still pending in the UI. Viewport resources must remain independent.
    e.request(&layered, r, g);
    settle(&mut e, r, g)?;
    e.motion.previews.clear_memory_cache();
    show(&mut e, r, g, &jump);
    ensure!(
        e.motion.previews.pending(),
        "Pose thumbnail generation pending during activation"
    );
    preview(&mut e, r, g, &jump, false)?;
    ensure!(
        e.motion.previews.bytes() > 0,
        "Evicted thumbnail cache repopulates after a rendered Preview"
    );
    let mut evidence = vec![];
    for record in records.iter().filter(|v| matches!(v.kind, Kind::Clip(_))) {
        if e.actual_motion_id() == Some(record.id.as_str()) {
            e.request(
                if record.hash == layered.hash {
                    &skinless
                } else {
                    &layered
                },
                r,
                g,
            );
            settle(&mut e, r, g)?;
        }
        let pixels = preview(&mut e, r, g, record, false)?;
        let duration = e.source.as_ref().unwrap().1.clips[e.source.as_ref().unwrap().2].duration;
        for t in [0., duration * 0.5, duration] {
            ensure!(
                e.request_with_preview(record, Some(t), r, g),
                "Valid clip boundary"
            );
            settle(&mut e, r, g)?;
            render(&mut e, r, g)?;
            ensure!(e.playback.time == t, "Exact requested time");
        }
        evidence.push(serde_json::json!({"record":record.id,"clip":record.kind,"frames":pixels,"identity":e.preview_identity()}));
    }
    // Same-result Preview preserves the established play/pause behavior.
    show(&mut e, r, g, &jump);
    if e.actual_motion_id() != Some(jump.id.as_str()) {
        preview(&mut e, r, g, &jump, false)?;
    }
    let generation = e.source_generation;
    let button = e.input_regions.as_ref().unwrap().motion_preview_buttons[&jump.id].center();
    click(&mut e, r, g, button);
    ensure!(
        !e.playback.playing && e.source_generation == generation,
        "Repeated main button pauses without another request"
    );
    click(&mut e, r, g, button);
    ensure!(e.playback.playing, "Repeated button resumes");
    // Bad records, clip indices and times preserve the last valid state.
    for failure in 0..4 {
        let mut bad = jump.clone();
        let time = if failure == 3 { f32::NAN } else { 0. };
        if failure == 0 {
            bad.id = "missing".into();
        }
        if failure == 1 {
            bad.kind = Kind::Clip(999);
        }
        if failure == 2 {
            bad.hash = "0".repeat(64);
        }
        ensure!(
            !e.request_with_preview(&bad, Some(time), r, g),
            "Invalid request rejected"
        );
        ensure!(
            e.source_generation == generation
                && e.motion
                    .preview_error
                    .as_ref()
                    .is_some_and(|e| e.contains("Preview failed")),
            "Failure keeps playback/selection and is actionable"
        );
        render(&mut e, r, g)?;
    }
    ensure!(
        !e.request_with_preview(&jump, Some(-1.), r, g),
        "Invalid time rejected without activation"
    );
    render(&mut e, r, g)?;
    // Fault-inject backend rejection/non-finite cached data, without altering
    // immutable blobs. Both failures must leave the previous rendered source.
    let original = e.cpu.get(&layered.hash).context("Layered CPU resident")?;
    for resource_failure in [false, true] {
        let mut invalid = (*original).clone();
        if resource_failure {
            let width = g.device.limits().max_texture_dimension_2d + 1;
            invalid.images.push(crate::scene::Image {
                width,
                height: 1,
                rgba: vec![255; width as usize * 4],
            });
        } else {
            invalid.nodes[0].rest =
                crate::scene::Local::Matrix(glam::Mat4::from_cols_array(&[f32::NAN; 16]));
        }
        e.cpu.insert(
            layered.hash.clone(),
            Arc::new(invalid),
            original.cpu_bytes(),
        );
        e.gpu_cache = ByteLru::new(512 * 1024 * 1024);
        ensure!(
            !e.request_with_preview(&layered, Some(0.), r, g),
            "Bad candidate rejected transactionally"
        );
        ensure!(
            e.source_generation == generation
                && e.actual_motion_id() == Some(jump.id.as_str())
                && e.motion.pending_preview.is_none(),
            "Fault preserves last playable source"
        );
        render(&mut e, r, g)?;
    }
    e.cpu
        .insert(layered.hash.clone(), original.clone(), original.cpu_bytes());
    e.model = None;
    e.refresh_mapping();
    preview(&mut e, r, g, &skinless, false)?;
    ensure!(
        e.source_preview.is_some(),
        "No target: original source plays"
    );
    let default = find("Default", Kind::Model);
    e.request(&default, r, g);
    settle(&mut e, r, g)?;
    let alternate = find("Skinless", Kind::Clip(1));
    preview(&mut e, r, g, &alternate, false)?;
    ensure!(
        e.source.as_ref().unwrap().2 == 1
            && e.mapping.as_ref().is_some_and(|m| !m.nodes.is_empty()),
        "Compatible target/multi-clip asset"
    );
    // Selected target skins and confirmed edits cannot leak to another source.
    e.request(&multi, r, g);
    e.request(&skinless, r, g);
    settle(&mut e, r, g)?;
    ensure!(
        e.mapping_feedback.phase == "Failed",
        "Preview after failed multi-skin compile"
    );
    let previous = e.mapping_settings.clone();
    e.mapping_settings.target_skin = Some(0);
    e.skin_changed(&previous);
    e.assign_joint(6, Some(5));
    e.assign_joint(11, Some(0));
    e.start_mapping(super::super::mapping_apply::Intent::Apply);
    settle(&mut e, r, g)?;
    let old_plan = e.active_plan_id().context("Manual active plan")?;
    let overrides = e.mapping_overrides.clone();
    preview(&mut e, r, g, &jump, false)?;
    ensure!(
        e.mapping_settings.target_skin == Some(0)
            && e.active_plan_id().as_ref() != Some(&old_plan)
            && e.mapping_overrides != overrides,
        "New source keeps target skin but invalidates old plan/overrides"
    );
    let previous = e.mapping_settings.clone();
    e.mapping_settings.target_skin = Some(1);
    e.skin_changed(&previous);
    preview(&mut e, r, g, &alternate, false)?;
    ensure!(
        e.mapping_settings.target_skin == Some(1),
        "Another target skin stays selected"
    );
    // Existing search suite also exercises real delayed/stale/error workers.
    motion_search(r, g, &root.join("preview-races"))?;
    e.motion.selected_result = Some(jump.id.clone());
    e.request_motion_index();
    settle(&mut e, r, g)?;
    ensure!(
        e.motion.selected_result.as_ref() == Some(&jump.id),
        "Reindex preserves selection"
    );
    preview(&mut e, r, g, &jump, true)?;
    e.ui_state.workspace = Workspace::Animate;
    e.motion.open = false;
    input(&mut e, r, g, vec![]);
    render(&mut e, r, g)?;
    e.ui_state.workspace = Workspace::Library;
    e.motion.open = true;
    input(&mut e, r, g, vec![]);
    render(&mut e, r, g)?;
    crate::session::save(&e.library.lock().unwrap(), &multi.hash, &jump.hash, 0)?;
    drop(e);
    let l = Library::open(location.clone())?;
    let (model, clip) = crate::session::load(&l).context("Restart selection")?;
    let mut e = Editor::new(l, context());
    e.input_regions = Some(InputRegions::default());
    e.request(&model, r, g);
    e.request(&clip, r, g);
    settle(&mut e, r, g)?;
    e.request_motion_index();
    settle(&mut e, r, g)?;
    preview(&mut e, r, g, &skinless, false)?;
    write_evidence(
        "preview-preview-interactions.json",
        &serde_json::to_vec_pretty(
            &serde_json::json!({"result":"PASS","managed_library":location,"rendered_transition_frames":true,"original_material_mismatch_reproduced":true,"record_evidence":evidence,"rendered_after_restart":e.preview_identity(),"race_suite":true,"os_input":false}),
        )?,
    )?;
    println!(
        "Motion Library Preview: transition-frame GPU rendering, bundled clips, recoverable failures, target skins, race guards and restart: PASS"
    );
    Ok(())
}
