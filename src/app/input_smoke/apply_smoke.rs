//! UI real UI skin selection, failed Apply recovery, active poses and persistence.
use super::*;
use crate::app::mapping_apply::Intent;
fn action(e: &mut Editor, r: &Renderer, g: &Gpu, name: &str) {
    let p = e.input_regions.as_ref().unwrap().mapping_controls[name].center();
    click(e, r, g, p);
}
fn key_action(e: &mut Editor, r: &Renderer, g: &Gpu, name: &str, key: egui::Key) {
    let id = e.input_regions.as_ref().unwrap().mapping_focus[name];
    e.ctx.memory_mut(|m| m.request_focus(id));
    input(e, r, g, vec![]);
    for pressed in [true, false] {
        input(
            e,
            r,
            g,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
    }
}
fn skin(e: &mut Editor, r: &Renderer, g: &Gpu, index: Option<usize>) {
    action(e, r, g, "target-skin");
    input(e, r, g, vec![]);
    let at = e.input_regions.as_ref().unwrap().skin_options[&(1, index)].center();
    click(e, r, g, at);
    input(e, r, g, vec![]);
    input(e, r, g, vec![]);
}
fn map(e: &mut Editor, r: &Renderer, g: &Gpu, source: usize, target: usize) {
    action(e, r, g, "map");
    for key in [(0, source), (1, target)] {
        let at = e.input_regions.as_ref().unwrap().mapping_joints[&key];
        click(e, r, g, at);
    }
    action(e, r, g, "confirm");
    input(e, r, g, vec![]);
}
pub(super) fn run(r: &Renderer, g: &Gpu, root: &std::path::Path) -> Result<()> {
    let location = root.join("apply-apply-managed");
    let mut l = Library::open(location.clone())?;
    for (name, bytes) in [
        ("Skinless Wave", {
            let mut b = crate::fixtures::robot(true, true);
            let mut alternate = b.json["animations"][0].clone();
            alternate["name"] = serde_json::json!("Wave alternate");
            b.json["animations"].as_array_mut().unwrap().push(alternate);
            b.finish()
        }),
        (
            "Multi-skin target",
            crate::fixtures::multi_skin_wave_target().finish(),
        ),
    ] {
        let a = import::from_bytes(&bytes, &Limits::default())?;
        l.import(&bytes, name, &a)?;
    }
    let target = l
        .manifest
        .records
        .iter()
        .find(|r| r.kind == Kind::Model)
        .unwrap()
        .clone();
    let clip = l
        .manifest
        .records
        .iter()
        .find(|r| r.kind == Kind::Clip(0))
        .unwrap()
        .clone();
    let mut e = Editor::new(l, context());
    e.input_regions = Some(InputRegions::default());
    e.request(&target, r, g);
    settle(&mut e, r, g)?;
    e.request(&clip, r, g);
    settle(&mut e, r, g)?;
    e.mapping_open = true;
    e.ui_state.workspace = Workspace::Retarget;
    input(&mut e, r, g, vec![]);
    ensure!(
        e.retarget.is_none() && e.status.contains("Multiple skins"),
        "Initial exact multiple-skins compiler failure: {}",
        e.status
    );
    map(&mut e, r, g, 6, 5);
    map(&mut e, r, g, 11, 0);
    let edits = e.mapping_overrides.clone();
    ensure!(edits.len() == 2, "Two confirmed manual overrides");
    key_action(&mut e, r, g, "apply", egui::Key::Enter);
    ensure!(
        e.mapping_feedback.phase == "Failed"
            && e.status.contains("Target Multi-skin target has 2 skins")
            && e.ui_state.workspace == Workspace::Retarget
            && e.mapping_overrides == edits,
        "Missing skin is actionable, stays in Retarget and preserves edits: {}",
        e.status
    );
    skin(&mut e, r, g, Some(0));
    ensure!(
        e.mapping_settings.source_skin.is_none()
            && e.mapping_settings.target_skin == Some(0)
            && e.mapping_overrides == edits
            && !e.status.contains("Multiple skins")
            && e.mapping_feedback.phase == "Pending",
        "Skin selection clears obsolete error and keeps compatible edits"
    );
    let generation = e.plan_generation.0;
    let regions = e.input_regions.as_ref().unwrap();
    let primary = regions.mapping_controls["apply"];
    let arrow = regions.mapping_controls["apply-menu"];
    ensure!(
        primary.height() == arrow.height()
            && primary.right() == arrow.left()
            && primary.intersect(arrow).area() == 0.,
        "Attached split button has separate equal-height hit regions"
    );
    action(&mut e, r, g, "apply-menu");
    input(&mut e, r, g, vec![]);
    ensure!(
        e.plan_generation.0 == generation
            && !e.plan_pending
            && e.input_regions
                .as_ref()
                .unwrap()
                .mapping_controls
                .contains_key("apply-only"),
        "Arrow opens Apply Only without triggering primary"
    );
    for key in [egui::Key::Enter, egui::Key::Space] {
        input(
            &mut e,
            r,
            g,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        input(
            &mut e,
            r,
            g,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        key_action(&mut e, r, g, "apply-menu", key);
        ensure!(
            e.plan_generation.0 == generation
                && !e.plan_pending
                && e.input_regions
                    .as_ref()
                    .unwrap()
                    .mapping_controls
                    .contains_key("apply-only"),
            "Keyboard arrow opens menu without applying"
        );
    }
    action(&mut e, r, g, "apply-only");
    settle(&mut e, r, g)?;
    ensure!(
        e.ui_state.workspace == Workspace::Retarget
            && e.retarget.as_ref().unwrap().settings.target_skin == Some(0)
            && e.mapping_feedback.phase == "Applied",
        "Secondary Apply only uses the same valid snapshot without navigation"
    );
    key_action(&mut e, r, g, "apply", egui::Key::Space);
    ensure!(
        !e.input_regions
            .as_ref()
            .unwrap()
            .mapping_controls
            .contains_key("apply-only"),
        "Primary Space never opens the options menu"
    );
    let request_id = e.plan_generation.0;
    ensure!(
        e.plan_pending
            && e.mapping_feedback.phase == "Applying"
            && e.mapping_feedback
                .request
                .as_ref()
                .unwrap()
                .settings
                .target_skin
                == Some(0),
        "Apply captures exact selected skin"
    );
    e.start_mapping(Intent::Preview);
    ensure!(e.plan_generation.0 == request_id, "Duplicate Apply ignored");
    settle(&mut e, r, g)?;
    ensure!(
        e.ui_state.workspace == Workspace::Animate
            && e.playback.playing
            && e.playback.time == 0.
            && !e.bind_pose
            && e.active_pair == Some((clip.hash.clone(), target.hash.clone(), 0)),
        "Successful Apply opens correct active target and starts playback"
    );
    ensure!(
        e.mapping_feedback.phase == "Applied"
            && e.retarget.as_ref().unwrap().overrides == edits
            && e.retarget.as_ref().unwrap().settings.target_skin == Some(0),
        "Actual active plan uses exact manual snapshot"
    );
    e.playback.time = 0.;
    let (a, w) = e.target_pose();
    ensure!(w.is_empty(), "Target pose: {w:?}");
    let asset = Arc::clone(&e.model.as_ref().unwrap().1);
    let p0 = e.rendered_pose_evidence(&asset, &a);
    e.playback.time = 0.75;
    let (b, w) = e.target_pose();
    ensure!(w.is_empty(), "Target pose at .75: {w:?}");
    let p1 = e.rendered_pose_evidence(&asset, &b);
    ensure!(
        p0["lbs_world_blake3"] != p1["lbs_world_blake3"]
            && !asset.globals(&a)[5].abs_diff_eq(asset.globals(&b)[5], 1e-5),
        "Source Wave -> manual transfer -> selected target skin -> rendered LBS vertices change"
    );
    let compiled_id = e.active_plan_id();
    action(&mut e, r, g, "back-retarget");
    input(&mut e, r, g, vec![]);
    let at = e.input_regions.as_ref().unwrap().mapping_joints[&(0, 6)];
    click(&mut e, r, g, at);
    action(&mut e, r, g, "unmap");
    action(&mut e, r, g, "apply");
    settle(&mut e, r, g)?;
    e.playback.time = 0.75;
    let (unmapped, _) = e.target_pose();
    ensure!(
        e.rendered_pose_evidence(&asset, &unmapped)["lbs_world_blake3"] != p1["lbs_world_blake3"]
            && e.active_plan_id() != compiled_id,
        "Recompiling changed overrides changes the actual skinned target and compiled identity"
    );
    action(&mut e, r, g, "back-retarget");
    input(&mut e, r, g, vec![]);
    action(&mut e, r, g, "undo");
    action(&mut e, r, g, "apply");
    settle(&mut e, r, g)?;
    e.playback.time = 0.75;
    let (restored, _) = e.target_pose();
    ensure!(
        e.rendered_pose_evidence(&asset, &restored)["lbs_world_blake3"] == p1["lbs_world_blake3"]
            && e.active_plan_id() == compiled_id,
        "Undo and reapply restore target deformation exactly"
    );
    action(&mut e, r, g, "preview-save");
    let save_gen = e.plan_generation.0;
    e.start_mapping(Intent::Save);
    ensure!(e.plan_generation.0 == save_gen, "Repeated Save ignored");
    settle(&mut e, r, g)?;
    let path = e
        .mapping_feedback
        .saved_path
        .clone()
        .context("Saved profile path")?;
    ensure!(
        path.exists()
            && path.starts_with(location.join("profiles"))
            && e.mapping_feedback.phase == "Saved"
            && e.status.contains("Mapping saved"),
        "Real saved profile and feedback"
    );
    action(&mut e, r, g, "back-retarget");
    input(&mut e, r, g, vec![]);
    skin(&mut e, r, g, Some(1));
    ensure!(
        e.mapping_overrides.is_empty() && e.retarget.is_none(),
        "Unrelated skin cannot inherit incompatible node IDs"
    );
    map(&mut e, r, g, 6, 18);
    e.start_mapping(Intent::Apply);
    settle(&mut e, r, g)?;
    ensure!(
        e.retarget.as_ref().unwrap().settings.target_skin == Some(1)
            && e.retarget
                .as_ref()
                .unwrap()
                .transfers
                .iter()
                .all(|t| asset.skins[1].joints.contains(&t.target)),
        "Another selected skin applies independently"
    );
    e.start_mapping(Intent::Apply);
    let old = e.mapping_feedback.request.clone().unwrap();
    skin(&mut e, r, g, Some(0));
    ensure!(
        e.mapping_overrides == edits,
        "Return to skin recovers its draft"
    );
    settle(&mut e, r, g)?;
    ensure!(
        e.retarget.is_none() && e.mapping_settings.target_skin == Some(0),
        "Old skin worker cannot activate"
    );
    e.accept_mapping(
        old,
        false,
        Err("Multiple skins: stale request".into()),
        None,
        vec![],
    );
    ensure!(
        e.mapping_feedback.phase == "Pending" && !e.status.contains("Multiple skins"),
        "Stale error cannot overwrite corrected selection"
    );
    e.start_mapping(Intent::Apply);
    settle(&mut e, r, g)?;
    ensure!(
        e.retarget.as_ref().unwrap().settings.target_skin == Some(0),
        "Recovery applies current skin after race"
    );
    e.start_mapping(Intent::Apply);
    let old_clip_request = e.mapping_feedback.request.clone().unwrap();
    let alternate = e
        .records
        .iter()
        .find(|r| r.hash == clip.hash && r.kind == Kind::Clip(1))
        .unwrap()
        .clone();
    e.request(&alternate, r, g);
    settle(&mut e, r, g)?;
    ensure!(
        e.source.as_ref().unwrap().2 == 1 && e.active_pair.as_ref().unwrap().2 == 1,
        "Active plan binding follows selected clip"
    );
    e.accept_mapping(
        old_clip_request,
        false,
        Err("Old clip error".into()),
        None,
        vec![],
    );
    ensure!(
        !e.status.contains("Old clip error") && e.source.as_ref().unwrap().2 == 1,
        "Older clip result cannot activate or overwrite feedback"
    );
    // A newer cold selection cancels an Apply at request time, before I/O completes.
    e.start_mapping(Intent::Apply);
    let before_load = e.plan_generation.0;
    let absent = Record {
        id: "missing-cold-target".into(),
        hash: "0000000000000000000000000000000000000000000000000000000000000000".into(),
        name: "Missing cold target".into(),
        kind: Kind::Model,
    };
    e.request(&absent, r, g);
    ensure!(
        !e.plan_pending && e.plan_generation.0 > before_load,
        "A newer requested selection cancels the old worker before activation"
    );
    settle(&mut e, r, g)?;
    e.request(&clip, r, g);
    settle(&mut e, r, g)?;
    e.mapping_open = true;
    e.ui_state.workspace = Workspace::Retarget;
    input(&mut e, r, g, vec![]);
    // A queued job keeps the click-time snapshot, and changed selection cancels it.
    skin(&mut e, r, g, Some(1));
    map(&mut e, r, g, 6, 18);
    e.start_mapping(Intent::Apply);
    e.request(&alternate, r, g);
    settle(&mut e, r, g)?;
    ensure!(
        e.mapping_settings.target_skin == Some(1)
            && e.mapping_overrides
                .iter()
                .any(|o| o.source == 6 && o.target == Some(18))
            && e.retarget.is_none(),
        "Changing clip preserves the selected skin and pending original-ID edits"
    );
    e.request(&clip, r, g);
    settle(&mut e, r, g)?;
    skin(&mut e, r, g, Some(0));
    e.jobs = 2;
    e.start_mapping(Intent::Apply);
    let queued = e.plan_requested.as_ref().unwrap().clone();
    ensure!(
        queued.settings.target_skin == Some(0) && queued.overrides == edits,
        "Queued request captures exact state"
    );
    skin(&mut e, r, g, Some(1));
    e.jobs = 0;
    settle(&mut e, r, g)?;
    ensure!(
        e.plan_requested.is_none() && e.retarget.is_none(),
        "Queued stale request cancelled"
    );
    skin(&mut e, r, g, Some(0));
    // I/O failure reports the actual failing library location, without claiming Saved.
    let bad = root.join("apply-write-failure");
    std::fs::create_dir_all(&bad)?;
    std::fs::write(bad.join("profiles"), b"not a directory")?;
    e.library.lock().unwrap().root = bad.clone();
    e.start_mapping(Intent::Save);
    settle(&mut e, r, g)?;
    ensure!(
        e.mapping_feedback.phase == "Failed"
            && e.mapping_feedback.saved_path.is_none()
            && e.status.contains("Could not write mapping profile"),
        "Disk error visible: {}",
        e.status
    );
    e.library.lock().unwrap().root = location.clone();
    e.start_mapping(Intent::Save);
    settle(&mut e, r, g)?;
    for scale in [1., 1.25, 1.5] {
        e.ctx.set_zoom_factor(scale);
        e.mapping_open = true;
        e.ui_state.workspace = Workspace::Retarget;
        for _ in 0..3 {
            frame(
                &mut e,
                r,
                g,
                egui::vec2(1280. / scale, 720. / scale),
                vec![],
            );
        }
        let regions = e.input_regions.as_ref().unwrap();
        let bounds = e.ctx.content_rect();
        for name in ["source-skin", "target-skin", "apply", "apply-details"] {
            if name == "source-skin" {
                continue;
            }
            ensure!(
                bounds.contains_rect(regions.mapping_controls[name]),
                "Reachable {name} at {scale}"
            );
        }
        ensure!(
            bounds.contains_rect(regions.mapping_save),
            "Reachable Save at {scale}"
        );
    }
    drop(e);
    let mut reopened = Editor::new(Library::open(location.clone())?, context());
    reopened.input_regions = Some(InputRegions::default());
    reopened.request(&target, r, g);
    settle(&mut reopened, r, g)?;
    reopened.request(&clip, r, g);
    settle(&mut reopened, r, g)?;
    ensure!(
        reopened.mapping_settings.target_skin == Some(0)
            && reopened.mapping_settings.source_skin.is_none()
            && reopened.mapping_overrides == edits
            && reopened.plan_origin == "Saved profile",
        "Restart restores exact skin and original-ID overrides: {}",
        reopened.status
    );
    reopened.start_mapping(Intent::Preview);
    settle(&mut reopened, r, g)?;
    reopened.playback.time = 0.75;
    let (pose, w) = reopened.target_pose();
    ensure!(
        w.is_empty() && !asset.globals(&pose)[5].abs_diff_eq(asset.globals(&a)[5], 1e-5),
        "Restored mapping applies and animates"
    );
    write_evidence(
        "apply-apply-interactions.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"result":"PASS","method":"egui pointer/key events; exact native renderer pose method; no OS input","source":clip.hash,"target":target.hash,"source_skin":null,"target_skin":0,"overrides":edits,"saved_profile":path,"pose_0":p0,"pose_075":p1,"operation":reopened.ui_evidence()["mapping_operation"],"os_input":false}),
        )?,
    )?;
    println!(
        "Apply missing-skin recovery, exact apply snapshot, active Wave poses, race guards, save/restart and compact controls: PASS"
    );
    Ok(())
}
