//! One bounded thumbnail worker, independent of the two engine load slots.
//! Generated previews are versioned derivatives of immutable managed blobs.
use super::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver, SyncSender},
};

pub(super) struct PoseTexture {
    pub time: f32,
    pub texture: egui::TextureHandle,
}
pub(super) struct Strip {
    pub frames: Vec<PoseTexture>,
}
struct Request {
    generation: u64,
    key: String,
    record: Record,
    times: Vec<f32>,
}
struct Response {
    generation: u64,
    key: String,
    result: std::result::Result<Vec<crate::motion_search::PoseImage>, String>,
}
pub(super) struct Previews {
    cache: ByteLru<String, Strip>,
    tx: SyncSender<Request>,
    rx: Receiver<Response>,
    generation: Arc<AtomicU64>,
    pending: std::collections::HashSet<(u64, String)>,
    errors: HashMap<String, String>,
    root: PathBuf,
    worker: std::thread::JoinHandle<()>,
}
pub(super) fn key(record: &Record, summary: &crate::motion_search::Summary) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(record.id.as_bytes());
    hash.update(record.hash.as_bytes());
    hash.update(&summary.version.to_le_bytes());
    hash.update(&crate::motion_search::PREVIEW_FRAME_VERSION.to_le_bytes());
    for medoid in &summary.medoids {
        hash.update(&medoid.time.to_bits().to_le_bytes());
    }
    hash.finalize().to_hex().to_string()
}
impl Previews {
    pub fn new(root: PathBuf, ctx: egui::Context) -> Self {
        let generation = Arc::new(AtomicU64::new(0));
        let (tx, rx, worker) = Self::spawn(root.clone(), ctx, Arc::clone(&generation));
        Self {
            cache: ByteLru::new(16 * 1024 * 1024),
            tx,
            rx,
            generation,
            pending: Default::default(),
            errors: Default::default(),
            root,
            worker,
        }
    }
    fn spawn(
        root: PathBuf,
        ctx: egui::Context,
        generation: Arc<AtomicU64>,
    ) -> (
        SyncSender<Request>,
        Receiver<Response>,
        std::thread::JoinHandle<()>,
    ) {
        let (tx, requests) = mpsc::sync_channel::<Request>(4);
        let (results, rx) = mpsc::sync_channel::<Response>(4);
        let worker_generation = Arc::clone(&generation);
        let worker = std::thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                if request.generation != worker_generation.load(Ordering::Acquire) {
                    continue;
                }
                let result = load(&root, &request).map_err(|e| format!("Pose preview: {e:#}"));
                if let Err(error) = &result {
                    crate::app_log::event(
                        "pose_thumbnail_failed",
                        serde_json::json!({"key":request.key,"generation":request.generation,"error":error}),
                    );
                }
                if results
                    .send(Response {
                        generation: request.generation,
                        key: request.key,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
        });
        (tx, rx, worker)
    }
    pub fn invalidate(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.pending.clear();
        self.errors.clear();
    }
    pub fn request(&mut self, record: &Record, summary: &crate::motion_search::Summary) {
        let key = key(record, summary);
        let generation = self.generation.load(Ordering::Acquire);
        if self.cache.contains(&key)
            || self.pending.contains(&(generation, key.clone()))
            || self.errors.contains_key(&key)
        {
            return;
        }
        let request = Request {
            generation,
            key: key.clone(),
            record: record.clone(),
            times: summary.medoids.iter().map(|m| m.time).collect(),
        };
        if self.tx.try_send(request).is_ok() {
            self.pending.insert((generation, key));
        }
    }
    pub fn drain(&mut self, ctx: &egui::Context) {
        if self.worker.is_finished() {
            for (_, key) in self.pending.drain() {
                self.errors.insert(
                    key,
                    "Pose thumbnail worker stopped; playback remains available".into(),
                );
            }
            self.generation.fetch_add(1, Ordering::AcqRel);
            let (tx, rx, worker) =
                Self::spawn(self.root.clone(), ctx.clone(), Arc::clone(&self.generation));
            let previous = std::mem::replace(&mut self.worker, worker);
            let panicked = previous.join().is_err();
            crate::app_log::event(
                "pose_worker_restarted",
                serde_json::json!({"panicked":panicked}),
            );
            self.tx = tx;
            self.rx = rx;
        }
        while let Ok(response) = self.rx.try_recv() {
            self.pending
                .remove(&(response.generation, response.key.clone()));
            if response.generation != self.generation.load(Ordering::Acquire) {
                continue;
            }
            match response.result {
                Ok(images) => {
                    let bytes = images.iter().map(|p| p.image.as_raw().len()).sum();
                    let frames = images
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [p.image.width() as usize, p.image.height() as usize],
                                p.image.as_raw(),
                            );
                            PoseTexture {
                                time: p.time,
                                texture: ctx.load_texture(
                                    format!("pose-{}-{i}", response.key),
                                    image,
                                    egui::TextureOptions::LINEAR,
                                ),
                            }
                        })
                        .collect();
                    self.cache
                        .insert(response.key, Arc::new(Strip { frames }), bytes);
                }
                Err(error) => {
                    if self.errors.len() >= 128 {
                        self.errors.clear();
                    }
                    self.errors.insert(response.key, error);
                }
            }
        }
    }
    pub fn get(&mut self, key: &str) -> Option<Arc<Strip>> {
        self.cache.get(&key.to_owned())
    }
    pub fn pin(&mut self, keys: Vec<String>) {
        self.cache.pin(keys);
    }
    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.cache.bytes()
    }
    /// Release optional derived textures independently of playable GPU assets.
    pub fn clear_memory_cache(&mut self) {
        self.cache = ByteLru::new(16 * 1024 * 1024);
    }
    pub fn error(&self, key: &str) -> Option<&str> {
        self.errors.get(key).map(String::as_str)
    }
}
fn load(root: &std::path::Path, request: &Request) -> Result<Vec<crate::motion_search::PoseImage>> {
    let folder = root.join("derived/motion-previews-v2");
    let mut times = request.times.clone();
    times.sort_by(f32::total_cmp);
    times.truncate(4);
    let cached: Result<Vec<crate::motion_search::PoseImage>> = {
        times
            .iter()
            .enumerate()
            .map(|(i, time)| {
                let path = folder.join(format!("{}-{i}.png", request.key));
                anyhow::ensure!(
                    std::fs::metadata(&path)?.len() < 2 * 1024 * 1024,
                    "Preview file budget"
                );
                let mut reader = image::ImageReader::open(&path)?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(260);
                limits.max_image_height = Some(270);
                limits.max_alloc = Some(2 * 1024 * 1024);
                reader.limits(limits);
                let image = reader.decode()?.into_rgba8();
                anyhow::ensure!(image.dimensions() == (260, 270), "Preview dimensions");
                Ok(crate::motion_search::PoseImage { time: *time, image })
            })
            .collect()
    };
    if let Ok(images) = cached {
        return Ok(images);
    }
    let Kind::Clip(index) = request.record.kind else {
        anyhow::bail!("Expected motion clip");
    };
    let bytes = import::read_file(
        &root
            .join("blobs")
            .join(format!("{}.glb", request.record.hash)),
        &Limits::default(),
    )?;
    anyhow::ensure!(
        blake3::hash(&bytes).to_hex().as_str() == request.record.hash,
        "Blob checksum mismatch"
    );
    let asset = import::from_bytes(&bytes, &Limits::default())?;
    anyhow::ensure!(
        asset.cpu_bytes() <= 128 * 1024 * 1024,
        "Preview asset budget"
    );
    let clip = asset.clips.get(index).context("Missing preview clip")?;
    let images = crate::motion_search::preview_strip(&asset, clip, &times);
    std::fs::create_dir_all(&folder)?;
    for (i, p) in images.iter().enumerate() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        p.image.write_to(&mut bytes, image::ImageFormat::Png)?;
        crate::library::atomic_publish(
            &folder.join(format!("{}-{i}.png", request.key)),
            bytes.get_ref(),
        )?;
    }
    Ok(images)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(label: &str) -> (PathBuf, Record, crate::motion_search::Summary) {
        let root = PathBuf::from("artifacts")
            .join(format!("preview-worker-{label}-{}", std::process::id()));
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        let (_, builder) = crate::motion_fixtures::motions().remove(0);
        let bytes = builder.finish();
        let hash = blake3::hash(&bytes).to_hex().to_string();
        std::fs::write(root.join("blobs").join(format!("{hash}.glb")), &bytes).unwrap();
        let asset = import::from_bytes(&bytes, &Limits::default()).unwrap();
        let summary = crate::motion_search::summarize(&asset, &asset.clips[0]).unwrap();
        (
            root,
            Record {
                id: format!("{hash}:clip:0"),
                hash,
                name: "Original walk".into(),
                kind: Kind::Clip(0),
            },
            summary,
        )
    }
    fn finish(p: &mut Previews, ctx: &egui::Context) {
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while p.pending() {
            p.drain(ctx);
            assert!(Instant::now() < deadline, "Thumbnail worker timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn thumbnail_error_and_cache_eviction_allow_later_requests() {
        let (root, record, summary) = fixture("cache");
        let ctx = egui::Context::default();
        let mut p = Previews::new(root, ctx.clone());
        let mut missing = record.clone();
        missing.hash = "0".repeat(64);
        missing.id = "missing".into();
        p.request(&missing, &summary);
        finish(&mut p, &ctx);
        assert!(p.error(&key(&missing, &summary)).is_some());
        p.request(&record, &summary);
        finish(&mut p, &ctx);
        assert!(p.get(&key(&record, &summary)).is_some());
        assert!(p.bytes() <= 16 * 1024 * 1024);
        p.cache = ByteLru::new(0);
        p.request(&record, &summary);
        finish(&mut p, &ctx);
        assert_eq!(p.bytes(), 0);
        p.cache = ByteLru::new(16 * 1024 * 1024);
        p.request(&record, &summary);
        finish(&mut p, &ctx);
        assert!(p.get(&key(&record, &summary)).is_some());
    }
    #[test]
    fn joined_failed_thumbnail_worker_clears_pending_and_restarts() {
        let (root, record, summary) = fixture("restart");
        let ctx = egui::Context::default();
        let mut p = Previews::new(root, ctx.clone());
        p.worker = std::thread::spawn(|| panic!("Deliberate isolated thumbnail worker regression"));
        p.pending.insert((0, "interrupted".into()));
        while !p.worker.is_finished() {
            std::thread::yield_now();
        }
        p.drain(&ctx);
        assert!(!p.pending());
        assert!(p.error("interrupted").is_some());
        p.request(&record, &summary);
        finish(&mut p, &ctx);
        assert!(p.get(&key(&record, &summary)).is_some());
    }
}
