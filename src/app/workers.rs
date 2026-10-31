//! Observe terminated workers without catching panics around editor invariants.
use super::*;
pub(super) enum Kind {
    Import,
    Load {
        hash: String,
        model: bool,
        generation: u64,
    },
    Mapping {
        generation: u64,
    },
    Contacts {
        generation: u64,
    },
    Index {
        generation: u64,
    },
}
pub(super) struct Worker {
    pub kind: Kind,
    pub handle: std::thread::JoinHandle<()>,
}
impl Editor {
    pub(super) fn watch_worker(&mut self, kind: Kind, handle: std::thread::JoinHandle<()>) {
        self.workers.push(Worker { kind, handle });
    }
    pub(super) fn drain_workers(&mut self) {
        let mut i = 0;
        while i < self.workers.len() {
            if !self.workers[i].handle.is_finished() {
                i += 1;
                continue;
            }
            let worker = self.workers.swap_remove(i);
            if worker.handle.join().is_ok() {
                continue;
            }
            self.jobs = self.jobs.saturating_sub(1);
            let reason = "Background worker panicked. See the application log for its backtrace; retry or choose another motion.";
            crate::app_log::event("engine_worker_failed", serde_json::json!({"reason":reason}));
            match worker.kind {
                Kind::Import => self.status = format!("Import failed · {reason}"),
                Kind::Load {
                    hash,
                    model,
                    generation,
                } => {
                    let key = (hash, model);
                    if self.pending.get(&key) == Some(&generation) {
                        self.pending.remove(&key);
                    }
                    if !model && self.clip_generation.accepts(generation) {
                        let name = self
                            .motion
                            .pending_preview
                            .as_ref()
                            .and_then(|p| self.records.iter().find(|r| r.id == p.record_id))
                            .map_or("Motion".into(), |r| r.name.clone());
                        self.preview_failed(&name, reason);
                    } else if model && self.model_generation.accepts(generation) {
                        self.status = format!("Character load failed · {reason}");
                    }
                }
                Kind::Mapping { generation } if self.plan_generation.accepts(generation) => {
                    self.plan_pending = false;
                    self.plan_requested = None;
                    self.retarget = None;
                    self.active_pair = None;
                    self.contacts = None;
                    self.mapping_message("Failed", format!("Mapping worker failed · {reason}"));
                }
                Kind::Contacts { generation } if self.contact_generation.accepts(generation) => {
                    self.contacts_requested = false;
                    self.mapping_feedback
                        .details
                        .push_str(&format!("\nContact preprocessing failed · {reason}"));
                }
                Kind::Index { generation } if self.motion.index_generation == Some(generation) => {
                    self.motion.index_generation = None;
                    self.motion.loading = false;
                    self.motion.dirty = false;
                    self.status = format!(
                        "Motion analysis failed · {reason} Imported motions remain available."
                    );
                }
                _ => {}
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn editor(label: &str) -> Editor {
        let root =
            PathBuf::from("artifacts").join(format!("worker-{label}-{}", std::process::id()));
        Editor::new(Library::open(root).unwrap(), egui::Context::default())
    }
    #[test]
    fn failed_current_load_releases_slot_and_pending_preview() {
        let mut e = editor("current");
        let generation = e.clip_generation.advance();
        e.jobs = 1;
        e.motion.pending_preview = Some(motion_browser::PreviewIntent {
            record_id: "isolated".into(),
            hash: "0".repeat(64),
            index: 0,
            generation,
            time: 0.,
        });
        e.pending.insert(("0".repeat(64), false), generation);
        let handle = std::thread::spawn(|| panic!("Deliberate isolated asset-worker regression"));
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        e.watch_worker(
            Kind::Load {
                hash: "0".repeat(64),
                model: false,
                generation,
            },
            handle,
        );
        e.drain_workers();
        assert_eq!(e.jobs, 0);
        assert!(e.pending.is_empty());
        assert!(e.motion.pending_preview.is_none());
        assert!(
            e.motion
                .preview_error
                .as_ref()
                .unwrap()
                .contains("Preview failed")
        );
    }
    #[test]
    fn failed_older_worker_does_not_clear_newer_preview() {
        let mut e = editor("stale");
        let older = e.clip_generation.advance();
        let newer = e.clip_generation.advance();
        e.jobs = 2;
        e.motion.pending_preview = Some(motion_browser::PreviewIntent {
            record_id: "newer".into(),
            hash: "0".repeat(64),
            index: 0,
            generation: newer,
            time: 0.,
        });
        e.pending.insert(("0".repeat(64), false), newer);
        let handle = std::thread::spawn(|| panic!("Deliberate stale asset-worker regression"));
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        e.watch_worker(
            Kind::Load {
                hash: "0".repeat(64),
                model: false,
                generation: older,
            },
            handle,
        );
        e.drain_workers();
        assert_eq!(e.jobs, 1);
        assert_eq!(e.pending[&("0".repeat(64), false)], newer);
        assert_eq!(e.motion.pending_preview.as_ref().unwrap().generation, newer);
        assert!(e.motion.preview_error.is_none());
    }
}
