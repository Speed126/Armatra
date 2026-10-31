//! Small recoverable selection generations, independent of immutable assets.
use crate::library::{Kind, Library, Record, atomic_publish};
use anyhow::Result;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
struct Selection {
    schema: u32,
    model: String,
    clip: String,
}
pub fn save(library: &Library, model: &str, source: &str, index: usize) -> Result<()> {
    let Some(m) = library
        .manifest
        .records
        .iter()
        .find(|r| r.hash == model && r.kind == Kind::Model)
    else {
        return Ok(());
    };
    let Some(c) = library
        .manifest
        .records
        .iter()
        .find(|r| r.hash == source && r.kind == Kind::Clip(index))
    else {
        return Ok(());
    };
    let selection = Selection {
        schema: 1,
        model: m.id.clone(),
        clip: c.id.clone(),
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    atomic_publish(
        &library
            .root
            .join("sessions")
            .join(format!("{stamp:032}.json")),
        &serde_json::to_vec(&selection)?,
    )
}
pub fn load(library: &Library) -> Option<(Record, Record)> {
    let mut paths: Vec<_> = std::fs::read_dir(library.root.join("sessions"))
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths.reverse();
    for p in paths.into_iter().take(256) {
        if std::fs::metadata(&p).ok()?.len() > 4096 {
            continue;
        }
        let Ok(bytes) = std::fs::read(p) else {
            continue;
        };
        let Ok(s) = serde_json::from_slice::<Selection>(&bytes) else {
            continue;
        };
        if s.schema != 1 {
            continue;
        }
        let m = library
            .manifest
            .records
            .iter()
            .find(|r| r.id == s.model && r.kind == Kind::Model);
        let c = library
            .manifest
            .records
            .iter()
            .find(|r| r.id == s.clip && matches!(r.kind, Kind::Clip(_)));
        if let (Some(m), Some(c)) = (m, c) {
            return Some((m.clone(), c.clone()));
        }
    }
    None
}
