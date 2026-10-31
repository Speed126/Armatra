//! The editable mapping, compile request and active preview are distinct states.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Intent {
    Restore,
    Apply,
    Preview,
    Save,
}
#[derive(Clone)]
pub(super) struct Request {
    pub generation: u64,
    pub source_hash: String,
    pub target_hash: String,
    pub source: Arc<Asset>,
    pub target: Arc<Asset>,
    pub clip: usize,
    pub settings: Settings,
    pub overrides: Vec<Override>,
    pub intent: Intent,
    pub label: String,
}
#[derive(Default)]
pub(super) struct Feedback {
    pub phase: &'static str,
    pub message: String,
    pub details: String,
    pub request: Option<Request>,
    pub saved_path: Option<PathBuf>,
    pub show_details: bool,
}
pub(super) type DraftKey = (String, String, Option<usize>, Option<usize>);
fn influence_warnings(q: &Request) -> Vec<String> {
    let Some(skin) = q
        .settings
        .target_skin
        .or((q.target.skins.len() == 1).then_some(0))
    else {
        return vec![];
    };
    let mut weighted = std::collections::HashSet::new();
    for node in &q.target.nodes {
        if node.skin != Some(skin) {
            continue;
        }
        if let Some(mesh) = node.mesh {
            for primitive in &q.target.meshes[mesh].primitives {
                for vertex in &primitive.vertices {
                    for (&joint, &weight) in vertex.joints.iter().zip(&vertex.weights) {
                        if weight > 0.
                            && let Some(&id) = q.target.skins[skin].joints.get(joint as usize)
                        {
                            weighted.insert(id);
                        }
                    }
                }
            }
        }
    }
    q.overrides.iter().filter_map(|o|{
        let target=o.target?;
        if !q.source.clips[q.clip].tracks.iter().any(|t|t.node==o.source){return None;}
        let mut descendants=vec![target];
        while let Some(n)=descendants.pop(){if weighted.contains(&n)||q.target.nodes[n].mesh.is_some(){return None;}descendants.extend(q.target.nodes[n].children.iter().copied());}
        Some(format!("{} → {} changes an unweighted end joint with no mesh descendants. Map a weighted bone to move the selected skin's mesh.",q.source.nodes[o.source].name,q.target.nodes[target].name))
    }).collect()
}
impl Editor {
    pub(super) fn active_plan_id(&self) -> Option<String> {
        self.retarget.as_ref().map(|p| {
            blake3::hash(
                &serde_json::to_vec(&(
                    &p.key,
                    &p.overrides,
                    &p.transfers,
                    &p.chains,
                    p.frame,
                    p.scale,
                    p.motion_source,
                    p.motion_target,
                ))
                .expect("validated finite plan"),
            )
            .to_hex()
            .to_string()
        })
    }
    pub(super) fn mapping_message(&mut self, phase: &'static str, message: String) {
        self.status = format!("{phase} · {message}");
        self.mapping_feedback.phase = phase;
        self.mapping_feedback.message = message;
        self.mapping_feedback.details.clear();
        self.mapping_feedback.saved_path = None;
    }
    pub(super) fn pending_mapping(&mut self) {
        self.mapping_feedback.request = None;
        self.mapping_message(
            "Pending",
            if self.mapping_overrides.is_empty() {
                "Mapping settings · Pending apply".into()
            } else {
                "Manual mapping · Pending apply".into()
            },
        );
    }
    fn request_current(&self, q: &Request) -> bool {
        self.plan_generation.accepts(q.generation)
            && self
                .source
                .as_ref()
                .is_some_and(|s| s.0 == q.source_hash && s.2 == q.clip)
            && self.model.as_ref().is_some_and(|t| t.0 == q.target_hash)
            && self.mapping_settings == q.settings
            && self.mapping_overrides == q.overrides
    }
    pub(super) fn cancel_mapping_request(&mut self) {
        self.plan_generation.advance();
        self.contact_generation.advance();
        self.plan_pending = false;
        self.plan_requested = None;
        self.contacts_requested = false;
    }
    pub(super) fn skin_changed(&mut self, previous: &Settings) {
        let (Some((s, source, _)), Some((t, target, _))) = (&self.source, &self.model) else {
            return;
        };
        let old = (
            s.clone(),
            t.clone(),
            previous.source_skin,
            previous.target_skin,
        );
        let new = (
            s.clone(),
            t.clone(),
            self.mapping_settings.source_skin,
            self.mapping_settings.target_skin,
        );
        if self.mapping_drafts.len() >= 32 {
            self.mapping_drafts.clear();
        }
        self.mapping_drafts
            .insert(old, self.mapping_overrides.clone());
        let rigs = [
            crate::rig::Rig::analyze_skin(source, self.mapping_settings.source_skin),
            crate::rig::Rig::analyze_skin(target, self.mapping_settings.target_skin),
        ];
        let candidate = self
            .mapping_drafts
            .get(&new)
            .unwrap_or(&self.mapping_overrides);
        let kept: Vec<_> = candidate
            .iter()
            .filter(|o| {
                rigs[0].joints.iter().any(|j| j.node == o.source)
                    && o.target
                        .is_none_or(|n| rigs[1].joints.iter().any(|j| j.node == n))
            })
            .cloned()
            .collect();
        let removed = candidate.len() - kept.len();
        self.mapping_overrides = kept;
        self.cancel_mapping_request();
        self.retarget = None;
        self.contacts = None;
        self.active_pair = None;
        self.automatic_rows.clear();
        self.ui_state.correspondence.operation = crate::ui::correspondence::Operation::Idle;
        self.ui_state.correspondence.frame();
        self.pending_mapping();
        if removed > 0 {
            self.mapping_feedback.details = format!(
                "{removed} incompatible overrides were suspended for this skin. Select the previous skin to recover its edits."
            );
            self.mapping_feedback.message = format!(
                "Skin changed · {removed} overrides suspended; previous skin edits retained"
            );
            self.status = self.mapping_feedback.message.clone();
        }
        self.ctx
            .request_discard("Invalidate old skin preview immediately");
    }
    fn validate_request(&self, q: &Request) -> Result<()> {
        for (hash, model) in self.pending.keys() {
            if (*model && *hash != q.target_hash) || (!*model && *hash != q.source_hash) {
                anyhow::bail!(
                    "{} selection is still loading. Wait for it to finish before applying.",
                    if *model { "Target" } else { "Source" }
                );
            }
        }
        for (side, name, asset, selected) in [
            (
                "Source",
                q.source.clips[q.clip].name.as_str(),
                &q.source,
                q.settings.source_skin,
            ),
            (
                "Target",
                self.model_name(),
                &q.target,
                q.settings.target_skin,
            ),
        ] {
            if let Some(index) = selected
                && index >= asset.skins.len()
            {
                anyhow::bail!(
                    "{side} {name}: skin {index} does not exist ({} skins).",
                    asset.skins.len()
                );
            }
            if asset.skins.len() > 1 && selected.is_none() && q.intent != Intent::Restore {
                anyhow::bail!(
                    "{side} {name} has {} skins. Select a {} skin before applying.",
                    asset.skins.len(),
                    side.to_lowercase()
                );
            }
        }
        anyhow::ensure!(
            q.source.clips[q.clip].supported && q.target.exact_mesh,
            "Unsupported deformation in the selected source clip or target model"
        );
        Ok(())
    }
    pub(super) fn start_mapping(&mut self, intent: Intent) {
        if self.plan_pending {
            return;
        }
        let (Some((s, source, clip)), Some((t, target, _))) = (&self.source, &self.model) else {
            self.mapping_message(
                "Failed",
                "Choose a source motion and target character first.".into(),
            );
            return;
        };
        let generation = self.plan_generation.advance();
        let q = Request {
            generation,
            source_hash: s.clone(),
            target_hash: t.clone(),
            source: Arc::clone(source),
            target: Arc::clone(target),
            clip: *clip,
            settings: self.mapping_settings.clone(),
            overrides: self.mapping_overrides.clone(),
            intent,
            label: format!(
                "{} → {} · Target {}",
                source.clips[*clip].name,
                self.model_name(),
                self.mapping_settings
                    .target_skin
                    .or((target.skins.len() == 1).then_some(0))
                    .map_or("animated hierarchy".into(), |i| format!("skin {i}"))
            ),
        };
        self.mapping_feedback.request = Some(q.clone());
        if let Err(e) = self.validate_request(&q) {
            self.retarget = None;
            self.contacts = None;
            self.active_pair = None;
            self.mapping_message("Failed", format!("{e:#}"));
            return;
        }
        self.plan_pending = true;
        self.ctx
            .request_discard("Show mapping request before worker completion");
        self.contact_generation.advance();
        self.contacts_requested = false;
        self.mapping_message(
            if intent == Intent::Save {
                "Saving"
            } else {
                "Applying"
            },
            format!(
                "{} · {}",
                if intent == Intent::Save {
                    "Saving mapping…"
                } else {
                    "Applying mapping…"
                },
                q.label
            ),
        );
        if self.jobs >= 2 {
            self.mapping_feedback.details="Waiting for background asset preprocessing. The mapping snapshot is already captured.".into();
            self.plan_requested = Some(q);
        } else {
            self.spawn_mapping(q);
        }
    }
    pub(super) fn spawn_mapping(&mut self, q: Request) {
        if !self.request_current(&q) {
            return;
        }
        let root = self.library.lock().expect("library").root.clone();
        let tx = self.tx.clone();
        self.jobs += 1;
        let generation = q.generation;
        let worker = std::thread::spawn(move || {
            let mut restored = false;
            let mut saved_path = None;
            let result = (|| -> Result<Arc<Plan>> {
                if q.intent == Intent::Restore
                    && q.overrides.is_empty()
                    && let Some(plan) = if q.settings == Settings::default() {
                        crate::profile::load_pair(&root, &q.source, &q.target)?
                    } else {
                        crate::profile::load(
                            &root,
                            &retarget::key(&q.source, &q.target, &q.settings),
                            &q.source,
                            &q.target,
                        )?
                    }
                {
                    restored = true;
                    return Ok(Arc::new(plan));
                }
                let plan = retarget::compile(&q.source, &q.target, &q.settings, &q.overrides)?;
                if q.intent == Intent::Save {
                    saved_path = Some(
                        crate::profile::save(&root, &plan, &q.source, &q.target).with_context(
                            || format!("Could not write mapping profile under {}", root.display()),
                        )?,
                    );
                }
                Ok(Arc::new(plan))
            })()
            .map_err(|e| format!("{e:#}"));
            let automatic = result
                .as_ref()
                .ok()
                .map(|p| {
                    if p.overrides.is_empty() {
                        p.rows.clone()
                    } else {
                        retarget::compile(&q.source, &q.target, &p.settings, &[])
                            .map(|p| p.rows)
                            .unwrap_or_default()
                    }
                })
                .unwrap_or_default();
            let _ = tx.send(Job::Planned {
                request: Box::new(q),
                restored,
                result,
                saved_path,
                automatic,
            });
        });
        self.watch_worker(workers::Kind::Mapping { generation }, worker);
    }
    pub(super) fn accept_mapping(
        &mut self,
        mut q: Request,
        restored: bool,
        result: std::result::Result<Arc<Plan>, String>,
        saved_path: Option<PathBuf>,
        automatic: Vec<retarget::Row>,
    ) {
        crate::app_log::event(
            "mapping_result",
            serde_json::json!({"generation":q.generation,"source":q.source_hash,"clip":q.clip,"target":q.target_hash,"source_skin":q.settings.source_skin,"target_skin":q.settings.target_skin,"success":result.is_ok(),"current":self.request_current(&q)}),
        );
        if !self.request_current(&q) {
            return;
        }
        self.plan_pending = false;
        match result {
            Ok(plan) => {
                if let Err(e) =
                    crate::profile::validate(&plan, &q.source, &q.target).and_then(|_| {
                        retarget::sample_checked(
                            &q.source,
                            &q.target,
                            &q.source.clips[q.clip],
                            &plan,
                            0.,
                        )
                        .map(|_| ())
                    })
                {
                    self.retarget = None;
                    self.contacts = None;
                    self.active_pair = None;
                    self.mapping_message(
                        "Failed",
                        format!("Compiled mapping could not evaluate: {e:#}"),
                    );
                    return;
                }
                // A restore is the only operation allowed to replace editor settings.
                if !restored && (plan.settings != q.settings || plan.overrides != q.overrides) {
                    self.retarget = None;
                    self.contacts = None;
                    self.active_pair = None;
                    self.mapping_message(
                        "Failed",
                        "Compilation returned a different mapping snapshot.".into(),
                    );
                    return;
                }
                self.plan_origin = if restored || saved_path.is_some() {
                    "Saved profile"
                } else {
                    "Compiled plan"
                }
                .into();
                q.settings = plan.settings.clone();
                q.overrides = plan.overrides.clone();
                q.label = format!(
                    "{} → {} · Target {}",
                    q.source.clips[q.clip].name,
                    self.model_name(),
                    q.settings
                        .target_skin
                        .or((q.target.skins.len() == 1).then_some(0))
                        .map_or("animated hierarchy".into(), |i| format!("skin {i}"))
                );
                self.mapping_settings = plan.settings.clone();
                self.mapping_overrides = plan.overrides.clone();
                self.automatic_rows = automatic.clone();
                if self.plan_cache.len() >= 32 {
                    self.plan_cache.clear();
                    self.automatic_cache.clear();
                }
                let key = (q.source_hash.clone(), q.target_hash.clone());
                self.plan_cache.insert(key.clone(), Arc::clone(&plan));
                self.automatic_cache.insert(key, automatic);
                self.active_pair = Some((q.source_hash.clone(), q.target_hash.clone(), q.clip));
                self.retarget = Some(plan);
                self.mapping_message(
                    if saved_path.is_some() {
                        "Saved"
                    } else {
                        "Applied"
                    },
                    format!(
                        "{} · {} manual correspondences · {}",
                        q.label,
                        q.overrides.iter().filter(|o| o.target.is_some()).count(),
                        if saved_path.is_some() {
                            "Mapping saved"
                        } else {
                            "Preview ready"
                        }
                    ),
                );
                self.mapping_feedback.saved_path = saved_path;
                self.mapping_feedback.request = Some(q.clone());
                self.mapping_feedback.details = format!(
                    "Request {} · Source {} clip {} · Target {}\nSource skin {:?} · Target skin {:?}",
                    q.generation,
                    q.source_hash,
                    q.clip,
                    q.target_hash,
                    self.mapping_settings.source_skin,
                    self.mapping_settings.target_skin
                );
                let warnings = influence_warnings(&q);
                if !warnings.is_empty() {
                    self.mapping_feedback
                        .details
                        .push_str(&format!("\n{}", warnings.join("\n")));
                    self.mapping_feedback.message.push_str(&format!(
                        " · {} unweighted endpoint (Details)",
                        warnings.len()
                    ));
                    self.status = format!(
                        "{} · {}",
                        self.mapping_feedback.phase, self.mapping_feedback.message
                    );
                }
                if q.intent == Intent::Preview {
                    self.mapping_open = false;
                    self.motion.open = false;
                    self.ui_state.workspace = Workspace::Animate;
                    self.source_preview = None;
                    self.bind_pose = false;
                    self.mesh = true;
                    self.playback.time = 0.;
                    self.playback.cycle = 0;
                    self.playback.playing = true;
                    if !self.playback.speed.is_finite() || self.playback.speed <= 0. {
                        self.playback.speed = 1.;
                    }
                    self.cursors.clear();
                    self.frame_requested = true;
                }
                self.request_contacts();
            }
            Err(e) => {
                self.retarget = None;
                self.contacts = None;
                self.active_pair = None;
                self.mapping_message("Failed", format!("{} · {e}", q.label));
                self.mapping_feedback.details = format!(
                    "Request {} · Source skin {:?} · Target skin {:?}\n{e}",
                    q.generation, q.settings.source_skin, q.settings.target_skin
                );
            }
        }
    }
    pub(super) fn mapping_feedback_ui(&mut self, ui: &mut egui::Ui) {
        let phase = if self.plan_pending {
            self.mapping_feedback.phase
        } else if self.mapping_dirty() && self.mapping_feedback.phase != "Failed" {
            "Pending"
        } else {
            self.mapping_feedback.phase
        };
        let color = match phase {
            "Failed" => theme::ERROR,
            "Saved" | "Applied" => theme::SUCCESS,
            _ => theme::ORANGE,
        };
        ui.horizontal(|ui| {
            if self.plan_pending {
                ui.add(egui::Spinner::new().size(12.));
            }
            ui.colored_label(color, if phase.is_empty() { "Pending" } else { phase });
            let text = if phase == "Pending" && self.mapping_feedback.phase != "Pending" {
                "Mapping edits · Pending apply"
            } else {
                self.mapping_feedback.message.as_str()
            };
            ui.add_sized(
                egui::vec2((ui.available_width() - 72.).max(20.), 20.),
                egui::Label::new(egui::RichText::new(text).size(11.))
                    .truncate()
                    .halign(egui::Align::Min),
            )
            .on_hover_text(text);
            let b = ui
                .button("Details")
                .on_hover_text("Source, target, selected skins, request and profile location");
            if let Some(r) = &mut self.input_regions {
                r.mapping_controls.insert("apply-details", b.rect);
            }
            if b.clicked() {
                self.mapping_feedback.show_details = !self.mapping_feedback.show_details;
            }
        });
    }
    pub(super) fn mapping_feedback_compact(&mut self, ui: &mut egui::Ui) {
        let phase = if self.mapping_dirty()
            && !self.plan_pending
            && self.mapping_feedback.phase != "Failed"
        {
            "Pending"
        } else {
            self.mapping_feedback.phase
        };
        let color = match phase {
            "Failed" => theme::ERROR,
            "Applied" | "Saved" => theme::SUCCESS,
            _ => theme::ORANGE,
        };
        ui.horizontal(|ui| {
            if self.plan_pending {
                ui.add(egui::Spinner::new().size(12.));
            }
            ui.colored_label(color, if phase.is_empty() { "Pending" } else { phase });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = ui
                    .button("Details")
                    .on_hover_text("Mapping operation, selected skins and saved profile location");
                if let Some(r) = &mut self.input_regions {
                    r.mapping_controls.insert("apply-details", b.rect);
                }
                if b.clicked() {
                    self.mapping_feedback.show_details = !self.mapping_feedback.show_details;
                }
            });
        });
        egui::ScrollArea::vertical()
            .id_salt("compact-apply-message")
            .max_height(44.)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(
                        if phase == "Pending" && self.mapping_feedback.phase != "Pending" {
                            "Mapping edits · Pending apply"
                        } else {
                            &self.mapping_feedback.message
                        },
                    )
                    .size(11.),
                );
            });
    }
    pub(super) fn mapping_details(&mut self, ctx: &egui::Context) {
        if !self.mapping_feedback.show_details {
            return;
        }
        let mut open = true;
        egui::Window::new("Mapping operation details")
            .open(&mut open)
            .default_width(420.)
            .constrain_to(ctx.content_rect().shrink(8.))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 120.).max(100.))
                    .show(ui, |ui| {
                        ui.strong(self.mapping_feedback.phase);
                        ui.label(&self.mapping_feedback.message);
                        ui.separator();
                        ui.label(&self.mapping_feedback.details);
                        if let Some(path) = &self.mapping_feedback.saved_path {
                            ui.label("Profile written to:");
                            ui.label(path.display().to_string());
                        }
                        if let Some(plan) = &self.retarget {
                            ui.label(format!(
                                "Active compiled identity: {}",
                                self.active_plan_id().unwrap_or_default()
                            ));
                            ui.label(format!("Active plan: {:?}", plan.key));
                        }
                    });
            });
        self.mapping_feedback.show_details = open;
    }
}

