//! One process per agent-browser session: a lock file in the data dir.
//!
//! A live holder always wins, whatever the lock age: on Linux a lock is only
//! taken over when its holder pid is dead (`/proc/<pid>` missing). Without
//! `/proc` every holder counts as live, so a 10-minute age rule frees old
//! locks there instead.

use std::path::{Path, PathBuf};

use crate::call::CallError;

/// A lock file whose holder cannot be read is racing, not dead: treat it
/// as live unless its mtime is older than this, so a contender never
/// steals a lock that is still being written.
const UNREADABLE_STALE_MS: u64 = 5_000;

/// A held session lock; dropping it releases the file when still ours.
#[derive(Debug)]
pub struct SessionLock {
    path: PathBuf,
    payload: String,
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
/// fails as `concurrent_call` naming holder pid and age; a dead holder
/// (Linux: no `/proc/<pid>`) loses the lock. Bad names fail as `usage`.
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
        Ok(handle) => {
            // Write through the handle we created: a contender must never
            // see an empty file and mistake our fresh lock for pid 0.
            use std::io::Write;
            let mut handle = handle;
            handle
                .write_all(payload.as_bytes())
                .map_err(|e| CallError {
                    code: "driver_error",
                    message: format!("could not write session lock: {e}"),
                    data: None,
                })?;
            Ok(SessionLock { path, payload })
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let (pid, ts, readable) = read_holder(&path);
            let age = now_ms().saturating_sub(ts);
            // A live holder always wins, whatever the lock age; an
            // unreadable holder (empty or corrupt file) counts as live too
            // unless its mtime shows it was abandoned. Without /proc only
            // the age frees a readable lock.
            #[cfg(target_os = "linux")]
            let contend = if readable {
                holder_live(pid)
            } else {
                !holder_stale(&path)
            };
            #[cfg(not(target_os = "linux"))]
            let contend = if readable {
                holder_live(pid) && age <= STALE_MS
            } else {
                !holder_stale(&path)
            };
            if contend {
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
            Ok(SessionLock { path, payload })
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

/// (pid, timestamp, readable) of the current holder. A file without a
/// parsable pid is unreadable: empty from a racing writer, or corrupt.
/// Its timestamp still falls back to the file mtime, so it is taken over
/// once stale, never stuck forever.
fn read_holder(path: &Path) -> (u32, u64, bool) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let pid = val
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .map(|p| p.min(u64::from(u32::MAX)) as u32);
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
    match pid {
        Some(pid) => (pid, ts, true),
        None => (0, ts, false),
    }
}

/// Whether an unreadable lock file was abandoned: only its mtime can tell
/// a stale file from one that is still being written.
fn holder_stale(path: &Path) -> bool {
    let mtime_ms = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    now_ms().saturating_sub(mtime_ms) > UNREADABLE_STALE_MS
}

/// Liveness of the holder: /proc on Linux; elsewhere (macOS, Windows) every holder counts
/// as alive (best-effort — the stale rule still frees old locks).
fn holder_live(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
    }
}

/// How long a lock may live on platforms without `/proc`, where every
/// holder counts as live and only age can free the lock.
#[cfg(not(target_os = "linux"))]
const STALE_MS: u64 = 10 * 60 * 1000;

/// Millis since the epoch, saturating.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
