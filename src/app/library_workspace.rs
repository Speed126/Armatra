use super::motion_browser::Action;
use super::*;
use crate::ui::icons::Icon;

fn region(ui: &mut egui::Ui, rect: egui::Rect, id: &str) -> egui::Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    )
}
impl Editor {
    pub(super) fn library_workspace(
        &mut self,
        ui: &mut egui::Ui,
        texture: egui::TextureId,
    ) -> ([u32; 2], Option<Action>) {
        let rect = ui.available_rect_before_wrap();
        let compact = rect.width() < 1100.;
        let error_height = if self.motion.preview_error.is_some() {
            48.
        } else {
            0.
        };
        let header = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(
                rect.width(),
                (if compact { 112. } else { 82. }) + error_height,
            ),
        );
        ui.painter().rect_filled(header, 0., theme::PANEL);
        let mut top = region(
            ui,
            egui::Rect::from_min_max(header.min, header.max - egui::vec2(0., error_height))
                .shrink2(egui::vec2(24., 12.)),
            "library-header",
        );
        let mut delete = None;
        top.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(180.);
                ui.label(egui::RichText::new("Motion library").font(theme::font(20., 500)));
                ui.label(
                    egui::RichText::new(format!(
                        "{} motions · {} similarity-ready",
                        self.records
                            .iter()
                            .filter(|r| matches!(r.kind, Kind::Clip(_)))
                            .count(),
                        self.motion.entries.len()
                    ))
                    .font(egui::FontId::monospace(11.))
                    .color(theme::SECONDARY),
                );
            });
            for (characters, icon, label) in [
                (false, Icon::Animate, "Motions"),
                (true, Icon::Mesh, "Characters"),
            ] {
                let button =
                    widgets::glyph_button(ui, icon, label, self.motion.characters == characters);
                if let Some(r) = &mut self.input_regions {
                    r.library_controls.insert(
                        if characters { "characters" } else { "motions" },
                        button.rect,
                    );
                }
                if button.clicked() {
                    self.motion.characters = characters;
                }
            }
            if !compact {
                ui.add_space(24.);
                let search = ui.add_sized(
                    egui::vec2(270., 32.),
                    egui::TextEdit::singleline(&mut self.motion.filter)
                        .hint_text("Find a motion…")
                        .margin(egui::vec2(10., 8.)),
                );
                if let Some(r) = &mut self.input_regions {
                    r.library_controls.insert("search", search.rect);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.drawer_buttons(ui,false,true);
                if let Some(id) = self.motion.selected_result.clone()
                    && self.records.iter().any(|r| r.id == id) {
                    let button = widgets::glyph_button(ui,Icon::Minus,"Delete motion",false)
                        .on_hover_text("Remove this library item. Original files are kept. Deleted bundled motions stay deleted.");
                    if let Some(r) = &mut self.input_regions { r.library_controls.insert("delete",button.rect); }
                    if button.clicked() { delete = Some(id); }
                }
                let refresh = widgets::glyph_button(ui, Icon::Automatic, "Refresh index", false);
                if let Some(r) = &mut self.input_regions {
                    r.library_controls.insert("refresh", refresh.rect);
                }
                if refresh.clicked() {
                    self.request_motion_index();
                }
                if self.motion.loading {
                    ui.spinner();
                }
                if !compact && ui.available_width() > 190. {
                    ui.label(
                        egui::RichText::new("Similar motions")
                            .font(theme::font(11., 500))
                            .color(theme::SECONDARY),
                    ).on_hover_text("Similarity compares motion descriptors and does not guarantee retarget compatibility.");
                }
            });
        });
        if let Some(id) = delete {
            self.delete_library_record(&id);
        }
        if compact {
            let search = widgets::search(
                &mut top,
                &mut self.motion.filter,
                "Find a motion or character…",
            );
            if let Some(r) = &mut self.input_regions {
                r.library_controls.insert("search", search.rect);
            }
        }
        if let Some(error) = &self.motion.preview_error {
            let mut feedback = region(
                ui,
                egui::Rect::from_min_max(
                    egui::pos2(header.left() + 24., header.bottom() - error_height),
                    header.right_bottom() - egui::vec2(24., 0.),
                ),
                "preview-error",
            );
            feedback.horizontal_wrapped(|ui| {
                ui.colored_label(theme::ERROR, error);
                if ui
                    .small_button("Diagnostics")
                    .on_hover_text("Open technical details and application log location")
                    .clicked()
                {
                    self.ui_state.labs = true;
                }
            });
        }
        let body = egui::Rect::from_min_max(header.left_bottom(), rect.max);
        let right_width = if compact {
            0.
        } else {
            (rect.width() * 0.3714).clamp(360., 540.)
        };
        let browser =
            egui::Rect::from_min_max(body.min, body.right_bottom() - egui::vec2(right_width, 0.));
        let mut list = region(ui, browser.shrink2(egui::vec2(10., 8.)), "library-browser");
        let mut action = if self.motion.characters {
            self.character_browser(&mut list)
        } else {
            self.motion_ui(&mut list)
        };
        let mut dimensions = [1, 1];
        if right_width > 0. {
            let preview = egui::Rect::from_min_max(browser.right_top(), body.max);
            let footer = egui::Rect::from_min_max(
                egui::pos2(preview.left(), preview.bottom() - 164.),
                preview.max,
            );
            let mut stage = region(
                ui,
                egui::Rect::from_min_max(preview.min, footer.right_top()),
                "library-stage",
            );
            let (d, record) = self.viewport_ui(&mut stage, texture);
            dimensions = d;
            if let Some(record) = record {
                action = Some(if record.kind == Kind::Model {
                    Action::SelectModel(record)
                } else {
                    Action::Preview(record, 0.)
                });
            }
            ui.painter().rect_filled(footer, 0., theme::PANEL);
            ui.painter().vline(
                preview.left(),
                preview.y_range(),
                egui::Stroke::new(1., theme::BORDER),
            );
            let mut meta = region(
                ui,
                footer.shrink2(egui::vec2(20., 16.)),
                "library-preview-meta",
            );
            if let Some((_, asset, index)) = &self.source {
                meta.label(
                    egui::RichText::new(if self.playback.playing {
                        "PREVIEWING"
                    } else {
                        "PREVIEW / PAUSED"
                    })
                    .font(theme::font(11., 500))
                    .color(theme::ORANGE),
                );
                meta.add(
                    egui::Label::new(
                        egui::RichText::new(&asset.clips[*index].name).font(theme::font(18., 500)),
                    )
                    .truncate(),
                )
                .on_hover_text(&asset.clips[*index].name);
                meta.label(
                    egui::RichText::new(format!(
                        "{:.2} s · {} animated channels",
                        asset.clips[*index].duration,
                        asset.clips[*index].tracks.len()
                    ))
                    .font(theme::font(11., 400))
                    .color(theme::SECONDARY),
                );
                if self.source_preview.is_some() && self.model.is_some() {
                    meta.label(egui::RichText::new("Original source · target needs mapping").font(theme::font(11.,400)).color(theme::SECONDARY)).on_hover_text("Similarity does not guarantee retarget compatibility. Choose Retarget to apply a mapping to the selected character; this preview shows the source motion.");
                }
            } else {
                meta.label("Choose a motion to preview");
            }
            meta.add_space(12.);
            if widgets::glyph_button(&mut meta, Icon::Animate, "Open in Animate", false).clicked() {
                action = Some(Action::OpenAnimate);
            }
        } else {
            let mut action_ui = region(
                ui,
                egui::Rect::from_min_size(
                    egui::pos2(header.right() - 180., header.bottom() - 42.),
                    egui::vec2(160., 32.),
                ),
                "library-compact-open",
            );
            if widgets::glyph_button(&mut action_ui, Icon::Animate, "Open in Animate", false)
                .clicked()
            {
                action = Some(Action::OpenAnimate);
            }
        }
        (dimensions, action)
    }
    fn character_browser(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut selected = None;
        let records: Vec<_> = self
            .records
            .iter()
            .filter(|r| {
                r.kind == Kind::Model
                    && r.name
                        .to_lowercase()
                        .contains(&self.motion.filter.to_lowercase())
            })
            .cloned()
            .collect();
        let columns = (ui.available_width() / 176.).floor().max(1.) as usize;
        egui::ScrollArea::vertical()
            .id_salt("character-grid")
            .show(ui, |ui| {
                for row in records.chunks(columns) {
                    ui.horizontal(|ui| {
                        for record in row {
                            let (rect, response) = ui
                                .allocate_exact_size(egui::vec2(168., 190.), egui::Sense::click());
                            let active = self.model.as_ref().is_some_and(|m| m.0 == record.hash);
                            if active || response.hovered() {
                                ui.painter().rect_filled(rect, 3., theme::ELEVATED);
                            }
                            if active {
                                ui.painter().rect_stroke(
                                    rect,
                                    3.,
                                    egui::Stroke::new(1., theme::TEXT),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if let Some(texture) = self.thumbnails.get(&record.hash) {
                                ui.painter().image(
                                    texture.id(),
                                    egui::Rect::from_min_size(
                                        rect.min + egui::vec2(20., 8.),
                                        egui::vec2(128., 128.),
                                    ),
                                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                    egui::Color32::WHITE,
                                );
                            }
                            let p = ui.painter_at(rect.shrink2(egui::vec2(6., 0.)));
                            p.text(
                                rect.left_top() + egui::vec2(10., 148.),
                                egui::Align2::LEFT_TOP,
                                &record.name,
                                theme::font(13., 500),
                                theme::TEXT,
                            );
                            p.text(
                                rect.left_top() + egui::vec2(10., 169.),
                                egui::Align2::LEFT_TOP,
                                if active {
                                    "Active character"
                                } else {
                                    "Use character"
                                },
                                theme::font(11., 400),
                                theme::SECONDARY,
                            );
                            if response.clicked() {
                                selected = Some(Action::SelectModel(record.clone()));
                            }
                            response.on_hover_text(&record.name);
                            if let Some(r) = &mut self.input_regions {
                                r.records.insert(record.id.clone(), rect);
                            }
                        }
                    });
                }
            });
        selected
    }
}
