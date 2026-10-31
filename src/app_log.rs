//! Local, bounded process diagnostics. Never serializes imported file contents.
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{Mutex, Once, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
static LOG: OnceLock<Option<(PathBuf, Mutex<File>)>> = OnceLock::new();
static HOOK: Once = Once::new();
fn log() -> &'static Option<(PathBuf, Mutex<File>)> {
    LOG.get_or_init(|| {
        let root = std::env::var_os("ARMATRA_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if let Some(p) = std::env::var_os("LOCALAPPDATA") {
                    PathBuf::from(p).join("Armatra/logs")
                } else if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
                    PathBuf::from(p).join("armatra/logs")
                } else {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(std::env::temp_dir)
                        .join(".local/state/armatra/logs")
                }
            });
        let result = (|| -> std::io::Result<_> {
            std::fs::create_dir_all(&root)?;
            let mut old: Vec<_> = std::fs::read_dir(&root)?
                .filter_map(Result::ok)
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with("armatra-")
                        && e.path().extension().is_some_and(|e| e == "jsonl")
                })
                .collect();
            old.sort_by_key(|e| e.file_name());
            let excess = old.len().saturating_sub(7);
            for e in old.into_iter().take(excess) {
                let _ = std::fs::remove_file(e.path());
            }
            let path = root.join(format!("armatra-{}-{}.jsonl", stamp(), std::process::id()));
            let file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
            Ok((path, Mutex::new(file)))
        })();
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("Application log unavailable: {e}");
                None
            }
        }
    })
}
fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn rotate(path: &std::path::Path, file: &mut File) -> std::io::Result<()> {
    file.flush()?;
    // Keep the preceding boundaries without renaming an open Windows handle.
    std::fs::copy(path, path.with_extension("previous.jsonl"))?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}
pub fn path() -> Option<&'static std::path::Path> {
    log().as_ref().map(|p| p.0.as_path())
}
pub fn event(name: &str, data: serde_json::Value) {
    if let Some((path, file)) = log()
        && let Ok(mut file) = file.lock()
    {
        // Preserve the most recent boundaries instead of stopping diagnostics
        // after a long session. A panic can arrive after thousands of previews.
        if file.metadata().is_ok_and(|m| m.len() > 8 * 1024 * 1024)
            && let Err(error) = rotate(path, &mut file)
        {
            eprintln!("Application log rotation failed: {error}");
            // Still attempt critical diagnostics if the filesystem cannot rotate.
            if !matches!(
                name,
                "panic" | "fatal_frame_error" | "gpu_uncaptured_error" | "gpu_device_lost"
            ) {
                return;
            }
        }
        let line = serde_json::json!({"ms":stamp(),"pid":std::process::id(),"thread":format!("{:?}",std::thread::current().id()),"event":name,"data":data});
        if serde_json::to_writer(&mut *file, &line).is_ok() {
            let _ = file.write_all(b"\n");
            let _ = file.flush();
            let _ = file.sync_data();
        }
    }
}
pub fn init() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            event("panic", serde_json::json!({"message":info.to_string(),"backtrace":std::backtrace::Backtrace::force_capture().to_string()}));
            previous(info);
        }));
        event("process_start", serde_json::json!({"source":option_env!("ARMATRA_BUILD_COMMIT"),"version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"panic":if cfg!(panic="abort") {"abort"}else{"unwind"}}));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotating_open_log_preserves_last_boundaries_and_accepts_next_event() {
        let root = PathBuf::from("artifacts").join(format!("log-rotation-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("armatra-test.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(b"last successful preview boundary\n")
            .unwrap();
        rotate(&path, &mut file).unwrap();
        file.write_all(b"critical next event\n").unwrap();
        file.flush().unwrap();
        assert_eq!(
            std::fs::read_to_string(path.with_extension("previous.jsonl")).unwrap(),
            "last successful preview boundary\n"
        );
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "critical next event\n"
        );
    }
}
