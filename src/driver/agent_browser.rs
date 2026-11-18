//! agent-browser: the only driver agent-webmcp runs itself, over a session.

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
                    args: serde_json::json!({"command": [&self.command, "--session", &self.session, "--json", "open", &req.page]}),
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
    use std::time::Duration;
    let child = std::process::Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    match wait_deadline(child, deadline)? {
        Some(output) => Ok(output),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timed out running --version",
        )),
    }
}

/// First `x.y.z` (plus an optional `-pre`) in `text`, if any.
fn parse_version(text: &str) -> Option<String> {
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
        let deadline_ms = opts.timeout_ms.saturating_add(15_000);
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
        open_argv.push("open".to_string());
        open_argv.push(req.page.clone());

        let _recorder = write_init_script()?;
        let mut open_argv = base.clone();
        if let Some(profile) = &opts.profile {
            open_argv.push("--profile".to_string());
            open_argv.push(profile.clone());
        }
        if let Some(cdp) = &opts.cdp {
            open_argv.push("--cdp".to_string());
            open_argv.push(cdp.clone());
        }
        open_argv.push("--init-script".to_string());
        open_argv.push(format!("@{}", _recorder.path.display()));
        open_argv.push("open".to_string());
        open_argv.push(req.page.clone());

        let started = std::time::Instant::now();
        let open_val = run_step(&self.command, &open_argv, deadline_ms)?;
        if !is_success(&open_val) {
            return Err(open_failure(&open_val));
        }
        let page_url = open_val
            .pointer("/data/url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&req.page);
        let title = open_val
            .pointer("/data/title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let reused = open_val
            .pointer("/data/lifecycle/reused")
            .or_else(|| open_val.pointer("/lifecycle/reused"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let budget_ms = opts.settle_ms.min(30_000);
        let announced = tool_names_at(&open_val, "/data/webmcp/tools");
        let result = if announced.iter().any(|n| n == &req.tool) {
            map_invoke(self.invoke_once(req, &base, deadline_ms, opts.timeout_ms)?)
        } else if is_blocked_page(title, page_url, "") {
            Err(blocked_page(page_url, title))
        } else if budget_ms == 0 {
            self.invoke_legacy(req, opts, &base, &open_argv, deadline_ms)
        } else {
            self.settle_invoke(req, opts, &base, deadline_ms, budget_ms, title, page_url)
        };
        if opts.fresh {
            let _ = run_command(
                &self.command,
                &[
                    "--session".to_string(),
                    session.clone(),
                    "close".to_string(),
                ],
                15_000,
            );
        }
        result.map(|output| RunOutcome {
            output,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reused,
        })
    }

    /// One invoke attempt: params file, invoke, parse. No retry here.
    fn invoke_once(
        &self,
        req: &CallRequest,
        base: &[String],
        deadline_ms: u64,
        timeout_ms: u64,
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
            timeout_ms.to_string(),
        ]);
        parse_envelope(&run_command(&self.command, &invoke_argv, deadline_ms)?)
    }

    /// Today's shape for settle_ms 0: invoke, re-open once on
    /// `webmcp_tool_not_found`, retry once.
    fn invoke_legacy(
        &self,
        req: &CallRequest,
        opts: &RunOptions,
        base: &[String],
        open_argv: &[String],
        deadline_ms: u64,
    ) -> Result<serde_json::Value, CallError> {
        let first = self.invoke_once(req, base, deadline_ms, opts.timeout_ms)?;
        if is_success(&first) || invoke_code(&first) != "webmcp_tool_not_found" {
            return map_invoke(first);
        }
        let reopened = run_step(&self.command, open_argv, deadline_ms)?;
        if !is_success(&reopened) {
            return Err(open_failure(&reopened));
        }
        let second = self.invoke_once(req, base, deadline_ms, opts.timeout_ms)?;
        if !is_success(&second) && invoke_code(&second) == "webmcp_tool_not_found" {
            return Err(tool_not_found(&req.tool));
        }
        map_invoke(second)
    }

    /// Settle for a late tool: poll, reload once, settle again, look once.
    /// The two settle phases share one budget, not one each.
    #[allow(clippy::too_many_arguments)]
    fn settle_invoke(
        &self,
        req: &CallRequest,
        opts: &RunOptions,
        base: &[String],
        deadline_ms: u64,
        budget_ms: u64,
        title: &str,
        page_url: &str,
    ) -> Result<serde_json::Value, CallError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(budget_ms);
        if self.poll_until_present(base, &req.tool, deadline) {
            return map_invoke(self.invoke_once(req, base, deadline_ms, opts.timeout_ms)?);
        }
        self.reload_and_wait(base, deadline_ms);
        if self.poll_until_present(base, &req.tool, deadline) {
            return map_invoke(self.invoke_once(req, base, deadline_ms, opts.timeout_ms)?);
        }
        if self.poll_once(base, &req.tool) {
            return map_invoke(self.invoke_once(req, base, deadline_ms, opts.timeout_ms)?);
        }
        if is_blocked_page(title, page_url, "") {
            return Err(blocked_page(page_url, title));
        }
        if read_recorder(&self.command, base).contains(&req.tool) {
            return Err(tool_removed(&req.tool));
        }
        Err(tool_not_found(&req.tool))
    }

    /// Poll `webmcp list <tool>` until it appears or the deadline passes.
    /// Failed polls count as absent; the budget bounds the waiting.
    fn poll_until_present(
        &self,
        base: &[String],
        tool: &str,
        deadline: std::time::Instant,
    ) -> bool {
        loop {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            if self.poll_once(base, tool) {
                return true;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(500)));
        }
    }

    /// One settle poll: lenient, so a busy page costs budget, not errors.
    fn poll_once(&self, base: &[String], tool: &str) -> bool {
        let mut argv = base.to_vec();
        argv.extend(["webmcp".to_string(), "list".to_string(), tool.to_string()]);
        let out = match run_command(&self.command, &argv, 15_000) {
            Ok(out) => out,
            Err(_) => return false,
        };
        let val = match parse_envelope(&out) {
            Ok(val) => val,
            Err(_) => return false,
        };
        is_success(&val) && tool_listed(&val, tool)
    }

    /// Reload once and wait for load, like the weekly scanner. A failed
    /// reload still gets its second look; transport errors propagate.
    fn reload_and_wait(&self, base: &[String], deadline_ms: u64) {
        let mut reload_argv = base.to_vec();
        reload_argv.push("reload".to_string());
        if run_step(&self.command, &reload_argv, deadline_ms).is_err() {
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
        let _ = run_command(&self.command, &wait_argv, 20_000);
    }
}

