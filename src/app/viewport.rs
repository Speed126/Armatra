use super::*;
impl Editor {
    pub(super) fn frame_character(&mut self) {
        self.frame_requested = true;
        if let Some(preview) = &mut self.source_preview {
            preview
                .camera
                .fit(&preview.asset, preview.asset.default_scene);
        } else if let Some((_, asset, _)) = &self.model {
            self.camera.fit(asset, self.scene);
        }
    }
    pub(super) fn prepare_source_preview(&mut self, renderer: &Renderer, gpu: &Gpu) {
        let fallback = self.ui_state.workspace != Workspace::Retarget
            && self
                .source
                .as_ref()
                .is_some_and(|s| s.1.clips[s.2].supported)
            && (self.model.is_none()
                || (self.retarget.is_none()
                    && self.mapping.as_ref().is_none_or(|m| m.nodes.is_empty())));
        if !fallback {
            self.source_preview = None;
            return;
        }
        let Some((hash, asset, index)) = &self.source else {
            return;
        };
        let preview_key = format!("{hash}:{index}");
        if self.status.starts_with("Retarget:") {
            self.status = format!(
                "Original source preview · target transfer: {}",
                self.status.trim_start_matches("Retarget: ")
            );
        }
        if self.mapping_feedback.phase == "Failed" && self.motion.preview_error.is_none() {
            self.status = format!(
                "Original source preview · Target transfer unavailable: {}",
                self.mapping_feedback.message
            );
        }
        if self
            .source_preview
            .as_ref()
            .is_some_and(|p| p.key == preview_key)
        {
            return;
        }
        match self
            .gpu_cache
            .get(hash)
            .map(Ok)
            .unwrap_or_else(|| renderer.upload(gpu, asset))
        {
            Ok(resident) => {
                let mut camera = resident.fitted_camera.clone();
                if asset.meshes.is_empty() {
                    let identity: Vec<_> = (0..asset.nodes.len()).map(Some).collect();
                    let visible = asset.visible_nodes(asset.default_scene);
                    let mut min = glam::Vec3::splat(f32::INFINITY);
                    let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
                    let mut cursors = vec![];
                    // Sampled framing covers root travel and End Sites for motion-only previews.
                    for sample in 0..=16 {
                        let pose = animation::evaluate(
                            asset,
                            &asset.clips[*index],
                            &identity,
                            asset.clips[*index].duration * sample as f32 / 16.,
                            &mut cursors,
                        );
                        let globals = asset.globals(&pose);
                        for &node in &visible {
                            let p = globals[node].w_axis.truncate();
                            if p.is_finite() {
                                min = min.min(p);
                                max = max.max(p);
                            }
                        }
                    }
                    if min.is_finite() && max.is_finite() {
                        camera.framing_bounds = Some((min, max));
                    }
                }
                camera.needs_fit = true;
                self.gpu_cache
                    .pin(self.model.iter().map(|m| m.0.clone()).chain([hash.clone()]));
                self.gpu_cache
                    .insert(hash.clone(), Arc::clone(&resident), resident.bytes);
                self.source_preview = Some(SourcePreview {
                    key: preview_key,
                    asset: Arc::clone(asset),
                    clip: *index,
                    resident,
                    camera,
                });
            }
            Err(error) => {
                self.source_preview = None;
                self.status = format!("Source preview unavailable: {error:#}");
            }
        }
    }
    pub(super) fn procedural_fixture(&self) -> bool {
        static HASHES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let hashes = HASHES.get_or_init(|| {
            [
                crate::fixtures::robot(false, false).finish(),
                crate::fixtures::cube().finish(),
                crate::rig_corpus::humanoid(crate::rig_corpus::Variant::default(), true)
                    .0
                    .finish(),
                crate::rig_corpus::humanoid(
                    crate::rig_corpus::Variant {
                        renamed: true,
                        proportions: true,
                        helpers: true,
                        ..Default::default()
                    },
                    false,
                )
                .0
                .finish(),
                crate::rig_corpus::humanoid(
                    crate::rig_corpus::Variant {
                        renamed: true,
                        frames: true,
                        ..Default::default()
                    },
                    false,
                )
                .0
                .finish(),
            ]
            .iter()
            .map(|bytes| blake3::hash(bytes).to_hex().to_string())
            .collect()
        });
        if self.source_preview.is_some() {
            self.source.as_ref().is_some_and(|s| hashes.contains(&s.0))
        } else {
            self.model.as_ref().is_some_and(|m| hashes.contains(&m.0))
        }
    }
    pub(super) fn viewport_ui(
        &mut self,
        ui: &mut egui::Ui,
        texture: egui::TextureId,
    ) -> ([u32; 2], Option<Record>) {
        let ctx = ui.ctx().clone();
        let mut selected = None;
        if self.active_pair.is_some()
            || self
                .mapping_feedback
                .request
                .as_ref()
                .is_some_and(|q| q.intent != super::mapping_apply::Intent::Restore)
        {
            ui.horizontal(|ui| {
                let back = ui
                    .button("Back to Retarget")
                    .on_hover_text("Continue editing the applied mapping");
                if let Some(r) = &mut self.input_regions {
                    r.mapping_controls.insert("back-retarget", back.rect);
                }
                if back.clicked() {
                    self.mapping_open = true;
                    self.motion.open = false;
                    self.ui_state.workspace = Workspace::Retarget;
                }
                let save = ui
                    .add_enabled(!self.plan_pending, egui::Button::new("Save Mapping"))
                    .on_hover_text("Compile and save the current mapping profile");
                if let Some(r) = &mut self.input_regions {
                    r.mapping_controls.insert("preview-save", save.rect);
                }
                if save.clicked() {
                    self.start_mapping(super::mapping_apply::Intent::Save);
                }
            });
            self.mapping_feedback_ui(ui);
            self.mapping_details(ui.ctx());
        }
        let size = ui.available_size().max(egui::vec2(1., 1.));
        let (toolbar, _) = ui.allocate_exact_size(egui::vec2(size.x, 40.), egui::Sense::hover());
        ui.painter().rect_filled(toolbar, 0., theme::PANEL);
        ui.painter().hline(
            toolbar.x_range(),
            toolbar.bottom(),
            egui::Stroke::new(1., theme::BORDER),
        );
        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("stage-toolbar")
                .max_rect(toolbar.shrink2(egui::vec2(12., 4.)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        if widgets::glyph_button(&mut bar, widgets::Icon::Mesh, "Mesh", self.mesh).clicked() {
            self.mesh = !self.mesh;
        }
        if widgets::glyph_button(&mut bar, widgets::Icon::Skeleton, "Skeleton", self.skeleton)
            .clicked()
        {
            self.skeleton = !self.skeleton;
        }
        bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if self.ui_state.workspace == Workspace::Animate {
                let width = ui.ctx().content_rect().width();
                self.drawer_buttons(ui, width < 1050., width < 900.);
            }

            let fit = widgets::icon(ui, widgets::Icon::Frame, false, "Frame character / F");
            if let Some(r) = &mut self.input_regions {
                r.fit = fit.rect;
                r.reset = fit.rect;
            }
            if fit.clicked() {
                self.frame_character();
            }
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.ui_state.preferences.grid, "Ground grid");
                ui.checkbox(&mut self.ui_state.preferences.axes, "Axis indicator");
                ui.label(
                    egui::RichText::new("Drag to orbit / right drag to pan")
                        .font(theme::font(11., 400))
                        .color(theme::SECONDARY),
                );
            });
        });
        let image_size = ui.available_size().max(egui::vec2(1., 1.));
        if let Some(preview) = &mut self.source_preview {
            let (camera, asset) = (&mut preview.camera, &preview.asset);
            camera.aspect = image_size.x / image_size.y.max(1.);
            if camera.needs_fit {
                camera.fit(asset, asset.default_scene);
            }
        }
        self.camera.aspect = image_size.x / image_size.y.max(1.);
        if self.camera.needs_fit
            && let Some((_, asset, _)) = &self.model
        {
            self.camera.fit(asset, self.scene);
        }
        let response =
            ui.add(egui::Image::new((texture, image_size)).sense(egui::Sense::click_and_drag()));
        let rect = response.rect;
        if let Some(r) = &mut self.input_regions {
            r.viewport = rect;
        }
        if let Some(preview) = &mut self.source_preview {
            preview.camera.interact(&response, ui);
        } else {
            self.camera.interact(&response, ui);
        }
        let p = ui.painter_at(rect);
        if self.model.is_some() || self.source_preview.is_some() {
            let compact =
                self.ui_state.workspace == Workspace::Animate && ctx.content_rect().width() < 1050.;
            if compact {
                let pickers = egui::Rect::from_min_size(
                    rect.left_top() + egui::vec2(12., 8.),
                    egui::vec2((rect.width() - 100.).min(390.), 32.),
                );
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("compact-active-assets")
                        .max_rect(pickers)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                for model in [true, false] {
                    let text = if model {
                        self.model_name().to_owned()
                    } else {
                        self.source
                            .as_ref()
                            .map_or("No motion", |s| s.1.clips[s.2].name.as_str())
                            .to_owned()
                    };
                    egui::ComboBox::from_id_salt(if model {
                        "stage-model"
                    } else {
                        "stage-motion"
                    })
                    .width(160.)
                    .selected_text(text)
                    .show_ui(&mut row, |ui| {
                        for record in self
                            .records
                            .iter()
                            .filter(|r| (r.kind == Kind::Model) == model)
                        {
                            let active = if model {
                                self.model.as_ref().is_some_and(|m| m.0 == record.hash)
                            } else {
                                self.source.as_ref().is_some_and(|s| {
                                    s.0 == record.hash && record.kind == Kind::Clip(s.2)
                                })
                            };
                            if ui.selectable_label(active, &record.name).clicked() {
                                selected = Some(record.clone());
                            }
                        }
                    });
                }
            } else {
                let mut label = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("stage-identity")
                        .max_rect(egui::Rect::from_min_size(
                            rect.left_top() + egui::vec2(20., 16.),
                            egui::vec2((rect.width() - 150.).max(100.), 60.),
                        ))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                label
                    .add(
                        egui::Label::new(
                            egui::RichText::new(if self.source_preview.is_some() {
                                "Original source preview"
                            } else {
                                self.model_name()
                            })
                            .font(theme::font(18., 500)),
                        )
                        .truncate(),
                    )
                    .on_hover_text(self.model_name());
                let motion = self
                    .source
                    .as_ref()
                    .map_or("Rest pose", |s| s.1.clips[s.2].name.as_str());
                label.add(
                    egui::Label::new(
                        egui::RichText::new(motion)
                            .font(theme::font(12., 400))
                            .color(theme::SECONDARY),
                    )
                    .truncate(),
                );
            }
            if rect.height() > 210. {
                p.text(
                    rect.left_bottom() + egui::vec2(16., -16.),
                    egui::Align2::LEFT_BOTTOM,
                    if self.procedural_fixture() {
                        "Procedural test fixture"
                    } else {
                        "Drag to orbit / right drag to pan / F to frame"
                    },
                    theme::font(11., 400),
                    theme::SECONDARY,
                );
            }
            if self.ui_state.preferences.axes {
                let camera = self
                    .source_preview
                    .as_ref()
                    .map_or(&self.camera, |p| &p.camera);
                let origin = rect.right_bottom() - egui::vec2(46., 38.);
                let right = glam::Vec3::new(camera.yaw.cos(), 0., -camera.yaw.sin());
                let back = (camera.eye() - camera.target).normalize();
                let up = back.cross(right);
                let mut axes = [
                    (glam::Vec3::X, "X", egui::Color32::from_rgb(197, 145, 131)),
                    (glam::Vec3::Y, "Y", theme::SUCCESS),
                    (glam::Vec3::Z, "Z", egui::Color32::from_rgb(154, 174, 185)),
                ];
                axes.sort_by(|a, b| a.0.dot(back).total_cmp(&b.0.dot(back)));
                for (axis, name, color) in axes {
                    let vector = egui::vec2(axis.dot(right), -axis.dot(up));
                    let end = origin + vector * 24.;
                    p.line_segment([origin, end], egui::Stroke::new(1.2, color));
                    p.text(
                        end + vector * 9.,
                        egui::Align2::CENTER_CENTER,
                        name,
                        theme::font(11., 400),
                        theme::SECONDARY,
                    );
                }
                p.circle_filled(origin, 2., theme::TEXT);
            }
        } else {
            let empty = egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(rect.width().min(430.) - 32., 190.),
            );
            let mut panel = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("empty-stage")
                    .max_rect(empty)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            panel.label(
                egui::RichText::new("Bring your characters to life.").font(theme::font(24., 500)),
            );
            panel.add_space(12.);
            panel.label("Import a character, or explore your Library motions.");
            panel.add_space(24.);
            panel.horizontal(|ui| {
                let import = widgets::primary(ui, "Import asset");
                if let Some(r) = &mut self.input_regions {
                    r.empty_import = import.rect;
                }
                if import.clicked() {
                    self.import(None);
                }
                let sample =
                    widgets::glyph_button(ui, widgets::Icon::Skeleton, "Browse motions", false);
                if let Some(r) = &mut self.input_regions {
                    r.empty_sample = sample.rect;
                }
                if sample.clicked() {
                    self.request_motion_index();
                }
            });
            panel.add_space(12.);
            panel.label("A demo character is included at assets/demo/RobotExpressive.glb.");
        }
        if self.show_anchors
            && self.source_preview.is_none()
            && let (Some(c), Some((_, source, index))) = (&self.contacts, &self.source)
        {
            let time = self.playback.time as f64
                + self.playback.cycle as f64 * source.clips[*index].duration as f64;
            let vp = self.camera.matrix(image_size.x / image_size.y.max(1.));
            for contact in c.active(time, self.playback.looping) {
                let clip = vp * contact.anchor.extend(1.);
                if clip.w > 0. {
                    let ndc = clip.truncate() / clip.w;
                    let pos = egui::pos2(
                        rect.center().x + ndc.x * rect.width() * 0.5,
                        rect.center().y - ndc.y * rect.height() * 0.5,
                    );
                    p.circle_stroke(pos, 7., egui::Stroke::new(2., theme::SUCCESS));
                }
            }
        }
        (
            [
                (image_size.x * ctx.pixels_per_point()).max(1.) as u32,
                (image_size.y * ctx.pixels_per_point()).max(1.) as u32,
            ],
            selected,
        )
    }
}
