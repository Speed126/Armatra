use super::*;

pub(super) fn scroll_body<R>(
    ui: &mut egui::Ui,
    name: &'static str,
    height: f32,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let id = ui.scope_id().with((name, "keyboard-offset"));
    let offset = ui.ctx().data_mut(|d| d.get_temp::<f32>(id)).unwrap_or(0.);
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), height));
    let focus = ui.interact(
        rect,
        ui.scope_id().with((name, "scroll-focus")),
        egui::Sense::click(),
    );
    if focus.clicked() {
        focus.request_focus();
    }
    let key = if focus.has_focus()
        || (ui.rect_contains_pointer(rect) && !ui.ctx().egui_wants_keyboard_input())
    {
        ui.input(|i| {
            if i.key_pressed(egui::Key::PageDown) {
                Some(offset + height * 0.8)
            } else if i.key_pressed(egui::Key::PageUp) {
                Some((offset - height * 0.8).max(0.))
            } else if i.key_pressed(egui::Key::ArrowDown) {
                Some(offset + 36.)
            } else if i.key_pressed(egui::Key::ArrowUp) {
                Some((offset - 36.).max(0.))
            } else {
                None
            }
        })
    } else {
        None
    };
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(name)
        .max_height(height);
    if let Some(offset) = key {
        scroll = scroll.vertical_scroll_offset(offset).animated(false);
    }
    let output = scroll.show(ui, body);
    ui.ctx()
        .data_mut(|d| d.insert_temp(id, output.state.clamped_offset().y));
    output
}

