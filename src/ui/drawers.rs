//! Foreground view state; never serialized with normal panel preferences.
use egui::{Context, Id, Rect};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Assets,
    Detail,
}
pub struct State {
    pub open: Option<Kind>,
    pub rect: Rect,
    pub opener: Option<Id>,
    pub controls: Vec<Rect>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: None,
            rect: Rect::NOTHING,
            opener: None,
            controls: Vec::new(),
        }
    }
}
impl State {
    pub fn toggle(&mut self, kind: Kind, opener: Id) {
        if self.open == Some(kind) {
            self.open = None;
        } else {
            self.open = Some(kind);
        }
        self.opener = Some(opener);
    }
    pub fn close(&mut self, ctx: &Context, restore_focus: bool) {
        self.open = None;
        if restore_focus && let Some(id) = self.opener {
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }
    /// Run before allocating any workspace hit regions. Dismissal consumes the
    /// pointer state for this pass, including the press that would start orbit.
    pub fn begin(&mut self, ctx: &Context) {
        if self.open.is_none() || ctx.current_pass_index() != 0 {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.close(ctx, true);
            return;
        }
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| {
                    !self.rect.contains(p) && !self.controls.iter().any(|r| r.contains(p))
                })
        });
        if outside {
            self.close(ctx, false);
            ctx.input_mut(|i| {
                i.pointer = Default::default();
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::PointerButton { .. }
                            | egui::Event::PointerMoved(_)
                            | egui::Event::MouseWheel { .. }
                    )
                });
            });
        }
    }
}
/// Explicit resize handles change saved preferences only through user input.
/// A constrained panel's measured size never feeds back into persistence.
pub fn resize(
    ui: &mut egui::Ui,
    id: &str,
    rect: Rect,
    value: &mut f32,
    vertical: bool,
    direction: f32,
    range: std::ops::RangeInclusive<f32>,
) {
    let response = ui
        .interact(rect, Id::unique(id), egui::Sense::drag())
        .on_hover_cursor(if vertical {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::ResizeHorizontal
        });
    if response.dragged() && ui.ctx().current_pass_index() == 0 {
        let delta = ui.input(|i| {
            if vertical {
                i.pointer.delta().y
            } else {
                i.pointer.delta().x
            }
        });
        *value = (*value + delta * direction).clamp(*range.start(), *range.end());
    }
    if response.hovered() || response.dragged() {
        ui.painter().rect_filled(rect, 0., super::theme::ORANGE);
    }
}
