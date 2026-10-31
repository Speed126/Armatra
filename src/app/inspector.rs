use super::*;
impl Editor {
    pub(super) fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.;
        if self.ui_state.drawer.open != Some(crate::ui::drawers::Kind::Detail) {
            ui.label(egui::RichText::new("Motion setup").font(theme::font(18., 500)));
        }
        egui::ScrollArea::vertical()
            .id_salt("inspector-scroll")
            .show(ui, |ui| {
                if self.ui_state.workspace == Workspace::Retarget
                    && let Some(node) = self.selected_pair
                    && let Some((_, asset, _)) = &self.source
                {
                    widgets::section(ui, "SELECTED JOINT");
                    ui.label(
                        egui::RichText::new(&asset.nodes[node].name).font(theme::font(15., 500)),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Source node {node:02} / selection is session-only"
                        ))
                        .font(theme::font(11., 400))
                        .color(theme::SECONDARY),
                    );
                }
                widgets::section(ui, "TARGET");
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(self.model_name()).font(theme::font(18., 500)),
                    )
                    .truncate(),
                )
                .on_hover_text(self.model_name());
                if let Some((_, asset, _)) = &self.model {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} joints / {} meshes",
                            asset.skins.iter().map(|s| s.joints.len()).sum::<usize>(),
                            asset.meshes.len()
                        ))
                        .font(theme::font(11., 400))
                        .color(theme::SECONDARY),
                    );
                }
                ui.add_space(12.);
                ui.separator();
                widgets::section(ui, "TRANSFER");
                if self.plan_pending {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Compiling preview...");
                    });
                } else if let Some(plan) = &self.retarget {
                    let dirty = plan.overrides != self.mapping_overrides
                        || plan.settings != self.mapping_settings;
                    widgets::status(
                        ui,
                        if dirty {
                            "Pending mapping edits"
                        } else {
                            "Retargeted / compiled plan"
                        },
                        if dirty {
                            theme::WARNING
                        } else {
                            theme::SUCCESS
                        },
                    );
                    ui.label(
                        egui::RichText::new(&self.plan_origin)
                            .font(theme::font(11., 400))
                            .color(theme::SECONDARY),
                    );
                    let review = plan
                        .rows
                        .iter()
                        .filter(|r| {
                            matches!(
                                r.confidence,
                                retarget::Confidence::Ambiguous | retarget::Confidence::Unmatched
                            )
                        })
                        .count();
                    if review > 0 {
                        ui.label(
                            egui::RichText::new(format!("{review} joints need review"))
                                .font(theme::font(11., 400))
                                .color(theme::WARNING),
                        );
                    }
                } else if let Some(mapping) = &self.mapping {
                    match &mapping.status {
                        compatibility::Compatibility::SameAsset => {
                            widgets::status(ui, "Native motion", theme::SUCCESS)
                        }
                        compatibility::Compatibility::Exact => {
                            widgets::status(ui, "Exact correspondence", theme::SUCCESS)
                        }
                        compatibility::Compatibility::NeedsMapping(reason) => {
                            widgets::status(ui, "Mapping required", theme::WARNING);
                            ui.label(
                                egui::RichText::new(reason)
                                    .font(theme::font(11., 400))
                                    .color(theme::SECONDARY),
                            );
                        }
                        compatibility::Compatibility::Unsupported(reason) => {
                            widgets::status(ui, "Unsupported deformation", theme::WARNING);
                            ui.label(reason);
                        }
                    }
                } else {
                    ui.label("Select a character and motion.");
                }
                ui.add_enabled_ui(self.model.is_some() && self.source.is_some(), |ui| {
                    if widgets::glyph_button(
                        ui,
                        widgets::Icon::Mapping,
                        "Review correspondence",
                        false,
                    )
                    .clicked()
                    {
                        self.mapping_open = true;
                        self.motion.open = false;
                        self.ui_state.workspace = Workspace::Retarget;
                        self.ui_state.drawer.open = None;
                    }
                });
                if self.retarget.is_some() {
                    ui.add_space(12.);
                    ui.separator();
                    widgets::section(ui, "CONTACT CORRECTION");
                    widgets::toggle(
                        ui,
                        widgets::Icon::Ik,
                        "Pelvis & foot IK",
                        &mut self.contact_correction,
                    )
                    .on_hover_text("Use existing deterministic contacts to reduce foot sliding.");
                    widgets::toggle(
                        ui,
                        widgets::Icon::Contact,
                        "Show foot anchors",
                        &mut self.show_anchors,
                    );
                    if let Some(contacts) = &self.contacts {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} detected intervals",
                                contacts.intervals.len()
                            ))
                            .font(theme::font(11., 400))
                            .color(theme::SUCCESS),
                        );
                        for notice in &contacts.warnings {
                            ui.label(
                                egui::RichText::new(notice)
                                    .font(theme::font(11., 400))
                                    .color(theme::WARNING),
                            );
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("Preparing contacts...")
                                .font(theme::font(11., 400))
                                .color(theme::SECONDARY),
                        );
                    }
                    ui.add_space(12.);
                    ui.separator();
                    widgets::section(ui, "ROOT MOTION");
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::hover());
                        crate::ui::icons::paint(
                            ui.painter(),
                            r,
                            widgets::Icon::Root,
                            theme::SECONDARY,
                        );
                        ui.label(match self.mapping_settings.root {
                            retarget::RootPolicy::ScaleHeight => "Scale by height",
                            retarget::RootPolicy::Unscaled => "Unscaled",
                            retarget::RootPolicy::InPlace => "In place",
                        });
                    });
                    if widgets::glyph_button(
                        ui,
                        widgets::Icon::Chevron,
                        "Advanced transfer settings",
                        false,
                    )
                    .clicked()
                    {
                        self.ui_state.workspace = Workspace::Retarget;
                        self.mapping_open = true;
                        self.motion.open = false;
                        self.ui_state.mapping_advanced = true;
                        self.ui_state.drawer.open = None;
                    }
                }
                ui.add_space(12.);
                ui.separator();
                egui::CollapsingHeader::new("Motion & geometry details").show(ui, |ui| {
                    if let Some((_, asset, index)) = &self.source {
                        ui.label(
                            egui::RichText::new(&asset.clips[*index].name)
                                .font(theme::font(13., 500)),
                        );
                        widgets::property(
                            ui,
                            "Duration",
                            format!("{:.3} seconds", asset.clips[*index].duration),
                        );
                        widgets::property(
                            ui,
                            "Channels",
                            asset.clips[*index].tracks.len().to_string(),
                        );
                    }
                    if let Some((_, asset, _)) = &self.model
                        && asset.scenes.len() > 1
                    {
                        egui::ComboBox::from_label("Scene")
                            .selected_text(&asset.scenes[self.scene].name)
                            .show_ui(ui, |ui| {
                                for (i, scene) in asset.scenes.iter().enumerate() {
                                    ui.selectable_value(&mut self.scene, i, &scene.name);
                                }
                            });
                    }
                    widgets::toggle(
                        ui,
                        widgets::Icon::Skeleton,
                        "Rest pose",
                        &mut self.bind_pose,
                    );
                    ui.label(
                        egui::RichText::new(
                            "Orbit: left drag / Pan: right or middle drag / Zoom: scroll",
                        )
                        .font(theme::font(11., 400))
                        .color(theme::SECONDARY),
                    );
                });
                if !self.render_warnings.is_empty() {
                    ui.add_space(12.);
                    widgets::status(ui, "Display notice / see Diagnostics", theme::WARNING);
                }
            });
    }
}
