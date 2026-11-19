//! agent-browser: the only driver agent-webmcp runs itself, over a session.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::{CallRequest, Detection, Driver, Kind, Plan, RunOptions, RunOutcome, Step};
use crate::call::CallError;
use crate::config::Config;

/// Earliest agent-browser with WebMCP on by default in managed Chrome.
const MIN_VERSION: &str = "0.43.0";
/// One-line install for a missing or outdated agent-browser.
const INSTALL: &str = "npm i -g agent-browser && agent-browser install";

/// Run driver: opens the page and invokes the tool in a named session.
pub struct AgentBrowser {
    command: String,
    session: String,
}

impl AgentBrowser {
    /// Build from config; the registry calls this so plans stay in sync.
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            command: cfg.drivers.agent_browser.command.clone(),
            session: cfg.drivers.agent_browser.session.clone(),
        }
    }
}

impl Driver for AgentBrowser {
    fn id(&self) -> &'static str {
        "agent-browser"
    }

    fn kind(&self) -> Kind {
        Kind::Run
    }

    fn install(&self) -> Option<&'static str> {
        Some(INSTALL)
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        detect_with(&self.command, &run_version)
    }

    fn run(&self, req: &CallRequest, opts: &RunOptions) -> Result<RunOutcome, CallError> {
        self.run_inner(req, opts)
    }

    fn plan(&self, req: &CallRequest) -> Plan {
        let input = serde_json::to_string(&req.input).unwrap_or_default();
        Plan {
            driver: self.id(),
            note: self.note().map(str::to_string),
            steps: vec![
                Step {
                    tool: "shell".to_string(),
                    args: serde_json::json!({"command": [&self.command, "--session", &self.session, "--json", "--args", "--enable-features=WebMCP", "open", &req.page]}),
                    why: "open the page in the session".to_string(),
                },
                Step {
                    tool: "shell".to_string(),
                    args: serde_json::json!({"command": [&self.command, "--session", &self.session, "--json", "webmcp", "invoke", &req.tool, "--params", &input]}),
                    why: "invoke the tool with the JSON input".to_string(),
                },
            ],
        }
    }
}

/// Detect agent-browser by running `<command> --version`; the runner is
/// injectable so tests can fake it.
pub fn detect_with(
    command: &str,
    run: &dyn Fn(&str) -> std::io::Result<std::process::Output>,
) -> Detection {
    match run(command) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Detection::Missing {
            install: INSTALL.to_string(),
            detail: None,
        },
        Err(e) => Detection::Missing {
            install: INSTALL.to_string(),
            detail: Some(format!("could not run {command} --version: {e}")),
        },
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            match parse_version(&text) {
                Some(version) if crate::setup::is_exact_version(&version) => {
                    if crate::setup::newer_available(&version, MIN_VERSION) {
                        Detection::TooOld {
                            version,
                            min: MIN_VERSION.to_string(),
                            install: INSTALL.to_string(),
                        }
                    } else {
                        Detection::Ready { version }
                    }
                }
                _ => Detection::Missing {
                    install: INSTALL.to_string(),
                    detail: Some(format!(
                        "could not parse a version from {command} --version"
                    )),
                },
            }
        }
    }
}

/// Run `<command> --version`, giving up after five seconds.
fn run_version(command: &str) -> std::io::Result<std::process::Output> {
    run_version_arg(command, "--version")
}

