//! Pointer/keyboard events go through the same egui UI as the native editor.
use super::*;
use crate::ui::correspondence::{Operation, Selection};
fn control(e: &Editor, name: &str) -> egui::Pos2 {
    e.input_regions.as_ref().unwrap().mapping_controls[name].center()
}
fn joint(e: &Editor, side: usize, node: usize) -> egui::Pos2 {
    e.input_regions.as_ref().unwrap().mapping_joints[&(side, node)]
}
fn press(e: &mut Editor, r: &Renderer, g: &Gpu, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        input(
            e,
            r,
            g,
            vec![
                egui::Event::ModifiersChanged(if pressed {
                    modifiers
                } else {
                    Default::default()
                }),
                egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: if pressed {
                        modifiers
                    } else {
                        Default::default()
                    },
                },
            ],
        );
    }
}
fn choose(e: &mut Editor, r: &Renderer, g: &Gpu, side: usize, node: usize) {
    let p = joint(e, side, node);
    click(e, r, g, p);
}
fn action(e: &mut Editor, r: &Renderer, g: &Gpu, name: &str) {
    let p = control(e, name);
    click(e, r, g, p);
}
fn click_sized(e: &mut Editor, r: &Renderer, g: &Gpu, size: egui::Vec2, at: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            e,
            r,
            g,
            size,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}
