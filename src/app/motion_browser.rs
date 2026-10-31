use super::*;

pub(super) struct MotionBrowser {
    pub open: bool,
    pub loading: bool,
    pub index_generation: Option<u64>,
    pub entries: Vec<crate::motion_search::Entry>,
    pub textures: HashMap<String, egui::TextureHandle>,
    pub skipped: usize,
    pub diagnostics: HashMap<String, MotionStatus>,
    pub generation: Generation,
    pub dirty: bool,
    pub query: usize,
    pub ranked_query: Option<usize>,
    pub ranking: Vec<(usize, f64)>,
    pub selected_result: Option<String>,
    pub filter: String,
    pub characters: bool,
    pub pending_preview: Option<PreviewIntent>,
    pub preview_error: Option<String>,
    pub display_pending: Option<(u64, String)>,
    pub displayed_count: u64,
    pub displayed: Vec<serde_json::Value>,
    pub frame_proof: Option<serde_json::Value>,
    pub previews: super::motion_previews::Previews,
}
pub(super) struct PreviewIntent {
    pub record_id: String,
    pub hash: String,
    pub index: usize,
    pub generation: u64,
    pub time: f32,
}
impl MotionBrowser {
    pub fn new(root: PathBuf, ctx: egui::Context) -> Self {
        Self {
            open: false,
            loading: false,
            index_generation: None,
            entries: vec![],
            textures: HashMap::new(),
            skipped: 0,
            diagnostics: HashMap::new(),
            generation: Generation::default(),
            dirty: false,
            query: 0,
            ranked_query: None,
            ranking: vec![],
            selected_result: None,
            filter: String::new(),
            characters: false,
            pending_preview: None,
            preview_error: None,
            display_pending: None,
            displayed_count: 0,
            displayed: vec![],
            frame_proof: None,
            previews: super::motion_previews::Previews::new(root, ctx),
        }
    }
}
pub(super) enum Action {
    Preview(Record, f32),
    TogglePlayback,
    SelectModel(Record),
    OpenAnimate,
}
#[derive(Clone, Debug)]
pub(super) struct MotionStatus {
    pub playable: bool,
    pub reason: String,
    pub preview: Option<image::RgbaImage>,
}
impl Editor {
    pub(super) fn delete_library_record(&mut self, id: &str) {
        let Some(record) = self.records.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        let result = self.library.lock().unwrap().remove(id);
        if let Err(error) = result {
            self.status = format!("Delete failed: {error:#}");
            return;
        }
        self.records.retain(|r| r.id != id);
        self.motion.selected_result = self
            .motion
            .selected_result
            .take()
            .filter(|selected| selected != id);
        let source = matches!(record.kind,Kind::Clip(index) if self.source.as_ref().is_some_and(|s|s.0==record.hash && s.2==index));
        let target =
            record.kind == Kind::Model && self.model.as_ref().is_some_and(|m| m.0 == record.hash);
        if self
            .motion
            .pending_preview
            .as_ref()
            .is_some_and(|p| p.record_id == id)
            || source
            || (matches!(record.kind, Kind::Clip(_))
                && self.pending.contains_key(&(record.hash.clone(), false)))
        {
            self.clip_generation.advance();
            self.motion.pending_preview = None;
            self.motion.display_pending = None;
        }
        if source {
            self.source = None;
            self.source_preview = None;
            self.playback.playing = false;
        }
        if record.kind == Kind::Model
            && (target || self.pending.contains_key(&(record.hash.clone(), true)))
        {
            self.model_generation.advance();
        }
        if target {
            self.model = None;
        }
        if source || target {
            self.refresh_mapping();
        }
        self.motion.entries.retain(|e| e.id != id);
        self.motion.ranked_query = None;
        self.motion.ranking.clear();
        self.motion.query = self
            .motion
            .query
            .min(self.motion.entries.len().saturating_sub(1));
        self.motion.previews.invalidate();
        self.motion.generation.advance();
        self.motion.dirty = true;
        self.status = format!("Deleted {} from Library · original files kept", record.name);
    }
    pub(super) fn request_motion_index(&mut self) {
        self.mapping_open = false;
        self.motion.open = true;
        self.motion.dirty = true;
        self.start_motion_index();
    }
    pub(super) fn start_motion_index(&mut self) {
        if self.motion.loading || self.jobs >= 2 {
            return;
        }
        self.motion.dirty = false;
        let generation = self.motion.generation.advance();
        self.motion.loading = true;
        self.motion.index_generation = Some(generation);
        self.motion.previews.invalidate();
        self.jobs += 1;
        let records: Vec<_> = self
            .records
            .iter()
            .filter(|r| matches!(r.kind, Kind::Clip(_)))
            .take(128)
            .cloned()
            .collect();
        let total = self
            .records
            .iter()
            .filter(|r| matches!(r.kind, Kind::Clip(_)))
            .count();
        let root = self.library.lock().unwrap().root.clone();
        let tx = self.tx.clone();
        let worker = std::thread::spawn(move || {
            let mut entries = Vec::new();
            let mut diagnostics = HashMap::new();
            let mut skipped = total.saturating_sub(records.len());
            for record in records {
                let id = record.id.clone();
                let mut playable = false;
                let mut fallback = None;
                let result = (|| -> Result<_> {
                    let Kind::Clip(index) = record.kind else {
                        unreachable!()
                    };
                    let path = root.join("blobs").join(format!("{}.glb", record.hash));
                    let bytes = import::read_file(&path, &Limits::default())?;
                    anyhow::ensure!(
                        blake3::hash(&bytes).to_hex().as_str() == record.hash,
                        "Blob checksum mismatch"
                    );
                    let asset = import::from_bytes(&bytes, &Limits::default())?;
                    let clip = asset.clips.get(index).context("Missing clip")?;
                    anyhow::ensure!(
                        clip.supported,
                        "Unsupported animation channels or deformation"
                    );
                    playable = true;
                    anyhow::ensure!(
                        asset.cpu_bytes() <= 128 * 1024 * 1024,
                        "Similarity analysis exceeds the 128 MiB asset budget; playback remains available"
                    );
                    fallback = Some(crate::motion_search::preview(&asset, clip, 0.));
                    let summary = crate::motion_search::summarize(&asset, clip)?;
                    let preview =
                        crate::motion_search::preview(&asset, clip, summary.medoids[0].time);
                    Ok(crate::motion_search::Entry {
                        id: record.id,
                        name: if record.name == format!("{} · {}", clip.name, clip.name) {
                            clip.name.clone()
                        } else {
                            record.name
                        },
                        summary,
                        preview,
                    })
                })();
                match result {
                    Ok(entry) => entries.push(entry),
                    Err(error) => {
                        skipped += 1;
                        diagnostics.insert(
                            id,
                            MotionStatus {
                                playable,
                                reason: format!("{error:#}"),
                                preview: fallback,
                            },
                        );
                    }
                }
            }
            let _ = tx.send(Job::Motion {
                generation,
                entries,
                skipped,
                diagnostics,
            });
        });
        self.watch_worker(workers::Kind::Index { generation }, worker);
    }

