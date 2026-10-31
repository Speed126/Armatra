//! Capture-only egui event playback. This is not operating-system pointer input.
use super::*;

enum Point {
    Joint(usize, usize),
    Control(&'static str),
}
enum Action {
    Wait,
    Press(Point, egui::PointerButton),
    Release(Point, egui::PointerButton),
    Move(Point),
}
pub(super) fn drive(editor: &Editor, input: &mut egui::RawInput, ready: u32) -> Result<bool> {
    let Ok(scenario) = std::env::var("ARMATRA_CAPTURE_MAPPING_SCENARIO") else {
        return Ok(true);
    };
    if ready < 14 {
        return Ok(false);
    }
    // Diagnostic input only: retain the unmodified Preview/render path.
    if scenario == "library-preview" || scenario == "library-medoid" {
        let name = std::env::var("ARMATRA_LIBRARY_TEST_MOTION").unwrap_or("Wave".into());
        let record = editor
            .records
            .iter()
            .rev()
            .find(|r| {
                r.name.split(" · ").next() == Some(name.as_str()) && matches!(r.kind, Kind::Clip(_))
            })
            .context("Reproduction motion record")?;
        let r = editor
            .input_regions
            .as_ref()
            .context("Reproduction input regions")?;
        let point = if scenario == "library-medoid" {
            r.motion_medoid_previews
                .iter()
                .find(|((id, _), _)| id == &record.id)
                .map(|(_, (rect, _))| rect.center())
                .context("Reproduction medoid")?
        } else {
            r.motion_preview_buttons
                .get(&record.id)
                .context("Reproduction Preview button")?
                .center()
        };
        if ready == 14 || ready == 15 {
            eprintln!(
                "Library before Preview event ready={ready}, record={}, clip={:?}, source={:?}, resident={:?}, target={:?}, skins={:?}/{:?}, active={:?}",
                record.id,
                record.kind,
                editor.source.as_ref().map(|s| (&s.0, s.2)),
                editor.source_preview.as_ref().map(|p| &p.key),
                editor.model.as_ref().map(|m| &m.0),
                editor.mapping_settings.source_skin,
                editor.mapping_settings.target_skin,
                editor.active_pair
            );
            input.events.push(egui::Event::PointerMoved(point));
            input.events.push(egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: ready == 14,
                modifiers: Default::default(),
            });
        }
        return Ok(ready > 20);
    }
    use Action::*;
    use Point::*;
    use egui::PointerButton::{Middle, Primary};
    let source = std::env::var("ARMATRA_CAPTURE_MAPPING_SOURCE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);
    let target = std::env::var("ARMATRA_CAPTURE_MAPPING_TARGET")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);
    let drag = || {
        vec![
            Press(Joint(0, source), Middle),
            Move(Joint(1, target)),
            Release(Joint(1, target), Middle),
        ]
    };
    let actions = match scenario.as_str() {
        "workspace" => vec![],
        "apply-options" => vec![
            Press(Control("apply-menu"), Primary),
            Release(Control("apply-menu"), Primary),
        ],
        "dragging" => vec![Press(Joint(0, source), Middle), Move(Joint(1, target))],
        "drag-confirm" => drag(),
        "button-empty" => vec![
            Press(Control("map"), Primary),
            Release(Control("map"), Primary),
        ],
        "button-filled" => vec![
            Press(Control("map"), Primary),
            Release(Control("map"), Primary),
            Press(Joint(0, source), Primary),
            Release(Joint(0, source), Primary),
            Press(Joint(1, target), Primary),
            Release(Joint(1, target), Primary),
        ],
        "pending" => {
            let mut a = drag();
            a.extend([
                Wait,
                Wait,
                Press(Control("confirm"), Primary),
                Release(Control("confirm"), Primary),
            ]);
            a
        }
        "conflict" => {
            let mut a = drag();
            a.extend([
                Wait,
                Wait,
                Press(Control("confirm"), Primary),
                Release(Control("confirm"), Primary),
                Press(Joint(0, 8), Middle),
                Move(Joint(1, target)),
                Release(Joint(1, target), Middle),
            ]);
            a
        }
        _ => anyhow::bail!("Unknown mapping capture scenario: {scenario}"),
    };
    let step = (ready - 14) as usize;
    let Some(action) = actions.get(step) else {
        return Ok(step >= actions.len() + 3);
    };
    if matches!(action, Wait) {
        return Ok(false);
    }
    let (point, button, pressed) = match action {
        Press(p, b) => (p, Some(*b), true),
        Release(p, b) => (p, Some(*b), false),
        Move(p) => (p, None, false),
        Wait => unreachable!(),
    };
    let r = editor
        .input_regions
        .as_ref()
        .context("Mapping capture input regions")?;
    let at = match point {
        Joint(side, node) => *r
            .mapping_joints
            .get(&(*side, *node))
            .context("Mapping capture joint")?,
        Control(name) => r
            .mapping_controls
            .get(name)
            .context("Mapping capture control")?
            .center(),
    };
    input.events.push(egui::Event::PointerMoved(at));
    if let Some(button) = button {
        input.events.push(egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Default::default(),
        });
    }
    Ok(false)
}
