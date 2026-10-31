use super::*;

impl Editor {
    pub(super) fn load_asset_thumbnails(&mut self) {
        let root = self.library.lock().expect("library").root.clone();
        for record in self
            .records
            .iter()
            .filter(|r| r.kind == Kind::Model)
            .take(128)
        {
            let path = root
                .join("thumbnails")
                .join(format!("{}-v1.png", record.hash));
            if image::image_dimensions(&path).is_ok_and(|(w, h)| w <= 256 && h <= 256)
                && let Ok(image) = image::open(path).map(|i| i.into_rgba8())
            {
                self.thumbnails.insert(
                    record.hash.clone(),
                    self.ctx.load_texture(
                        &record.hash,
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width() as usize, image.height() as usize],
                            image.as_raw(),
                        ),
                        egui::TextureOptions::LINEAR,
                    ),
                );
            }
        }
    }

    pub(super) fn assets_ui(&mut self, ui: &mut egui::Ui) -> Option<Record> {
        let mut selected = None;
        let mut delete = None;
        if self.ui_state.drawer.open != Some(crate::ui::drawers::Kind::Assets) {
            ui.heading("Assets");
        }
        let drawer = self.ui_state.drawer.open == Some(crate::ui::drawers::Kind::Assets);
        if !drawer {
            ui.label(
                egui::RichText::new("Your collection, always local")
                    .font(theme::font(11., 400))
                    .color(theme::SECONDARY),
            );
            ui.add_space(8.);
        }
        let search_response = widgets::search(ui, &mut self.search, "Search your collection…");
        if let Some(r) = &mut self.input_regions {
            r.assets_search = search_response.rect;
        }
        ui.horizontal(|ui| {
            for (value, label) in [
                (Collection::All, "All"),
                (Collection::Models, "Models"),
                (Collection::Motions, "Motions"),
            ] {
                if widgets::nav(ui, label, self.ui_state.collection == value).clicked() {
                    self.ui_state.collection = value;
                }
            }
        });
        ui.add_space(10.);
        let search = self.search.to_lowercase();
        let records: Vec<_> = self
            .records
            .iter()
            .filter(|r| {
                r.name.to_lowercase().contains(&search)
                    && match self.ui_state.collection {
                        Collection::All => true,
                        Collection::Models => r.kind == Kind::Model,
                        Collection::Motions => matches!(r.kind, Kind::Clip(_)),
                    }
            })
            .cloned()
            .collect();
        if !drawer {
            ui.label(
                egui::RichText::new(format!("{} ASSETS", records.len()))
                    .font(theme::font(11., 400))
                    .color(theme::SECONDARY),
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("asset-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if records.is_empty() {
                    if self.records.is_empty() {
                        widgets::empty(
                            ui,
                            "Build your collection",
                            "Drop a GLB here, or use Import to add characters and motions.",
                        );
                        ui.label("Demo character: assets/demo/RobotExpressive.glb");
                    } else {
                        widgets::empty(
                            ui,
                            "No matching assets",
                            "Try another name or collection filter.",
                        );
                        if ui.button("Clear search").clicked() {
                            self.search.clear();
                            self.ui_state.collection = Collection::All;
                        }
                    }
                }
                for record in &records {
                    let active = match record.kind {
                        Kind::Model => self.model.as_ref().is_some_and(|m| m.0 == record.hash),
                        Kind::Clip(i) => self
                            .source
                            .as_ref()
                            .is_some_and(|s| s.0 == record.hash && s.2 == i),
                    };
                    let loading = self.pending.keys().any(|(h, _)| h == &record.hash);
                    let failed = self.failures.contains(&record.hash);
                    let metadata = if loading {
                        "Loading…".into()
                    } else if failed {
                        "Could not load · click to retry".into()
                    } else {
                        match record.kind {
                            Kind::Model => {
                                self.cpu
                                    .peek(&record.hash)
                                    .map_or("Character · GLB".into(), |a| {
                                        format!(
                                            "{} joints · {} meshes",
                                            a.skins.iter().map(|s| s.joints.len()).sum::<usize>(),
                                            a.meshes.len()
                                        )
                                    })
                            }
                            Kind::Clip(i) => self
                                .cpu
                                .peek(&record.hash)
                                .and_then(|a| a.clips.get(i))
                                .map_or("Motion · GLB".into(), |c| {
                                    format!("{:.2} s · {} tracks", c.duration, c.tracks.len())
                                }),
                        }
                    };
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 64.),
                        egui::Sense::click(),
                    );
                    let p = ui.painter_at(rect);
                    p.rect_filled(
                        rect,
                        theme::RADIUS,
                        if active {
                            theme::SELECTED
                        } else if response.hovered() {
                            theme::HOVER
                        } else {
                            theme::PANEL
                        },
                    );
                    if active {
                        crate::ui::icons::paint(
                            &p,
                            egui::Rect::from_min_size(
                                rect.right_top() + egui::vec2(-22., 8.),
                                egui::vec2(16., 16.),
                            ),
                            widgets::Icon::Check,
                            theme::TEXT,
                        );
                        p.rect_filled(
                            egui::Rect::from_min_size(
                                rect.left_top() + egui::vec2(0., 8.),
                                egui::vec2(3., 48.),
                            ),
                            2.,
                            theme::ORANGE,
                        );
                    }
                    let thumb = egui::Rect::from_min_size(
                        rect.left_top() + egui::vec2(10., 9.),
                        egui::vec2(46., 46.),
                    );
                    p.rect_filled(thumb, 4., theme::BACKGROUND);
                    if let Some(texture) = self
                        .motion
                        .textures
                        .get(&record.id)
                        .or_else(|| self.thumbnails.get(&record.hash))
                    {
                        p.image(
                            texture.id(),
                            thumb,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                            egui::Color32::WHITE,
                        );
                    } else {
                        widgets::paint_icon(
                            ui,
                            thumb.shrink(12.),
                            if record.kind == Kind::Model {
                                widgets::Icon::Model
                            } else {
                                widgets::Icon::Motion
                            },
                            theme::SECONDARY,
                        );
                    }
                    // Clip names are elided to the card, with the full name in the tooltip.
                    let text_rect = egui::Rect::from_min_max(
                        rect.left_top() + egui::vec2(66., 12.),
                        rect.right_bottom() - egui::vec2(if active { 26. } else { 6. }, 8.),
                    );
                    let text_p = ui.painter_at(text_rect);
                    let color = if active {
                        theme::TEXT
                    } else {
                        theme::SECONDARY
                    };
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        record.name.clone(),
                        egui::FontId::proportional(14.),
                        color,
                    );
                    job.wrap.max_width = text_rect.width();
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    job.wrap.overflow_character = Some('…');
                    text_p.galley(
                        text_rect.left_top(),
                        ui.fonts_mut(|f| f.layout_job(job)),
                        color,
                    );
                    text_p.text(
                        text_rect.left_top() + egui::vec2(0., 23.),
                        egui::Align2::LEFT_TOP,
                        &metadata,
                        egui::FontId::proportional(11.),
                        if failed { theme::ERROR } else { theme::MUTED },
                    );
                    if loading {
                        p.circle_stroke(thumb.center(), 10., egui::Stroke::new(2., theme::ORANGE));
                    }
                    if let Some(r) = &mut self.input_regions {
                        r.records
                            .insert(record.id.clone(), response.rect.intersect(ui.clip_rect()));
                    }
                    if response.clicked() {
                        selected = Some(record.clone());
                    }
                    response.context_menu(|ui| {
                        if ui
                            .button("Delete from Library")
                            .on_hover_text(
                                "Original files are kept. Deleted bundled motions stay deleted.",
                            )
                            .clicked()
                        {
                            delete = Some(record.id.clone());
                            ui.close();
                        }
                    });
                    response
                        .on_hover_text(format!(
                            "{}\n{metadata}\n{}",
                            record.name,
                            if active {
                                "Selected"
                            } else {
                                "Click to preview"
                            }
                        ))
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                }
            });
        if let Some(id) = delete {
            self.delete_library_record(&id);
            selected = None;
        }
        selected
    }
}
