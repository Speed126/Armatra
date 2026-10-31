//! Capture-only input playback. Never assigns skins, overrides, plans or outcomes.
use super::*;
#[derive(Clone, Debug)]
enum Point {
    Control(&'static str),
    Skin(usize),
    Joint(usize, usize),
    PickerRow(usize, usize),
    Time(f32),
}
#[derive(Clone, Debug)]
enum Step {
    Click(Point),
    Text(String),
    Wait(u32),
    Idle,
}
pub(super) struct Driver {
    name: String,
    steps: Vec<Step>,
    at: usize,
    pressed: bool,
    wait: u32,
    frames: u32,
    click_at: Option<egui::Pos2>,
    stable_at: Option<(usize, egui::Pos2, u32)>,
    pub trace: Vec<serde_json::Value>,
}
impl Driver {
    pub fn new() -> Result<Option<Self>> {
        let Ok(name) = std::env::var("ARMATRA_APPLY_SCENARIO") else {
            return Ok(None);
        };
        let pairs: Vec<(usize, Option<usize>)> = serde_json::from_str(
            &std::env::var("ARMATRA_APPLY_PAIRS").unwrap_or("[[6,5],[11,0]]".into()),
        )?;
        let mut steps = vec![];
        use Point::*;
        use Step::*;
        if name.starts_with("restored") {
            if name == "restored-preview" {
                steps.extend([
                    Click(Control("apply")),
                    Idle,
                    Wait(3),
                    Click(Control("play")),
                    Click(Time(0.75)),
                    Wait(3),
                ]);
            }
        } else {
            // Confirm edits before the missing-skin failure; recovery must retain them.
            for (source, target) in pairs {
                let Some(target) = target else {
                    steps.extend([Click(Joint(0, source)), Click(Control("unmap")), Wait(2)]);
                    continue;
                };
                steps.extend([
                    Click(Control("map")),
                    Wait(2),
                    Click(Joint(0, source)),
                    Click(Control("target-picker")),
                    Click(Control("picker-search")),
                    Text(target.to_string()),
                    Wait(2),
                    Click(PickerRow(1, target)),
                    Wait(2),
                    Click(Control("confirm")),
                    Wait(2),
                ]);
            }
            if std::env::var_os("ARMATRA_APPLY_SINGLE_SKIN").is_none() {
                steps.extend([Click(Control("apply")), Wait(3)]);
            }
            if name != "missing" {
                steps.extend([
                    Click(Control("target-skin")),
                    Wait(2),
                    Click(Skin(0)),
                    Wait(3),
                ]);
                if name != "pending" {
                    steps.push(Click(Control("apply")));
                    if name != "applying" {
                        steps.extend([Idle, Wait(3)]);
                        match name.as_str() {
                            "applied" => steps.extend([Click(Control("back-retarget")), Wait(3)]),
                            "pose-a" | "pose-b" | "saved" => {
                                steps.extend([
                                    Click(Control("play")),
                                    Click(Time(if name == "pose-b" { 0.75 } else { 0. })),
                                    Wait(3),
                                ]);
                                if name == "saved" {
                                    steps.extend([Click(Control("preview-save")), Idle, Wait(3)]);
                                }
                            }
                            _ => anyhow::bail!("Unknown Apply capture scenario {name}"),
                        }
                    }
                }
            }
        }
        Ok(Some(Self {
            name,
            steps,
            at: 0,
            pressed: false,
            wait: 0,
            frames: 0,
            click_at: None,
            stable_at: None,
            trace: vec![],
        }))
    }
    pub fn progress_capture(&self) -> bool {
        self.name == "applying"
    }
    pub fn drive(&mut self, e: &Editor, input: &mut egui::RawInput, ready: u32) -> Result<bool> {
        self.frames += 1;
        anyhow::ensure!(
            self.frames < 2400,
            "Apply native input timed out at {}: {}",
            self.at,
            e.status
        );
        if ready < 14 {
            return Ok(false);
        }
        let Some(step) = self.steps.get(self.at).cloned() else {
            return Ok(true);
        };
        match step {
            Step::Wait(count) => {
                self.wait += 1;
                if self.wait >= count {
                    self.at += 1;
                    self.wait = 0;
                }
            }
            Step::Idle => {
                if !e.plan_pending && e.jobs == 0 {
                    anyhow::ensure!(
                        e.retarget.is_some(),
                        "Real native apply failed: {}",
                        e.status
                    );
                    self.at += 1;
                }
            }
            Step::Text(text) => {
                anyhow::ensure!(
                    e.ctx.text_edit_focused(),
                    "Native joint search did not acquire text focus"
                );
                input.events.push(egui::Event::Text(text));
                self.at += 1;
            }
            Step::Click(point) => {
                if self.wait > 0 {
                    self.wait -= 1;
                    return Ok(false);
                }
                let r = e.input_regions.as_ref().context("Apply input geometry")?;
                let mut at = match point {
                    Point::Control("play") => r.play.center(),
                    Point::Control(name) => r
                        .mapping_controls
                        .get(name)
                        .with_context(|| {
                            format!("Missing {name} at step {}: {}", self.at, e.status)
                        })?
                        .center(),
                    Point::Skin(index) => r
                        .skin_options
                        .get(&(1, Some(index)))
                        .context("Target skin choice")?
                        .center(),
                    Point::Joint(side, node) => *r
                        .mapping_joints
                        .get(&(side, node))
                        .context("Source joint geometry")?,
                    Point::PickerRow(side, node) => r
                        .mapping_picker_options
                        .get(&(side, node))
                        .context("Searchable target joint")?
                        .center(),
                    Point::Time(time) => {
                        let duration = e.source.as_ref().context("Source clip")?.1.clips
                            [e.source.as_ref().unwrap().2]
                            .duration;
                        egui::pos2(
                            r.seek.left() + r.seek.width() * (time / duration),
                            r.seek.center().y,
                        )
                    }
                };
                if !self.pressed {
                    let count = self
                        .stable_at
                        .filter(|(step, previous, _)| {
                            *step == self.at && previous.distance(at) < 0.2
                        })
                        .map_or(0, |(_, _, n)| n + 1);
                    self.stable_at = Some((self.at, at, count));
                    if count < 2 {
                        return Ok(false);
                    }
                } else if let Some(pressed_at) = self.click_at {
                    at = pressed_at;
                }
                // Compact pickers live in the real scrolling panel. Reveal
                // their row with pointer wheel input instead of clicking a
                // laid-out but clipped row (which could hit Cancel underneath).
                let scroll_body = match point {
                    Point::PickerRow(..) | Point::Control("picker-search") => {
                        Some("mapping-confirmation")
                    }
                    Point::Control("unmap" | "restore-auto" | "filter") => Some("mapping-detail"),
                    _ => None,
                };
                if let Some(name) = scroll_body
                    && let Some((body, _)) = r.utility_scrolls.get(name)
                    && !body.contains(at)
                {
                    input.events.extend([
                        egui::Event::PointerMoved(body.left_top() + egui::vec2(8., 8.)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0., if at.y > body.bottom() { -85. } else { 85. }),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            modifiers: Default::default(),
                        },
                    ]);
                    self.wait = 12;
                    return Ok(false);
                }
                if matches!(point, Point::Control("confirm")) {
                    anyhow::ensure!(
                        r.mapping_confirm_enabled,
                        "Native proposal not ready: {:?}",
                        e.mapping_evidence()
                    );
                }
                if matches!(point, Point::PickerRow(..))
                    && let Some(body) = r.mapping_picker_clip
                    && !body.contains(at)
                {
                    let visible = r
                        .utility_scrolls
                        .get("mapping-confirmation")
                        .map_or(body, |(outer, _)| body.intersect(*outer));
                    input.events.extend([
                        egui::Event::PointerMoved(visible.center()),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0., if at.y > body.bottom() { -85. } else { 85. }),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            modifiers: Default::default(),
                        },
                    ]);
                    self.wait = 12;
                    return Ok(false);
                }
                self.pressed = !self.pressed;
                self.click_at = if self.pressed { Some(at) } else { None };
                input.events.extend([
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: self.pressed,
                        modifiers: Default::default(),
                    },
                ]);
                self.trace.push(serde_json::json!({"step":self.at,"action":format!("{point:?}"),"pressed":self.pressed,"position":[at.x,at.y],"phase_before":e.mapping_feedback.phase,"generation":e.plan_generation.0,"selection_before":e.mapping_evidence(),"confirm_enabled":r.mapping_confirm_enabled}));
                if !self.pressed {
                    self.at += 1;
                    if self.at == self.steps.len() && self.progress_capture() {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}
