//! Native pointer/key playback. No direct source/target/pose/plan assignments.
use super::*;
#[derive(Clone, Debug)]
enum Point {
    Control(&'static str),
    Preview(String, bool),
    Model(String),
    Row(String),
}
#[derive(Clone, Debug)]
enum Step {
    Click(Point),
    Text(String),
    Wait(u32),
    Displayed(String),
    Idle,
}
pub(super) struct Driver {
    mode: String,
    steps: Vec<Step>,
    at: usize,
    pressed: bool,
    point: Option<egui::Pos2>,
    wait: u32,
    frames: u32,
    scroll_ticks: u32,
    before: u64,
    pub trace: Vec<serde_json::Value>,
}
impl Driver {
    pub fn new() -> Option<Self> {
        std::env::var("ARMATRA_LIBRARY_TEST_SCENARIO")
            .ok()
            .map(|mode| Self {
                mode,
                steps: vec![],
                at: 0,
                pressed: false,
                point: None,
                wait: 0,
                frames: 0,
                scroll_ticks: 0,
                before: 0,
                trace: vec![],
            })
    }
    fn filter(steps: &mut Vec<Step>, text: String) {
        steps.extend([
            Step::Click(Point::Control("search")),
            Step::Text(text),
            Step::Wait(3),
        ]);
    }
    fn motion_name(_e: &Editor, r: &Record) -> String {
        r.name.clone()
    }
    fn init(&mut self, e: &Editor) -> Result<()> {
        if self.mode.starts_with("repro-") {
            let name = std::env::var("ARMATRA_LIBRARY_TEST_MOTION").unwrap_or("Wave".into());
            let record = e
                .records
                .iter()
                .rev()
                .find(|r| {
                    r.name.split(" · ").next() == Some(name.as_str())
                        && matches!(r.kind, Kind::Clip(_))
                })
                .context("Native reproduction record")?;
            Self::filter(&mut self.steps, record.name.clone());
            self.steps.extend([
                Step::Click(Point::Preview(
                    record.id.clone(),
                    self.mode == "repro-medoid",
                )),
                Step::Displayed(record.id.clone()),
                Step::Wait(4),
                Step::Idle,
            ]);
            return Ok(());
        }
        let mut clips: Vec<_> = e
            .records
            .iter()
            .filter(|r| matches!(r.kind, Kind::Clip(_)))
            .cloned()
            .collect();
        clips.sort_by(|a, b| a.id.cmp(&b.id));
        if self.mode != "stress" {
            // Short regression scenarios must still exercise their explicitly
            // imported multi-clip fixture after automatic first-run seeding.
            // The stress scenario intentionally includes the entire library.
            let library = e.library.lock().expect("library");
            let imported: Vec<_> = clips
                .iter()
                .filter(|r| {
                    !library
                        .manifest
                        .bundled_completed
                        .values()
                        .any(|hash| hash == &r.hash)
                })
                .cloned()
                .collect();
            if imported.len() >= 2 {
                clips = imported;
            }
        }
        anyhow::ensure!(clips.len() >= 2, "Native test requires multiple motions");
        let targets: Vec<_> = e
            .records
            .iter()
            .filter(|r| r.kind == Kind::Model)
            .take(4)
            .cloned()
            .collect();
        let count = if self.mode == "stress" { 100 } else { 2 };
        let mut last = e.actual_motion_id().unwrap_or_default().to_string();
        for i in 0..count {
            if self.mode == "stress" && i % 20 == 0 && !targets.is_empty() {
                let target = &targets[(i / 20) % targets.len()];
                self.steps.push(Step::Click(Point::Control("characters")));
                Self::filter(&mut self.steps, target.name.clone());
                self.steps.extend([
                    Step::Click(Point::Model(target.id.clone())),
                    Step::Idle,
                    Step::Click(Point::Control("motions")),
                ]);
            }
            if i > 0 && i % 25 == 0 {
                self.steps
                    .extend([Step::Click(Point::Control("refresh")), Step::Idle]);
            }
            let mut index = i % clips.len();
            if clips[index].id == last {
                index = (index + 1) % clips.len();
            }
            let record = &clips[index];
            last = record.id.clone();
            Self::filter(&mut self.steps, Self::motion_name(e, record));
            if i == 0 && e.motion.entries.iter().any(|v| v.id == record.id) {
                self.steps
                    .extend([Step::Click(Point::Row(record.id.clone())), Step::Wait(3)]);
            }
            let medoid = i % 2 == 1 && e.motion.entries.iter().any(|v| v.id == record.id);
            self.steps.extend([
                Step::Click(Point::Preview(record.id.clone(), medoid)),
                Step::Displayed(record.id.clone()),
                Step::Wait(3),
            ]);
            if self.mode == "stress" && i % 20 == 19 {
                self.steps.extend([
                    Step::Click(Point::Control("animate")),
                    Step::Wait(4),
                    Step::Click(Point::Control("library")),
                    Step::Wait(3),
                ]);
            }
        }
        self.steps.push(Step::Idle);
        Ok(())
    }
    pub fn drive(&mut self, e: &Editor, input: &mut egui::RawInput, ready: u32) -> Result<bool> {
        self.frames += 1;
        anyhow::ensure!(
            self.frames < 18000,
            "Native Library input timed out at step {}: {}",
            self.at,
            e.status
        );
        if ready < 14 {
            return Ok(false);
        }
        if self.steps.is_empty() {
            self.init(e)?;
        }
        let Some(step) = self.steps.get(self.at).cloned() else {
            return Ok(true);
        };
        match step {
            Step::Wait(n) => {
                self.wait += 1;
                if self.wait >= n {
                    self.at += 1;
                    self.wait = 0;
                }
            }
            Step::Idle => {
                if e.jobs == 0 && !e.motion.loading {
                    self.at += 1;
                }
            }
            Step::Displayed(id) => {
                if e.motion.displayed_count > self.before {
                    anyhow::ensure!(
                        e.actual_motion_id() == Some(id.as_str())
                            && e.playback.playing
                            && e.motion.pending_preview.is_none(),
                        "Displayed wrong/unready preview"
                    );
                    anyhow::ensure!(
                        e.motion
                            .displayed
                            .last()
                            .is_some_and(|f| f["frame_proof"].is_object()),
                        "Native activation must produce actual GPU frame readback"
                    );
                    if let Some(pair) = &e.active_pair {
                        anyhow::ensure!(
                            e.source
                                .as_ref()
                                .is_some_and(|s| s.0 == pair.0 && s.2 == pair.2)
                                && e.model.as_ref().is_some_and(|m| m.0 == pair.1),
                            "Native stale active plan"
                        );
                    }
                    self.trace.push(serde_json::json!({"step":self.at,"displayed":e.preview_identity(),"count":e.motion.displayed_count}));
                    self.at += 1;
                }
            }
            Step::Text(text) => {
                anyhow::ensure!(
                    e.ctx.egui_wants_keyboard_input(),
                    "Native Library search must own focus"
                );
                let modifiers = egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                };
                for pressed in [true, false] {
                    input.events.push(egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers,
                    });
                }
                input.events.push(egui::Event::Text(text.clone()));
                self.trace
                    .push(serde_json::json!({"step":self.at,"text":text}));
                self.at += 1;
            }
            Step::Click(point) => {
                let regions = e
                    .input_regions
                    .as_ref()
                    .context("Native Library input regions")?;
                let at = if self.pressed {
                    Some(self.point.context("Native pointer press")?)
                } else {
                    match &point {
                        Point::Control("animate") => Some(regions.animate_open.center()),
                        Point::Control("library") => Some(regions.motion_open.center()),
                        Point::Control(name) => Some(
                            regions
                                .library_controls
                                .get(name)
                                .context("Native Library control")?
                                .center(),
                        ),
                        Point::Model(id) => regions.records.get(id).map(|r| r.center()),
                        Point::Row(id) => regions
                            .motion_results
                            .get(id)
                            .map(|r| r.left_top() + egui::vec2(190., 20.)),
                        Point::Preview(id, true) => regions
                            .motion_medoid_previews
                            .iter()
                            .find(|((r, _), _)| r == id)
                            .map(|(_, v)| v.0.center()),
                        Point::Preview(id, false) => {
                            regions.motion_preview_buttons.get(id).map(|r| r.center())
                        }
                    }
                };
                let motion = matches!(point, Point::Row(_) | Point::Preview(_, _));
                let at =
                    at.filter(|at| self.pressed || !motion || regions.motion_scroll.contains(*at));
                let Some(at) = at else {
                    let waiting_texture = matches!(&point,Point::Preview(id,true) if !regions.motion_medoid_previews.keys().any(|(row,_)|row==id) && regions.motion_results.get(id).is_some_and(|r|r.height()>90.));
                    if motion && !waiting_texture && regions.motion_scroll.is_positive() {
                        // Short titles can legitimately match several imports.
                        // Reach their real visible controls with wheel input.
                        let delta = if self.scroll_ticks.is_multiple_of(200) {
                            10000.
                        } else {
                            -96.
                        };
                        input
                            .events
                            .push(egui::Event::PointerMoved(regions.motion_scroll.center()));
                        input.events.push(egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            delta: egui::vec2(0., delta),
                            modifiers: Default::default(),
                        });
                        self.scroll_ticks += 1;
                        self.trace.push(serde_json::json!({"step":self.at,"wheel_points":delta,"control":format!("{point:?}")}));
                    }
                    return Ok(false);
                };
                self.scroll_ticks = 0;
                if !self.pressed {
                    self.before = e.motion.displayed_count;
                    self.point = Some(at);
                }
                self.pressed = !self.pressed;
                input.events.extend([
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: self.pressed,
                        modifiers: Default::default(),
                    },
                ]);
                self.trace.push(serde_json::json!({"step":self.at,"control":format!("{point:?}"),"pressed":self.pressed,"position":[at.x,at.y],"before":e.preview_identity()}));
                if !self.pressed {
                    self.at += 1;
                    self.point = None;
                }
            }
        }
        Ok(false)
    }
}
