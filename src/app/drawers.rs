use super::*;
use crate::ui::drawers::Kind as Drawer;
impl Editor {
    pub(super) fn drawer_buttons(&mut self, ui: &mut egui::Ui, assets: bool, detail: bool) {
        for (show, kind, icon, name) in [
            (assets, Drawer::Assets, widgets::Icon::Assets, "Open Assets"),
            (
                detail,
                Drawer::Detail,
                widgets::Icon::Inspector,
                "Open Detail",
            ),
        ] {
            if !show {
                continue;
            }
            let button = widgets::icon(ui, icon, self.ui_state.drawer.open == Some(kind), name);
            self.ui_state.drawer.controls.push(button.rect);
            if let Some(r) = &mut self.input_regions {
                if kind == Drawer::Assets {
                    r.assets_drawer = button.rect;
                    r.assets_opener = Some(button.id);
                } else {
                    r.detail_drawer = button.rect;
                }
            }
            if button.clicked() && ui.ctx().current_pass_index() == 0 {
                self.ui_state.drawer.toggle(kind, button.id);
            }
        }
    }

    pub(super) fn drawer_ui(
        &mut self,
        ctx: &egui::Context,
        workspace: egui::Rect,
    ) -> Option<Record> {
        let kind = self.ui_state.drawer.open?;
        let width = 304f32.min((workspace.width() - 240.).max(200.));
        let top = (workspace.top()
            + if self.ui_state.workspace == Workspace::Retarget {
                116.
            } else {
                40.
            })
        .min(workspace.bottom() - 180.);
        let bottom = workspace.bottom()
            - if self.ui_state.workspace == Workspace::Retarget {
                if workspace.width() < 1100. {
                    134.
                } else {
                    174.
                }
            } else {
                0.
            };
        let x = if kind == Drawer::Assets {
            workspace.left()
        } else {
            workspace.right() - width
        };
        let rect = egui::Rect::from_min_max(
            egui::pos2(x, top),
            egui::pos2(x + width, bottom.max(top + 180.)),
        );
        self.ui_state.drawer.rect = rect;
        let mut selected = None;
        let mut close = false;
        egui::Area::new(egui::Id::unique("workspace-drawer"))
            .fade_in(false)
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .movable(false)
            .constrain(false)
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                ui.painter().rect_filled(rect, 0., theme::PANEL);
                ui.painter().rect_stroke(
                    rect,
                    0.,
                    egui::Stroke::new(1., theme::BORDER),
                    egui::StrokeKind::Inside,
                );
                let mut body = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("drawer-body")
                        .max_rect(rect.shrink(12.))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                body.set_clip_rect(rect.shrink(1.));
                body.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(if kind == Drawer::Assets {
                            "Assets"
                        } else {
                            "Detail"
                        })
                        .font(theme::font(18., 500)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let button =
                            widgets::glyph_button(ui, widgets::Icon::Close, "Close", false);
                        close = button.clicked();
                        if let Some(r) = &mut self.input_regions {
                            r.drawer_close = button.rect;
                        }
                    });
                });
                body.separator();
                if kind == Drawer::Assets {
                    selected = self.assets_ui(&mut body);
                } else {
                    self.inspector_ui(&mut body);
                }
            });
        if let Some(r) = &mut self.input_regions {
            r.drawer = rect;
        }
        if close {
            self.ui_state.drawer.close(ctx, true);
        }
        selected
    }
}
