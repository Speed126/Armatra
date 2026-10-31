use super::*;
use widgets::Icon;

impl Editor {
    pub(super) fn ui_evidence(&self) -> serde_json::Value {
        let camera = self
            .source_preview
            .as_ref()
            .map_or(&self.camera, |p| &p.camera);
        let rect = |r: egui::Rect| [r.left(), r.top(), r.right(), r.bottom()];
        let geometry=self.input_regions.as_ref().map(|r|serde_json::json!({
            "status":rect(r.status_rect),"workspace":rect(r.workspace_rect),"viewport":rect(r.viewport),
            "selected_row":self.selected_pair.and_then(|id|r.mapping_rows.get(&id)).map(|r|rect(*r)),
            "canvas":rect(r.mapping_canvas),"source_display_scale":r.mapping_shared_scale,"true_scale":self.ui_state.correspondence.true_scale,
            "selected_labels":r.mapping_labels.iter().map(|r|rect(*r)).collect::<Vec<_>>(),
            "selected_points":r.mapping_selected_points.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),
            "selected_bones":r.mapping_selected_bones.iter().map(|b|[[b[0].x,b[0].y],[b[1].x,b[1].y]]).collect::<Vec<_>>(),
            "drawn_pair":r.mapping_drawn_pair
        }));
        let timeline=self.input_regions.as_ref().map(|r|serde_json::json!({
            "track":rect(r.seek),"clip":rect(r.timeline_clip),"hover":r.timeline_hover,
            "bars":r.timeline_contacts.iter().map(|b|serde_json::json!({"side":format!("{:?}",b.lane),"source":b.source,"target":b.target,"start":b.start,"end":b.end,"rect":rect(b.rect)})).collect::<Vec<_>>()
        }));
        let mappings=self.retarget.as_ref().map(|plan|self.mapping_links(&plan.rows).iter().map(|l|serde_json::json!({"source":l.source,"target":l.target,"status":l.status.label()})).collect::<Vec<_>>());
        serde_json::json!({
            "workspace":format!("{:?}",self.ui_state.workspace),"skinning":self.skinning_report,
            "stage":{
                "camera":{"target":camera.target.to_array(),"distance":camera.distance,"radius":camera.radius,"yaw":camera.yaw,"pitch":camera.pitch},"original_source_preview":self.source_preview.is_some(),
                "mesh":self.mesh,"skeleton":self.skeleton,"bind_pose":self.bind_pose,"correction":self.contact_correction,
                "procedural_fixture":self.procedural_fixture(),"grid":self.ui_state.preferences.grid,"axes":self.ui_state.preferences.axes,
                "grid_units":self.source_preview.as_ref().and_then(|p|p.resident.grids.get(p.asset.default_scene)).or_else(||self.model.as_ref().and_then(|m|m.2.grids.get(self.scene)))
            },
            "utility_windows":self.input_regions.as_ref().map(|r|r.utility_windows.iter().map(|(name,value)|(*name,rect(*value))).collect::<std::collections::BTreeMap<_,_>>()),
            "utility_scrolls":self.input_regions.as_ref().map(|r|r.utility_scrolls.iter().map(|(name,(bounds,offset))|(*name,serde_json::json!({"rect":rect(*bounds),"offset":offset}))).collect::<std::collections::BTreeMap<_,_>>()),
            "mappings":mappings,
            "mapping_edit":self.mapping_evidence(),"active_plan_id":self.active_plan_id(),
            "mapping_controls":self.input_regions.as_ref().map(|r|r.mapping_controls.iter().map(|(name,r)|(*name,rect(*r))).collect::<std::collections::BTreeMap<_,_>>()),
            "mapping_confirm_enabled":self.input_regions.as_ref().is_some_and(|r|r.mapping_confirm_enabled),
            "motion":{"selected":self.motion.selected_result,"previewing":self.actual_motion_id(),"imported":self.records.iter().filter(|r|matches!(r.kind,Kind::Clip(_))).count(),"similarity_ready":self.motion.entries.len(),"diagnostics":self.motion.diagnostics.iter().map(|(id,s)|(id,serde_json::json!({"playable":s.playable,"reason":s.reason}))).collect::<std::collections::BTreeMap<_,_>>(),"source_generation":self.source_generation,"pending_generation":self.motion.pending_preview.as_ref().map(|p|p.generation),"pose_texture_bytes":self.motion.previews.bytes(),"preview_error":self.motion.preview_error,"displayed_count":self.motion.displayed_count,"displayed":self.motion.displayed},
            "preview_placement":if self.ui_state.preferences.continuous {"Continuous locomotion"} else {"Repeat at origin"},
            "selected_pair":self.selected_pair,"joint_selection_persisted":false,
            "plan_pending":self.plan_pending,"pending_overrides":self.mapping_overrides,
            "mapping_operation":{"phase":self.mapping_feedback.phase,"message":self.mapping_feedback.message,"details":self.mapping_feedback.details,"saved_path":self.mapping_feedback.saved_path,"generation":self.plan_generation.0,"active_pair":self.active_pair,"active_plan":self.retarget.as_ref().map(|p|&p.key),"active_settings":self.retarget.as_ref().map(|p|&p.settings),"active_overrides":self.retarget.as_ref().map(|p|&p.overrides),"request":self.mapping_feedback.request.as_ref().map(|q|serde_json::json!({"generation":q.generation,"source":q.source_hash,"target":q.target_hash,"clip":q.clip,"source_skin":q.settings.source_skin,"target_skin":q.settings.target_skin,"overrides":q.overrides,"intent":format!("{:?}",q.intent)})),"status":self.status},
            "rig_skins":{"source":self.mapping_settings.source_skin,"target":self.mapping_settings.target_skin},
            "rig_context":{
                "source":self.source.as_ref().map(|s|serde_json::json!({"asset":s.0,"clip":s.2,"clip_name":s.1.clips[s.2].name,"tracks":s.1.clips[s.2].tracks.len(),"skin_count":s.1.skins.len(),"selected_skin":self.mapping_settings.source_skin,"joint_ids":crate::rig::Rig::analyze_skin(&s.1,self.mapping_settings.source_skin).joints.iter().map(|j|j.node).collect::<Vec<_>>()})),
                "target":self.model.as_ref().map(|t|serde_json::json!({"asset":t.0,"skin_count":t.1.skins.len(),"selected_skin":self.mapping_settings.target_skin,"joint_ids":crate::rig::Rig::analyze_skin(&t.1,self.mapping_settings.target_skin).joints.iter().map(|j|j.node).collect::<Vec<_>>()}))
            },
            "plan_dirty":self.retarget.as_ref().is_none_or(|p|p.settings!=self.mapping_settings||p.overrides!=self.mapping_overrides),
            "time":self.playback.time,"cycle":self.playback.cycle,"playing":self.playback.playing,"timeline":timeline,
            "drawer":format!("{:?}",self.ui_state.drawer.open),"drawer_rect":rect(self.ui_state.drawer.rect),
            "normal_preferences":{"assets_width":self.ui_state.preferences.assets_width,"inspector_width":self.ui_state.preferences.inspector_width,"timeline_height":self.ui_state.preferences.timeline_height},
            "cameras":self.ui_state.correspondence.cameras.map(|c|serde_json::json!({"yaw":c.yaw,"pitch":c.pitch,"zoom":c.zoom,"pan":[c.pan.x,c.pan.y]})),
            "geometry":geometry
        })
    }
    pub(super) fn model_name(&self) -> &str {
        self.model
            .as_ref()
            .and_then(|m| {
                self.records
                    .iter()
                    .find(|r| r.hash == m.0 && r.kind == Kind::Model)
            })
            .map_or("No character selected", |r| r.name.as_str())
    }

    pub(super) fn ui(
        &mut self,
        root: &mut egui::Ui,
        texture: egui::TextureId,
        renderer: &Renderer,
        gpu: &Gpu,
    ) -> [u32; 2] {
        if let Some(r) = &mut self.input_regions {
            *r = InputRegions::default();
        }
        let ctx = root.ctx().clone();
        if self.mapping_open {
            self.ui_state.workspace = Workspace::Retarget;
        }
        if self.motion.open {
            self.ui_state.workspace = Workspace::Library;
        }
        self.ui_state.drawer.begin(&ctx);
        self.ui_state.drawer.controls.clear();
        let mut import = false;
        egui::Panel::top("header").exact_size(52.).frame(egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(16,8))).show(root, |ui| {
            ui.horizontal_centered(|ui| {
                let (brand,_)=ui.allocate_exact_size(egui::vec2(168.,32.),egui::Sense::hover());
                crate::ui::icons::mark(ui.painter(),egui::Rect::from_min_size(brand.min,egui::vec2(28.,32.)));
                crate::ui::icons::wordmark(ui.painter(),egui::Rect::from_min_size(brand.min+egui::vec2(36.,5.),egui::vec2(100.,22.)));
                ui.add_space(24.);
                for (workspace, label, icon) in [(Workspace::Animate, "Animate",Icon::Animate), (Workspace::Retarget, "Retarget",Icon::Retarget), (Workspace::Library, "Library",Icon::Library)] {
                    let response = widgets::glyph_button(ui,icon,label,self.ui_state.workspace == workspace);
                    if let Some(r) = &mut self.input_regions {
                        match workspace {
                            Workspace::Retarget => r.mapping_open = response.rect,
                            Workspace::Library => r.motion_open = response.rect,
                            Workspace::Animate => r.animate_open = response.rect,
                        }
                    }
                    if response.clicked() {
                        self.ui_state.drawer.close(&ctx, false);
                        self.ui_state.workspace = workspace;
                        self.mapping_open = workspace == Workspace::Retarget;
                        self.motion.open = false;
                        if workspace == Workspace::Library {
                            if self.motion.entries.is_empty() { self.request_motion_index(); }
                            else { self.motion.open = true; }
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    import = ui.add_enabled_ui(self.jobs < 2, |ui| widgets::primary(ui, "+  Import")).inner
                        .on_hover_text("Import GLB, local glTF, or BVH skeletal motion · Ctrl+O\nYou can also drop files anywhere in the window.").clicked();
                    if ui.available_width()>240. && ui.button("Help").clicked() { self.ui_state.help = !self.ui_state.help; }
                    if widgets::icon(ui, Icon::Settings, self.ui_state.settings, "Settings").clicked() { self.ui_state.settings = !self.ui_state.settings; }
                    if widgets::icon(ui, Icon::Labs, self.ui_state.labs, "Labs / Diagnostics").clicked() { self.ui_state.labs = !self.ui_state.labs; }
                });
            });
        });
        let status = egui::Panel::bottom("status")
            .exact_size(24.)
            .frame(
                egui::Frame::NONE
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(14, 4)),
            )
            .show(root, |ui| {
                ui.spacing_mut().interact_size.y = 14.;
                ui.horizontal(|ui| {
                    if self.jobs > 0 {
                        ui.add(egui::Spinner::new().size(14.).color(theme::ORANGE));
                    }
                    ui.add(
                        egui::Label::new(egui::RichText::new(&self.status).size(12.).color(
                            if self.status.contains("failed")
                                || self.status.starts_with("Failed")
                                || self.status.starts_with("Retarget:")
                            {
                                theme::ERROR
                            } else {
                                theme::SECONDARY
                            },
                        ))
                        .truncate(),
                    )
                    .on_hover_text(&self.status);
                });
            });
        if let Some(r) = &mut self.input_regions {
            r.status_rect = status.response.rect;
        }
        let retarget = self.ui_state.workspace == Workspace::Retarget;
        let library = self.ui_state.workspace == Workspace::Library;
        let mut select = None;
        let mut motion_action = None;
        if !retarget && !self.records.is_empty() {
            let compact = ctx.content_rect().width() < 1050. || ctx.content_rect().height() < 650.;
            let timeline = egui::Panel::bottom(if library {
                "timeline-library"
            } else if compact {
                "timeline-compact"
            } else {
                "timeline"
            })
            .resizable(false)
            .exact_size(if library {
                82.
            } else if compact {
                124.
            } else {
                self.ui_state.preferences.timeline_height
            })
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(root, |ui| self.timeline_ui(ui));
            if !compact && !library {
                let r = timeline.response.rect;
                crate::ui::drawers::resize(
                    root,
                    "timeline-resize",
                    egui::Rect::from_min_max(r.left_top(), r.right_top() + egui::vec2(0., 4.)),
                    &mut self.ui_state.preferences.timeline_height,
                    true,
                    -1.,
                    130. ..=240.,
                );
            }
            if !library && ctx.content_rect().width() >= 1050. {
                let assets = egui::Panel::left("workspace-normal-assets")
                    .resizable(false)
                    .exact_size(self.ui_state.preferences.assets_width)
                    .frame(theme::panel())
                    .show(root, |ui| self.assets_ui(ui));
                select = assets.inner;
                let r = assets.response.rect;
                crate::ui::drawers::resize(
                    root,
                    "assets-resize",
                    egui::Rect::from_min_max(r.right_top() - egui::vec2(4., 0.), r.right_bottom()),
                    &mut self.ui_state.preferences.assets_width,
                    false,
                    1.,
                    220. ..=400.,
                );
            }
            if !library && ctx.content_rect().width() >= 900. {
                let inspector = egui::Panel::right("workspace-normal-inspector")
                    .resizable(false)
                    .exact_size(self.ui_state.preferences.inspector_width)
                    .frame(theme::panel())
                    .show(root, |ui| self.inspector_ui(ui));
                let r = inspector.response.rect;
                crate::ui::drawers::resize(
                    root,
                    "inspector-resize",
                    egui::Rect::from_min_max(r.left_top(), r.left_bottom() + egui::vec2(4., 0.)),
                    &mut self.ui_state.preferences.inspector_width,
                    false,
                    -1.,
                    240. ..=400.,
                );
            }
        }
        let mut dimensions = [1, 1];
        let mut workspace_rect = egui::Rect::NOTHING;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BACKGROUND))
            .show(root, |ui| {
                workspace_rect = ui.max_rect();
                if let Some(r) = &mut self.input_regions {
                    r.workspace_rect = ui.max_rect();
                }
                match self.ui_state.workspace {
                    Workspace::Retarget => {
                        if let Some(record) = self.mapping_ui(ui, ui.available_height()) {
                            select = Some(record);
                        }
                    }
                    Workspace::Library => {
                        (dimensions, motion_action) = self.library_workspace(ui, texture);
                    }
                    Workspace::Animate => {
                        let (d, record) = self.viewport_ui(ui, texture);
                        dimensions = d;
                        if record.is_some() {
                            select = record;
                        }
                    }
                }
            });
        if let Some(record) = self.drawer_ui(&ctx, workspace_rect) {
            select = Some(record);
        }
        self.utility_ui(&ctx, gpu);
        if !ctx.egui_wants_keyboard_input() && ctx.current_pass_index() == 0 {
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) && self.source.is_some() {
                self.playback.playing = !self.playback.playing;
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
                import = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F)) {
                self.frame_character();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
                self.seek_start();
            }
        }
        if import {
            self.import(None);
        }
        if let Some(record) = select {
            self.request(&record, renderer, gpu);
        }
        if ctx.current_pass_index() == 0
            && let Some(action) = motion_action
        {
            match action {
                super::motion_browser::Action::Preview(record, time) => {
                    self.request_with_preview(&record, Some(time), renderer, gpu);
                }
                super::motion_browser::Action::TogglePlayback => {
                    self.playback.playing = !self.playback.playing
                }
                super::motion_browser::Action::SelectModel(record) => {
                    self.request(&record, renderer, gpu)
                }
                super::motion_browser::Action::OpenAnimate => {
                    self.ui_state.workspace = Workspace::Animate;
                    self.motion.open = false;
                    self.mapping_open = false;
                }
            }
        }
        dimensions
    }

    fn seek_start(&mut self) {
        self.playback.time = 0.;
        self.playback.cycle = 0;
        self.cursors.clear();
    }

    fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        let duration = self.source.as_ref().map_or(0., |s| s.1.clips[s.2].duration);
        let name = self
            .source
            .as_ref()
            .map_or("No motion", |s| s.1.clips[s.2].name.as_str())
            .to_owned();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(duration > 0., |ui| {
                if widgets::transport(ui, Icon::Start, false, false, "Jump to start · Home")
                    .clicked()
                {
                    self.seek_start();
                }
                let play = widgets::transport(
                    ui,
                    if self.playback.playing {
                        Icon::Pause
                    } else {
                        Icon::Play
                    },
                    true,
                    true,
                    "Play / pause · Space",
                );
                if let Some(r) = &mut self.input_regions {
                    r.play = play.rect;
                }
                if play.clicked() {
                    self.playback.playing = !self.playback.playing;
                }
                if widgets::transport(
                    ui,
                    Icon::Loop,
                    false,
                    self.playback.looping,
                    "Loop playback",
                )
                .clicked()
                {
                    self.playback.looping = !self.playback.looping;
                    self.playback.cycle = 0;
                }
            });
            ui.add_space(12.);
            ui.label(
                egui::RichText::new(format!("{:06.3}", self.playback.time))
                    .font(egui::FontId::monospace(20.)),
            );
            ui.label(
                egui::RichText::new(format!("/ {duration:.3} s"))
                    .font(egui::FontId::monospace(11.))
                    .color(theme::SECONDARY),
            );
            ui.add_space(14.);
            let speed = ui
                .add(
                    egui::DragValue::new(&mut self.playback.speed)
                        .speed(0.05)
                        .range(-4. ..=4.)
                        .suffix("×"),
                )
                .on_hover_text("Playback speed · drag or type · negative values play backward");
            if let Some(r) = &mut self.input_regions {
                r.speed = speed.rect;
            }
            self.loop_placement_ui(ui, "timeline-placement");
            if ui.available_width() > 180. {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&name)
                                .font(theme::font(12., 400))
                                .color(theme::SECONDARY),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&name);
                });
            }
        });
        let lanes = if let (Some(contacts), Some((_, source, _)), Some((_, target, _))) =
            (&self.contacts, &self.source, &self.model)
        {
            let source_rig =
                crate::rig::Rig::analyze_skin(source, self.mapping_settings.source_skin);
            let target_rig =
                crate::rig::Rig::analyze_skin(target, self.mapping_settings.target_skin);
            crate::ui::timeline::contact_lanes(
                &contacts.intervals,
                [&source_rig, &target_rig],
                [source, target],
            )
        } else {
            vec![]
        };
        let model = crate::ui::timeline::Model {
            name: &name,
            duration,
            time: self.playback.time,
            lanes: &lanes,
        };
        let output = if self.ui_state.workspace == Workspace::Library {
            crate::ui::timeline::mini(ui, &model)
        } else {
            crate::ui::timeline::show(ui, &model)
        };
        if let Some(time) = output.seek {
            self.playback.time = time;
            self.playback.cycle = 0;
            self.cursors.clear();
        }
        if let Some(r) = &mut self.input_regions {
            r.seek = output.track;
            r.timeline_clip = output.clip;
            r.timeline_hover = output.hover;
            r.timeline_contacts = output.bars;
        }
    }
}