impl Editor {
    /// The exact target-pose path consumed by the native renderer and regression tests.
    pub(super) fn target_pose(&mut self) -> (Vec<crate::scene::Local>, Vec<String>) {
        let Some((_, asset, _)) = self.model.clone() else {
            return (vec![], vec![]);
        };
        let asset = &*asset;
        let mut correction_warnings = Vec::new();
        if self.retarget.is_some()
            && !self.active_pair.as_ref().is_some_and(|(s, t, i)| {
                self.source.as_ref().is_some_and(|p| p.0 == *s && p.2 == *i)
                    && self.model.as_ref().is_some_and(|p| p.0 == *t)
            })
        {
            return (
                asset.rest_pose(),
                vec!["Stale retarget preview rejected".into()],
            );
        }
        let pose = if !self.bind_pose
            && let (Some((_, source, i)), Some(mapping)) = (&self.source, &self.mapping)
            && (!mapping.nodes.is_empty() || self.retarget.is_some())
        {
            if let Some(plan) = &self.retarget {
                if let Some(c) = &self.contacts {
                    let time = self.playback.time as f64
                        + self.playback.cycle as f64 * source.clips[*i].duration as f64;
                    if self.contact_correction {
                        let corrected = crate::ik::sample(
                            source,
                            asset,
                            &source.clips[*i],
                            plan,
                            c,
                            time,
                            self.playback.looping,
                        );
                        correction_warnings = corrected.warnings;
                        corrected.pose
                    } else {
                        match c.raw_pose_checked(
                            source,
                            asset,
                            &source.clips[*i],
                            plan,
                            self.playback.time as f64
                                + self.playback.cycle as f64 * source.clips[*i].duration as f64,
                            self.playback.looping,
                        ) {
                            Ok(p) => p,
                            Err(e) => {
                                correction_warnings
                                    .push(format!("Retarget disabled at this time: {e:#}"));
                                asset.rest_pose()
                            }
                        }
                    }
                } else {
                    match retarget::sample_checked(
                        source,
                        asset,
                        &source.clips[*i],
                        plan,
                        self.playback.time,
                    ) {
                        Ok(p) => p,
                        Err(e) => {
                            correction_warnings
                                .push(format!("Retarget disabled at this time: {e:#}"));
                            asset.rest_pose()
                        }
                    }
                }
            } else {
                let mut pose = animation::evaluate(
                    asset,
                    &source.clips[*i],
                    &mapping.nodes,
                    self.playback.time,
                    &mut self.cursors,
                );
                if self.ui_state.preferences.continuous && self.playback.looping {
                    animation::place_cycle(
                        asset,
                        &mut pose,
                        self.native_travel,
                        self.playback.cycle,
                    );
                }
                pose
            }
        } else {
            asset.rest_pose()
        };
        (pose, correction_warnings)
    }
    pub(super) fn rendered_pose_evidence(
        &self,
        asset: &Asset,
        pose: &[crate::scene::Local],
    ) -> serde_json::Value {
        let globals = asset.globals(pose);
        let selected = self
            .retarget
            .as_ref()
            .and_then(|p| p.settings.target_skin)
            .or((asset.skins.len() == 1).then_some(0));
        let rig = crate::rig::Rig::analyze_skin(asset, selected);
        let mut vertices = vec![];
        let mut weighted = std::collections::BTreeMap::<usize, usize>::new();
        for node in asset.visible_nodes(self.scene) {
            if asset.nodes[node].skin != selected || selected.is_none() {
                continue;
            }
            if let Some(mesh) = asset.nodes[node].mesh
                && let Ok(palette) = asset.palette(node, &globals)
            {
                for primitive in &asset.meshes[mesh].primitives {
                    for vertex in &primitive.vertices {
                        for (&index, &weight) in vertex.joints.iter().zip(&vertex.weights) {
                            if weight > 0.
                                && let Some(&node) =
                                    asset.skins[selected.unwrap()].joints.get(index as usize)
                            {
                                *weighted.entry(node).or_default() += 1;
                            }
                        }
                        vertices.push(
                            globals[node]
                                .transform_point3(crate::scene::skin_position(vertex, &palette)),
                        );
                    }
                }
            }
        }
        let mut digest = blake3::Hasher::new();
        for p in &vertices {
            for v in p.to_array() {
                digest.update(&v.to_le_bytes());
            }
        }
        let probes: Vec<_> = vertices
            .iter()
            .step_by((vertices.len() / 128).max(1))
            .map(|p| p.to_array())
            .collect();
        serde_json::json!({"time":self.playback.time,"cycle":self.playback.cycle,"target_skin":selected,"plan":self.retarget.as_ref().map(|p|&p.key),"active_plan_id":self.active_plan_id(),"motion_target":self.retarget.as_ref().map(|p|p.motion_target),"active_pair":self.active_pair,"source_clip":self.source.as_ref().map(|s|s.2),"joints":rig.joints.iter().map(|j|serde_json::json!({"node":j.node,"world":globals[j.node].to_cols_array()})).collect::<Vec<_>>(),"skinned_vertices":vertices.len(),"weighted_joint_vertices":weighted,"lbs_world_blake3":digest.finalize().to_hex().as_str(),"lbs_world_probes":probes})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animated_unweighted_endpoints_are_reported_but_weighted_descendants_count() {
        let source = import::from_bytes(
            &crate::fixtures::robot(true, true).finish(),
            &Limits::default(),
        )
        .unwrap();
        let mut target = import::from_bytes(
            &crate::fixtures::multi_skin_wave_target().finish(),
            &Limits::default(),
        )
        .unwrap();
        for mesh in &mut target.meshes {
            for primitive in &mut mesh.primitives {
                for vertex in &mut primitive.vertices {
                    for joint in &mut vertex.joints {
                        if *joint == 5 {
                            *joint = 4;
                        }
                    }
                }
            }
        }
        let mut request = Request {
            generation: 1,
            source_hash: "source".into(),
            target_hash: "target".into(),
            source: Arc::new(source),
            target: Arc::new(target),
            clip: 0,
            settings: Settings {
                target_skin: Some(0),
                ..Default::default()
            },
            overrides: vec![Override {
                source: 6,
                target: Some(5),
            }],
            intent: Intent::Apply,
            label: "Wave".into(),
        };
        let warnings = influence_warnings(&request);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unweighted end joint"));
        request.overrides[0].target = Some(4);
        assert!(influence_warnings(&request).is_empty());
        request.overrides[0] = Override {
            source: 11,
            target: Some(0),
        };
        assert!(influence_warnings(&request).is_empty());
    }
}
