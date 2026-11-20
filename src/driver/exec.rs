//! Shared subprocess, temp-file, budget, and error helpers for run drivers.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::call::CallError;

/// Cap for piped output kept in memory, total across both streams.
const DRAIN_CAP: usize = 8 * 1024 * 1024;

/// Piped output collected while waiting; memory is capped, not the child.
pub(super) struct ChildOutput {
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) truncated: bool,
}

/// Per-call budgets: every phase gets min(its cap, remaining total).
pub(super) struct CallBudget {
    /// Hard deadline: timeout + settle + 30 s grace.
    pub(super) deadline: std::time::Instant,
    /// Per-invoke tool budget, passed to the tool itself.
    pub(super) timeout_ms: u64,
    /// The total, reported as `budgetMs`.
    pub(super) total_ms: u64,
}

impl CallBudget {
    pub(super) fn new(timeout_ms: u64, settle_ms: u64, started: std::time::Instant) -> Self {
        let total_ms = timeout_ms.saturating_add(settle_ms).saturating_add(30_000);
        Self {
            deadline: started + std::time::Duration::from_millis(total_ms),
            timeout_ms,
            total_ms,
        }
    }

    /// Milliseconds left of the total, saturating.
    pub(super) fn remaining_ms(&self) -> u64 {
        self.deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    /// Budget for one phase: its hardcoded cap or what is left.
    pub(super) fn phase(&self, cap_ms: u64, phase: &str) -> Result<u64, CallError> {
        self.checkpoint(phase)?;
        Ok(cap_ms.min(self.remaining_ms()))
    }

    /// Fail with a phase-naming timeout when the total is exhausted.
    pub(super) fn checkpoint(&self, phase: &str) -> Result<(), CallError> {
        if self.remaining_ms() == 0 {
            return Err(CallError {
                code: "timeout",
                message: format!("timed out before {phase}; total call budget exhausted"),
                data: None,
            });
        }
        Ok(())
    }

    /// Cap without failing: for lenient steps that may simply be skipped.
    pub(super) fn cap(&self, hard_ms: u64) -> u64 {
        hard_ms.min(self.remaining_ms())
    }
}

/// Run `<command> <arg>`, giving up after five seconds.
pub(super) fn run_version_arg(command: &str, arg: &str) -> std::io::Result<std::process::Output> {
    run_probe(command, &[arg])
}

/// Run a short probe command with a 5-second timeout, preserving `ErrorKind::NotFound`
/// and the real `ExitStatus`.
pub(super) fn run_probe(command: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    use std::time::Duration;
    let mut child = std::process::Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let budget = Arc::new(AtomicUsize::new(DRAIN_CAP));
    let out_rx = drain_stream(child.stdout.take(), budget);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    match wait_deadline(child, deadline)? {
        Some(output) => {
            let (stdout, _) = out_rx.recv().unwrap_or_default();
            Ok(std::process::Output {
                status: output.status,
                stdout,
                stderr: Vec::new(),
            })
        }
        None => {
            let grace = std::time::Duration::from_secs(2);
            let _ = out_rx.recv_timeout(grace);
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("timed out running {command}"),
            ))
        }
    }
}

/// First `x.y.z` (plus an optional `-pre`) in `text`, if any.
pub(super) fn parse_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut j = i;
        for _ in 0..2 {
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'.' {
                j += 1;
            } else {
                j = i;
                break;
            }
        }
        if j == i {
            i += 1;
            continue;
        }
        let end = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == end {
            i += 1;
            continue;
        }
        if j < bytes.len() && bytes[j] == b'-' {
            let mut k = j + 1;
            while k < bytes.len()
                && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'.' || bytes[k] == b'-')
            {
                k += 1;
            }
            if k > j + 1 {
                j = k;
            }
        }
        return Some(text[i..j].to_string());
    }
    None
}

