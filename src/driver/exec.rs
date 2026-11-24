//! Shared subprocess, temp-file, budget, and error helpers for run drivers.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
/// and the real `ExitStatus`. The output reader is cancellable and joined,
/// so a descendant that keeps the pipe open cannot wedge the probe.
pub(super) fn run_probe(command: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    use std::time::Duration;
    let mut child = std::process::Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let budget = Arc::new(AtomicUsize::new(DRAIN_CAP));
    let drain_cancelled = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    if let Some(ref out) = child.stdout {
        let _ = set_fd_nonblocking(out);
    }
    let (out_rx, out_handle) = drain_stream(
        child.stdout.take(),
        budget,
        Arc::clone(&drain_cancelled),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    match wait_deadline(child, deadline)? {
        Some(output) => {
            // Drain FIRST, cancel only on timeout: a fast child can write
            // stdout and exit before the reader thread is scheduled. Setting
            // `drain_cancelled` before `recv_timeout` would let the reader
            // exit without reading bytes already in the pipe, so detection
            // would see empty stdout and mark an installed driver missing.
            let remaining = deadline
                .saturating_duration_since(std::time::Instant::now())
                .max(Duration::from_millis(500));
            match out_rx.recv_timeout(remaining) {
                Ok((stdout, _)) => {
                    drain_cancelled.store(true, Ordering::Relaxed);
                    join_bounded(out_handle);
                    Ok(std::process::Output {
                        status: output.status,
                        stdout,
                        stderr: Vec::new(),
                    })
                }
                Err(_) => {
                    drain_cancelled.store(true, Ordering::Relaxed);
                    join_bounded(out_handle);
                    let (stdout, _) = out_rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default();
                    Ok(std::process::Output {
                        status: output.status,
                        stdout,
                        stderr: Vec::new(),
                    })
                }
            }
        }
        None => {
            drain_cancelled.store(true, Ordering::Relaxed);
            join_bounded(out_handle);
            let _ = out_rx.recv_timeout(Duration::from_secs(2));
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
///
/// The reader checks `cancelled` before every `read` and on every
/// `WouldBlock` (pipes are set non-blocking by the caller on Unix), so a
/// descendant that keeps the pipe open after the direct child exits cannot
/// wedge the thread: setting the flag makes it drop the pipe handle and
/// exit. Join the returned handle (bounded) after `recv`/`recv_timeout`
/// resolves so no reader outlives the call.
pub(super) fn drain_stream<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    budget: Arc<AtomicUsize>,
    cancelled: Arc<AtomicBool>,
) -> (
    std::sync::mpsc::Receiver<(Vec<u8>, bool)>,
    std::thread::JoinHandle<()>,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 32 * 1024];
            loop {
                if cancelled.load(Ordering::Relaxed) {
                    break;
                }
                let n = match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if cancelled.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
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
    (rx, handle)
}

/// Join a helper thread with a bounded wait: normally it already finished
/// and this returns at once. If a wedged pipe keeps it alive past a short
/// grace period — a non-direct descendant holding the read end without
/// reading, on any platform — detach it instead of hanging shutdown by
/// dropping the handle (which releases the join resources and detaches the
/// thread, unlike `mem::forget`). The direct child is always reaped by
/// `wait_deadline` before the drain, so termination of owned processes is
/// already handled; detached output is moot because the command already
/// exited or timed out.
fn join_bounded(handle: std::thread::JoinHandle<()>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if handle.is_finished() {
            let _ = handle.join();
            return;
        }
        if std::time::Instant::now() >= deadline {
            drop(handle);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
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
    let drain_cancelled = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        if let Some(ref out) = child.stdout {
            let _ = set_fd_nonblocking(out);
        }
        if let Some(ref err) = child.stderr {
            let _ = set_fd_nonblocking(err);
        }
    }
    let (out_rx, out_handle) = drain_stream(
        child.stdout.take(),
        budget.clone(),
        Arc::clone(&drain_cancelled),
    );
    let (err_rx, err_handle) = drain_stream(child.stderr.take(), budget, Arc::clone(&drain_cancelled));
    // Stops both readers (releasing their pipes) and reaps them with a
    // bounded wait. Callable on every exit path so no reader outlives the
    // call, whether collection succeeded, timed out, or the child did.
    let mut out_handle = Some(out_handle);
    let mut err_handle = Some(err_handle);
    let mut stop_readers = || {
        drain_cancelled.store(true, Ordering::Relaxed);
        if let Some(h) = out_handle.take() {
            join_bounded(h);
        }
        if let Some(h) = err_handle.take() {
            join_bounded(h);
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    match wait_deadline(child, deadline).map_err(|e| CallError {
        code: "driver_error",
        message: format!("could not wait for {command}: {e}"),
        data: None,
    })? {
        Some(_) => {
            // A lingering helper process can hold a pipe open after the
            // child exits; bound both drains by the same deadline so the
            // caller can still attempt session cleanup on timeout. A
            // timeout cancels the readers (releasing their pipes) and
            // joins them before reporting, preserving the warning that
            // the operation may already have completed.
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let out = match out_rx.recv_timeout(remaining) {
                Ok(out) => out,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    stop_readers();
                    return Err(output_timed_out(command, phase));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Default::default(),
            };
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let err = match err_rx.recv_timeout(remaining) {
                Ok(err) => err,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    stop_readers();
                    return Err(output_timed_out(command, phase));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Default::default(),
            };
            let (out, out_truncated) = out;
            let (err, err_truncated) = err;
            stop_readers();
            Ok(ChildOutput {
                stdout: String::from_utf8_lossy(&out).into_owned(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
                truncated: out_truncated || err_truncated,
            })
        }
        None => {
            // The child is dead; readers only linger if an orphaned
            // grandchild holds the pipe. Cancel and reap them (bounded)
            // rather than hanging the call or leaking them.
            stop_readers();
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

/// An output-collection timeout: the child already exited, so a helper may
/// be holding a pipe open after the work completed. Retrying could repeat
/// a write, so tell the caller to check first instead of advising a larger
/// timeout (which only extends the same bounded drain).
pub(super) fn output_timed_out(driver: &str, phase: &str) -> CallError {
    CallError {
        code: "timeout",
        message: format!(
            "{driver} finished but its output could not be collected during {phase}; check whether the operation completed before running it again"
        ),
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

/// Make any pipe fd non-blocking so its reader/feeder thread can poll a
/// cancellation flag instead of blocking forever on a pipe nobody drains.
#[cfg(unix)]
fn set_fd_nonblocking<F: std::os::unix::io::AsRawFd>(f: &F) -> std::io::Result<()> {
    let fd = f.as_raw_fd();
    // SAFETY: F_GETFL/F_SETFL on an owned pipe fd; no pointers cross the ABI.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Feed `data` into `stdin` in bounded chunks, polling `cancelled`
/// between them, then return it so the caller drops it and the child sees
/// EOF. Chunking plus the non-blocking pipe (Unix, set up by the caller)
/// means a descendant that inherits the pipe without reading can never wedge
/// the writer: once the command exits or times out, cancellation is observed
/// within one chunk and the pipe and buffer are released. Normal completion
/// is unchanged: every byte is written, then EOF is delivered on drop.
fn feed_stdin(
    stdin: std::process::ChildStdin,
    data: Vec<u8>,
    cancelled: &AtomicBool,
) -> std::process::ChildStdin {
    use std::io::Write as _;
    let mut stdin = stdin;
    let mut rest = &data[..];
    while !rest.is_empty() {
        if cancelled.load(Ordering::Relaxed) {
            return stdin;
        }
        // One bounded chunk per write so cancellation is checked
        // frequently even where the fd is blocking.
        let chunk = &rest[..rest.len().min(64 * 1024)];
        match stdin.write(chunk) {
            Ok(0) => std::thread::sleep(std::time::Duration::from_millis(1)),
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // Broken pipe (child gone) or any other failure: hand the pipe
            // back; the caller releases it and the main wait already
            // recorded the outcome.
            Err(_) => return stdin,
        }
    }
    // Caller drops the returned handle, delivering EOF.
    stdin
}

/// Spawn, pipe `stdin_data` in, drain, and wait; timeouts kill without
/// closing anything. The writer thread drops stdin after writing so the
/// child always sees EOF; secrets passed this way never appear in argv.
pub(super) fn run_command_raw_with_stdin(
    command: &str,
    args: &[String],
    stdin_data: &[u8],
    deadline_ms: u64,
    phase: &str,
) -> Result<ChildOutput, CallError> {
    let mut child = std::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| CallError {
            code: "driver_error",
            message: format!("could not run {command}: {e}"),
            data: None,
        })?;
    let budget = Arc::new(AtomicUsize::new(DRAIN_CAP));
    let drain_cancelled = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        if let Some(ref out) = child.stdout {
            let _ = set_fd_nonblocking(out);
        }
        if let Some(ref err) = child.stderr {
            let _ = set_fd_nonblocking(err);
        }
    }
    let (out_rx, out_handle) = drain_stream(
        child.stdout.take(),
        budget.clone(),
        Arc::clone(&drain_cancelled),
    );
    let (err_rx, err_handle) = drain_stream(
        child.stderr.take(),
        budget,
        Arc::clone(&drain_cancelled),
    );
    // Stops both readers (releasing their pipes) and reaps them with a
    // bounded wait. Callable on every exit path below so no reader
    // outlives the call — whether collection succeeded, timed out while a
    // descendant held a pipe open, or the child itself timed out.
    let mut out_handle = Some(out_handle);
    let mut err_handle = Some(err_handle);
    let mut stop_readers = || {
        drain_cancelled.store(true, Ordering::Relaxed);
        if let Some(h) = out_handle.take() {
            join_bounded(h);
        }
        if let Some(h) = err_handle.take() {
            join_bounded(h);
        }
    };
    // Feed stdin on its own thread so a slow reader cannot stall the wait
    // below. The writer is cancellable and joined before returning, so no
    // blocked writer can outlive the call holding the pipe and script —
    // even when an owned descendant inherits the pipe without reading it.
    let stdin_opt = child.stdin.take();
    // The feeder relies on a non-blocking pipe to observe cancellation. If
    // that cannot be arranged, fail immediately instead of spawning a
    // writer that a wedged pipe could block forever: kill and reap the
    // child, stop the readers, and return. Dropping our write end delivers
    // EOF to any surviving descendant instead of hanging it.
    #[cfg(unix)]
    if let Some(e) = stdin_opt
        .as_ref()
        .and_then(|stdin| set_fd_nonblocking(stdin).err())
    {
        let _ = child.kill();
        let _ = child.wait();
        stop_readers();
        return Err(CallError {
            code: "driver_error",
            message: format!("could not make stdin non-blocking for {command}: {e}"),
            data: None,
        });
    }
    let writer = stdin_opt.map(|stdin| {
        let owned = stdin_data.to_vec();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_writer = Arc::clone(&cancelled);
        let handle =
            std::thread::spawn(move || feed_stdin(stdin, owned, &cancel_writer));
        (handle, cancelled)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    let waited = wait_deadline(child, deadline).map_err(|e| CallError {
        code: "driver_error",
        message: format!("could not wait for {command}: {e}"),
        data: None,
    });
    if let Some((handle, cancelled)) = writer {
        cancelled.store(true, Ordering::Relaxed);
        join_bounded(handle);
    }
    match waited? {
        Some(_) => {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let out = match out_rx.recv_timeout(remaining) {
                Ok(out) => out,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    stop_readers();
                    return Err(output_timed_out(command, phase));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Default::default(),
            };
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let err = match err_rx.recv_timeout(remaining) {
                Ok(err) => err,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    stop_readers();
                    return Err(output_timed_out(command, phase));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Default::default(),
            };
            let (out, out_truncated) = out;
            let (err, err_truncated) = err;
            stop_readers();
            Ok(ChildOutput {
                stdout: String::from_utf8_lossy(&out).into_owned(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
                truncated: out_truncated || err_truncated,
            })
        }
        None => {
            // The child is dead; readers only linger if an orphaned
            // grandchild holds a pipe. Cancel and reap them (bounded)
            // rather than hanging the call or leaking them.
            stop_readers();
            Err(timed_out(command, phase))
        }
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

#[cfg(all(test, unix))]
mod tests {
    use super::run_command_raw_with_stdin;

    /// Normal EOF delivery: the child reads everything the writer sends.
    #[test]
    fn stdin_writer_delivers_and_closes() {
        let out = match run_command_raw_with_stdin("cat", &[], b"hello-stdin", 10_000, "test") {
            Ok(out) => out,
            Err(e) => panic!("cat echoes stdin: {} ({})", e.message, e.code),
        };
        assert_eq!(out.stdout, "hello-stdin");
    }

    /// An instantly-exiting child with a larger-than-pipe input still
    /// returns promptly: the writer is cancelled and joined instead of
    /// blocking forever on a pipe nobody drains.
    #[test]
    fn stdin_writer_does_not_outlive_an_exited_command() {
        let big = vec![7u8; 2 * 1024 * 1024];
        let start = std::time::Instant::now();
        let out = match run_command_raw_with_stdin("true", &[], &big, 10_000, "test") {
            Ok(out) => out,
            Err(e) => panic!("instant exit must not hang the writer: {} ({})", e.message, e.code),
        };
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        assert!(out.stdout.is_empty());
    }

    /// The timeout path also joins the writer: a child that ignores stdin
    /// still reports `timeout` instead of hanging the feeder.
    #[test]
    fn stdin_writer_cancelled_on_timeout() {
        let big = vec![7u8; 1024 * 1024];
        let err = match run_command_raw_with_stdin("sleep", &["30".to_string()], &big, 2_000, "test")
        {
            Ok(_) => panic!("sleep 30 must time out"),
            Err(e) => e,
        };
        assert_eq!(err.code, "timeout");
    }

    /// A descendant that inherits the pipe without reading must not wedge
    /// the call: the direct child exits at once, the backgrounded
    /// grandchild holds stdin open, and the writer must still let the call
    /// return within the deadline instead of hanging in `join`.
    #[test]
    fn stdin_writer_returns_when_descendant_holds_pipe() {
        let big = vec![7u8; 2 * 1024 * 1024];
        let start = std::time::Instant::now();
        let script = "sleep 30 >/dev/null 2>&1 & exit 0".to_string();
        let out =
            match run_command_raw_with_stdin("sh", &["-c".to_string(), script], &big, 10_000, "test")
            {
                Ok(out) => out,
                Err(e) => panic!(
                    "exited child must not hang the writer: {} ({})",
                    e.message, e.code
                ),
            };
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "returned only after the deadline"
        );
        let _ = out;
    }

    /// A descendant that inherits stdout without closing it must not wedge
    /// output collection: the direct child exits at once, the backgrounded
    /// grandchild holds the pipe's write end open, and the reader must be
    /// cancelled so the call reports `output_timed_out` within the deadline
    /// instead of hanging in `recv_timeout` forever.
    #[test]
    fn stdout_reader_cancelled_when_descendant_holds_pipe() {
        use super::run_command_raw;
        let start = std::time::Instant::now();
        // NOTE: stderr alone is silenced so the grandchild keeps the stdout
        // write end inherited from the direct child.
        let script = "sleep 30 2>/dev/null & exit 0".to_string();
        let err = match run_command_raw(
            "sh",
            &["-c".to_string(), script],
            3_000,
            "test",
        ) {
            Ok(_) => panic!("wedged stdout must time out collection"),
            Err(e) => e,
        };
        assert_eq!(err.code, "timeout", "{err:?}");
        assert!(
            err.message.contains("could not be collected"),
            "keeps the completed-operation warning: {err:?}"
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "returned only after the deadline"
        );
    }

    /// Cancellation exits the reader worker itself, not just the caller:
    /// with the write end held open (no data, no EOF), the reader parks in
    /// `WouldBlock`; setting `cancelled` must let `join_bounded` rejoin
    /// the thread well before the detach deadline instead of leaking it.
    #[test]
    fn drain_reader_exits_on_cancel() {
        use super::{drain_stream, join_bounded, set_fd_nonblocking};
        use std::os::unix::net::UnixStream;
        use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
        use std::time::{Duration, Instant};
        let (reader, _writer) = UnixStream::pair().unwrap();
        set_fd_nonblocking(&reader).unwrap();
        let budget = Arc::new(AtomicUsize::new(1024));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (_rx, handle) =
            drain_stream(Some(reader), Arc::clone(&budget), Arc::clone(&cancelled));
        std::thread::sleep(Duration::from_millis(50));
        cancelled.store(true, Ordering::Relaxed);
        let start = Instant::now();
        join_bounded(handle);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "cancelled reader must exit, not ride out the detach deadline"
        );
    }
}