/// Run one agent-browser step and parse its envelope.
fn run_step(
    command: &str,
    argv: &[String],
    deadline_ms: u64,
) -> Result<serde_json::Value, CallError> {
    parse_envelope(&run_command(command, argv, deadline_ms)?)
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

/// The page never registered the tool.
fn tool_not_found(tool: &str) -> CallError {
    CallError {
        code: "tool_not_found",
        message: format!(
            "the page did not register {tool}; it may need a login or WebMCP may be off"
        ),
        data: None,
    }
}

/// The page registered the tool, then removed it before the call.
fn tool_removed(tool: &str) -> CallError {
    CallError {
        code: "tool_not_found",
        message: format!(
            "the page registered {tool} then removed it; it may need interaction first"
        ),
        data: None,
    }
}

/// Names the recorder saw, via one `eval`. Lenient: failure means none.
fn read_recorder(command: &str, base: &[String]) -> Vec<String> {
    const READ: &str = r#"(()=>{try{return (window.__wmcSeen||[]).filter(function(n){return typeof n==="string";});}catch(e){return [];}})();"#;
    let mut argv = base.to_vec();
    argv.extend(["eval".to_string(), READ.to_string()]);
    let Ok(out) = run_command(command, &argv, 15_000) else {
        return Vec::new();
    };
    let Ok(val) = parse_envelope(&out) else {
        return Vec::new();
    };
    if !is_success(&val) {
        return Vec::new();
    }
    val.get("data")
        .and_then(serde_json::Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A temp file holding the invoke input; removed on drop, come what may.
struct TempFile {
    path: std::path::PathBuf,
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

/// Write the recorder init script next to it; both are removed after.
fn write_init_script() -> Result<TempFile, CallError> {
    write_temp("recorder", RECORDER_SRC).map_err(|e| CallError {
        code: e.code,
        message: e.message.replace("params file", "recorder script"),
        data: e.data,
    })
}

/// Init script logging tool registrations from the very first call.
/// Never throws; no-ops without document.modelContext; under 20 lines.
const RECORDER_SRC: &str = r#"(()=>{try{
if(window.__wmcSeen)return;
window.__wmcSeen=[];
var mc=(document&&document.modelContext)||null;
if(!mc||typeof mc.addEventListener!=="function")return;
mc.addEventListener("toolchange",function(ev){
try{var n=ev&&(ev.toolName||ev.name);if(n)window.__wmcSeen.push(String(n));}catch(e){}
});
}catch(e){}})();
"#;

/// Write text to a 0600 temp file with a unique name.
fn write_temp(name: &str, contents: &str) -> Result<TempFile, CallError> {
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
fn run_command(command: &str, args: &[String], deadline_ms: u64) -> Result<String, CallError> {
    let child = std::process::Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| CallError {
            code: "driver_error",
            message: format!("could not run {command}: {e}"),
            data: None,
        })?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    match wait_deadline(child, deadline).map_err(|e| CallError {
        code: "driver_error",
        message: format!("could not wait for {command}: {e}"),
        data: None,
    })? {
        Some(output) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        None => Err(CallError {
            code: "timeout",
            message: "agent-browser did not answer in time; retry with a larger --timeout"
                .to_string(),
            data: None,
        }),
    }
}

/// Wait for a child until the deadline; kill it past the deadline.
/// `Ok(None)` means it timed out (and was killed).
fn wait_deadline(
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
fn parse_envelope(stdout: &str) -> Result<serde_json::Value, CallError> {
    serde_json::from_str(stdout).map_err(|_| CallError {
        code: "driver_error",
        message: "agent-browser printed something that is not JSON; is it 0.43+?".to_string(),
        data: Some(serde_json::json!({
            "stdout": stdout.chars().take(200).collect::<String>(),
        })),
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

/// Map a failed `open`: navigation/timeout means unreachable, else driver.
fn open_failure(open: &serde_json::Value) -> CallError {
    let code = envelope_code(open);
    let lowered = code.to_lowercase();
    if lowered.contains("navigation") || lowered.contains("timeout") {
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

fn invoke_code(invoke: &serde_json::Value) -> &str {
    envelope_code(invoke)
}

/// Map a final invoke envelope to output or a typed error.
fn map_invoke(invoke: serde_json::Value) -> Result<serde_json::Value, CallError> {
    if !is_success(&invoke) {
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