pub(super) fn shortcut(ui: &mut egui::Ui, action: &str, keys: &[&str]) {
    ui.label(egui::RichText::new(action).font(theme::font(12., 400)));
    ui.horizontal(|ui| {
        for key in keys {
            egui::Frame::new()
                .fill(theme::ELEVATED)
                .stroke(egui::Stroke::new(1., theme::BORDER))
                .corner_radius(3.)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(*key).monospace().size(11.));
                });
        }
    });
    ui.end_row();
}
impl Editor {
    pub(super) fn set_preview_placement(&mut self, continuous: bool) {
        self.ui_state.preferences.continuous = continuous;
        self.playback.cycle = 0;
        self.cursors.clear();
        self.request_contacts();
    }
    pub(super) fn loop_placement_ui(&mut self, ui: &mut egui::Ui, id: &str) {
        let old = self.ui_state.preferences.continuous;
        ui.add_enabled_ui(!self.plan_pending, |ui| {
            egui::ComboBox::from_id_salt(id).width(142.)
                .selected_text(if old { "Continuous travel" } else { "Repeat at origin" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.ui_state.preferences.continuous, false, "Repeat at origin");
                    ui.selectable_value(&mut self.ui_state.preferences.continuous, true, "Continuous locomotion");
                }).response.on_hover_text("Repeat preserves motion inside each cycle, including jumps and falls. Continuous adds completed-cycle root travel.");
        });
        if old != self.ui_state.preferences.continuous {
            self.set_preview_placement(self.ui_state.preferences.continuous);
        }
    }
    pub(super) fn utility_ui(&mut self, ctx: &egui::Context, gpu: &Gpu) {
        self.labs_ui(ctx, gpu);
        if std::env::var_os("ARMATRA_ICON_QA").is_some()
            && std::env::var_os("ARMATRA_CAPTURE").is_some()
        {
            self.icon_qa(ctx);
        }
        let area = ctx.content_rect().shrink(12.);
        let body_height = (area.height() - 120.).max(80.);
        let mut help = self.ui_state.help;
        let mut close = false;
        let help_window = egui::Window::new("Help & shortcuts")
            .fade_in(false)
            .fade_out(false)
            .open(&mut help)
            .default_width(600.)
            .default_height((area.height() - 32.).min(740.))
            .max_width(area.width())
            .constrain_to(area)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Armatra reference").font(theme::font(20., 500)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = ui.button("Close").clicked();
                    });
                });
                let scroll = scroll_body(ui, "help-body", body_height, super::help::contents);
                if let Some(r) = &mut self.input_regions {
                    r.utility_scrolls
                        .insert("help", (scroll.inner_rect, scroll.state.clamped_offset().y));
                }
            });
        if let (Some(window), Some(r)) = (help_window, &mut self.input_regions) {
            r.utility_windows.insert("help", window.response.rect);
        }
        self.ui_state.help = help && !close;
        let mut settings = self.ui_state.settings;
        close = false;
        let before = serde_json::to_vec(&self.ui_state.preferences).unwrap_or_default();
        let settings_window = egui::Window::new("Settings").fade_in(false).fade_out(false).open(&mut settings)
            .default_width(500.).max_width(area.width()).constrain_to(area).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Make Armatra yours").font(theme::font(20., 500)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { close = ui.button("Close").clicked(); });
                });
                let scroll = scroll_body(ui,"settings-body",body_height, |ui| {
                    widgets::section(ui, "APPEARANCE");
                    ui.label("Interface scale");
                    if ui.add(egui::Slider::new(&mut self.ui_state.preferences.scale, 0.85..=1.5).suffix("×")).changed() { ctx.set_zoom_factor(self.ui_state.preferences.scale); }
                    ui.add_space(12.); widgets::section(ui, "VIEWPORT DEFAULTS");
                    ui.checkbox(&mut self.ui_state.preferences.grid, "Ground grid");
                    ui.checkbox(&mut self.ui_state.preferences.axes, "Axis indicator");
                    ui.checkbox(&mut self.mesh, "Show mesh");
                    ui.checkbox(&mut self.skeleton, "Show skeleton");
                    ui.add_space(12.); widgets::section(ui, "PLAYBACK");
                    ui.label("Default speed");
                    ui.add(egui::Slider::new(&mut self.ui_state.preferences.playback_speed, -3. ..=3.).suffix("×"));
                    ui.checkbox(&mut self.ui_state.preferences.looping, "Loop new sessions");
                    self.loop_placement_ui(ui, "settings-placement");
                    if let Some(plan) = &self.retarget { ui.label(egui::RichText::new(format!("Saved profile policy: {:?}. Preview uses the placement above; profiles change only when saved in Retarget.", plan.settings.contacts.looping)).size(11.).color(theme::SECONDARY)); }
                    ui.add_space(12.); widgets::section(ui, "WORKSPACE");
                    for (label, value, range) in [
                        ("Assets panel", &mut self.ui_state.preferences.assets_width, 220. ..=400.),
                        ("Inspector", &mut self.ui_state.preferences.inspector_width, 240. ..=400.),
                        ("Timeline", &mut self.ui_state.preferences.timeline_height, 130. ..=240.),
                    ] { ui.add(egui::Slider::new(value, range).text(label)); }
                    ui.add_space(12.); widgets::section(ui, "LIBRARY / IMPORT");
                    ui.label("Managed library location");
                    ui.add(egui::Label::new(self.library.lock().expect("library").root.display().to_string()).selectable(true).wrap());
                    ui.label("Imports are copied into content-addressed storage. Original bundled motions are seeded once; deleting one keeps it deleted.");
                    ui.add_space(16.); ui.separator();
                    if ui.button("Restore sensible defaults").clicked() {
                        self.ui_state.preferences = Preferences::default(); self.mesh = true; self.skeleton = false;
                        ctx.set_zoom_factor(1.); self.playback.cycle = 0; self.request_contacts();
                    }
                });
                if let Some(r)=&mut self.input_regions {r.utility_scrolls.insert("settings",(scroll.inner_rect,scroll.state.clamped_offset().y));}
            });
        if let (Some(window), Some(r)) = (settings_window, &mut self.input_regions) {
            r.utility_windows.insert("settings", window.response.rect);
        }
        self.ui_state.settings = settings && !close;
        self.ui_state.preferences.mesh = self.mesh;
        self.ui_state.preferences.skeleton = self.skeleton;
        if before != serde_json::to_vec(&self.ui_state.preferences).unwrap_or_default()
            && let Err(error) = self
                .ui_state
                .preferences
                .save(&self.library.lock().expect("library").root)
        {
            self.status = format!("Settings save failed: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn focused_body_accepts_page_keys_without_pointer_hover() {
        let ctx = egui::Context::default();
        let mut offset = 0.;
        for step in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500., 240.),
                    )),
                    events: if step == 1 {
                        vec![
                            egui::Event::PointerMoved(egui::pos2(700., 400.)),
                            egui::Event::Key {
                                key: egui::Key::PageDown,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: Default::default(),
                            },
                        ]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                },
                |ui| {
                    if step == 1 {
                        ui.memory_mut(|m| {
                            m.request_focus(ui.scope_id().with(("focus-test", "scroll-focus")))
                        });
                    }
                    let result = super::scroll_body(ui, "focus-test", 120., |ui| {
                        for i in 0..40 {
                            ui.label(format!("Diagnostic line {i}"));
                        }
                    });
                    offset = result.state.clamped_offset().y;
                },
            );
            output.textures_delta.clear();
        }
        assert!(
            offset > 50.,
            "Focused scroll body must accept PageDown away from the pointer"
        );
    }
}