/// Run `<command> <arg>`, giving up after five seconds. Lightpanda detects
/// with `version` (bare `--version` is a fatal error there), so the arg
/// varies by driver but the bounded spawn stays in one place.
pub(super) fn run_version_arg(command: &str, arg: &str) -> std::io::Result<std::process::Output> {
    use std::time::Duration;
    let mut child = std::process::Command::new(command)
        .arg(arg)
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
                "timed out running --version",
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

/// Per-call budgets: every phase gets min(its cap, remaining total).
struct CallBudget {
    /// Hard deadline: timeout + settle + 30 s grace.
    deadline: std::time::Instant,
    /// Per-invoke tool budget, passed to the tool itself.
    timeout_ms: u64,
    /// The total, reported as `budgetMs`.
    total_ms: u64,
}

impl CallBudget {
    fn new(timeout_ms: u64, settle_ms: u64, started: std::time::Instant) -> Self {
        let total_ms = timeout_ms.saturating_add(settle_ms).saturating_add(30_000);
        Self {
            deadline: started + std::time::Duration::from_millis(total_ms),
            timeout_ms,
            total_ms,
        }
    }

    /// Milliseconds left of the total, saturating.
    fn remaining_ms(&self) -> u64 {
        self.deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    /// Budget for one phase: its hardcoded cap or what is left.
    fn phase(&self, cap_ms: u64, phase: &str) -> Result<u64, CallError> {
        self.checkpoint(phase)?;
        Ok(cap_ms.min(self.remaining_ms()))
    }

    /// Fail with a phase-naming timeout when the total is exhausted.
    fn checkpoint(&self, phase: &str) -> Result<(), CallError> {
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
    fn cap(&self, hard_ms: u64) -> u64 {
        hard_ms.min(self.remaining_ms())
    }
}

/// Piped output collected while waiting; memory is capped, not the child.
pub(super) struct ChildOutput {
    pub(super) stdout: String,
    pub(super) stderr: String,
    truncated: bool,
}

/// Cap for piped output kept in memory, total across both streams.
const DRAIN_CAP: usize = 8 * 1024 * 1024;

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

/// A timeout error naming the phase that was running.
fn timed_out(phase: &str) -> CallError {
    CallError {
        code: "timeout",
        message: format!(
            "agent-browser did not answer during {phase}; retry with a larger --timeout"
        ),
        data: None,
    }
}

/// The page navigated mid-call, ending the invocation.
fn context_moved() -> CallError {
    CallError {
        code: "driver_error",
        message: "agent-browser dropped the invocation: the page navigated mid-call (webmcp_context_changed); re-run the call"
            .to_string(),
        data: Some(serde_json::json!({ "code": "webmcp_context_changed" })),
    }
}

impl AgentBrowser {
    fn run_inner(&self, req: &CallRequest, opts: &RunOptions) -> Result<RunOutcome, CallError> {
        let session = if opts.fresh {
            format!(
                "agent-webmcp-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            )
        } else {
            self.session.clone()
        };
        // One process per session for the whole run; fresh sessions are
        // unique per run, so they skip the lock. The guard releases on drop,
        // covering every return below; contention returns early here.
        let _session_lock = if opts.fresh {
            None
        } else {
            Some(super::lock::acquire(
                &crate::store::data_dir().join("sessions"),
                &session,
            )?)
        };
        let mut base = vec![
            "--session".to_string(),
            session.clone(),
            "--json".to_string(),
        ];
        if opts.headed {
            base.push("--headed".to_string());
        }
        let mut open_argv = base.clone();
        if let Some(profile) = &opts.profile {
            open_argv.push("--profile".to_string());
            open_argv.push(profile.clone());
        }
        if let Some(cdp) = &opts.cdp {
            open_argv.push("--cdp".to_string());
            open_argv.push(cdp.clone());
        }
        for arg in &opts.extra_args {
            open_argv.push("--args".to_string());
            open_argv.push(arg.clone());
        }
        // Always launch with the WebMCP feature: without it, registration is
        // a per-page lottery. agent-browser merges repeated --args into the
        // one --enable-features switch, so no dedupe here.
        open_argv.push("--args".to_string());
        open_argv.push("--enable-features=WebMCP".to_string());
        open_argv.push("open".to_string());
        open_argv.push(req.page.clone());

        let started = std::time::Instant::now();
        let budgets = CallBudget::new(opts.timeout_ms, opts.settle_ms.min(30_000), started);
        let outcome = self.run_phases(req, opts, &base, &open_argv, &session, &budgets, started);
        // Timeouts already attempted a close inside run_command; a fresh
        // session only needs its close when the call did not time out.
        let timed_out = matches!(&outcome, Err(e) if e.code == "timeout");
        if !timed_out
            && opts.fresh
            && let Ok(ms) = budgets.phase(15_000, "close")
        {
            self.close_session(&session, ms);
        }
        outcome
    }

    /// Open, settle, and invoke; the caller closes the session afterwards.
    #[allow(clippy::too_many_arguments)]
    fn run_phases(
        &self,
        req: &CallRequest,
        opts: &RunOptions,
        base: &[String],
        open_argv: &[String],
        session: &str,
        budgets: &CallBudget,
        started: std::time::Instant,
    ) -> Result<RunOutcome, CallError> {
        let open_cap = opts.timeout_ms.saturating_add(15_000);
        let opened = run_step(
            &self.command,
            open_argv,
            budgets.phase(open_cap, "open")?,
            session,
            "open",
        )?;
        if !is_success(&opened) {
            return Err(open_failure(&opened));
        }
        let page_url = opened
            .pointer("/data/url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&req.page);
        let title = opened
            .pointer("/data/title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let reused = opened
            .pointer("/data/lifecycle/reused")
            .or_else(|| opened.pointer("/lifecycle/reused"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let settle_ms = opts.settle_ms.min(30_000);
        let announced = tool_names_at(&opened, "/data/webmcp/tools");
        let result = if announced.iter().any(|n| n == &req.tool) {
            map_invoke(self.invoke_once(req, base, session, budgets)?, &req.tool)
        } else if is_blocked_page(title, page_url, "") {
            Err(blocked_page(page_url, title))
        } else if settle_ms == 0 {
            map_invoke(self.invoke_once(req, base, session, budgets)?, &req.tool)
        } else {
            self.settle_invoke(
                req,
                base,
                session,
                budgets,
                settle_ms,
                announced.is_empty(),
                title,
                page_url,
            )
        };
        result.map(|output| RunOutcome {
            output,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reused,
            budget_ms: budgets.total_ms,
        })
    }

    /// Best-effort session close; never fails the call.
    fn close_session(&self, session: &str, deadline_ms: u64) {
        let _ = run_command_raw(
            &self.command,
            &[
                "--session".to_string(),
                session.to_string(),
                "close".to_string(),
            ],
            deadline_ms,
            "close",
        );
    }

    /// One blocking invoke: the daemon waits for the tool itself, so the
    /// record comes back terminal. Immediate envelopes (old daemons, CLI
    /// errors) skip the wait, and `map_invoke` classifies everything.
    fn invoke_once(
        &self,
        req: &CallRequest,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<serde_json::Value, CallError> {
        let input_json = serde_json::to_string(&req.input).unwrap_or_else(|_| "{}".to_string());
        let params = write_params_file(&input_json)?;
        let mut invoke_argv = base.to_vec();
        invoke_argv.extend([
            "webmcp".to_string(),
            "invoke".to_string(),
            req.tool.clone(),
            "--params".to_string(),
            format!("@{}", params.path.display()),
            "--timeout".to_string(),
            budgets.timeout_ms.to_string(),
        ]);
        let cap = budgets.timeout_ms.saturating_add(15_000);
        parse_envelope(&run_command(
            &self.command,
            &invoke_argv,
            budgets.phase(cap, "invoke")?,
            session,
            "invoke",
        )?)
    }

    /// Settle for a late tool: first look, probe, poll, reload once, settle
    /// again, look once. The two settle phases share one budget, not one each.
    #[allow(clippy::too_many_arguments)]
    fn settle_invoke(
        &self,
        req: &CallRequest,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
        settle_ms: u64,
        announced_empty: bool,
        title: &str,
        page_url: &str,
    ) -> Result<serde_json::Value, CallError> {
        let stop_at = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(settle_ms))
            .unwrap_or(budgets.deadline);
        let stop_at = stop_at.min(budgets.deadline);
        // The first look: found → invoke; empty plus an empty announcement →
        // probe the JS API before concluding anything.
        if self.poll_once(base, &req.tool, budgets.cap(15_000), session) {
            return map_invoke(self.invoke_once(req, base, session, budgets)?, &req.tool);
        }
        if announced_empty
            && let Some(api) = self.probe_api(base, session, budgets)
            && api != "object"
        {
            return Err(webmcp_unsupported(&api));
        }
        if self.poll_until_present(base, &req.tool, stop_at, session) {
            return map_invoke(self.invoke_once(req, base, session, budgets)?, &req.tool);
        }
        self.reload_and_wait(base, session, budgets);
        if self.poll_until_present(base, &req.tool, stop_at, session) {
            return map_invoke(self.invoke_once(req, base, session, budgets)?, &req.tool);
        }
        budgets.checkpoint("list")?;
        if self.poll_once(base, &req.tool, budgets.cap(15_000), session) {
            return map_invoke(self.invoke_once(req, base, session, budgets)?, &req.tool);
        }
        if is_blocked_page(title, page_url, "") {
            return Err(blocked_page(page_url, title));
        }
        Err(tool_not_found(&req.tool))
    }

    /// Poll `webmcp list <tool>` until it appears or the deadline passes.
    /// Failed polls count as absent; the budget bounds the waiting.
    fn poll_until_present(
        &self,
        base: &[String],
        tool: &str,
        stop_at: std::time::Instant,
        session: &str,
    ) -> bool {
        loop {
            let now = std::time::Instant::now();
            if now >= stop_at {
                return false;
            }
            let remaining = stop_at.saturating_duration_since(now);
            let per_poll_ms = remaining
                .min(std::time::Duration::from_millis(15_000))
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            if self.poll_once(base, tool, per_poll_ms, session) {
                return true;
            }
            let remaining = stop_at.saturating_duration_since(std::time::Instant::now());
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(500)));
        }
    }

    /// One settle poll: lenient, so a busy page costs budget, not errors.
    fn poll_once(&self, base: &[String], tool: &str, per_poll_ms: u64, session: &str) -> bool {
        let mut argv = base.to_vec();
        argv.extend(["webmcp".to_string(), "list".to_string(), tool.to_string()]);
        let out = match run_command(&self.command, &argv, per_poll_ms, session, "list") {
            Ok(out) => out,
            Err(_) => return false,
        };
        let val = match parse_envelope(&out) {
            Ok(val) => val,
            Err(_) => return false,
        };
        is_success(&val) && tool_listed(&val, tool)
    }

    /// The JS API shape, or None when the probe itself failed.
    fn probe_api(&self, base: &[String], session: &str, budgets: &CallBudget) -> Option<String> {
        let mut argv = base.to_vec();
        argv.extend([
            "eval".to_string(),
            "typeof document.modelContext".to_string(),
        ]);
        let out = run_command(&self.command, &argv, budgets.cap(15_000), session, "eval").ok()?;
        let val = parse_envelope(&out).ok()?;
        if !is_success(&val) {
            return None;
        }
        val.pointer("/data/result")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }

    /// Reload once and wait for load, like the weekly scanner. A failed
    /// reload still gets its second look; transport errors propagate.
    fn reload_and_wait(&self, base: &[String], session: &str, budgets: &CallBudget) {
        let mut reload_argv = base.to_vec();
        reload_argv.push("reload".to_string());
        if run_step(
            &self.command,
            &reload_argv,
            budgets.phase(15_000, "reload").unwrap_or(0),
            session,
            "reload",
        )
        .is_err()
        {
            return;
        }
        let mut wait_argv = base.to_vec();
        wait_argv.extend([
            "wait".to_string(),
            "--load".to_string(),
            "load".to_string(),
            "--timeout".to_string(),
            "15000".to_string(),
        ]);
        if budgets.remaining_ms() > 0 {
            let _ = run_command(
                &self.command,
                &wait_argv,
                budgets.cap(20_000),
                session,
                "wait",
            );
        }
    }
}

/// Run one agent-browser step and parse its envelope.
fn run_step(
    command: &str,
    argv: &[String],
    deadline_ms: u64,
    session: &str,
    phase: &str,
) -> Result<serde_json::Value, CallError> {
    parse_envelope(&run_command(command, argv, deadline_ms, session, phase)?)
}

/// Tool names at a JSON pointer to a `[{name, ...}]` array, if any.
fn tool_names_at(value: &serde_json::Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| t.get("name"))
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a `webmcp list` response registers the tool.
fn tool_listed(list: &serde_json::Value, tool: &str) -> bool {
    tool_names_at(list, "/data/tools")
        .iter()
        .any(|name| name == tool)
}

/// Whether title+url+body text look like an unsolved bot check, mirroring
/// the scanner's `isBlockedPage`: bot-check text and challenge pages.
/// The caller only consults this when no tools registered.
pub fn is_blocked_page(title: &str, url: &str, body: &str) -> bool {
    let haystack = format!("{url} {title} {body}").to_lowercase();
    if haystack
        .split_once("blocked by a ")
        .is_some_and(|(_, rest)| rest.contains("bot challenge"))
    {
        return true;
    }
    [
        "just a moment",
        "attention required",
        "access denied",
        "verify you are human",
        "are you a robot",
        "cf-challenge",
        "bot challenge",
        "captcha",
    ]
    .iter()
    .any(|signal| haystack.contains(signal))
}

/// A blocked page stops the call instead of reporting no tools.
fn blocked_page(url: &str, title: &str) -> CallError {
    CallError {
        code: "page_unreachable",
        message: "the page shows a bot check; it was not solved".to_string(),
        data: Some(serde_json::json!({ "url": url, "title": title })),
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

/// A temp file holding the invoke input; removed on drop, come what may.
pub(super) struct TempFile {
    pub(super) path: std::path::PathBuf,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Write the input JSON to a 0600 temp file with a unique name.
fn write_params_file(input_json: &str) -> Result<TempFile, CallError> {
    write_temp("params", input_json)
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
                    message: format!("could not write params file: {e}"),
                    data: None,
                });
            }
        }
    }
    Err(CallError {
        code: "driver_error",
        message: "could not write params file: no unique name".to_string(),
        data: None,
    })
}

/// Run a child with args, captured output, and a hard deadline.
/// Run a child with args, drained output, and a hard deadline. `session`
/// names the agent-browser session to close if the child has to be killed:
/// the daemon cannot reap Chrome/grandchildren without a process group.
fn run_command(
    command: &str,
    args: &[String],
    deadline_ms: u64,
    session: &str,
    phase: &str,
) -> Result<ChildOutput, CallError> {
    match run_command_raw(command, args, deadline_ms, phase) {
        Ok(out) => Ok(out),
        Err(e) if e.code == "timeout" => {
            let _ = run_command_raw(
                command,
                &[
                    "--session".to_string(),
                    session.to_string(),
                    "close".to_string(),
                ],
                15_000,
                "close",
            );
            Err(e)
        }
        Err(e) => Err(e),
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
            Err(timed_out(phase))
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

/// Parse agent-browser stdout; anything else is a driver error naming 0.43.
fn parse_envelope(out: &ChildOutput) -> Result<serde_json::Value, CallError> {
    serde_json::from_str(&out.stdout).map_err(|_| {
        let mut data = serde_json::json!({
            "stdout": out.stdout.chars().take(200).collect::<String>(),
            "stderr": out.stderr.chars().take(500).collect::<String>(),
        });
        if out.truncated {
            data["truncated"] = serde_json::Value::Bool(true);
        }
        CallError {
            code: "driver_error",
            message: "agent-browser printed something that is not JSON; is it 0.43+?".to_string(),
            data: Some(data),
        }
    })
}

fn is_success(envelope: &serde_json::Value) -> bool {
    envelope
        .get("success")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn envelope_code(envelope: &serde_json::Value) -> &str {
    envelope
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

fn envelope_error(envelope: &serde_json::Value) -> &str {
    envelope
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// Map a failed `open`: navigation/timeout/DNS means unreachable, else driver.
fn open_failure(open: &serde_json::Value) -> CallError {
    let code = envelope_code(open);
    let lowered = code.to_lowercase();
    let error = envelope_error(open).to_lowercase();
    let net = [
        "err_name_not_resolved",
        "err_connection_refused",
        "err_address_unreachable",
        "err_internet_disconnected",
    ]
    .iter()
    .any(|n| lowered.contains(n) || error.contains(n));
    if lowered.contains("navigation") || lowered.contains("timeout") || net {
        CallError {
            code: "page_unreachable",
            message: if envelope_error(open).is_empty() {
                "could not open the page".to_string()
            } else {
                envelope_error(open).to_string()
            },
            data: Some(serde_json::json!({ "code": code })),
        }
    } else {
        CallError {
            code: "driver_error",
            message: if envelope_error(open).is_empty() {
                format!("agent-browser could not open the page ({code})")
            } else {
                envelope_error(open).to_string()
            },
            data: Some(serde_json::json!({ "code": code, "error": envelope_error(open) })),
        }
    }
}

/// Map a final invoke envelope to output or a typed error. A blocking
/// invoke always reports `success: true` with the record under `data`,
/// even when the tool itself failed, so the switch below is on
/// `data.status`. Absent status means an old daemon: read the output.
fn map_invoke(invoke: serde_json::Value, tool: &str) -> Result<serde_json::Value, CallError> {
    if !is_success(&invoke) {
        if envelope_code(&invoke) == "webmcp_tool_not_found" {
            return Err(tool_not_found(tool));
        }
        if envelope_code(&invoke) == "webmcp_ambiguous_tool" {
            let scope = match invoke
                .pointer("/data/frames")
                .and_then(serde_json::Value::as_array)
            {
                Some(frames) => format!("in {} frames", frames.len()),
                None => "in more than one frame".to_string(),
            };
            return Err(CallError {
                code: "ambiguous_tool",
                message: format!(
                    "the page registers {tool} {scope}; agent-webmcp cannot pick one yet — run it with your browser driver"
                ),
                data: Some(
                    serde_json::json!({ "code": envelope_code(&invoke), "error": envelope_error(&invoke) }),
                ),
            });
        }
        return Err(CallError {
            code: "driver_error",
            message: if envelope_error(&invoke).is_empty() {
                format!("agent-browser invoke failed ({})", envelope_code(&invoke))
            } else {
                envelope_error(&invoke).to_string()
            },
            data: Some(
                serde_json::json!({ "code": envelope_code(&invoke), "error": envelope_error(&invoke) }),
            ),
        });
    }
    let data = invoke
        .get("data")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    match data.get("status").and_then(serde_json::Value::as_str) {
        Some("failed") => {
            let error = data
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if error.contains("webmcp_context_changed") {
                return Err(context_moved());
            }
            return Err(CallError {
                code: "tool_error",
                message: if error.is_empty() {
                    "the tool reported an error".to_string()
                } else {
                    error.to_string()
                },
                data: Some(data),
            });
        }
        Some("timed_out") => return Err(timed_out("invoke")),
        Some("canceled") | Some("cancelled") => {
            return Err(CallError {
                code: "driver_error",
                message: "the invocation was canceled".to_string(),
                data: Some(data),
            });
        }
        // Completed, absent (old daemons), or unknown: read the output.
        _ => {}
    }
    if data
        .get("output")
        .and_then(|o| o.get("isError"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        let output = data.get("output").cloned().unwrap_or(data);
        return Err(CallError {
            code: "tool_error",
            message: tool_error_text(&output),
            data: Some(output),
        });
    }
    Ok(data.get("output").cloned().unwrap_or(data))
}

/// The tool's own error text: first text content, else a fallback.
fn tool_error_text(output: &serde_json::Value) -> String {
    output
        .get("content")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("text").and_then(serde_json::Value::as_str))
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| "the tool reported an error".to_string())
}
