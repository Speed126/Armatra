//! Immutable blobs and checksum-protected atomic manifest generations (schema 1).
use crate::{
    import::{self, Limits},
    scene::Asset,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NONCE: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    Model,
    Clip(usize),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub hash: String,
    pub name: String,
    pub kind: Kind,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub generation: u64,
    pub records: Vec<Record>,
    #[serde(default)]
    pub bundled_version: u32,
    #[serde(default)]
    pub bundled_completed: std::collections::BTreeMap<String, String>,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    checksum: String,
    payload: String,
}
pub struct Library {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub recovery: Option<String>,
    _writer_lock: std::fs::File,
}
pub fn default_root() -> PathBuf {
    if let Some(root) = std::env::var_os("ARMATRA_LIBRARY") {
        return root.into();
    }
    #[cfg(target_os = "windows")]
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(root).join("Armatra").join("library");
    }
    #[cfg(target_os = "macos")]
    if let Some(root) = std::env::var_os("HOME") {
        return PathBuf::from(root).join("Library/Application Support/Armatra/library");
    }
    if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(root).join("armatra/library");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/armatra/library")
}
pub fn atomic_publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("publication directory")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}-{}-{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
impl Library {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("blobs"))?;
        fs::create_dir_all(root.join("manifests"))?;
        fs::create_dir_all(root.join("thumbnails"))?;
        let writer_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("writer.lock"))?;
        writer_lock
            .try_lock()
            .context("Library is already open in another Armatra process")?;
        let mut generations: Vec<_> = fs::read_dir(root.join("manifests"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        generations.sort();
        generations.reverse();
        let mut recovery = None;
        for path in &generations {
            let candidate = (|| -> Result<Manifest> {
                if fs::metadata(path)?.len() > 16 * 1024 * 1024 {
                    bail!("manifest too large");
                }
                let env: Envelope = serde_json::from_slice(&fs::read(path)?)?;
                if blake3::hash(env.payload.as_bytes()).to_hex().as_str() != env.checksum {
                    bail!("manifest checksum");
                }
                let manifest: Manifest = serde_json::from_str(&env.payload)?;
                for record in &manifest.records {
                    if record.hash.len() != 64
                        || !record.hash.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        bail!("invalid content hash");
                    }
                }
                Ok(manifest)
            })();
            match candidate {
                Ok(manifest) => {
                    if manifest.schema != 1 {
                        bail!(
                            "Library schema {} needs migration; this build supports schema 1",
                            manifest.schema
                        );
                    }
                    return Ok(Self {
                        root,
                        manifest,
                        recovery,
                        _writer_lock: writer_lock,
                    });
                }
                Err(e) => {
                    recovery = Some(format!(
                        "Recovered past invalid generation {}: {e}",
                        path.display()
                    ))
                }
            }
        }
        if !generations.is_empty() {
            bail!("All library generations are invalid; preserve the library for recovery");
        }
        Ok(Self {
            root,
            manifest: Manifest {
                schema: 1,
                generation: 0,
                records: Vec::new(),
                bundled_version: 0,
                bundled_completed: Default::default(),
            },
            recovery,
            _writer_lock: writer_lock,
        })
    }
    pub fn blob_path(&self, hash: &str) -> PathBuf {
        self.root.join("blobs").join(format!("{hash}.glb"))
    }
    /// User startup seeds once, before the UI reads records. Tool/test libraries
    /// may still use `open` without user-facing content.
    pub fn open_seeded(root: PathBuf) -> Result<Self> {
        let mut library = Self::open(root)?;
        library.seed_bundled(crate::bundled::VERSION, crate::bundled::MOTIONS)?;
        Ok(library)
    }
    pub fn seed_bundled(&mut self, version: u32, motions: &[crate::bundled::Motion]) -> Result<()> {
        if self.manifest.bundled_version >= version {
            return Ok(());
        }
        let mut next = self.manifest.clone();
        for motion in motions {
            if next.bundled_completed.contains_key(motion.id) {
                continue;
            }
            let asset = import::from_bytes(motion.bytes, &Limits::default())?;
            anyhow::ensure!(
                asset.meshes.is_empty() && !asset.clips.is_empty(),
                "Bundled content must be motion-only"
            );
            let hash = self.store(motion.bytes)?;
            let existing = next.records.len();
            Self::add_records(&mut next, &hash, motion.name, &asset);
            for record in next.records.iter_mut().skip(existing) {
                if let Kind::Clip(index) = record.kind {
                    record.name = asset.clips[index].name.clone();
                }
            }
            next.bundled_completed.insert(motion.id.into(), hash);
        }
        next.bundled_version = version;
        // Records and the completed seed ledger publish in one recoverable
        // manifest transaction, including on interrupted first startup.
        self.publish(next)
    }
    /// Remove only the library item; keep immutable blobs/recovery generations
    /// and all original files. The bundled completion ledger is unchanged.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let mut next = self.manifest.clone();
        next.records.retain(|r| r.id != id);
        if next.records.len() != self.manifest.records.len() {
            self.publish(next)?;
        }
        Ok(())
    }
    pub fn load(&self, hash: &str) -> Result<Asset> {
        let bytes = import::read_file(&self.blob_path(hash), &Limits::default())?;
        if blake3::hash(&bytes).to_hex().as_str() != hash {
            bail!("Stored GLB checksum mismatch: {hash}");
        }
        Ok(import::from_bytes(&bytes, &Limits::default())?)
    }
    pub fn import(&mut self, bytes: &[u8], name: &str, asset: &Asset) -> Result<String> {
        let hash = self.store(bytes)?;
        let mut next = self.manifest.clone();
        Self::add_records(&mut next, &hash, name, asset);
        self.publish(next)?;
        Ok(hash)
    }
    fn store(&self, bytes: &[u8]) -> Result<String> {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let path = self.blob_path(&hash);
        if !path.exists() {
            atomic_publish(&path, bytes)?;
        } else if blake3::hash(&fs::read(&path)?).to_hex().as_str() != hash {
            bail!("Existing blob is corrupt; import was not committed");
        }
        Ok(hash)
    }
    fn add_records(next: &mut Manifest, hash: &str, name: &str, asset: &Asset) {
        if !asset.meshes.is_empty() {
            next.records.push(Record {
                id: format!("{hash}:model"),
                hash: hash.into(),
                name: name.into(),
                kind: Kind::Model,
            });
        }
        for (index, clip) in asset.clips.iter().enumerate() {
            next.records.push(Record {
                id: format!("{hash}:clip:{index}"),
                hash: hash.into(),
                name: format!("{} · {name}", clip.name),
                kind: Kind::Clip(index),
            });
        }
        let mut seen = std::collections::HashSet::new();
        next.records.retain(|r| seen.insert(r.id.clone()));
    }
    fn publish(&mut self, mut next: Manifest) -> Result<()> {
        // Skip occupied/corrupt generation names recovered at startup. Never overwrite a generation.
        next.generation += 1;
        while self
            .root
            .join("manifests")
            .join(format!("{:020}.json", next.generation))
            .exists()
        {
            next.generation += 1;
        }
        let payload = serde_json::to_string(&next)?;
        let env = Envelope {
            checksum: blake3::hash(payload.as_bytes()).to_hex().to_string(),
            payload,
        };
        let encoded = serde_json::to_vec_pretty(&env)?;
        if encoded.len() > 16 * 1024 * 1024 {
            bail!(
                "Manifest exceeds the 16 MiB publication limit; existing generations remain valid"
            );
        }
        atomic_publish(
            &self
                .root
                .join("manifests")
                .join(format!("{:020}.json", next.generation)),
            &encoded,
        )?;
        self.manifest = next;
        Ok(())
    }
}