/// Drain one pipe into memory, keeping at most the shared budget allows.
/// Always reads to EOF so the child can never block on a full pipe; bytes
/// past the budget are counted but dropped, and flagged as truncated.
pub(super) fn drain_stream<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    budget: Arc<AtomicUsize>,
) -> std::sync::mpsc::Receiver<(Vec<u8>, bool)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 32 * 1024];
            loop {
                let n = match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut pos = 0;
                while pos < n {
                    let remaining = budget.load(Ordering::Relaxed);
                    if remaining == 0 {
                        truncated = true;
                        break;
                    }
                    let take = remaining.min(n - pos);
                    if budget
                        .compare_exchange_weak(
                            remaining,
                            remaining - take,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        )
                        .is_ok()
                    {
                        kept.extend_from_slice(&chunk[pos..pos + take]);
                        pos += take;
                    }
                }
            }
        }
        let _ = tx.send((kept, truncated));
    });
    rx
}

/// Spawn, drain, and wait; timeouts kill without closing anything.
pub(super) fn run_command_raw(
    command: &str,
    args: &[String],
    deadline_ms: u64,
    phase: &str,
) -> Result<ChildOutput, CallError> {
    let mut child = std::process::Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| CallError {
            code: "driver_error",
            message: format!("could not run {command}: {e}"),
            data: None,
        })?;
    let budget = Arc::new(AtomicUsize::new(DRAIN_CAP));
    let out_rx = drain_stream(child.stdout.take(), budget.clone());
    let err_rx = drain_stream(child.stderr.take(), budget);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    match wait_deadline(child, deadline).map_err(|e| CallError {
        code: "driver_error",
        message: format!("could not wait for {command}: {e}"),
        data: None,
    })? {
        Some(_) => {
            let (out, out_truncated) = out_rx.recv().unwrap_or_default();
            let (err, err_truncated) = err_rx.recv().unwrap_or_default();
            Ok(ChildOutput {
                stdout: String::from_utf8_lossy(&out).into_owned(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
                truncated: out_truncated || err_truncated,
            })
        }
        None => {
            // The child is dead; readers only linger if an orphaned
            // grandchild holds the pipe. Wait briefly, then detach them
            // rather than hang the call.
            let grace = std::time::Duration::from_secs(2);
            let _ = out_rx.recv_timeout(grace);
            let _ = err_rx.recv_timeout(grace);
            Err(timed_out(command, phase))
        }
    }
}

/// Wait for a child until the deadline; kill it past the deadline.
/// `Ok(None)` means it timed out (and was killed).
pub(super) fn wait_deadline(
    mut child: std::process::Child,
    deadline: std::time::Instant,
) -> std::io::Result<Option<std::process::Output>> {
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map(Some);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// A timeout error naming the driver and phase that was running.
pub(super) fn timed_out(driver: &str, phase: &str) -> CallError {
    CallError {
        code: "timeout",
        message: format!("{driver} did not answer during {phase}; retry with a larger --timeout"),
        data: None,
    }
}

/// The browser cannot run WebMCP at all: diagnose, do not settle.
pub(super) fn webmcp_unsupported(api: &str) -> CallError {
    CallError {
        code: "webmcp_unsupported",
        message: "this browser does not expose document.modelContext (needs Chrome 149+ with --enable-features=WebMCP); agent-webmcp passes it automatically — upgrade agent-browser/Chrome"
            .to_string(),
        data: Some(serde_json::json!({ "api": api })),
    }
}

/// The page never registered the tool.
pub(super) fn tool_not_found(tool: &str) -> CallError {
    CallError {
        code: "tool_not_found",
        message: format!(
            "the page did not register {tool}; it may need a login or WebMCP may be off"
        ),
        data: None,
    }
}

/// A temp file holding the invoke input or script; removed on drop.
pub(super) struct TempFile {
    pub(super) path: std::path::PathBuf,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Write text to a 0600 temp file with a unique name.
pub(super) fn write_temp(name: &str, contents: &str) -> Result<TempFile, CallError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            ".agent-webmcp-{name}-{}-{n}.json",
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(contents.as_bytes()).map_err(|e| CallError {
                    code: "driver_error",
                    message: format!("could not write params file: {e}"),
                    data: None,
                })?;
                return Ok(TempFile { path });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(CallError {
                    code: "driver_error",
                    message: format!("could not create params file: {e}"),
                    data: None,
                });
            }
        }
    }
    Err(CallError {
        code: "driver_error",
        message: "could not pick a unique params file name".to_string(),
        data: None,
    })
}