    pub(super) fn actual_motion_id(&self) -> Option<&str> {
        let (hash, _, index) = self.source.as_ref()?;
        self.records
            .iter()
            .find(|r| r.hash == *hash && r.kind == Kind::Clip(*index))
            .map(|r| r.id.as_str())
    }
    pub(super) fn rank_motion_results(&mut self) {
        if self.motion.entries.is_empty() {
            return;
        }
        self.motion.query = self.motion.query.min(self.motion.entries.len() - 1);
        if self.motion.ranked_query == Some(self.motion.query) {
            return;
        }
        let query = &self.motion.entries[self.motion.query];
        let mut ranking: Vec<_> = self
            .motion
            .entries
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.motion.query)
            .filter_map(|(i, e)| {
                crate::motion_search::similarity(&query.summary, &e.summary)
                    .ok()
                    .map(|score| (i, score))
            })
            .collect();
        ranking.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        self.motion.ranking = ranking;
        self.motion.ranked_query = Some(self.motion.query);
    }
    pub(super) fn motion_ui(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        use crate::ui::icons::Icon;
        self.rank_motion_results();
        let mut action = None;
        let motions: Vec<_> = self
            .records
            .iter()
            .filter(|r| matches!(r.kind, Kind::Clip(_)))
            .cloned()
            .collect();
        if motions.is_empty() {
            widgets::empty(
                ui,
                if self.motion.loading {
                    "Indexing motions…"
                } else {
                    "No motions in your library"
                },
                "Import motions to build your collection. Deleted bundled motions stay deleted.",
            );
            if !self.motion.loading && widgets::primary(ui, "Index motions").clicked() {
                self.request_motion_index();
            }
            return None;
        }
        if !self.motion.entries.is_empty() {
            let query_rect =
                egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 48.));
            let mut query = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("library-query")
                    .max_rect(query_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            query.label(
                egui::RichText::new("Similar to")
                    .font(theme::font(12., 400))
                    .color(theme::SECONDARY),
            );
            egui::ComboBox::from_id_salt("motion-query")
                .width(148.)
                .selected_text(&self.motion.entries[self.motion.query].name)
                .show_ui(&mut query, |ui| {
                    for (i, entry) in self.motion.entries.iter().enumerate() {
                        ui.selectable_value(&mut self.motion.query, i, &entry.name);
                    }
                });
            if let Some(id) = &self.motion.selected_result
                && let Some(entry) = self.motion.entries.iter().find(|e| e.id == *id)
            {
                query.add(
                    egui::Label::new(
                        egui::RichText::new(format!("Selected: {}", entry.name))
                            .font(theme::font(12., 500)),
                    )
                    .truncate(),
                );
            }
            query.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new("Distance")
                        .font(theme::font(11., 400))
                        .color(theme::SECONDARY),
                );
            });
            ui.allocate_space(query_rect.size());
            self.rank_motion_results();
        } else {
            ui.label(if self.motion.loading {
                "Indexing motions…"
            } else {
                "Motions available, similarity unavailable"
            });
        }
        let preview_id = self.actual_motion_id().map(str::to_owned);
        let pins = self
            .motion
            .entries
            .iter()
            .filter(|e| {
                Some(&e.id) == self.motion.selected_result.as_ref()
                    || Some(&e.id) == preview_id.as_ref()
            })
            .filter_map(|e| {
                self.records
                    .iter()
                    .find(|r| r.id == e.id)
                    .map(|r| super::motion_previews::key(r, &e.summary))
            })
            .collect();
        self.motion.previews.pin(pins);
        let filter = self.motion.filter.to_lowercase();
        let mut ranking = self.motion.ranking.clone();
        if !self.motion.entries.is_empty() {
            ranking.insert(0, (self.motion.query, 0.));
        }
        let height = (ui.available_height() - 24.).max(40.);
        let scroll=egui::ScrollArea::vertical().id_salt("motion-results").max_height(height).auto_shrink([false,false]).show(ui,|ui| {
            let mut visible = 0;
            for record in &motions {
                if self.motion.entries.iter().any(|e| e.id == record.id) || !record.name.to_lowercase().contains(&filter) { continue; }
                visible += 1;
                let status = self.motion.diagnostics.get(&record.id);
                let playable = status.is_none_or(|s| s.playable);
                let selected = self.motion.selected_result.as_ref() == Some(&record.id);
                let frame = egui::Frame::new().fill(if selected {theme::ELEVATED} else {theme::PANEL}).inner_margin(12.);
                let row = frame.show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if let Some(texture) = self.motion.textures.get(&record.id) {
                            ui.image((texture.id(), egui::vec2(88., 88.)));
                        }
                        ui.vertical(|ui| {
                            if ui.selectable_label(selected, &record.name).clicked() { self.motion.selected_result = Some(record.id.clone()); }
                            ui.label(egui::RichText::new(if status.is_some() {
                                if playable { "Playable · similarity unavailable" } else { "Imported · cannot play" }
                            } else if self.motion.loading { "Imported · analysis pending" } else { "Imported · descriptor not indexed (128-clip budget)" }).color(theme::SECONDARY));
                            let active = preview_id.as_ref() == Some(&record.id);
                            let response = ui.add_enabled(playable, egui::Button::new(if active { "Play / pause preview" } else { "Preview" }));
                            if response.clicked() { action = Some(if active {Action::TogglePlayback} else {Action::Preview(record.clone(), 0.)}); }
                            if let Some(r) = &mut self.input_regions { r.motion_preview_buttons.insert(record.id.clone(), response.rect); }
                            if let Some(status) = status { egui::CollapsingHeader::new("Analysis details").id_salt(&record.id).show(ui, |ui| { ui.label(&status.reason); }); }
                        });
                    });
                });
                if let Some(r) = &mut self.input_regions { r.motion_results.insert(record.id.clone(), row.response.rect.intersect(ui.clip_rect())); }
                ui.add_space(8.);
            }
            for (rank,(index,distance)) in ranking.iter().enumerate() {
                let (id,name,summary)= {
                    let entry=&self.motion.entries[*index];
                    (entry.id.clone(),entry.name.clone(),entry.summary.clone())
                };
                let Some(record)=self.records.iter().find(|r|r.id==id).cloned() else {continue;};
                if !filter.is_empty()&&!name.to_lowercase().contains(&filter)&&!record.name.to_lowercase().contains(&filter) {continue;}
                visible += 1;
                let selected=self.motion.selected_result.as_ref()==Some(&id);
                let active=preview_id.as_ref()==Some(&id);
                let row_height=if selected {194.}else{120.};
                let (_,rect)=ui.allocate_space(egui::vec2(ui.available_width(),row_height));
                if !ui.is_rect_visible(rect) {continue;}
                self.motion.previews.request(&record,&summary);
                let key=super::motion_previews::key(&record,&summary);
                let strip=self.motion.previews.get(&key);
                let response=ui.interact(rect,ui.scope_id().with(("motion-result",&id)),egui::Sense::click());
                let p=ui.painter_at(rect);
                let fill=if selected {theme::ELEVATED}else if active {theme::SELECTED}else if response.hovered() {theme::PANEL}else{theme::BACKGROUND};
                p.rect_filled(rect,3.,fill);
                if selected {p.line_segment([rect.left_top(),rect.left_bottom()],egui::Stroke::new(2.,theme::TEXT));}
                if active {p.line_segment([rect.left_top()+egui::vec2(2.,0.),rect.left_bottom()+egui::vec2(2.,0.)],egui::Stroke::new(2.,theme::ORANGE));}
                if response.has_focus() {p.rect_stroke(rect,3.,egui::Stroke::new(1.,theme::TEXT),egui::StrokeKind::Inside);}
                p.text(rect.left_top()+egui::vec2(18.,13.),egui::Align2::LEFT_TOP,format!("{:02}",rank+1),egui::FontId::monospace(11.),theme::SECONDARY);
                p.text(rect.right_top()+egui::vec2(-16.,12.),egui::Align2::RIGHT_TOP,format!("{distance:.4}"),egui::FontId::monospace(12.),theme::TEXT);
                let title_x=if selected {46.}else{184.};
                let title=egui::pos2(rect.left()+title_x,rect.top()+12.);
                let title_max=if selected {166.} else {(rect.width()-title_x-104.).max(60.)};
                let mut job=egui::text::LayoutJob::simple_singleline(name.clone(),theme::font(18.,500),theme::TEXT);
                job.wrap.max_width=title_max;job.wrap.max_rows=1;job.wrap.overflow_character=Some('…');
                p.galley(title,ui.fonts_mut(|f|f.layout_job(job)),theme::TEXT);
                let representative=summary.medoids.first().map_or(0.,|m|m.time);
                p.text(title+egui::vec2(0.,32.),egui::Align2::LEFT_TOP,if selected {format!("{:.2} s",summary.duration)}else{format!("{:.2} s · pose {representative:.3} s",summary.duration)},egui::FontId::monospace(12.),theme::SECONDARY);
                let pending=self.motion.pending_preview.as_ref().is_some_and(|p|p.record_id==id);
                let button_y=if selected {104.}else{66.};
                let button=egui::Rect::from_min_size(egui::pos2(title.x,rect.top()+button_y),egui::vec2(if active {150.}else{112.},32.));
                let button_response=ui.interact(button,ui.scope_id().with(("preview-motion",&id)),egui::Sense::click());
                if button_response.hovered() {p.rect_filled(button,3.,theme::HOVER);}
                crate::ui::icons::paint(&p,egui::Rect::from_center_size(button.left_center()+egui::vec2(10.,0.),egui::vec2(16.,16.)),if active&&self.playback.playing {Icon::Pause}else{Icon::Play},if active {theme::ORANGE}else{theme::SECONDARY});
                let label=if pending {"Loading preview…"}else if active&&self.playback.playing {"Previewing now"}else if active {"Preview paused"}else{"Preview"};
                p.text(button.left_center()+egui::vec2(26.,0.),egui::Align2::LEFT_CENTER,label,theme::font(12.,500),if active {theme::ORANGE}else{theme::SECONDARY});
                button_response.widget_info(||egui::WidgetInfo::labeled(egui::Role::Button,ui.is_enabled(),format!("{label} {name}")));
                let mut child_clicked=button_response.clicked();
                if button_response.clicked() {action=Some(if active {Action::TogglePlayback}else{Action::Preview(record.clone(),representative)});}
                if let Some(r)=&mut self.input_regions {r.motion_preview_buttons.insert(id.clone(),button);}
                if selected {
                    crate::ui::icons::paint(&p,egui::Rect::from_min_size(title+egui::vec2(0.,62.),egui::vec2(16.,16.)),Icon::Check,theme::TEXT);
                    p.text(title+egui::vec2(24.,70.),egui::Align2::LEFT_CENTER,"Selected result",theme::font(12.,500),theme::TEXT);
                    p.text(title+egui::vec2(0.,144.),egui::Align2::LEFT_TOP,if strip.is_some() {format!("{} real poses",summary.medoids.len())}else{"Preparing poses…".into()},theme::font(11.,400),theme::SECONDARY);
                    p.text(title+egui::vec2(0.,162.),egui::Align2::LEFT_TOP,"Shared ¾ framing",theme::font(11.,400),theme::SECONDARY);
                }
                let count=if selected {summary.medoids.len().min(4)}else{1};
                let width=if selected {((rect.width()-if rect.width()<800. {218.}else{262.})/count.max(1) as f32-12.).clamp(72.,130.)}else{116.};
                let height=if selected {width*135./130.}else{104.};
                let start_x=if selected {rect.right()-32.-count as f32*(width+12.)+12.}else{rect.left()+48.};
                for column in 0..count {
                    let field=egui::Rect::from_min_size(egui::pos2(start_x+column as f32*(width+12.),rect.top()+if selected {32.}else{8.}),egui::vec2(width,height));
                    p.rect_filled(field,2.,theme::BACKGROUND);
                    let frame=strip.as_ref().and_then(|strip|if selected {strip.frames.get(column)}else{strip.frames.iter().find(|f|f.time==representative).or_else(||strip.frames.first())});
                    if let Some(frame)=frame {
                        let factor=(field.width()/frame.texture.size_vec2().x).min(field.height()/frame.texture.size_vec2().y);
                        let image=egui::Rect::from_center_size(field.center(),frame.texture.size_vec2()*factor);
                        p.image(frame.texture.id(),image,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);
                        let pose=ui.interact(field,ui.scope_id().with(("medoid",&id,column)),egui::Sense::click());
                        if pose.hovered()||pose.has_focus() {p.rect_stroke(field,2.,egui::Stroke::new(1.,theme::TEXT),egui::StrokeKind::Inside);}
                        pose.widget_info(||egui::WidgetInfo::labeled(egui::Role::Button,ui.is_enabled(),format!("Preview {name} at {:.6} seconds",frame.time)));
                        if pose.clicked() {action=Some(Action::Preview(record.clone(),frame.time));child_clicked=true;}
                        if let Some(r)=&mut self.input_regions {r.motion_medoid_previews.insert((id.clone(),column),(field,frame.time));}
                        pose.on_hover_text(format!("{name} · actual representative pose {:.6} s\nClick to preview this exact pose",frame.time));
                        if selected {p.text(egui::pos2(field.center().x,field.bottom()+8.),egui::Align2::CENTER_TOP,format!("{:.3} s",frame.time),egui::FontId::monospace(11.),theme::SECONDARY);}
                    } else {
                        p.text(field.center(),egui::Align2::CENTER_CENTER,if self.motion.previews.error(&key).is_some() {"Unavailable"}else{"Loading…"},theme::font(11.,400),theme::MUTED);
                    }
                }
                if response.clicked()&&!child_clicked&&!selected&&ui.ctx().current_pass_index()==0 {self.motion.selected_result=Some(id.clone());response.request_focus();ui.ctx().request_discard("Expand selected motion poses");}
                response.widget_info(||egui::WidgetInfo::labeled(egui::Role::Button,ui.is_enabled(),format!("{name}, rank {}, duration {:.3} seconds, distance {distance:.4}, {}{}",rank+1,summary.duration,if selected {"selected result"}else{""},if active {", active preview"}else{""})));
                if let Some(r)=&mut self.input_regions {r.motion_results.insert(id.clone(),rect.intersect(ui.clip_rect()));}
                response.on_hover_text(format!("{name}\nDistance {distance:.6}; motion similarity is separate from retarget compatibility."));
                p.hline(rect.x_range(),rect.bottom()+4.,egui::Stroke::new(1.,theme::BORDER));
            }
            if visible == 0 { widgets::empty(ui, "No matching motions", "Clear the search to show all imported clips."); }
        });
        if let Some(r) = &mut self.input_regions {
            r.motion_scroll = scroll.inner_rect;
        }
        ui.label(egui::RichText::new("Same motion family can share poses; duration distinguishes tempo. Similarity is separate from compatibility.").font(theme::font(11.,400)).color(theme::SECONDARY));
        if self.motion.previews.pending() {
            ui.ctx().request_repaint();
        }
        action
    }
}