fn selection(e: &Editor) -> &Selection {
    let Operation::Selecting(s) = &e.ui_state.correspondence.operation else {
        panic!("Expected shared confirmation selection")
    };
    s
}
pub(super) fn run(r: &Renderer, g: &Gpu, root: &std::path::Path) -> Result<()> {
    let location = root.join("mapping-mapping-managed");
    let mut library = Library::open(location.clone())?;
    let (mut source, _) = crate::rig_corpus::humanoid(Default::default(), true);
    source.json["nodes"][7]["name"] = serde_json::json!("mixamorig_LeftArm");
    let (target, ids) = crate::rig_corpus::humanoid(
        crate::rig_corpus::Variant {
            permuted: true,
            proportions: true,
            ..Default::default()
        },
        false,
    );
    for (name, bytes) in [
        ("Mapping source", source.finish()),
        ("Mapping target", target.finish()),
    ] {
        let a = import::from_bytes(&bytes, &Limits::default())?;
        library.import(&bytes, name, &a)?;
    }
    let model = library
        .manifest
        .records
        .iter()
        .find(|r| r.name == "Mapping target" && r.kind == Kind::Model)
        .unwrap()
        .clone();
    let clip = library
        .manifest
        .records
        .iter()
        .find(|r| matches!(r.kind, Kind::Clip(_)))
        .unwrap()
        .clone();
    let mut e = Editor::new(library, context());
    e.input_regions = Some(InputRegions::default());
    e.request(&model, r, g);
    settle(&mut e, r, g)?;
    e.request(&clip, r, g);
    settle(&mut e, r, g)?;
    e.mapping_open = true;
    e.ui_state.workspace = Workspace::Retarget;
    settle(&mut e, r, g)?;
    let t7 = ids[7].unwrap();
    let t8 = ids[8].unwrap();
    let initial = e.mapping_overrides.clone();
    let key = e.retarget.as_ref().unwrap().key.clone();
    let generation = e.plan_generation.0;
    action(&mut e, r, g, "map");
    ensure!(
        selection(&e).source.is_none()
            && selection(&e).target.is_none()
            && !e.input_regions.as_ref().unwrap().mapping_confirm_enabled,
        "Button opens empty nonmodal fields, disabled Confirm"
    );
    choose(&mut e, r, g, 0, 7);
    ensure!(
        selection(&e).source == Some(7) && selection(&e).target.is_none(),
        "Source click populates only source"
    );
    choose(&mut e, r, g, 1, t7);
    ensure!(
        selection(&e).target == Some(t7) && e.mapping_overrides == initial,
        "Target uses original permuted ID without committing"
    );
    action(&mut e, r, g, "source-field");
    choose(&mut e, r, g, 0, 8);
    ensure!(
        selection(&e).source == Some(8) && selection(&e).target == Some(t7),
        "Replace source field"
    );
    action(&mut e, r, g, "target-field");
    choose(&mut e, r, g, 1, t8);
    ensure!(selection(&e).target == Some(t8), "Replace target field");
    action(&mut e, r, g, "cancel");
    ensure!(e.mapping_overrides == initial, "Cancel is mutation free");
    let source = joint(&e, 0, 7);
    let target = joint(&e, 1, t7);
    let bone = (joint(&e, 0, 7) + joint(&e, 0, 8).to_vec2()) * 0.5;
    drag(
        &mut e,
        r,
        g,
        bone,
        target - bone,
        egui::PointerButton::Middle,
    );
    ensure!(
        selection(&e).source == Some(7),
        "Bone drag resolves the original parent joint ID"
    );
    press(&mut e, r, g, egui::Key::Escape, Default::default());
    input(
        &mut e,
        r,
        g,
        vec![
            egui::Event::ModifiersChanged(egui::Modifiers::SHIFT),
            egui::Event::PointerMoved(source),
            egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::SHIFT,
            },
        ],
    );
    input(&mut e, r, g, vec![egui::Event::PointerMoved(target)]);
    input(
        &mut e,
        r,
        g,
        vec![
            egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::SHIFT,
            },
            egui::Event::ModifiersChanged(Default::default()),
        ],
    );
    ensure!(
        selection(&e).source == Some(7) && selection(&e).target == Some(t7),
        "Shift+left drag uses same confirmation flow"
    );
    press(&mut e, r, g, egui::Key::Escape, Default::default());
    drag(
        &mut e,
        r,
        g,
        source,
        target - source,
        egui::PointerButton::Middle,
    );
    ensure!(
        selection(&e).source == Some(7)
            && selection(&e).target == Some(t7)
            && e.mapping_overrides == initial,
        "Middle drag opens confirmation with exact original IDs, no override before confirm"
    );
    press(&mut e, r, g, egui::Key::Escape, Default::default());
    ensure!(
        matches!(e.ui_state.correspondence.operation, Operation::Idle)
            && e.mapping_overrides == initial,
        "Escape cancels drag proposal"
    );
    drag(
        &mut e,
        r,
        g,
        source,
        egui::vec2(2., 0.),
        egui::PointerButton::Middle,
    );
    ensure!(
        matches!(e.ui_state.correspondence.operation, Operation::Idle),
        "Below-threshold gesture is not a mapping drag"
    );
    let rect = e.input_regions.as_ref().unwrap().mapping_canvas;
    let empty = rect.left_top() + egui::vec2(12., 65.);
    let delta = egui::vec2(25., 11.);
    let before = e.ui_state.correspondence.cameras;
    drag(&mut e, r, g, empty, delta, egui::PointerButton::Middle);
    ensure!(
        (e.ui_state.correspondence.cameras[0].pan - before[0].pan - delta).length() < 0.1
            && matches!(e.ui_state.correspondence.operation, Operation::Idle),
        "Empty middle drag pans"
    );
    let source = joint(&e, 0, 7);
    let before = e.ui_state.correspondence.cameras;
    drag(&mut e, r, g, source, delta, egui::PointerButton::Secondary);
    ensure!(
        (e.ui_state.correspondence.cameras[0].pan - before[0].pan - delta).length() < 0.1,
        "Right drag on joint pans"
    );
    let before = e.ui_state.correspondence.cameras;
    drag(&mut e, r, g, empty, delta, egui::PointerButton::Primary);
    ensure!(
        e.ui_state.correspondence.cameras[0].yaw != before[0].yaw
            && e.ui_state.correspondence.cameras[1].yaw == e.ui_state.correspondence.cameras[0].yaw,
        "Left background orbit stays linked without copying pan"
    );
    e.ui_state.correspondence.frame();
    input(&mut e, r, g, vec![]);
    let source = joint(&e, 0, 7);
    let empty_target =
        e.input_regions.as_ref().unwrap().mapping_canvas.right_top() + egui::vec2(-12., 65.);
    drag(
        &mut e,
        r,
        g,
        source,
        empty_target - source,
        egui::PointerButton::Middle,
    );
    ensure!(
        matches!(e.ui_state.correspondence.operation, Operation::Idle)
            && e.mapping_overrides == initial,
        "Drop on empty target canvas cancels"
    );
    let target = joint(&e, 1, t7);
    drag(
        &mut e,
        r,
        g,
        source,
        target - source,
        egui::PointerButton::Middle,
    );
    action(&mut e, r, g, "confirm");
    ensure!(
        e.mapping_overrides
            .iter()
            .any(|o| o.source == 7 && o.target == Some(t7))
            && e.mapping_pending(7),
        "Confirmed manual mapping pending preview"
    );
    ensure!(
        e.plan_generation.0 == generation
            && e.retarget.as_ref().unwrap().key == key
            && !e.plan_pending,
        "Confirm never compiles or saves"
    );
    let saved = e.mapping_overrides.clone();
    action(&mut e, r, g, "map");
    choose(&mut e, r, g, 1, t7);
    choose(&mut e, r, g, 0, 8);
    ensure!(
        !e.input_regions.as_ref().unwrap().mapping_confirm_enabled
            && e.input_regions
                .as_ref()
                .unwrap()
                .mapping_controls
                .contains_key("replace"),
        "Conflict shows explicit replacement warning and disabled Confirm"
    );
    action(&mut e, r, g, "confirm");
    ensure!(
        e.mapping_overrides == saved,
        "Disabled conflict confirmation does not steal target"
    );
    action(&mut e, r, g, "replace");
    action(&mut e, r, g, "confirm");
    ensure!(
        e.mapping_overrides
            .iter()
            .any(|o| o.source == 7 && o.target.is_none())
            && e.mapping_overrides
                .iter()
                .any(|o| o.source == 8 && o.target == Some(t7)),
        "Explicit replacement unmaps old source and assigns new source"
    );
    let replaced = e.mapping_overrides.clone();
    let command = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    press(&mut e, r, g, egui::Key::Z, command);
    ensure!(
        e.mapping_overrides == saved,
        "One Undo restores both replaced overrides"
    );
    press(&mut e, r, g, egui::Key::Y, command);
    ensure!(
        e.mapping_overrides == replaced,
        "Ctrl+Y redo restores replacement"
    );
    action(&mut e, r, g, "undo");
    press(
        &mut e,
        r,
        g,
        egui::Key::Z,
        egui::Modifiers {
            shift: true,
            ..command
        },
    );
    ensure!(e.mapping_overrides == replaced, "Ctrl+Shift+Z redo");
    action(&mut e, r, g, "unmap");
    ensure!(
        e.mapping_overrides
            .iter()
            .any(|o| o.source == 8 && o.target.is_none()),
        "Unmap creates explicit None override"
    );
    action(&mut e, r, g, "restore-auto");
    ensure!(
        !e.mapping_overrides.iter().any(|o| o.source == 8),
        "Restore Auto removes override"
    );
    ensure!(
        e.mapping_links(&e.retarget.as_ref().unwrap().rows)
            .iter()
            .any(|l| l.source == 8 && l.target == Some(t8)),
        "Restore Auto immediately shows real automatic candidate"
    );
    // Search pickers are the precise pointer alternative to small projected bones.
    action(&mut e, r, g, "map");
    choose(&mut e, r, g, 0, 7);
    action(&mut e, r, g, "target-picker");
    action(&mut e, r, g, "picker-search");
    input(&mut e, r, g, vec![egui::Event::Text(t7.to_string())]);
    let option = e.input_regions.as_ref().unwrap().mapping_picker_options[&(1, t7)].center();
    click(&mut e, r, g, option);
    ensure!(
        selection(&e).source == Some(7) && selection(&e).target == Some(t7),
        "Search picker resolves original IDs"
    );
    press(&mut e, r, g, egui::Key::Enter, Default::default());
    ensure!(
        matches!(e.ui_state.correspondence.operation, Operation::Idle)
            && e.mapping_overrides
                .iter()
                .any(|o| o.source == 7 && o.target == Some(t7)),
        "Enter confirms focused ready panel"
    );
    // Idempotent confirmation does not dirty history or reorder overrides.
    let same = e.mapping_overrides.clone();
    action(&mut e, r, g, "map");
    choose(&mut e, r, g, 0, 7);
    choose(&mut e, r, g, 1, t7);
    action(&mut e, r, g, "confirm");
    ensure!(
        e.mapping_overrides == same,
        "Identical assignment is a no-op"
    );
    // Retire the closed floating area's previous-frame hit-test registration.
    input(&mut e, r, g, vec![]);
    action(&mut e, r, g, "filter");
    ensure!(
        e.ctx.text_edit_focused(),
        "Mapping filter click owns text focus: focused {:?}, filter {:?}, region {:?}",
        e.ctx.memory(|m| m.focused()),
        e.mapping_filter,
        e.input_regions.as_ref().unwrap().mapping_controls["filter"]
    );
    input(&mut e, r, g, vec![egui::Event::Text("m".into())]);
    press(&mut e, r, g, egui::Key::M, Default::default());
    press(&mut e, r, g, egui::Key::Z, command);
    ensure!(
        matches!(e.ui_state.correspondence.operation, Operation::Idle)
            && e.mapping_overrides == same,
        "Text editing suppresses mapping M/Undo: operation {:?}, before {:?}, after {:?}, filter {:?}, focused {:?}",
        e.ui_state.correspondence.operation,
        same,
        e.mapping_overrides,
        e.mapping_filter,
        e.ctx.memory(|m| m.focused())
    );
    e.mapping_filter.clear();
    choose(&mut e, r, g, 0, 7);
    press(&mut e, r, g, egui::Key::M, Default::default());
    ensure!(
        matches!(e.ui_state.correspondence.operation, Operation::Selecting(_)),
        "M opens outside text input"
    );
    press(&mut e, r, g, egui::Key::Escape, Default::default());
    action(&mut e, r, g, "apply");
    settle(&mut e, r, g)?;
    ensure!(
        e.retarget.as_ref().unwrap().overrides == same && !e.mapping_pending(7),
        "Apply compiles pending edits"
    );
    ensure!(
        e.ui_state.workspace == Workspace::Animate,
        "Apply & Preview reveals the target animation"
    );
    action(&mut e, r, g, "back-retarget");
    input(&mut e, r, g, vec![]);
    let save = e.input_regions.as_ref().unwrap().mapping_save.center();
    click(&mut e, r, g, save);
    settle(&mut e, r, g)?;
    // Compact proposal retains both canvases and reachable fields/actions.
    for scale in [1., 1.25, 1.5] {
        e.ctx.set_zoom_factor(scale);
        e.ui_state.correspondence.operation.begin(None, None);
        for _ in 0..3 {
            frame(
                &mut e,
                r,
                g,
                egui::vec2(1280. / scale, 720. / scale),
                vec![],
            );
        }
        let bounds = e.ctx.content_rect();
        let regions = e.input_regions.as_ref().unwrap();
        ensure!(
            bounds.contains_rect(regions.utility_windows["mapping-confirmation"]),
            "Confirmation fits compact scale {scale}"
        );
        for name in [
            "map",
            "apply",
            "source-field",
            "target-field",
            "cancel",
            "confirm",
        ] {
            ensure!(
                bounds.contains_rect(regions.mapping_controls[name]),
                "Reachable {name} at scale {scale}"
            );
        }
        ensure!(
            !regions.utility_windows["mapping-confirmation"].intersects(regions.mapping_canvas),
            "Nonmodal panel does not cover either skeleton canvas at {scale}: window {:?}, canvas {:?}",
            regions.utility_windows["mapping-confirmation"],
            regions.mapping_canvas
        );
        let size = egui::vec2(1280. / scale, 720. / scale);
        let at = joint(&e, 0, 8);
        click_sized(&mut e, r, g, size, at);
        let at = joint(&e, 1, t7);
        click_sized(&mut e, r, g, size, at);
        for _ in 0..3 {
            frame(&mut e, r, g, size, vec![]);
        }
        ensure!(
            !e.input_regions.as_ref().unwrap().mapping_confirm_enabled,
            "Compact conflict still requires explicit replacement"
        );
        for _ in 0..8 {
            let regions = e.input_regions.as_ref().unwrap();
            let body = regions.utility_scrolls["mapping-confirmation"].0;
            if body.contains(regions.mapping_controls["replace"].center()) {
                break;
            }
            frame(
                &mut e,
                r,
                g,
                size,
                vec![
                    egui::Event::PointerMoved(body.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., -100.),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: Default::default(),
                    },
                ],
            );
            for _ in 0..12 {
                frame(&mut e, r, g, size, vec![]);
            }
        }
        let regions = e.input_regions.as_ref().unwrap();
        let checkbox = regions.mapping_controls["replace"];
        ensure!(
            regions.utility_scrolls["mapping-confirmation"]
                .0
                .contains(checkbox.center()),
            "Replacement reachable by real wheel input at scale {scale}"
        );
        click_sized(&mut e, r, g, size, checkbox.center());
        let at = control(&e, "confirm");
        click_sized(&mut e, r, g, size, at);
        ensure!(
            matches!(e.ui_state.correspondence.operation, Operation::Idle)
                && e.mapping_overrides
                    .iter()
                    .any(|o| o.source == 8 && o.target == Some(t7)),
            "Compact confirm works at scale {scale}"
        );
        let at = control(&e, "undo");
        click_sized(&mut e, r, g, size, at);
        ensure!(
            e.mapping_overrides == same,
            "Compact undo restores grouped replacement at scale {scale}"
        );
    }
    drop(e);
    let mut reopened = Editor::new(Library::open(location.clone())?, context());
    reopened.input_regions = Some(InputRegions::default());
    reopened.request(&model, r, g);
    settle(&mut reopened, r, g)?;
    reopened.request(&clip, r, g);
    settle(&mut reopened, r, g)?;
    ensure!(
        reopened.mapping_overrides == same && reopened.plan_origin == "Saved profile",
        "Saved mappings survive restart"
    );
    ensure!(
        super::super::help::LEGEND
            .iter()
            .flat_map(|(_, rows)| *rows)
            .any(|(icon, name, _)| *icon == crate::ui::icons::Icon::MapJoints
                && *name == "Map Joints"),
        "Help uses actual application icons"
    );
    write_evidence(
        "mapping-mapping-interactions.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"result":"PASS","managed_library":location,"source":clip.hash,"target":model.hash,"source_node":7,"target_node":t7,"pointer_keyboard_cases":20,"os_pointer_input":false,"method":"Actual egui pointer/key events through Editor::ui; headless renderer initialized","limits":"OS SendInput not verified; Windows helper startup failed"}),
        )?,
    )?;
    println!(
        "Joint mapping workflows: pointer/key confirmation, conflicts, undo/redo, search, persistence and compact layouts: PASS"
    );
    Ok(())
}
