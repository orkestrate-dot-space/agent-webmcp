//! One process per agent-browser session: a lock file in the data dir.
//!
//! Windows is best-effort: without /proc every holder counts as alive, so
//! only the 30 s stale rule frees the lock.

use std::path::{Path, PathBuf};

use crate::call::CallError;

/// A lock older than this is stale: take it over, noting `stole_stale`.
const STALE_MS: u64 = 30_000;

/// A held session lock; dropping it releases the file when still ours.
#[derive(Debug)]
pub struct SessionLock {
    path: PathBuf,
    payload: String,
    /// True when this acquire replaced a lock older than 30 s.
    pub stole_stale: bool,
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Only release our own lock: a replaced-by-someone-else file stays.
        let mine = std::fs::read_to_string(&self.path).ok();
        if mine.as_deref() == Some(self.payload.as_str()) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Acquire `<dir>/<name>.lock`, atomic on first create. A live holder
/// within the stale window fails as `concurrent_call` naming holder pid
/// and age; anything else takes the lock over. Bad names fail as `usage`.
pub fn acquire(dir: &Path, name: &str) -> Result<SessionLock, CallError> {
    check_name(name)?;
    std::fs::create_dir_all(dir).map_err(|e| CallError {
        code: "driver_error",
        message: format!("could not create sessions dir: {e}"),
        data: None,
    })?;
    let path = dir.join(format!("{name}.lock"));
    let payload = format!("{{\"pid\":{},\"ts\":{}}}", std::process::id(), now_ms());
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(_) => {
            std::fs::write(&path, &payload).map_err(|e| CallError {
                code: "driver_error",
                message: format!("could not write session lock: {e}"),
                data: None,
            })?;
            Ok(SessionLock {
                path,
                payload,
                stole_stale: false,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let (pid, ts) = read_holder(&path);
            let age = now_ms().saturating_sub(ts);
            if holder_live(pid) && age <= STALE_MS {
                return Err(CallError {
                    code: "concurrent_call",
                    message: format!(
                        "session {name:?} is in use by pid {pid} (lock age {}s); wait or use --fresh",
                        age / 1000
                    ),
                    data: Some(serde_json::json!({
                        "pid": pid,
                        "ageMs": age,
                        "session": name,
                    })),
                });
            }
            std::fs::write(&path, &payload).map_err(|e| CallError {
                code: "driver_error",
                message: format!("could not take over session lock: {e}"),
                data: None,
            })?;
            Ok(SessionLock {
                path,
                payload,
                stole_stale: age > STALE_MS,
            })
        }
        Err(e) => Err(CallError {
            code: "driver_error",
            message: format!("could not create session lock: {e}"),
            data: None,
        }),
    }
}

/// Session names become file names; keep them to letters, digits, `-`
/// and `_` so a hostile project config cannot escape the sessions dir.
fn check_name(name: &str) -> Result<(), CallError> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(CallError::usage(format!(
            "invalid session name {name:?}; use letters, digits, '-' and '_'"
        )))
    }
}

/// (pid, timestamp) of the current holder; corrupt files read as pid 0
/// with the file mtime, so they are taken over, never stuck forever.
fn read_holder(path: &Path) -> (u32, u64) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let pid = val
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    let ts = val
        .get("ts")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        })
        .unwrap_or(0);
    (pid, ts)
}

/// Liveness of the holder: /proc on unix; elsewhere every holder counts
/// as alive (best-effort — the stale rule still frees old locks).
fn holder_live(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Millis since the epoch, saturating.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
