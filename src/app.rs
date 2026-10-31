use crate::{
    animation::{self, Cursor, Playback},
    cache::{ByteLru, Generation},
    compatibility::{self, Mapping},
    import::{self, Limits},
    library::{Kind, Library, Record},
    render::{Camera, Gpu, GpuAsset, Renderer, Target},
    retarget::{self, Override, Plan, Settings},
    scene::Asset,
    ui::{
        state::{Collection, Preferences, UiState, Workspace},
        theme, widgets,
    },
};
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};
mod apply_capture;
mod asset_browser;
mod diagnostics_view;
mod drawers;
mod help;
mod input_smoke;
mod inspector;
mod labs;
mod library_capture;
mod library_preview;
mod library_workspace;
mod mapping_advanced;
mod mapping_apply;
mod mapping_capture;
mod mapping_editor;
mod mapping_interaction;
mod motion_browser;
mod motion_previews;
mod viewport;
mod workers;
mod workspace;
pub fn ui_smoke() -> Result<()> {
    input_smoke::run()
}
pub fn cache_benchmark() -> Result<()> {
    input_smoke::cache_workload()
}

enum Job {
    Motion {
        generation: u64,
        entries: Vec<crate::motion_search::Entry>,
        skipped: usize,
        diagnostics: HashMap<String, motion_browser::MotionStatus>,
    },
    Contacts {
        generation: u64,
        result: std::result::Result<Arc<crate::contacts::Contacts>, String>,
    },
    Planned {
        request: Box<mapping_apply::Request>,
        restored: bool,
        result: std::result::Result<Arc<Plan>, String>,
        saved_path: Option<PathBuf>,
        automatic: Vec<retarget::Row>,
    },
    Import {
        hash: String,
        asset: Arc<Asset>,
        records: Vec<Record>,
    },
    Loaded {
        hash: String,
        asset: std::result::Result<Arc<Asset>, String>,
        kind: Kind,
        generation: u64,
    },
    Failed(String),
    Cancelled,
}
struct Editor {
    ui_state: UiState,
    motion: motion_browser::MotionBrowser,
    motion_auto: bool,
    library: Arc<Mutex<Library>>,
    records: Vec<Record>,
    cpu: ByteLru<String, Asset>,
    gpu_cache: ByteLru<String, GpuAsset>,
    model: Option<(String, Arc<Asset>, Arc<GpuAsset>)>,
    source: Option<(String, Arc<Asset>, usize)>,
    source_generation: u64,
    source_preview: Option<SourcePreview>,
    source_travel: Option<animation::RootTravel>,
    native_travel: Option<animation::RootTravel>,
    mapping: Option<Mapping>,
    retarget: Option<Arc<Plan>>,
    contacts: Option<Arc<crate::contacts::Contacts>>,
    contact_cache: ByteLru<String, crate::contacts::Contacts>,
    plan_requested: Option<mapping_apply::Request>,
    contacts_requested: bool,
    contact_correction: bool,
    show_anchors: bool,
    plan_cache: HashMap<(String, String), Arc<Plan>>,
    plan_generation: Generation,
    contact_generation: Generation,
    mapping_feedback: mapping_apply::Feedback,
    mapping_drafts: HashMap<mapping_apply::DraftKey, Vec<Override>>,
    active_pair: Option<(String, String, usize)>,
    mapping_pair: Option<(String, String)>,
    capture_pose: Option<Vec<crate::scene::Local>>,
    plan_pending: bool,
    plan_origin: String,
    mapping_open: bool,
    mapping_filter: String,
    mapping_settings: Settings,
    mapping_overrides: Vec<Override>,
    mapping_history: mapping_interaction::History,
    automatic_rows: Vec<retarget::Row>,
    automatic_cache: HashMap<(String, String), Vec<retarget::Row>>,
    selected_pair: Option<usize>,
    mappings: HashMap<(String, String, usize), Mapping>,
    model_generation: Generation,
    clip_generation: Generation,
    tx: mpsc::SyncSender<Job>,
    rx: mpsc::Receiver<Job>,
    jobs: usize,
    workers: Vec<workers::Worker>,
    status: String,
    search: String,
    playback: Playback,
    cursors: Vec<Cursor>,
    camera: Camera,
    frame_requested: bool,
    skeleton: bool,
    dqs: bool,
    mesh: bool,
    bind_pose: bool,
    scene: usize,
    last_frame: Instant,
    frame_ms: f32,
    render_warnings: Vec<String>,
    skinning_report: crate::render::SkinningReport,
    ctx: egui::Context,
    thumbnails: HashMap<String, egui::TextureHandle>,
    paths: std::collections::VecDeque<PathBuf>,
    pending: HashMap<(String, bool), u64>,
    failures: std::collections::HashSet<String>,
    input_regions: Option<InputRegions>,
}
struct SourcePreview {
    key: String,
    asset: Arc<Asset>,
    clip: usize,
    resident: Arc<GpuAsset>,
    camera: Camera,
}
struct InputRegions {
    assets_drawer: egui::Rect,
    empty_sample: egui::Rect,
    empty_import: egui::Rect,
    detail_drawer: egui::Rect,
    assets_opener: Option<egui::Id>,
    drawer: egui::Rect,
    drawer_close: egui::Rect,
    animate_open: egui::Rect,
    assets_search: egui::Rect,
    motion_open: egui::Rect,
    motion_scroll: egui::Rect,
    motion_results: HashMap<String, egui::Rect>,
    motion_preview_buttons: HashMap<String, egui::Rect>,
    motion_medoid_previews: HashMap<(String, usize), (egui::Rect, f32)>,
    library_controls: HashMap<&'static str, egui::Rect>,
    mapping_open: egui::Rect,
    mapping_save: egui::Rect,
    mapping_reset: egui::Rect,
    mapping_rows: HashMap<usize, egui::Rect>,
    mapping_combos: HashMap<usize, egui::Rect>,
    mapping_options: HashMap<(usize, Option<usize>), egui::Rect>,
    mapping_popup: egui::Rect,
    mapping_canvas: egui::Rect,
    status_rect: egui::Rect,
    workspace_rect: egui::Rect,
    mapping_reassign: egui::Rect,
    mapping_frame: egui::Rect,
    mapping_joints: HashMap<(usize, usize), egui::Pos2>,
    mapping_labels: Vec<egui::Rect>,
    mapping_selected_points: Vec<egui::Pos2>,
    mapping_selected_bones: Vec<[egui::Pos2; 2]>,
    mapping_drawn_pair: Option<usize>,
    mapping_shared_scale: f32,
    mapping_controls: HashMap<&'static str, egui::Rect>,
    mapping_focus: HashMap<&'static str, egui::Id>,
    skin_options: HashMap<(usize, Option<usize>), egui::Rect>,
    mapping_picker_options: HashMap<(usize, usize), egui::Rect>,
    mapping_picker_clip: Option<egui::Rect>,
    mapping_confirm_enabled: bool,
    timeline_clip: egui::Rect,
    timeline_hover: Option<f32>,
    timeline_contacts: Vec<crate::ui::timeline::ContactBar>,
    play: egui::Rect,
    seek: egui::Rect,
    speed: egui::Rect,
    fit: egui::Rect,
    reset: egui::Rect,
    viewport: egui::Rect,
    utility_windows: HashMap<&'static str, egui::Rect>,
    utility_scrolls: HashMap<&'static str, (egui::Rect, f32)>,
    records: HashMap<String, egui::Rect>,
}
impl Drop for Editor {
    fn drop(&mut self) {
        if let Ok(library) = self.library.lock() {
            self.ui_state.preferences.mesh = self.mesh;
            self.ui_state.preferences.skeleton = self.skeleton;
            if let Err(error) = self.ui_state.preferences.save(&library.root) {
                eprintln!("UI preferences: {error:#}");
            }
        }
        if let (Some((model, _, _)), Some((source, _, index))) = (&self.model, &self.source)
            && let Ok(library) = self.library.lock()
            && let Err(error) = crate::session::save(&library, model, source, *index)
        {
            eprintln!("Selection save: {error:#}");
        }
    }
}
impl Default for InputRegions {
    fn default() -> Self {
        Self {
            assets_drawer: egui::Rect::NOTHING,
            empty_sample: egui::Rect::NOTHING,
            empty_import: egui::Rect::NOTHING,
            detail_drawer: egui::Rect::NOTHING,
            assets_opener: None,
            drawer: egui::Rect::NOTHING,
            drawer_close: egui::Rect::NOTHING,
            animate_open: egui::Rect::NOTHING,
            assets_search: egui::Rect::NOTHING,
            motion_open: egui::Rect::NOTHING,
            motion_scroll: egui::Rect::NOTHING,
            motion_results: HashMap::new(),
            motion_preview_buttons: HashMap::new(),
            motion_medoid_previews: HashMap::new(),
            library_controls: HashMap::new(),
            mapping_open: egui::Rect::NOTHING,
            mapping_save: egui::Rect::NOTHING,
            mapping_reset: egui::Rect::NOTHING,
            mapping_rows: HashMap::new(),
            mapping_combos: HashMap::new(),
            mapping_options: HashMap::new(),
            mapping_popup: egui::Rect::NOTHING,
            mapping_canvas: egui::Rect::NOTHING,
            status_rect: egui::Rect::NOTHING,
            workspace_rect: egui::Rect::NOTHING,
            mapping_reassign: egui::Rect::NOTHING,
            mapping_frame: egui::Rect::NOTHING,
            mapping_joints: HashMap::new(),
            mapping_labels: Vec::new(),
            mapping_selected_points: Vec::new(),
            mapping_selected_bones: Vec::new(),
            mapping_drawn_pair: None,
            mapping_shared_scale: 0.,
            mapping_controls: HashMap::new(),
            mapping_focus: HashMap::new(),
            skin_options: HashMap::new(),
            mapping_picker_options: HashMap::new(),
            mapping_picker_clip: None,
            mapping_confirm_enabled: false,
            timeline_clip: egui::Rect::NOTHING,
            timeline_hover: None,
            timeline_contacts: Vec::new(),
            play: egui::Rect::NOTHING,
            seek: egui::Rect::NOTHING,
            speed: egui::Rect::NOTHING,
            fit: egui::Rect::NOTHING,
            reset: egui::Rect::NOTHING,
            viewport: egui::Rect::NOTHING,
            utility_windows: HashMap::new(),
            utility_scrolls: HashMap::new(),
            records: HashMap::new(),
        }
    }
}
impl Editor {
    fn new(library: Library, ctx: egui::Context) -> Self {
        theme::apply(&ctx);
        let mut preferences = Preferences::load(&library.root);
        ctx.set_zoom_factor(preferences.scale);
        if let Some(scale) = std::env::var("ARMATRA_UI_SCALE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
        {
            ctx.set_zoom_factor(scale.clamp(0.85, 1.5));
            preferences.scale = scale.clamp(0.85, 1.5);
        }
        let mesh = preferences.mesh;
        let skeleton = preferences.skeleton;
        let mut ui_state = UiState::new(preferences);
        ui_state.labs = std::env::var_os("ARMATRA_LABS_OPEN").is_some();
        ui_state.help = std::env::var_os("ARMATRA_HELP_OPEN").is_some();
        ui_state.settings = std::env::var_os("ARMATRA_SETTINGS_OPEN").is_some();
        if std::env::var_os("ARMATRA_CAPTURE").is_some()
            && std::env::var_os("ARMATRA_CAPTURE_CONTINUOUS").is_some()
        {
            ui_state.preferences.continuous = true;
        }
        let playback = Playback {
            speed: ui_state.preferences.playback_speed,
            looping: ui_state.preferences.looping,
            ..Default::default()
        };
        let records = library.manifest.records.clone();
        let (tx, rx) = mpsc::sync_channel(4);
        let status = library
            .recovery
            .clone()
            .unwrap_or_else(|| "Ready · Explore Library motions or import a character".into());
        let mut editor = Self {
            ui_state,
            motion: motion_browser::MotionBrowser::new(library.root.clone(), ctx.clone()),
            motion_auto: std::env::var_os("ARMATRA_MOTION_OPEN").is_some(),
            library: Arc::new(Mutex::new(library)),
            records,
            cpu: ByteLru::new(256 * 1024 * 1024),
            gpu_cache: ByteLru::new(512 * 1024 * 1024),
            model: None,
            source: None,
            source_generation: 0,
            source_preview: None,
            source_travel: None,
            native_travel: None,
            mapping: None,
            retarget: None,
            contacts: None,
            contact_cache: ByteLru::new(16 * 1024 * 1024),
            plan_requested: None,
            contacts_requested: false,
            contact_correction: true,
            show_anchors: false,
            plan_cache: HashMap::new(),
            plan_generation: Generation::default(),
            contact_generation: Generation::default(),
            mapping_feedback: Default::default(),
            mapping_drafts: HashMap::new(),
            active_pair: None,
            mapping_pair: None,
            capture_pose: None,
            plan_pending: false,
            plan_origin: String::new(),
            mapping_open: std::env::var_os("ARMATRA_MAPPING_OPEN").is_some(),
            mapping_filter: String::new(),
            mapping_settings: Settings::default(),
            mapping_overrides: Vec::new(),
            mapping_history: Default::default(),
            automatic_rows: Vec::new(),
            automatic_cache: HashMap::new(),
            selected_pair: None,
            mappings: HashMap::new(),
            model_generation: Generation::default(),
            clip_generation: Generation::default(),
            tx,
            rx,
            jobs: 0,
            workers: vec![],
            status,
            search: String::new(),
            playback,
            cursors: Vec::new(),
            camera: Camera::default(),
            frame_requested: false,
            skeleton,
            dqs: std::env::var_os("ARMATRA_DQS").is_some(),
            mesh,
            bind_pose: false,
            scene: 0,
            last_frame: Instant::now(),
            frame_ms: 0.,
            render_warnings: Vec::new(),
            skinning_report: Default::default(),
            ctx,
            thumbnails: HashMap::new(),
            paths: std::collections::VecDeque::new(),
            pending: HashMap::new(),
            failures: std::collections::HashSet::new(),
            input_regions: std::env::var_os("ARMATRA_CAPTURE").map(|_| InputRegions::default()),
        };
        editor.load_asset_thumbnails();
        editor
    }
    fn import(&mut self, path: Option<PathBuf>) {
        if self.jobs >= 2 {
            self.status = "Two imports/loads are already running. Please wait.".into();
            return;
        }
        self.jobs += 1;
        self.status = "Importing and validating asset…".into();
        let tx = self.tx.clone();
        let library = Arc::clone(&self.library);
        let worker = std::thread::spawn(move || {
            let Some(path) = path.or_else(|| {
                rfd::FileDialog::new()
                    .add_filter("Characters and skeletal motion", &["glb", "gltf", "bvh"])
                    .pick_file()
            }) else {
                let _ = tx.send(Job::Cancelled);
                return;
            };
            let result = (|| -> Result<Job> {
                let (bytes, asset) = import::from_path(&path, &Limits::default())?;
                let asset = Arc::new(asset);
                let mut library = library
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Library lock unavailable"))?;
                let hash = library.import(
                    &bytes,
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                    &asset,
                )?;
                Ok(Job::Import {
                    hash,
                    asset,
                    records: library.manifest.records.clone(),
                })
            })();
            let _ =
                tx.send(result.unwrap_or_else(|e| Job::Failed(format!("Import failed: {e:#}"))));
        });
        self.watch_worker(workers::Kind::Import, worker);
    }
    fn request(&mut self, record: &Record, renderer: &Renderer, gpu: &Gpu) {
        let changed = match record.kind {
            Kind::Model => self.model.as_ref().is_none_or(|m| m.0 != record.hash),
            Kind::Clip(index) => self
                .source
                .as_ref()
                .is_none_or(|s| s.0 != record.hash || s.2 != index),
        };
        if changed {
            self.cancel_mapping_request();
            if self.mapping_dirty() {
                self.pending_mapping();
            } else {
                self.mapping_message(
                    "Applied",
                    "Current mapping retained while the asset selection loads.".into(),
                );
            }
        }
        self.request_with_preview(record, None, renderer, gpu);
    }
    fn request_with_preview(
        &mut self,
        record: &Record,
        time: Option<f32>,
        renderer: &Renderer,
        gpu: &Gpu,
    ) -> bool {
        let generation = match record.kind {
            Kind::Model => self.model_generation.advance(),
            Kind::Clip(_) => self.clip_generation.advance(),
        };
        if let Some(time) = time {
            crate::app_log::event(
                "preview_click",
                serde_json::json!({"record":record.id,"clip":match record.kind {Kind::Clip(i)=>Some(i),_=>None},"generation":generation,"time":time,"target":self.model.as_ref().map(|m|&m.0),"source_skin":self.mapping_settings.source_skin,"target_skin":self.mapping_settings.target_skin,"active_plan":self.active_plan_id()}),
            );
            if !time.is_finite()
                || !self
                    .records
                    .iter()
                    .any(|r| r.id == record.id && r.hash == record.hash && r.kind == record.kind)
                || !matches!(record.kind, Kind::Clip(_))
            {
                self.preview_failed(&record.name,"Motion record is missing, stale, or has an invalid preview time. Refresh the library.");
                return false;
            }
            self.motion.preview_error = None;
            if self
                .source
                .as_ref()
                .is_none_or(|s| s.0 != record.hash || record.kind != Kind::Clip(s.2))
            {
                self.cancel_mapping_request();
            }
        }
        if let Kind::Clip(index) = record.kind {
            self.motion.pending_preview =
                time.filter(|t| t.is_finite())
                    .map(|time| motion_browser::PreviewIntent {
                        record_id: record.id.clone(),
                        hash: record.hash.clone(),
                        index,
                        generation,
                        time,
                    });
        }
        if let Some(asset) = self.cpu.get(&record.hash) {
            self.preview_boundary("preview_asset_cache_hit");
            self.activate(
                record.hash.clone(),
                asset,
                record.kind.clone(),
                generation,
                renderer,
                gpu,
            );
            return !matches!(record.kind, Kind::Clip(_)) || self.source_generation == generation;
        }
        if self.jobs >= 2 {
            if matches!(record.kind, Kind::Clip(_)) {
                self.motion.pending_preview = None;
            }
            if time.is_some() {
                self.preview_failed(
                    &record.name,
                    "Two background operations are active. Retry when they finish.",
                );
            } else {
                self.status = "Waiting for active loads; select again when they finish".into();
            }
            return false;
        }
        self.jobs += 1;
        self.preview_boundary("preview_asset_load_begin");
        self.status = format!("Loading {}…", record.name);
        self.pending.insert(
            (record.hash.clone(), record.kind == Kind::Model),
            generation,
        );
        let library = Arc::clone(&self.library);
        let tx = self.tx.clone();
        let record = record.clone();
        let watched = workers::Kind::Load {
            hash: record.hash.clone(),
            model: record.kind == Kind::Model,
            generation,
        };
        let worker = std::thread::spawn(move || {
            let result = (|| -> Result<Asset> {
                library
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Library unavailable"))?
                    .load(&record.hash)
            })();
            crate::app_log::event(
                "asset_load_complete",
                serde_json::json!({"hash":record.hash,"generation":generation,"success":result.is_ok(),"error":result.as_ref().err().map(|e|format!("{e:#}"))}),
            );
            let _ = tx.send(Job::Loaded {
                hash: record.hash,
                asset: result
                    .map(Arc::new)
                    .map_err(|e| format!("Load failed: {e:#}")),
                kind: record.kind,
                generation,
            });
        });
        self.watch_worker(watched, worker);
        true
    }
    fn activate(
        &mut self,
        hash: String,
        asset: Arc<Asset>,
        kind: Kind,
        generation: u64,
        renderer: &Renderer,
        gpu: &Gpu,
    ) {
        match kind {
            Kind::Model => {
                let cached = self.gpu_cache.get(&hash);
                let warm = cached.is_some();
                let resident = cached
                    .map(Ok)
                    .unwrap_or_else(|| renderer.upload(gpu, &asset));
                match resident {
                    Ok(resident) => {
                        if !warm {
                            self.thumbnail(&hash, &asset, &resident, renderer, gpu);
                        }
                        self.scene = asset.default_scene;
                        self.camera = resident.fitted_camera.clone();
                        self.camera.needs_fit = true;
                        self.model =
                            Some((hash.clone(), Arc::clone(&asset), Arc::clone(&resident)));
                        self.gpu_cache.pin([hash.clone()]);
                        self.gpu_cache
                            .insert(hash.clone(), resident.clone(), resident.bytes);
                        self.status = "Model ready".into();
                    }
                    Err(e) => self.status = format!("GPU upload failed: {e:#}"),
                }
            }
            Kind::Clip(index) => {
                let intent = self
                    .motion
                    .pending_preview
                    .as_ref()
                    .filter(|p| p.generation == generation && p.hash == hash && p.index == index)
                    .map(|p| (p.record_id.clone(), p.time));
                if let Some((id, time)) = &intent {
                    let record = self.records.iter().find(|r| r.id == *id).cloned();
                    let result = record
                        .as_ref()
                        .context("Motion record removed during load")
                        .and_then(|r| self.preflight_preview(r, &asset, *time, renderer, gpu));
                    if let Err(error) = result {
                        self.preview_failed(
                            record.as_ref().map_or("Motion", |r| r.name.as_str()),
                            &format!("{error:#}"),
                        );
                        return;
                    }
                }
                if index >= asset.clips.len() || !asset.clips[index].supported {
                    self.motion.pending_preview = None;
                    self.status = "Imported motion cannot play: missing clip or unsupported animation channels/deformation".into();
                    return;
                }
                self.source = Some((hash.clone(), Arc::clone(&asset), index));
                self.source_travel = animation::root_travel(
                    &asset,
                    &asset.clips[index],
                    &(0..asset.nodes.len()).map(Some).collect::<Vec<_>>(),
                );
                self.source_generation = generation;
                self.motion.display_pending = intent.map(|(id, _)| (generation, id));
                self.playback.time = 0.;
                self.playback.cycle = 0;
                self.playback.playing = true;
                self.cursors.clear();
                self.status = "Animation ready".into();
                if let Some(intent) = self.motion.pending_preview.take()
                    && intent.generation == generation
                    && self.clip_generation.accepts(generation)
                    && intent.hash == hash
                    && intent.index == index
                {
                    self.playback.time = intent.time.clamp(0., asset.clips[index].duration);
                    self.playback.cycle = 0;
                    self.cursors.clear();
                }
            }
        }
        self.preview_boundary("preview_clip_activated");
        self.refresh_mapping();
        self.preview_boundary("preview_mapping_reconciled");
        let pinned: Vec<_> = self
            .model
            .iter()
            .map(|m| m.0.clone())
            .chain(self.source.iter().map(|s| s.0.clone()))
            .collect();
        self.cpu.pin(pinned);
        self.cpu.insert(hash, Arc::clone(&asset), asset.cpu_bytes());
    }
    fn thumbnail(
        &mut self,
        hash: &str,
        asset: &Asset,
        resident: &GpuAsset,
        renderer: &Renderer,
        gpu: &Gpu,
    ) {
        if self.thumbnails.contains_key(hash) {
            return;
        }
        let root = match self.library.try_lock() {
            Ok(l) => l.root.clone(),
            Err(_) => return,
        };
        let path = root.join("thumbnails").join(format!("{hash}-v1.png"));
        let result = (|| -> Result<image::RgbaImage> {
            if path.exists() {
                return Ok(image::open(&path)?.into_rgba8());
            }
            let target = Target::new(&gpu.device, [128, 128]);
            let mut camera = Camera::default();
            camera.fit(asset, asset.default_scene);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(
                gpu,
                &mut encoder,
                &target,
                &camera,
                asset,
                resident,
                &asset.globals(&asset.rest_pose()),
                asset.default_scene,
                false,
                true,
            );
            gpu.queue.submit([encoder.finish()]);
            let image = crate::diagnostics::read_target(gpu, &target)?;
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, image::ImageFormat::Png)?;
            crate::library::atomic_publish(&path, bytes.get_ref())?;
            Ok(image)
        })();
        if let Ok(image) = result {
            if self.thumbnails.len() >= 128
                && let Some(key) = self.thumbnails.keys().next().cloned()
            {
                self.thumbnails.remove(&key);
            }
            self.thumbnails.insert(
                hash.into(),
                self.ctx.load_texture(
                    hash,
                    egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                ),
            );
        }
    }
    fn refresh_mapping(&mut self) {
        let pair = self
            .source
            .as_ref()
            .zip(self.model.as_ref())
            .map(|(s, t)| (s.0.clone(), t.0.clone()));
        let same_target = pair
            .as_ref()
            .zip(self.mapping_pair.as_ref())
            .is_some_and(|(a, b)| a.1 == b.1);
        let target_skin = same_target
            .then_some(self.mapping_settings.target_skin)
            .flatten();
        let same_pair = pair.is_some() && pair == self.mapping_pair;
        self.mapping_pair = pair;
        let draft = same_pair.then(|| {
            (
                self.mapping_settings.clone(),
                self.mapping_overrides.clone(),
                self.retarget.clone(),
            )
        });
        self.native_travel = None;
        self.cancel_mapping_request();
        self.mapping_feedback = Default::default();
        self.active_pair = None;
        self.retarget = None;
        self.contacts = None;
        self.plan_requested = None;
        self.contacts_requested = false;
        self.mapping_overrides.clear();
        self.mapping_history = Default::default();
        self.automatic_rows.clear();
        self.ui_state.correspondence.operation = crate::ui::correspondence::Operation::Idle;
        self.mapping_settings = Settings::default();
        self.mapping_settings.target_skin = target_skin;
        if let Some((settings, overrides, _)) = &draft {
            self.mapping_settings = settings.clone();
            self.mapping_overrides = overrides.clone();
        }
        self.selected_pair = None;
        self.ui_state.correspondence.orphan_target = None;
        self.ui_state.correspondence.candidates.clear();
        self.ui_state.mapping_revealed = None;
        self.mapping =
            if let (Some((target, a, _)), Some((source, b, index))) = (&self.model, &self.source) {
                let key = (source.clone(), target.clone(), *index);
                if self.mappings.len() > 512 {
                    self.mappings.clear();
                }
                Some(
                    self.mappings
                        .entry(key)
                        .or_insert_with(|| {
                            compatibility::compile(b, a, &b.clips[*index], source == target)
                        })
                        .clone(),
                )
            } else {
                None
            };
        self.cursors.clear();
        if let (Some((_, target, _)), Some((_, source, index)), Some(mapping)) =
            (&self.model, &self.source, &self.mapping)
        {
            self.native_travel =
                animation::root_travel(target, &source.clips[*index], &mapping.nodes);
        }
        if self
            .mapping
            .as_ref()
            .is_some_and(|m| matches!(m.status, compatibility::Compatibility::NeedsMapping(_)))
        {
            if let Some((_, _, plan)) = draft {
                // Plans transfer the same asset hierarchy across clips; contacts
                // are clip-specific. Never replace a pending skin/edit draft.
                self.retarget = plan;
                if self.retarget.is_some() {
                    self.automatic_rows = self
                        .mapping_pair
                        .as_ref()
                        .and_then(|p| self.automatic_cache.get(p))
                        .cloned()
                        .unwrap_or_default();
                    self.active_pair = self
                        .source
                        .as_ref()
                        .zip(self.model.as_ref())
                        .map(|(s, t)| (s.0.clone(), t.0.clone(), s.2));
                    if self.mapping_dirty() {
                        self.pending_mapping();
                    } else {
                        self.mapping_message(
                            "Applied",
                            "Clip changed · Active mapping retained".into(),
                        );
                    }
                    self.request_contacts();
                } else {
                    self.pending_mapping();
                }
                return;
            }
            if let (Some((source, _, _)), Some((target, _, _))) = (&self.source, &self.model)
                && let Some(plan) = self.plan_cache.get(&(source.clone(), target.clone()))
                && plan.settings.target_skin == self.mapping_settings.target_skin
                && plan.settings.source_skin == self.mapping_settings.source_skin
            {
                self.mapping_settings = plan.settings.clone();
                self.mapping_overrides = plan.overrides.clone();
                self.automatic_rows = self
                    .automatic_cache
                    .get(&(source.clone(), target.clone()))
                    .cloned()
                    .unwrap_or_default();
                self.retarget = Some(Arc::clone(plan));
                self.active_pair = Some((
                    source.clone(),
                    target.clone(),
                    self.source.as_ref().unwrap().2,
                ));
                self.plan_origin = "Resident plan".into();
                self.mapping_message(
                    "Applied",
                    "Resident mapping restored · Preview ready".into(),
                );
                self.request_contacts();
            } else {
                self.request_plan(false);
            }
        }
    }
    fn request_plan(&mut self, save: bool) {
        self.compile_mapping(save, true);
    }
    fn compile_mapping(&mut self, save: bool, restore: bool) {
        self.start_mapping(if save {
            mapping_apply::Intent::Save
        } else if restore {
            mapping_apply::Intent::Restore
        } else {
            mapping_apply::Intent::Apply
        });
    }
    fn request_contacts(&mut self) {
        self.contacts = None;
        let (Some((_, source, index)), Some((_, target, _)), Some(plan)) =
            (&self.source, &self.model, &self.retarget)
        else {
            return;
        };
        // Preview placement is a workstation preference, independent of saved profiles.
        let mut effective = (**plan).clone();
        effective.settings.contacts.looping = if self.ui_state.preferences.continuous {
            crate::contacts::LoopPolicy::Accumulate
        } else {
            crate::contacts::LoopPolicy::Repeat
        };
        effective.key = retarget::key(source, target, &effective.settings);
        let plan = Arc::new(effective);
        let key = crate::contacts::identity(&source.clips[*index], &plan, glam::Mat4::IDENTITY);
        if let Some(c) = self.contact_cache.get(&key) {
            self.contact_cache.pin([key]);
            self.contacts = Some(c);
            return;
        }
        if self.jobs >= 2 {
            self.contacts_requested = true;
            return;
        }
        let source = Arc::clone(source);
        let target = Arc::clone(target);
        let plan = Arc::clone(&plan);
        let index = *index;
        let tx = self.tx.clone();
        let root = self.library.lock().expect("library").root.clone();
        let generation = self.contact_generation.advance();
        self.jobs += 1;
        let worker = std::thread::spawn(move || {
            let result = crate::contacts::cached(
                &root,
                &source,
                &target,
                &source.clips[index],
                &plan,
                glam::Mat4::IDENTITY,
            )
            .map(Arc::new)
            .map_err(|e| format!("Contact preprocessing: {e:#}"));
            let _ = tx.send(Job::Contacts { generation, result });
        });
        self.watch_worker(workers::Kind::Contacts { generation }, worker);
    }
    fn drain(&mut self, renderer: &Renderer, gpu: &Gpu) {
        self.drain_workers();
        while let Ok(job) = self.rx.try_recv() {
            self.jobs = self.jobs.saturating_sub(1);
            match job {
                Job::Motion {
                    generation,
                    entries,
                    skipped,
                    diagnostics,
                } => {
                    if self.motion.index_generation.is_none_or(|g| g == generation) {
                        self.motion.loading = false;
                        self.motion.index_generation = None;
                    }
                    if !self.motion.generation.accepts(generation) {
                        continue;
                    }
                    self.motion.skipped = skipped;
                    self.motion.textures.clear();
                    for (id, status) in &diagnostics {
                        if let Some(preview) = &status.preview {
                            self.motion.textures.insert(
                                id.clone(),
                                self.ctx.load_texture(
                                    format!("fallback-{id}"),
                                    egui::ColorImage::from_rgba_unmultiplied(
                                        [128, 128],
                                        preview.as_raw(),
                                    ),
                                    Default::default(),
                                ),
                            );
                        }
                    }
                    self.motion.diagnostics = diagnostics;
                    for entry in &entries {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [128, 128],
                            entry.preview.as_raw(),
                        );
                        self.motion.textures.insert(
                            entry.id.clone(),
                            self.ctx.load_texture(
                                format!("medoid-{}", entry.id),
                                image,
                                Default::default(),
                            ),
                        );
                    }
                    let query_id = self
                        .motion
                        .entries
                        .get(self.motion.query)
                        .map(|e| e.id.clone());
                    self.motion.query = query_id
                        .and_then(|id| entries.iter().position(|e| e.id == id))
                        .unwrap_or(0);
                    if self
                        .motion
                        .selected_result
                        .as_ref()
                        .is_some_and(|id| !self.records.iter().any(|e| e.id == *id))
                    {
                        self.motion.selected_result = None;
                    }
                    self.motion.entries = entries;
                    self.motion.ranked_query = None;
                }
                Job::Contacts { generation, result } => {
                    if self.contact_generation.accepts(generation) {
                        match result {
                            Ok(data) => {
                                self.contact_cache.pin([data.key.clone()]);
                                self.contact_cache.insert(
                                    data.key.clone(),
                                    Arc::clone(&data),
                                    data.cpu_bytes(),
                                );
                                self.contacts = Some(data)
                            }
                            Err(e) => self.mapping_feedback.details.push_str(&format!("\n{e}")),
                        }
                    }
                }
                Job::Planned {
                    request,
                    restored,
                    result,
                    saved_path,
                    automatic,
                } => {
                    self.accept_mapping(*request, restored, result, saved_path, automatic);
                }
                Job::Import {
                    hash,
                    asset,
                    records,
                } => {
                    self.records = records;
                    self.motion.generation.advance();
                    self.motion.dirty = true;
                    if !asset.meshes.is_empty() {
                        let generation = self.model_generation.advance();
                        self.activate(
                            hash.clone(),
                            Arc::clone(&asset),
                            Kind::Model,
                            generation,
                            renderer,
                            gpu,
                        );
                    }
                    if !asset.clips.is_empty() {
                        let generation = self.clip_generation.advance();
                        self.activate(hash, asset, Kind::Clip(0), generation, renderer, gpu);
                    }
                }
                Job::Loaded {
                    hash,
                    asset,
                    kind,
                    generation,
                } => {
                    let current = match kind {
                        Kind::Model => self.model_generation.accepts(generation),
                        Kind::Clip(_) => self.clip_generation.accepts(generation),
                    };
                    if current {
                        match asset {
                            Ok(asset) => {
                                self.failures.remove(&hash);
                                self.activate(
                                    hash.clone(),
                                    asset,
                                    kind.clone(),
                                    generation,
                                    renderer,
                                    gpu,
                                );
                            }
                            Err(error) => {
                                if self.motion.pending_preview.as_ref().is_some_and(|p| {
                                    p.generation == generation
                                        && p.hash == hash
                                        && kind == Kind::Clip(p.index)
                                }) {
                                    let name = self
                                        .motion
                                        .pending_preview
                                        .as_ref()
                                        .and_then(|p| {
                                            self.records.iter().find(|r| r.id == p.record_id)
                                        })
                                        .map_or("Motion".into(), |r| r.name.clone());
                                    self.preview_failed(&name, &error);
                                } else {
                                    self.status = error;
                                }
                                self.failures.insert(hash.clone());
                            }
                        }
                    }
                    let key = (hash, kind == Kind::Model);
                    if self.pending.get(&key) == Some(&generation) {
                        self.pending.remove(&key);
                    }
                }
                Job::Failed(error) => self.status = error,
                Job::Cancelled => self.status = "Import cancelled".into(),
            }
        }
        self.motion.previews.drain(&self.ctx);
        if self.jobs < 2 {
            if let Some(request) = self.plan_requested.take() {
                self.spawn_mapping(request);
            } else if self.contacts_requested {
                self.contacts_requested = false;
                self.request_contacts();
            }
        }
        if self.jobs == 0
            && let Some(path) = self.paths.pop_front()
        {
            self.import(Some(path));
        }
        if self.jobs == 0 && self.paths.is_empty() && self.motion_auto && !self.records.is_empty() {
            self.motion_auto = false;
            self.request_motion_index();
        }
        if self.jobs == 0 && self.paths.is_empty() && self.motion.dirty && !self.motion.loading {
            self.start_motion_index();
        }
    }
}
struct Shell {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    renderer: Renderer,
    target: Target,
    texture: egui::TextureId,
    editor: Editor,
    capture: Option<Capture>,
    capture_done: bool,
}
struct Capture {
    root: PathBuf,
    frame: u32,
    count: u32,
    ready: u32,
    apply: Option<apply_capture::Driver>,
    library: Option<library_capture::Driver>,
}
impl Shell {
    async fn new(window: Arc<Window>, library: Library) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(Arc::clone(&window))?;
        let (gpu, adapter) = Gpu::new(&instance, Some(&surface)).await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("Surface configuration unavailable")?;
        config.format = config.format.remove_srgb_suffix();
        surface.configure(&gpu.device, &config);
        let apply_driver = apply_capture::Driver::new()?;
        let capture = std::env::var_os("ARMATRA_CAPTURE").map(|p| Capture {
            root: PathBuf::from(p),
            frame: 0,
            count: std::env::var("ARMATRA_CAPTURE_FRAMES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            ready: 0,
            apply: apply_driver,
            library: library_capture::Driver::new(),
        });
        if capture.is_some() {
            anyhow::ensure!(
                surface
                    .get_capabilities(&adapter)
                    .usages
                    .contains(wgpu::TextureUsages::COPY_SRC),
                "Surface capture unsupported by adapter"
            );
            config.usage |= wgpu::TextureUsages::COPY_SRC;
            surface.configure(&gpu.device, &config);
        }
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let egui_state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        let mut egui_renderer = egui_wgpu::Renderer::new(
            &gpu.device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );
        let target = Target::new(&gpu.device, [800, 600]);
        let texture = egui_renderer.register_native_texture(
            &gpu.device,
            &target.sample,
            wgpu::FilterMode::Linear,
        );
        let renderer = Renderer::new(&gpu);
        let editor = Editor::new(library, ctx.clone());
        Ok(Self {
            window,
            surface,
            config,
            gpu,
            ctx,
            egui_state,
            egui_renderer,
            renderer,
            target,
            texture,
            editor,
            capture,
            capture_done: false,
        })
    }
    fn frame(&mut self) -> Result<()> {
        if let Some(reason) = self
            .gpu
            .lost
            .lock()
            .map_err(|_| anyhow::anyhow!("GPU state unavailable"))?
            .as_ref()
        {
            anyhow::bail!("Graphics device lost: {reason}");
        }
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.editor.drain(&self.renderer, &self.gpu);
        self.editor
            .prepare_source_preview(&self.renderer, &self.gpu);
        let now = Instant::now();
        let measured_dt = now.duration_since(self.editor.last_frame).as_secs_f32();
        let dt = if self.capture.is_some() {
            1. / 30.
        } else {
            measured_dt
        };
        self.editor.last_frame = now;
        self.editor.frame_ms = self.editor.frame_ms * 0.9 + measured_dt * 100.;
        if let Some(capture) = &mut self.capture
            && self.editor.jobs == 0
            && !self.editor.motion.previews.pending()
            && self.editor.paths.is_empty()
        {
            capture.ready += 1;
            if capture.ready == 1
                && let Ok(name) = std::env::var("ARMATRA_CAPTURE_PREVIEW")
                && let Some(record) = self
                    .editor
                    .records
                    .iter()
                    .rev()
                    .find(|r| {
                        r.name.split(" \u{b7} ").next() == Some(name.as_str())
                            && matches!(r.kind, Kind::Clip(_))
                    })
                    .cloned()
            {
                self.editor
                    .request_with_preview(&record, Some(0.433), &self.renderer, &self.gpu);
            }
            if capture.ready == 10 {
                if let Ok(filter) = std::env::var("ARMATRA_CAPTURE_FILTER") {
                    self.editor.motion.filter = filter;
                }
                if let Ok(kind) = std::env::var("ARMATRA_CAPTURE_DRAWER") {
                    self.editor.ui_state.drawer.open = match kind.as_str() {
                        "assets" => Some(crate::ui::drawers::Kind::Assets),
                        "detail" => Some(crate::ui::drawers::Kind::Detail),
                        _ => None,
                    };
                }
                if let Ok(name) = std::env::var("ARMATRA_CAPTURE_QUERY")
                    && let Some(i) = self
                        .editor
                        .motion
                        .entries
                        .iter()
                        .position(|e| e.name == name)
                {
                    self.editor.motion.query = i;
                }
                if let Ok(name) = std::env::var("ARMATRA_CAPTURE_RESULT") {
                    self.editor.motion.selected_result = self
                        .editor
                        .motion
                        .entries
                        .iter()
                        .find(|e| e.name == name)
                        .map(|e| e.id.clone());
                }
                if let Some(time) = std::env::var("ARMATRA_CAPTURE_TIME")
                    .ok()
                    .and_then(|v| v.parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                {
                    let duration = self
                        .editor
                        .source
                        .as_ref()
                        .map_or(0., |s| s.1.clips[s.2].duration);
                    self.editor.playback.time = time.clamp(0., duration.max(0.));
                    self.editor.playback.cycle = 0;
                    if let Some(cycle) = std::env::var("ARMATRA_CAPTURE_CYCLE")
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                    {
                        self.editor.playback.cycle = cycle.clamp(-1000, 1000);
                    }
                    self.editor.playback.playing = false;
                    self.editor.cursors.clear();
                }
                self.editor.selected_pair = std::env::var("ARMATRA_MAPPING_JOINT")
                    .ok()
                    .and_then(|v| v.parse::<usize>().ok())
                    .filter(|&i| {
                        self.editor
                            .source
                            .as_ref()
                            .is_some_and(|s| i < s.1.nodes.len())
                    });
            }
            if capture.ready == 10 && std::env::var_os("ARMATRA_CAPTURE_PLAY").is_some() {
                self.editor.playback.playing = true;
            }
            if capture.ready == 10 && std::env::var_os("ARMATRA_CAPTURE_DQS").is_some() {
                self.editor.dqs = true;
            }
            if capture.ready == 10 {
                for (name, selected) in [
                    (
                        "ARMATRA_CAPTURE_SOURCE_SKIN",
                        &mut self.editor.mapping_settings.source_skin,
                    ),
                    (
                        "ARMATRA_CAPTURE_TARGET_SKIN",
                        &mut self.editor.mapping_settings.target_skin,
                    ),
                ] {
                    if let Ok(value) = std::env::var(name) {
                        *selected = Some(value.parse().context("Invalid capture skin index")?);
                    }
                }
            }
            if capture.ready == 13 && std::env::var_os("ARMATRA_CAPTURE_FRAME_CHARACTER").is_some()
            {
                self.editor.frame_requested = true;
            }
            if capture.ready == 10
                && let Ok(edit) = std::env::var("ARMATRA_CAPTURE_MAPPING_EDIT")
                && let Some(source) = self.editor.selected_pair
            {
                let target = if edit == "unmap" {
                    None
                } else {
                    edit.parse::<usize>().ok()
                };
                self.editor.assign_joint(source, target);
            }
            if capture.ready == 11 && std::env::var_os("ARMATRA_CAPTURE_COMPILE").is_some() {
                self.editor.compile_mapping(false, false);
            }
            if capture.count > 1 && std::env::var_os("ARMATRA_CAPTURE_STABLE_CAMERA").is_none() {
                let frame = capture.frame;
                self.editor.camera.yaw = 0.6 + frame as f32 * 0.005;
                self.editor.camera.distance =
                    self.editor.camera.radius * (2.8 + 0.45 * (frame as f32 * 0.025).sin());
                if self.editor.retarget.is_some() {
                    self.editor.contact_correction = frame >= 90;
                    self.editor.show_anchors = frame >= 90;
                    self.editor.mapping_open = (240..360).contains(&frame);
                    self.editor.ui_state.workspace = if self.editor.mapping_open {
                        Workspace::Retarget
                    } else {
                        Workspace::Animate
                    };
                    self.editor.selected_pair = Some(7);
                    if (150..210).contains(&frame) {
                        self.editor.playback.playing = false;
                        self.editor.playback.cycle = 0;
                        self.editor.playback.time = self
                            .editor
                            .source
                            .as_ref()
                            .map_or(0., |s| s.1.clips[s.2].duration)
                            * (1. - (frame - 150) as f32 / 60.);
                        self.editor.status = "DEMO · Deterministic backward scrubbing".into();
                    } else {
                        self.editor.playback.playing = true;
                        self.editor.playback.speed = 0.8;
                        self.editor.status = if frame < 90 {
                            "DEMO · Raw rest-relative transfer"
                        } else if frame < 240 {
                            "DEMO · Contact-corrected motion and target anchors"
                        } else {
                            "DEMO · Saved manual mapping and reusable plans"
                        }
                        .into();
                    }
                    if let (Some((_, a, _)), Some((_, source, index)), Some(p), Some(c)) = (
                        &self.editor.model,
                        &self.editor.source,
                        &self.editor.retarget,
                        &self.editor.contacts,
                    ) {
                        let time = self.editor.playback.time as f64
                            + self.editor.playback.cycle as f64
                                * source.clips[*index].duration as f64;
                        let pose = c.raw_pose(source, a, &source.clips[*index], p, time, true);
                        let global = a.globals(&pose);
                        let rest = a.globals(&a.rest_pose());
                        if let Some((min, max)) = a.default_bounds {
                            let mut delta = global[p.motion_target].w_axis.truncate()
                                - rest[p.motion_target].w_axis.truncate();
                            delta.y = 0.;
                            self.editor.camera.target = (min + max) * 0.5 + delta;
                        }
                    }
                } else if (150..210).contains(&frame) {
                    self.editor.playback.playing = false;
                    self.editor.playback.time = 3. * (1. - (frame - 150) as f32 / 60.);
                    self.editor.status = "DEMO · Precise backward scrubbing".into();
                } else {
                    self.editor.playback.playing = true;
                    self.editor.playback.speed = if frame < 300 { 0.5 } else { 2. };
                    self.editor.status =
                        "DEMO · Exact cross-file playback · Orbit and speed controls".into();
                }
            }
        }
        if self.capture.as_ref().is_some_and(|c| c.count > 1)
            && std::env::var_os("ARMATRA_CAPTURE_STABLE_CAMERA").is_some()
        {
            self.editor.playback.playing = true;
            self.editor.playback.speed = 1.;
            self.editor.status = "Playing authored motion".into();
        }
        if let Some((_, source, i)) = &self.editor.source {
            self.editor.playback.tick(dt, source.clips[*i].duration);
        }
        // Acquire before running egui so skipped/occluded frames do not abandon
        // unacknowledged texture deltas (egui 0.37 requires explicit consumption).
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.gpu.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            e => anyhow::bail!("Surface acquisition: {e:?}"),
        };
        let mut input = self.egui_state.take_egui_input(&self.window);
        if self
            .capture
            .as_ref()
            .is_some_and(|c| c.apply.is_some() || c.library.is_some())
        {
            // Recorded input owns this test stream. Live focus loss, pointer
            // movement or keys from another desktop window must not cancel its
            // popups. This supplies logical egui focus, never OS input/focus.
            input.events.clear();
            input.focused = true;
        }
        let mapping_capture_done = if let Some(capture) = &mut self.capture {
            if let Some(driver) = &mut capture.library {
                driver.drive(&self.editor, &mut input, capture.ready)?
            } else if let Some(driver) = &mut capture.apply {
                driver.drive(&self.editor, &mut input, capture.ready)?
            } else {
                mapping_capture::drive(&self.editor, &mut input, capture.ready)?
            }
        } else {
            true
        };
        if self.capture.as_ref().is_some_and(|c| c.ready == 6)
            && let Ok(name) = std::env::var("ARMATRA_CAPTURE_SCROLL")
            && let Some((rect, _)) = self
                .editor
                .input_regions
                .as_ref()
                .and_then(|r| r.utility_scrolls.get(name.as_str()))
        {
            input.events.push(egui::Event::PointerMoved(rect.center()));
            input.events.push(egui::Event::Key {
                key: egui::Key::PageDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            });
        }
        if self.capture.is_some()
            && let Some(time) = std::env::var("ARMATRA_CAPTURE_HOVER_TIME")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite())
            && let Some(regions) = &self.editor.input_regions
            && regions.timeline_clip.is_positive()
        {
            let duration = self
                .editor
                .source
                .as_ref()
                .map_or(0., |s| s.1.clips[s.2].duration);
            input.events.push(egui::Event::PointerMoved(egui::pos2(
                crate::ui::timeline::x_at(time, regions.seek, duration),
                regions.timeline_clip.center().y,
            )));
        }
        let mut dimensions = self.target.size;
        let mut output = self.ctx.run_ui(input, |root| {
            dimensions = self
                .editor
                .ui(root, self.texture, &self.renderer, &self.gpu);
        });
        // UI actions may synchronously activate a cached clip. Reconcile before
        // drawing THIS frame, not the next one. CPU and GPU assets stay paired.
        self.editor
            .prepare_source_preview(&self.renderer, &self.gpu);
        self.egui_state
            .handle_platform_output(&self.window, output.platform_output);
        let max = self.gpu.device.limits().max_texture_dimension_2d;
        dimensions = [dimensions[0].clamp(1, max), dimensions[1].clamp(1, max)];
        if dimensions != self.target.size {
            self.target = Target::new(&self.gpu.device, dimensions);
            self.egui_renderer.update_egui_texture_from_wgpu_texture(
                &self.gpu.device,
                &self.target.sample,
                wgpu::FilterMode::Linear,
                self.texture,
            );
        }
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.editor.preview_boundary("preview_pose_render_begin");
        self.renderer.dqs.set(self.editor.dqs);
        if let Some(preview) = &mut self.editor.source_preview {
            let (asset, index, resident, camera) = (
                &preview.asset,
                preview.clip,
                &preview.resident,
                &mut preview.camera,
            );
            let mut pose = animation::evaluate(
                asset,
                &asset.clips[index],
                &(0..asset.nodes.len()).map(Some).collect::<Vec<_>>(),
                self.editor.playback.time,
                &mut self.editor.cursors,
            );
            if self.editor.ui_state.preferences.continuous && self.editor.playback.looping {
                animation::place_cycle(
                    asset,
                    &mut pose,
                    self.editor.source_travel,
                    self.editor.playback.cycle,
                );
            }
            self.renderer
                .show_grid
                .set(self.editor.ui_state.preferences.grid);
            if self.editor.frame_requested
                && let Some((min, max)) = if self.capture.is_some()
                    && std::env::var_os("ARMATRA_CAPTURE_FULL_GEOMETRY").is_some()
                {
                    asset.geometry_pose_bounds(asset.default_scene, false, &pose)
                } else {
                    asset.character_pose_bounds(asset.default_scene, &pose)
                }
            {
                camera.fit_bounds(min, max, camera.aspect);
            }
            self.editor.render_warnings = self.renderer.render(
                &self.gpu,
                &mut encoder,
                &self.target,
                camera,
                asset,
                resident,
                &asset.globals(&pose),
                asset.default_scene,
                self.editor.skeleton || asset.meshes.is_empty(),
                self.editor.mesh,
            );
            self.editor.skinning_report = self.renderer.skinning_report.get();
        } else if let Some((_, asset, resident)) = self.editor.model.clone() {
            let (pose, correction_warnings) = self.editor.target_pose();
            if self.capture.is_some() {
                self.editor.capture_pose = Some(pose.clone());
            }
            if self.editor.frame_requested
                && let Some((min, max)) = if self.capture.is_some()
                    && std::env::var_os("ARMATRA_CAPTURE_FULL_GEOMETRY").is_some()
                {
                    asset.geometry_pose_bounds(self.editor.scene, false, &pose)
                } else {
                    asset.character_pose_bounds(self.editor.scene, &pose)
                }
            {
                self.editor
                    .camera
                    .fit_bounds(min, max, self.editor.camera.aspect);
            }
            self.renderer
                .show_grid
                .set(self.editor.ui_state.preferences.grid);
            self.editor.render_warnings = self.renderer.render(
                &self.gpu,
                &mut encoder,
                &self.target,
                &self.editor.camera,
                &asset,
                &resident,
                &asset.globals(&pose),
                self.editor.scene,
                self.editor.skeleton,
                self.editor.mesh,
            );
            self.editor.render_warnings.extend(correction_warnings);
            self.editor.skinning_report = self.renderer.skinning_report.get();
        } else {
            self.editor.skinning_report = Default::default();
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: theme::STAGE[0],
                            g: theme::STAGE[1],
                            b: theme::STAGE[2],
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        self.editor.frame_requested = false;
        self.editor.preview_boundary("preview_pose_render_encoded");
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.egui_renderer
                    .update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
            }
        }
        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: output.pixels_per_point,
        };
        let extra = self.egui_renderer.update_buffers(
            &self.gpu.device,
            &self.gpu.queue,
            &mut encoder,
            &jobs,
            &screen,
        );
        let view = frame.texture.create_view(&Default::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut pass, &jobs, &screen);
        }
        self.gpu
            .queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        self.editor.preview_boundary("preview_gpu_submitted");
        for id in &output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        output.textures_delta.clear();
        if self.capture.as_ref().is_some_and(|c| c.library.is_some())
            && self.editor.motion.display_pending.is_some()
        {
            let image = crate::diagnostics::read_target(&self.gpu, &self.target)?;
            self.editor.motion.frame_proof = Some(
                serde_json::json!({"width":image.width(),"height":image.height(),"blake3":blake3::hash(image.as_raw()).to_hex().as_str(),"gpu_readback":true}),
            );
        }
        if let Some(capture) = &mut self.capture
            && capture.ready > 13
            && mapping_capture_done
            && (self.editor.jobs == 0
                || (capture.apply.as_ref().is_some_and(|d| d.progress_capture())
                    && self.editor.plan_pending))
            && !self.editor.motion.previews.pending()
            && self.editor.paths.is_empty()
        {
            std::fs::create_dir_all(&capture.root)?;
            let image = crate::diagnostics::read_texture(&self.gpu, &frame.texture)?;
            image.save(capture.root.join(if capture.count == 1 {
                "editor.png".into()
            } else {
                format!("frame-{:04}.png", capture.frame)
            }))?;
            if capture.frame == 0 {
                let mut session = serde_json::json!({"version":crate::VERSION,"motion_indexed":self.editor.motion.entries.len(),"motion_skipped":self.editor.motion.skipped,"dqs_requested":self.editor.dqs,"render_warnings":self.editor.render_warnings,"target":self.editor.model.as_ref().map(|m|&m.0),"source":self.editor.source.as_ref().map(|s|&s.0),"plan_origin":self.editor.plan_origin,"plan":self.editor.retarget.as_ref().map(|p|&p.key),"overrides":self.editor.retarget.as_ref().map(|p|&p.overrides),"contact_intervals":self.editor.contacts.as_ref().map(|c|c.intervals.len()),"contact_interval_data":self.editor.contacts.as_ref().map(|c|&c.intervals)});
                session["ui"] = self.editor.ui_evidence();
                session["input_playback"] = serde_json::json!({"method":"isolated egui pointer/key event stream with logical window focus through real native UI; no forced mapping/skin/plan state; no OS input/focus","events":capture.apply.as_ref().map(|d|&d.trace).or_else(||capture.library.as_ref().map(|d|&d.trace))});
                session["rendered_pose"] = self
                    .editor
                    .capture_pose
                    .as_ref()
                    .zip(self.editor.model.as_ref())
                    .map(|(pose, (_, asset, _))| self.editor.rendered_pose_evidence(asset, pose))
                    .unwrap_or(serde_json::Value::Null);
                session["capture"] = serde_json::json!({"kind":"Native composed winit GPU surface readback","source_commit":option_env!("ARMATRA_BUILD_COMMIT"),"binary_blake3":blake3::hash(&std::fs::read(std::env::current_exe()?)?).to_hex().as_str(),"physical_size":[size.width,size.height],"viewport_size":self.target.size,"ui_scale":self.editor.ctx.zoom_factor(),"pixels_per_point":self.editor.ctx.pixels_per_point(),"adapter":self.gpu.info.name,"backend":format!("{:?}",self.gpu.info.backend),"driver":self.gpu.info.driver_info,"manual_os_input":false});
                std::fs::write(
                    capture.root.join("session.json"),
                    serde_json::to_vec_pretty(&session)?,
                )?;
            }
            capture.frame += 1;
            if capture.frame >= capture.count {
                self.capture_done = true;
            }
        }
        self.gpu.queue.present(frame);
        self.editor.preview_displayed();
        Ok(())
    }
}
struct Application {
    shell: Option<Shell>,
    library: Option<Library>,
    error: Option<String>,
    paths: Vec<PathBuf>,
}
impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.shell.is_some() {
            return;
        }
        let result = (|| -> Result<Shell> {
            let window_size = std::env::var("ARMATRA_WINDOW_SIZE")
                .ok()
                .and_then(|s| {
                    let (w, h) = s.split_once('x')?;
                    Some((
                        w.parse::<f64>().ok()?.clamp(800., 3840.),
                        h.parse::<f64>().ok()?.clamp(540., 2160.),
                    ))
                })
                .unwrap_or((1440., 900.));
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title("Armatra · Animation Workspace")
                        .with_inner_size(winit::dpi::LogicalSize::new(window_size.0, window_size.1))
                        .with_min_inner_size(winit::dpi::LogicalSize::new(800., 540.)),
                )?,
            );
            pollster::block_on(Shell::new(
                window,
                self.library.take().context("Library unavailable")?,
            ))
        })();
        match result {
            Ok(mut shell) => {
                shell.editor.paths.extend(self.paths.drain(..));
                if shell.editor.paths.is_empty() {
                    let restored =
                        crate::session::load(&shell.editor.library.lock().expect("library"));
                    if let Some((model, clip)) = restored {
                        for record in [model, clip] {
                            shell.editor.request(&record, &shell.renderer, &shell.gpu);
                        }
                    } else if std::env::var_os("ARMATRA_REUSE_LIBRARY").is_some() {
                        let model = shell
                            .editor
                            .records
                            .iter()
                            .find(|r| r.kind == Kind::Model)
                            .cloned();
                        let clip = shell
                            .editor
                            .records
                            .iter()
                            .rev()
                            .find(|r| matches!(r.kind, Kind::Clip(_)))
                            .cloned();
                        for record in model.into_iter().chain(clip) {
                            shell.editor.request(&record, &shell.renderer, &shell.gpu);
                        }
                    }
                }
                self.shell = Some(shell);
            }
            Err(e) => {
                let text = format!("Armatra could not start.\n{e:#}");
                eprintln!("{text}");
                rfd::MessageDialog::new()
                    .set_title("Armatra initialization failed")
                    .set_description(&text)
                    .set_level(rfd::MessageLevel::Error)
                    .show();
                self.error = Some(text);
                event_loop.exit();
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(shell) = &mut self.shell else {
            return;
        };
        let _ = shell.egui_state.on_window_event(&shell.window, &event);
        match event {
            WindowEvent::CloseRequested => {
                crate::app_log::event("window_close_requested", serde_json::json!({}));
                event_loop.exit();
            }
            WindowEvent::DroppedFile(path) => shell.editor.import(Some(path)),
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    shell.config.width = size.width;
                    shell.config.height = size.height;
                    shell.surface.configure(&shell.gpu.device, &shell.config);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = shell.frame() {
                    shell.editor.status = format!("Rendering failed: {e:#}");
                    crate::app_log::event(
                        "fatal_frame_error",
                        serde_json::json!({"error":format!("{e:#}"),"preview":shell.editor.preview_identity()}),
                    );
                    eprintln!("{}", shell.editor.status);
                    self.error = Some(shell.editor.status.clone());
                    event_loop.exit();
                }
                if shell.capture_done {
                    event_loop.exit();
                }
            }
            _ => (),
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(shell) = &self.shell {
            shell.window.request_redraw();
        }
    }
}
pub fn run(paths: Vec<PathBuf>) -> Result<()> {
    let library = Library::open_seeded(crate::library::default_root())
        .context("Could not open persistent library")?;
    let event_loop = EventLoop::new()?;
    let mut app = Application {
        shell: None,
        library: Some(library),
        error: None,
        paths,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        anyhow::bail!(error);
    }
    Ok(())
}
