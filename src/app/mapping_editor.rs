use super::*;
use crate::{
    retarget::Confidence,
    rig::Rig,
    ui::{correspondence as c, icons::Icon},
};

fn region(ui: &mut egui::Ui, rect: egui::Rect, id: &str) -> egui::Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    )
}
impl Editor {
    pub(super) fn mapping_links(&self, rows: &[retarget::Row]) -> Vec<c::Link> {
        rows.iter()
            .map(|r| {
                let automatic = self
                    .automatic_rows
                    .iter()
                    .find(|a| a.source == r.source)
                    .unwrap_or(r);
                let base = if self.automatic_rows.is_empty() && r.confidence == Confidence::Manual {
                    None
                } else {
                    automatic.target
                };
                let base = base.filter(|&target| {
                    !self
                        .mapping_overrides
                        .iter()
                        .any(|o| o.source != r.source && o.target == Some(target))
                });
                let manual = self.mapping_overrides.iter().find(|o| o.source == r.source);
                c::Link {
                    source: r.source,
                    target: manual.map_or(base, |o| o.target),
                    status: if manual.is_some_and(|o| o.target.is_none())
                        || (manual.is_none() && base.is_none())
                    {
                        c::Status::Unmapped
                    } else if manual.is_some() {
                        c::Status::Manual
                    } else {
                        match automatic.confidence {
                            Confidence::High => c::Status::Automatic,
                            Confidence::Manual => c::Status::Ambiguous,
                            Confidence::Ambiguous => c::Status::Ambiguous,
                            Confidence::Unmatched => c::Status::Unmapped,
                        }
                    },
                }
            })
            .collect()
    }
    pub(super) fn mapping_ui(&mut self, ui: &mut egui::Ui, _height: f32) -> Option<Record> {
        let rect = ui.available_rect_before_wrap();
        let compact = ui.ctx().content_rect().width() < 1200.;
        let mut settings_before = self.mapping_settings.clone();
        let header = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), if compact { 84. } else { 140. }),
        );
        ui.painter().rect_filled(header, 0., theme::PANEL);
        let mut select = None;
        let mut compile = false;
        let mut apply_only = false;
        let mut save = false;
        let mut top = region(ui, header.shrink2(egui::vec2(20., 8.)), "rig-selectors");
        let width = ((header.width() - 320.) * 0.5).clamp(120., 300.);
        top.horizontal(|ui|{
            for side in 0..2 {
                if side==1 {ui.add_space(20.);}
                ui.vertical(|ui|{
                    ui.set_width(width);
                    if !compact {ui.label(egui::RichText::new(if side==0 {"SOURCE MOTION"}else{"TARGET CHARACTER"}).font(theme::font(11.,500)).color(theme::SECONDARY));}
                    let name=if side==0 {self.source.as_ref().map_or("Choose motion",|s|s.1.clips[s.2].name.as_str())}else{self.model_name()};
                    egui::ComboBox::from_id_salt(("rig-selector",side)).width(width).truncate().selected_text(name).show_ui(ui,|ui|{
                        for record in self.records.iter().filter(|r|if side==0 {matches!(r.kind,Kind::Clip(_))}else{r.kind==Kind::Model}) {
                            let active=if side==0 {self.source.as_ref().is_some_and(|s|s.0==record.hash && record.kind==Kind::Clip(s.2))}else{self.model.as_ref().is_some_and(|m|m.0==record.hash)};
                            if ui.selectable_label(active,&record.name).clicked(){select=Some(record.clone());}
                        }
                    }).response.on_hover_text(if side==0 {"Source motion"}else{"Target character"});
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                let (apply,menu)=ui.add_enabled_ui(!self.plan_pending,|ui|widgets::split_primary(ui,Icon::Check,if self.plan_pending {"Applying…"}else{"Apply & Preview"})).inner;
                egui::Popup::menu(&menu).show(|ui|{
                    let b=ui.add_enabled(!self.plan_pending,egui::Button::new("Apply Only"));
                    if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("apply-only",b.rect);}
                    if b.clicked(){apply_only=true;ui.close();}
                });
                if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("apply-menu",menu.rect);}
                if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("apply",apply.rect);}
                if let Some(r)=&mut self.input_regions {r.mapping_focus.insert("apply",apply.id);r.mapping_focus.insert("apply-menu",menu.id);}
                compile=apply.on_hover_text("Apply this exact mapping, then open the animated target from the beginning").clicked();
            });
        });
        top.add_space(4.);
        top.horizontal(|ui| {
            for side in 0..2 {
                if side == 1 {
                    ui.add_space(20.);
                }
                let asset = if side == 0 {
                    self.source.as_ref().map(|s| Arc::clone(&s.1))
                } else {
                    self.model.as_ref().map(|m| Arc::clone(&m.1))
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 24.),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(width);
                        if let Some(asset) = asset {
                            let selected = if side == 0 {
                                &mut self.mapping_settings.source_skin
                            } else {
                                &mut self.mapping_settings.target_skin
                            };
                            super::mapping_advanced::skin_selector(
                                ui,
                                &asset,
                                selected,
                                side,
                                self.input_regions.as_mut(),
                            );
                        }
                    },
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.drawer_buttons(ui, true, true)
            });
        });
        if settings_before.source_skin != self.mapping_settings.source_skin
            || settings_before.target_skin != self.mapping_settings.target_skin
        {
            self.skin_changed(&settings_before);
            settings_before = self.mapping_settings.clone();
        }
        if !compact {
            top.add_space(3.);
            self.mapping_feedback_ui(&mut top);
        }
        let (Some((_, source, _)), Some((_, target, _))) = (&self.source, &self.model) else {
            let mut empty = region(
                ui,
                egui::Rect::from_min_max(header.left_bottom() + egui::vec2(20., 20.), rect.max),
                "empty-retarget",
            );
            widgets::empty(
                &mut empty,
                "Choose a source and target",
                "Select a rigged motion and character above. Correspondence appears when both are ready.",
            );
            return select;
        };
        let source = Arc::clone(source);
        let target = Arc::clone(target);
        self.mapping_context();
        let rigs = [
            Rig::analyze_skin(&source, self.mapping_settings.source_skin),
            Rig::analyze_skin(&target, self.mapping_settings.target_skin),
        ];
        let rows = self
            .retarget
            .as_ref()
            .map(|p| p.rows.clone())
            .unwrap_or_else(|| {
                rigs[0]
                    .joints
                    .iter()
                    .map(|d| retarget::Row {
                        source: d.node,
                        target: None,
                        confidence: Confidence::Unmatched,
                        reason: "Compile to find correspondence".into(),
                        margin: 0.,
                    })
                    .collect()
            });
        if self
            .selected_pair
            .is_some_and(|id| !rows.iter().any(|r| r.source == id))
        {
            self.selected_pair = None;
        }
        let body = egui::Rect::from_min_max(header.left_bottom(), rect.max);
        let detail_width = if compact { 264. } else { 320. };
        let detail = egui::Rect::from_min_max(
            egui::pos2(body.right() - detail_width, body.top()),
            body.max,
        );
        let left = egui::Rect::from_min_max(body.min, detail.left_bottom());
        let health = egui::Rect::from_min_size(left.min, egui::vec2(left.width(), 35.));
        let transport = egui::Rect::from_min_max(
            egui::pos2(
                left.left(),
                left.bottom() - if compact { 62. } else { 108. },
            ),
            left.max,
        );
        let tools = egui::Rect::from_min_max(
            egui::pos2(left.left(), transport.top() - 34.),
            transport.right_top(),
        );
        let canvas_rect = egui::Rect::from_min_max(health.left_bottom(), tools.right_top());
        let (geometry, clicked) = c::interact(
            ui,
            canvas_rect,
            &mut self.ui_state.correspondence,
            [&rigs[0], &rigs[1]],
        );
        let mut links = self.mapping_links(&rows);
        if let Some(hit) = clicked {
            if matches!(
                self.ui_state.correspondence.operation,
                c::Operation::Selecting(_)
            ) {
                self.ui_state.correspondence.operation.choose(hit);
            } else if hit.side == 0 {
                self.selected_pair = Some(hit.node);
                self.ui_state.correspondence.orphan_target = None;
                self.ui_state.correspondence.candidates.clear();
            } else {
                let candidates: Vec<_> = links
                    .iter()
                    .filter(|l| l.target == Some(hit.node))
                    .map(|l| l.source)
                    .collect();
                self.selected_pair = if candidates.len() == 1 {
                    Some(candidates[0])
                } else {
                    None
                };
                self.ui_state.correspondence.orphan_target = Some(hit.node);
                self.ui_state.correspondence.candidates = if candidates.len() > 1 {
                    candidates
                } else {
                    vec![]
                };
            }
        }
        ui.painter().rect_filled(detail, 0., theme::PANEL);
        ui.painter().vline(
            detail.left(),
            detail.y_range(),
            egui::Stroke::new(1., theme::BORDER),
        );
        let footer = egui::Rect::from_min_max(
            egui::pos2(
                detail.left() + 18.,
                detail.bottom() - if compact { 72. } else { 130. },
            ),
            detail.right_bottom() - egui::vec2(18., 8.),
        );
        if compact {
            let mut feedback = region(
                ui,
                egui::Rect::from_min_size(
                    detail.min + egui::vec2(18., 8.),
                    egui::vec2(detail.width() - 36., 80.),
                ),
                "compact-apply-feedback",
            );
            self.mapping_feedback_compact(&mut feedback);
        }
        let detail_body = egui::Rect::from_min_max(
            detail.min + egui::vec2(18., if compact { 94. } else { 16. }),
            egui::pos2(detail.right() - 18., footer.top() - 8.),
        );
        let mut detail_host = region(ui, detail_body, "pair-detail");
        let scrolled=egui::ScrollArea::vertical().id_salt("mapping-detail-body").max_height(detail_body.height()).show(&mut detail_host,|d|{
        if compact {
            d.spacing_mut().item_spacing.y = 6.;
        }
        d.label(
            egui::RichText::new("CORRESPONDENCE")
                .font(theme::font(11., 500))
                .color(theme::SECONDARY),
        );
        if let Some(id) = self.selected_pair {
            d.add(
                egui::Label::new(
                    egui::RichText::new(&source.nodes[id].name).font(theme::font(18., 500)),
                )
                .truncate(),
            )
            .on_hover_text(&source.nodes[id].name);
            let mut chosen = links.iter().find(|l| l.source == id).and_then(|l| l.target);
            let old = chosen;
            let combo = d
                .add_enabled_ui(!self.plan_pending, |ui| {
                    egui::ComboBox::from_id_salt(("pair", id))
                        .width(ui.available_width() - 8.)
                        .selected_text(
                            egui::RichText::new(
                                chosen.map_or("Unmapped", |n| target.nodes[n].name.as_str()),
                            )
                            .font(theme::font(14., 500))
                            .color(theme::ORANGE),
                        )
                        .show_ui(ui, |ui| {
                            if let Some(r) = &mut self.input_regions {
                                let clip = ui.clip_rect();
                                r.mapping_popup = egui::Rect::from_min_max(
                                    egui::pos2(ui.max_rect().left().max(clip.left()), clip.top()),
                                    egui::pos2(
                                        ui.max_rect().right().min(clip.right()),
                                        clip.bottom(),
                                    ),
                                );
                            }
                            let response = ui.selectable_value(&mut chosen, None, "Unmapped");
                            if let Some(r) = &mut self.input_regions {
                                r.mapping_options.insert((id, None), response.rect);
                            }
                            for joint in &rigs[1].joints {
                                let response = ui
                                    .scope(|ui| {
                                        ui.selectable_value(
                                            &mut chosen,
                                            Some(joint.node),
                                            &target.nodes[joint.node].name,
                                        )
                                    })
                                    .inner
                                    .on_hover_text("Propose this target. Review names and any conflict, then confirm.");
                                if let Some(r) = &mut self.input_regions {
                                    r.mapping_options
                                        .insert((id, Some(joint.node)), response.rect);
                                }
                            }
                        })
                })
                .inner;
            if let Some(r) = &mut self.input_regions {
                r.mapping_combos.insert(id, combo.response.rect);
            }
            if chosen != old {
                if chosen.is_some() {
                    self.ui_state
                        .correspondence
                        .operation
                        .begin(Some(id), chosen);
                } else {
                    self.assign_joint(id, None);
                }
                links = self.mapping_links(&rows);
            }
            let status = links
                .iter()
                .find(|l| l.source == id)
                .map_or(c::Status::Unmapped, |l| l.status);
            d.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10., 14.), egui::Sense::hover());
                c::marker(ui.painter(), r.center(), status, 3.);
                ui.colored_label(
                    status.color(),
                    if status == c::Status::Manual && self.mapping_pending(id) {
                        if compact {"Manual · Pending apply"}else{"Manual mapping · Pending apply"}
                    } else {
                        status.label()
                    },
                );
            });
            if !compact {
                let pending_edit = self.mapping_overrides.iter().any(|o| o.source == id)
                    && self.retarget.as_ref().is_none_or(|p| {
                        p.settings != self.mapping_settings || p.overrides != self.mapping_overrides
                    });
                d.label(
                    egui::RichText::new(if pending_edit {
                        "Pending manual edit. Apply & Preview updates animation."
                    } else {
                        rows.iter()
                            .find(|r| r.source == id)
                            .map_or("", |r| r.reason.as_str())
                    })
                    .font(theme::font(11., 400))
                    .color(theme::SECONDARY),
                );
                d.add_space(8.);
            }
            d.horizontal(|ui|{ui.add_enabled_ui(!self.plan_pending,|ui|{
                let unmap=widgets::glyph_button(ui,Icon::Unmap,"Unmap",false).on_hover_text("Explicitly leave this source joint unmapped. Creates a manual unmapped override.");
                let auto=widgets::glyph_button(ui,Icon::RestoreAuto,"Restore Auto",false).on_hover_text("Remove the manual override and return to automatic correspondence. Apply & Preview updates animation.");
                if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("unmap",unmap.rect);r.mapping_controls.insert("restore-auto",auto.rect);}
                if unmap.clicked(){self.assign_joint(id,None);links=self.mapping_links(&rows);}
                if auto.clicked(){self.restore_auto(id);links=self.mapping_links(&rows);}
            });});
        } else {
            d.label(
                egui::RichText::new(if self.ui_state.correspondence.candidates.is_empty() {
                    "Select a joint"
                } else {
                    "Choose source correspondence"
                })
                .font(theme::font(18., 500)),
            );
            for candidate in self.ui_state.correspondence.candidates.clone() {
                if d.button(&source.nodes[candidate].name).clicked() {
                    self.selected_pair = Some(candidate);
                    self.ui_state.correspondence.candidates.clear();
                }
            }
        }
        d.add_space(if compact { 4. } else { 12. });
        d.separator();
        d.label(
            egui::RichText::new("MAPPING LIST")
                .font(theme::font(11., 500))
                .color(theme::SECONDARY),
        );
        let filter_response = widgets::search(d, &mut self.mapping_filter, "Find joint…");
        if let Some(r) = &mut self.input_regions {
            r.mapping_controls.insert("filter", filter_response.rect);
        }
        let filter = self.mapping_filter.to_lowercase();
        let list_height = (footer.top() - d.cursor().top() - 8.).max(40.);
        let matches = |l: &&c::Link| {
            filter.is_empty()
                || source.nodes[l.source].name.to_lowercase().contains(&filter)
                || l.target
                    .is_some_and(|n| target.nodes[n].name.to_lowercase().contains(&filter))
        };
        let mut ordered: Vec<_> = links
            .iter()
            .filter(|l| matches(l) || Some(l.source) == self.selected_pair)
            .collect();
        let previous_selection = self.selected_pair;
        ordered.sort_by_key(|l| {
            if Some(l.source) == self.selected_pair && !matches(l) {
                0
            } else {
                1
            }
        });
        let reveal_key = self
            .selected_pair
            .map(|id| (id, self.mapping_filter.clone()));
        let reveal = reveal_key != self.ui_state.mapping_revealed
            || list_height != self.ui_state.mapping_reveal_height
            || detail_body.height()!=self.ui_state.mapping_detail_height;
        if reveal {
            self.ui_state.mapping_reveal_frame = Some(ui.ctx().cumulative_frame_nr());
        }
        let force_reveal =
            self.ui_state.mapping_reveal_frame == Some(ui.ctx().cumulative_frame_nr());
        let visible_ids: Vec<_> = ordered.iter().map(|l| l.source).collect();
        let row_step = 40. + d.spacing().item_spacing.y;
        let mut navigated = false;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("mapping-rows")
            .max_height(list_height)
            .auto_shrink([false, false]);
        if force_reveal
            && let Some(index) = ordered
                .iter()
                .position(|l| Some(l.source) == self.selected_pair)
        {
            scroll = scroll.vertical_scroll_offset(
                (index as f32 * row_step - (list_height - 40.) * 0.5).max(0.),
            );
        }
        let mut selected_rect=None;
        scroll.show(d, |ui| {
            for link in ordered {
                let source_name = &source.nodes[link.source].name;
                let target_name = link
                    .target
                    .map_or("Unmapped", |n| target.nodes[n].name.as_str());
                if !filter.is_empty()
                    && Some(link.source) != self.selected_pair
                    && !source_name.to_lowercase().contains(&filter)
                    && !target_name.to_lowercase().contains(&filter)
                {
                    continue;
                }
                let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), 40.));
                let response = ui.interact(
                    rect,
                    ui.scope_id().with(("mapping-row", link.source)),
                    egui::Sense::click(),
                );
                let active = self.selected_pair == Some(link.source);
                if active {selected_rect=Some(rect);}
                if active || response.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        3.,
                        if active {
                            theme::ELEVATED
                        } else {
                            theme::HOVER
                        },
                    );
                }
                if active {
                    ui.painter().line_segment(
                        [rect.left_top(), rect.left_bottom()],
                        egui::Stroke::new(2., theme::ORANGE),
                    );
                }
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        3.,
                        egui::Stroke::new(1., theme::TEXT),
                        egui::StrokeKind::Inside,
                    );
                }
                c::marker(
                    ui.painter(),
                    rect.left_center() + egui::vec2(12., 0.),
                    link.status,
                    3.,
                );
                let p = ui.painter_at(rect.shrink2(egui::vec2(2., 0.)));
                p.text(
                    rect.left_top() + egui::vec2(25., 5.),
                    egui::Align2::LEFT_TOP,
                    source_name,
                    theme::font(12., 500),
                    theme::TEXT,
                );
                p.text(
                    rect.left_top() + egui::vec2(25., 23.),
                    egui::Align2::LEFT_TOP,
                    target_name,
                    theme::font(11., 400),
                    theme::SECONDARY,
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        ui.is_enabled(),
                        format!("{source_name} to {target_name}, {}", link.status.label()),
                    )
                });
                if let Some(r) = &mut self.input_regions {
                    r.mapping_rows
                        .insert(link.source, rect.intersect(ui.clip_rect()));
                }
                if response.clicked()
                    || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    response.request_focus();
                    self.selected_pair = Some(link.source);
                    self.ui_state.correspondence.orphan_target = None;
                }
                if response.has_focus() && ui.ctx().current_pass_index() == 0 && !navigated {
                    let direction = ui.input(|i| {
                        if i.key_pressed(egui::Key::ArrowDown) {
                            1
                        } else if i.key_pressed(egui::Key::ArrowUp) {
                            -1
                        } else {
                            0
                        }
                    });
                    if direction != 0
                        && let Some(index) = visible_ids.iter().position(|id| *id == link.source)
                    {
                        navigated = true;
                        let next = (index as isize + direction)
                            .clamp(0, visible_ids.len() as isize - 1)
                            as usize;
                        self.selected_pair = Some(visible_ids[next]);
                        ui.memory_mut(|m| {
                            m.request_focus(ui.scope_id().with(("mapping-row", visible_ids[next])))
                        });
                    }
                }
                response.on_hover_text(format!(
                    "{source_name} → {target_name}\n{}",
                    link.status.label()
                ));
            }
        });
        if force_reveal && let Some(rect)=selected_rect {d.scroll_to_rect_animation(rect,Some(egui::Align::BOTTOM),egui::style::ScrollAnimation::none());}
        if previous_selection != self.selected_pair {
            ui.ctx()
                .request_discard("Selected correspondence updates detail and canvas together");
        }
        if reveal && reveal_key.is_some() {
            self.ui_state.mapping_revealed = reveal_key;
            self.ui_state.mapping_reveal_height = list_height;
            self.ui_state.mapping_detail_height=detail_body.height();
            ui.ctx().request_discard("Reveal selected mapping row");
        }
        });
        if let Some(r) = &mut self.input_regions {
            r.utility_scrolls.insert(
                "mapping-detail",
                (scrolled.inner_rect, scrolled.state.clamped_offset().y),
            );
        }
        let paint = c::paint(
            ui,
            &geometry,
            &c::Model {
                rigs: [&rigs[0], &rigs[1]],
                assets: [&source, &target],
                links: &links,
                selected: if matches!(self.ui_state.correspondence.operation, c::Operation::Idle) {
                    self.selected_pair
                } else {
                    None
                },
            },
            &self.ui_state.correspondence,
        );
        if let Some(r) = &mut self.input_regions {
            r.mapping_canvas = canvas_rect;
            r.mapping_joints = geometry
                .joints
                .iter()
                .enumerate()
                .flat_map(|(side, v)| v.iter().map(move |j| ((side, j.node), j.point)))
                .collect();
            r.mapping_labels = paint.labels;
            r.mapping_selected_points = paint.selected_points;
            r.mapping_selected_bones = paint.selected_bones;
            r.mapping_drawn_pair = self.selected_pair;
            r.mapping_shared_scale = geometry.scale;
        }
        let mut h = region(ui, health.shrink2(egui::vec2(20., 6.)), "mapping-health");
        h.horizontal(|ui| {
            self.mapping_actions(ui);
            for (status, label) in [
                (c::Status::Automatic, "automatic"),
                (c::Status::Manual, "manual"),
                (c::Status::Ambiguous, "ambiguous"),
                (c::Status::Unmapped, "unmapped"),
            ] {
                let count = links.iter().filter(|l| l.status == status).count();
                let (r, _) = ui.allocate_exact_size(egui::vec2(8., 16.), egui::Sense::hover());
                c::marker(ui.painter(), r.center(), status, 3.);
                if !compact {
                    ui.label(
                        egui::RichText::new(format!("{count} {label}"))
                            .font(theme::font(11., 400))
                            .color(status.color()),
                    );
                }
                ui.add_space(if compact { 4. } else { 10. });
            }
        });
        let mut t = region(ui, tools.shrink2(egui::vec2(16., 1.)), "canvas-tools");
        t.horizontal(|ui| {
            if widgets::glyph_button(
                ui,
                Icon::Link,
                "Linked view",
                !self.ui_state.correspondence.unlinked,
            )
            .on_hover_text(
                "Synchronize orientation only. Each side keeps its own zoom and pan. Double-click a side to fit it.",
            )
            .clicked()
            {
                self.ui_state.correspondence.unlinked = !self.ui_state.correspondence.unlinked;
                if !self.ui_state.correspondence.unlinked {
                    self.ui_state.correspondence.link_orientation(0);
                }
            }
            if ui.available_width() > 760. {
                ui.label(
                    egui::RichText::new("Middle-drag joint to map · Right drag pan · Double-click fit")
                        .font(theme::font(11., 400))
                        .color(theme::SECONDARY),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let frame = widgets::icon(
                    ui,
                    Icon::Frame,
                    false,
                    "Frame both skeletons independently",
                );
                if let Some(r) = &mut self.input_regions {
                    r.mapping_frame = frame.rect;
                }
                if frame.clicked() {
                    self.ui_state.correspondence.frame();
                }
                ui.toggle_value(&mut self.ui_state.correspondence.true_scale, "True scale")
                    .on_hover_text("Compare physical rig heights. Default fitted views only change display framing, never retarget scale.");
                for (icon, factor, tip) in [
                    (Icon::Plus, 1.15, "Zoom in"),
                    (Icon::Minus, 1. / 1.15, "Zoom out"),
                ] {
                    if widgets::icon(ui, icon, false, tip).clicked() {
                        for camera in &mut self.ui_state.correspondence.cameras {
                            camera.zoom = (camera.zoom * factor).clamp(0.4, 4.);
                        }
                    }
                }
            });
        });
        let mut f = region(ui, footer, "mapping-footer");
        f.separator();
        let dirty = self.retarget.as_ref().is_none_or(|p| {
            p.settings != self.mapping_settings || p.overrides != self.mapping_overrides
        });
        let response = f
            .add_enabled_ui(!self.plan_pending, |ui| {
                widgets::glyph_button(ui, Icon::Save, "Save Mapping", false)
                    .on_hover_text("Compile and persist the current mapping profile")
            })
            .inner;
        if let Some(r) = &mut self.input_regions {
            r.mapping_save = response.rect;
        }
        save |= response.clicked();
        f.label(
            egui::RichText::new(if dirty {
                "Uncompiled changes"
            } else {
                &self.plan_origin
            })
            .font(theme::font(11., 400))
            .color(if dirty {
                theme::WARNING
            } else {
                theme::SUCCESS
            }),
        );
        if !compact {
            f.horizontal(|ui| {
                let reset =
                    ui.add_enabled(!self.plan_pending, egui::Button::new("Reset overrides"));
                if let Some(r) = &mut self.input_regions {
                    r.mapping_reset = reset.rect;
                }
                if reset.clicked() {
                    self.edit_overrides(Vec::new(), self.selected_pair);
                }
                if ui.button("Advanced…").clicked() {
                    self.ui_state.mapping_advanced = !self.ui_state.mapping_advanced;
                }
            });
        } else {
            // Secondary mutations live in an overflow rather than consuming
            // compact canvas/list height. Save and status stay in view.
            let more = egui::Rect::from_min_size(
                egui::pos2(footer.right() - 56., footer.top() + 10.),
                egui::vec2(56., 32.),
            );
            let mut menu = region(ui, more, "compact-mapping-actions");
            menu.menu_button("Actions", |ui| {
                let reset =
                    ui.add_enabled(!self.plan_pending, egui::Button::new("Reset overrides"));
                if let Some(r) = &mut self.input_regions {
                    r.mapping_reset = reset.rect;
                }
                if reset.clicked() {
                    self.edit_overrides(Vec::new(), self.selected_pair);
                    ui.close();
                }
                if ui.button("Advanced settings").clicked() {
                    self.ui_state.mapping_advanced = true;
                    ui.close();
                }
            });
        }
        if self.ui_state.mapping_advanced {
            let ctx = ui.ctx().clone();
            let mut open = true;
            egui::Window::new("Advanced retarget settings")
                .open(&mut open)
                .default_width(470.)
                .show(&ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(400.)
                        .show(ui, |ui| {
                            ui.add_enabled_ui(!self.plan_pending, |ui| {
                                self.mapping_advanced(ui, &source, &target)
                            });
                            if let Some(p) = &self.retarget {
                                for warning in &p.warnings {
                                    ui.colored_label(theme::WARNING, warning);
                                }
                            }
                        });
                });
            self.ui_state.mapping_advanced = open;
        }
        ui.painter().rect_filled(transport, 0., theme::PANEL);
        let mut b = region(
            ui,
            transport.shrink2(egui::vec2(12., 8.)),
            "retarget-transport",
        );
        b.horizontal(|ui| {
            let play = widgets::icon(
                ui,
                if self.playback.playing {
                    Icon::Pause
                } else {
                    Icon::Play
                },
                true,
                "Play / pause · Space",
            );
            if let Some(r) = &mut self.input_regions {
                r.play = play.rect;
            }
            if play.clicked() {
                self.playback.playing = !self.playback.playing;
            }
            ui.label(egui::RichText::new(format!("{:06.3}", self.playback.time)).monospace());
            ui.label(
                egui::RichText::new(format!(
                    "/ {:.3} s",
                    self.source.as_ref().map_or(0., |s| s.1.clips[s.2].duration)
                ))
                .font(theme::font(11., 400))
                .color(theme::SECONDARY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::glyph_button(ui, Icon::Animate, "View animation", false).clicked() {
                    self.mapping_open = false;
                    self.ui_state.workspace = Workspace::Animate;
                }
            });
        });
        let track = egui::Rect::from_min_max(
            egui::pos2(transport.left() + 20., transport.top() + 47.),
            egui::pos2(transport.right() - 20., transport.top() + 61.),
        );
        let seek = ui.interact(
            track,
            ui.scope_id().with("retarget-seek"),
            egui::Sense::click_and_drag(),
        );
        let duration = self.source.as_ref().map_or(0., |s| s.1.clips[s.2].duration);
        if duration > 0.
            && (seek.clicked() || seek.dragged())
            && let Some(pos) = seek.interact_pointer_pos()
        {
            self.playback.time = crate::ui::timeline::time_at(pos.x, track, duration);
            self.playback.cycle = 0;
            self.cursors.clear();
        }
        if let Some(r) = &mut self.input_regions {
            r.seek = track;
        }
        ui.painter().hline(
            track.x_range(),
            track.center().y,
            egui::Stroke::new(2., theme::BORDER),
        );
        let x =
            track.left() + track.width() * (self.playback.time / duration.max(0.001)).clamp(0., 1.);
        ui.painter()
            .vline(x, track.y_range(), egui::Stroke::new(2., theme::ORANGE));
        if !compact {
            b.add_space(20.);
            b.label(
                egui::RichText::new("Rest skeletons · Animation preview opens in Animate")
                    .font(theme::font(11., 400))
                    .color(theme::SECONDARY),
            );
        }
        if settings_before.source_skin != self.mapping_settings.source_skin
            || settings_before.target_skin != self.mapping_settings.target_skin
        {
            self.skin_changed(&settings_before);
        }
        if compile || save || apply_only {
            self.start_mapping(if save {
                super::mapping_apply::Intent::Save
            } else if compile {
                super::mapping_apply::Intent::Preview
            } else {
                super::mapping_apply::Intent::Apply
            });
        }
        self.mapping_details(ui.ctx());
        self.mapping_shortcuts(ui.ctx());
        c::paint_operation(ui, &geometry, &self.ui_state.correspondence.operation);
        self.mapping_panel(ui.ctx(), detail, [&rigs[0], &rigs[1]], [&source, &target]);
        select
    }
}
