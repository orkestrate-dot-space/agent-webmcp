//! agent-browser: the only driver agent-webmcp runs itself, over a session.

use super::exec::{
    CallBudget, ChildOutput, TempFile, parse_version, run_command_raw, run_command_raw_with_stdin,
    run_version_arg, tool_not_found, webmcp_unsupported, write_temp,
};
use super::{
    CallRequest, CustomInspect, Detection, Driver, InspectOutcome, InspectedTool, Kind,
    NativeCollisionPolicy, Plan, RunOptions, RunOutcome, Step, custom_inspected,
    group_custom_inspects, map_custom_envelope, native_denied, native_needs_confirmation,
    parse_inspected_tools, unshadowed,
};
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
    headed: bool,
    profile: Option<String>,
    cdp: Option<String>,
    extra_args: Vec<String>,
}

impl AgentBrowser {
    /// Build from config; the registry calls this so plans stay in sync.
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            command: cfg.drivers.agent_browser.command.clone(),
            session: cfg.drivers.agent_browser.session.clone(),
            headed: cfg.drivers.agent_browser.headed,
            profile: cfg.drivers.agent_browser.profile.clone(),
            cdp: cfg.drivers.agent_browser.cdp.clone(),
            extra_args: cfg.drivers.agent_browser.extra_args.clone(),
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

    fn inspect(
        &self,
        url: &str,
        custom: &[CustomInspect],
        opts: &RunOptions,
    ) -> Result<InspectOutcome, CallError> {
        self.inspect_inner(url, custom, opts)
    }

    fn plan(&self, req: &CallRequest) -> Plan {
        let input = serde_json::to_string(&req.input).unwrap_or_default();
        if let Some(c) = &req.custom {
            let short: String = c.common.tool_hash.chars().take(12).collect();
            // Custom plans never emit raw `open` + `eval --stdin` shell
            // steps: those would skip the runtime's HTTP refusal, the
            // `http://***/***` abort-route setup, and the host-side origin
            // checks. Direct the operator to the guarded CLI call, which
            // implements all of them. An `http://` page needs a clean
            // throwaway session (`--fresh`): the runtime refuses cleartext
            // custom pages in persistent sessions.
            let mut command = vec![
                "agent-webmcp".to_string(),
                "call".to_string(),
                "--driver".to_string(),
                "agent-browser".to_string(),
            ];
            if Self::is_http_url(&req.page) {
                command.push("--fresh".to_string());
            }
            let target = format!("{}#{}", req.page, req.tool);
            command.extend([target, "--params".to_string(), "@-".to_string()]);
            return Plan {
                driver: self.id(),
                note: self.note().map(str::to_string),
                steps: vec![Step {
                    // `--params @-` reads the input from standard input:
                    // embedding it in argv would expose secrets to anyone
                    // with process-argument access (CWE-214). The operator
                    // ran `call --plan` with the input, so they pipe that
                    // same JSON on stdin.
                    tool: "shell".to_string(),
                    args: serde_json::json!({"command": command}),
                    why: format!(
                        "run custom tool {} from {} (sha {}) through the guarded CLI, which installs the cleartext-HTTP abort route and re-checks the page origin before evaluating custom code; pipe the input JSON on standard input",
                        req.tool,
                        c.dir.display(),
                        short
                    ),
                }],
            };
        }
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

/// Restrict a session name to agent-browser's session alphabet
/// (`[A-Za-z0-9_-]`), replacing anything else with `-`.
fn sanitize_session_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "session".to_string()
    } else {
        cleaned
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

struct PreparedSession {
    session: String,
    _lock: Option<super::lock::SessionLock>,
    base: Vec<String>,
    open_argv: Vec<String>,
}

static FRESH_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl AgentBrowser {
    fn prepare_session(
        &self,
        url: &str,
        fresh: bool,
        force_headed: bool,
    ) -> Result<PreparedSession, CallError> {
        // A per-tool headed override must not flip the shared reusable
        // session headed for later ordinary calls: give it a separate
        // reusable session derived from the configured one.
        let headed_session = force_headed && !self.headed;
        let session = if fresh {
            let seq = FRESH_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            format!(
                "agent-webmcp-{}-{}-{seq}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            )
        } else if headed_session {
            format!("{}-headed", sanitize_session_name(&self.session))
        } else {
            self.session.clone()
        };
        let lock = if fresh {
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
        if self.headed || force_headed {
            base.push("--headed".to_string());
        }
        let mut open_argv = base.clone();
        if let Some(profile) = &self.profile {
            open_argv.push("--profile".to_string());
            open_argv.push(profile.clone());
        }
        if let Some(cdp) = &self.cdp {
            open_argv.push("--cdp".to_string());
            open_argv.push(cdp.clone());
        }
        for arg in &self.extra_args {
            open_argv.push("--args".to_string());
            open_argv.push(arg.clone());
        }
        open_argv.push("--args".to_string());
        open_argv.push("--enable-features=WebMCP".to_string());
        open_argv.push("open".to_string());
        open_argv.push(url.to_string());
        Ok(PreparedSession {
            session,
            _lock: lock,
            base,
            open_argv,
        })
    }

    fn run_inner(&self, req: &CallRequest, opts: &RunOptions) -> Result<RunOutcome, CallError> {
        let force_headed = req.custom.as_ref().is_some_and(|c| c.common.headed);
        let prep = self.prepare_session(&req.page, opts.fresh, force_headed)?;
        let started = std::time::Instant::now();
        let budgets = CallBudget::new(opts.timeout_ms, opts.settle_ms.min(30_000), started);
        let outcome = self.run_phases(
            req,
            opts,
            &prep.base,
            &prep.open_argv,
            &prep.session,
            &budgets,
            started,
        );
        if opts.fresh {
            // A fresh session must not leak: close it with a bounded
            // allowance independent of the remaining budget. When the
            // budget expired before a command even started, no timeout
            // wrapper attempted a close and inferring closure from the
            // `timeout` code would leak the open browser; re-closing an
            // already-closed session is harmless.
            self.close_session(&prep.session, 5_000);
        }
        outcome
    }

    fn inspect_inner(
        &self,
        url: &str,
        custom: &[CustomInspect],
        opts: &RunOptions,
    ) -> Result<InspectOutcome, CallError> {
        let force_headed = custom.iter().any(|c| c.common.headed);
        let prep = self.prepare_session(url, opts.fresh, force_headed)?;
        let started = std::time::Instant::now();
        let settle_ms = opts.settle_ms.min(30_000);
        let budgets = CallBudget::new(opts.timeout_ms, settle_ms, started);
        let outcome = self.inspect_phases(
            url,
            custom,
            opts,
            &prep.base,
            &prep.open_argv,
            &prep.session,
            &budgets,
            settle_ms,
            started,
        );
        if opts.fresh {
            // Same bounded close as `run_inner`: never leak the throwaway
            // session, even when the budget expired before a command ran.
            self.close_session(&prep.session, 5_000);
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn inspect_phases(
        &self,
        url: &str,
        custom: &[CustomInspect],
        opts: &RunOptions,
        base: &[String],
        open_argv: &[String],
        session: &str,
        budgets: &CallBudget,
        settle_ms: u64,
        started: std::time::Instant,
    ) -> Result<InspectOutcome, CallError> {
        let open_cap = opts.timeout_ms.saturating_add(15_000);
        // Credential boundary before any navigation: an `http://` inspect
        // must never run in a persistent session (named reuse, profile, or
        // CDP all carry cookies), while an `https://` page in a persistent
        // session gets a pre-navigation abort route so a redirect hop to
        // `http://` cannot carry them either. The guard below removes the
        // route when this inspect finishes (`let _x` lives to the end of
        // the scope; bare `_` would drop immediately).
        if Self::is_http_url(url) && self.uses_persistent_session(opts.fresh) {
            return Err(Self::http_session_refused(url));
        }
        let _http_route_guard =
            if !Self::is_http_url(url) && self.uses_persistent_session(opts.fresh) {
                Some(self.block_http_hops(base, session, budgets)?)
            } else {
                None
            };
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
            .unwrap_or(url)
            .to_string();
        let title = opened
            .pointer("/data/title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let reused = opened
            .pointer("/data/lifecycle/reused")
            .or_else(|| opened.pointer("/lifecycle/reused"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let announced = parse_inspected_tools(&opened, "/data/webmcp/tools");
        // A list timeout closes the session: propagate it instead of
        // treating the dead session as an empty native catalog.
        let mut tools = self.list_all_tools(base, budgets.cap(15_000), session)?;
        if tools.is_empty() && !announced.is_empty() {
            tools = announced.clone();
        }
        if tools.is_empty() {
            if is_blocked_page(title, &page_url, "") {
                return Err(blocked_page(&page_url, title));
            }
            if announced.is_empty()
                && let Some(api) = self.probe_api(base, session, budgets)?
                && api != "object"
            {
                // Without local custom tools there is nothing more to probe.
                // With custom tools, treat the missing native API as an empty
                // native list and continue to the custom availability probes
                // below (custom code runs through `eval`, not `modelContext`).
                if custom.is_empty() {
                    return Err(webmcp_unsupported(&api));
                }
            }
            if settle_ms > 0 {
                let stop_at = std::time::Instant::now()
                    .checked_add(std::time::Duration::from_millis(settle_ms))
                    .unwrap_or(budgets.deadline)
                    .min(budgets.deadline);
                tools = self.poll_all_tools(base, stop_at, budgets, session)?;
                if tools.is_empty() {
                    // A reload/wait timeout closed the session: stop instead
                    // of listing and probing custom tools against a fresh
                    // browser. Recoverable failures keep the second look.
                    self.reload_and_wait(base, session, budgets)?;
                    tools = self.poll_all_tools(base, stop_at, budgets, session)?;
                }
                if tools.is_empty() {
                    budgets.checkpoint("list")?;
                    tools = self.list_all_tools(base, budgets.cap(15_000), session)?;
                }
            }
        }
        if tools.is_empty() && is_blocked_page(title, &page_url, "") {
            return Err(blocked_page(&page_url, title));
        }
        // Custom tools: probe live availability without calling execute().
        let pending = unshadowed(custom, &tools);
        if !pending.is_empty() {
            let probed = self.probe_custom_avail(base, session, budgets, &pending)?;
            tools.extend(probed);
        }
        Ok(InspectOutcome {
            page: page_url,
            tools,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reused,
            budget_ms: budgets.total_ms,
        })
    }

    /// One `webmcp list` for inspection: transport timeouts propagate (the
    /// wrapper closes the session, so an empty list would lie about a dead
    /// tab); other transport failures and daemon errors stay lenient empty
    /// results, since a busy or tool-less page is not a failure.
    fn list_all_tools(
        &self,
        base: &[String],
        per_poll_ms: u64,
        session: &str,
    ) -> Result<Vec<InspectedTool>, CallError> {
        let mut argv = base.to_vec();
        argv.extend(["webmcp".to_string(), "list".to_string()]);
        let out = match run_command(&self.command, &argv, per_poll_ms.max(1), session, "list") {
            Ok(out) => out,
            Err(e) if e.code == "timeout" => return Err(e),
            Err(_) => return Ok(Vec::new()),
        };
        let Ok(val) = parse_envelope(&out) else {
            return Ok(Vec::new());
        };
        if !is_success(&val) {
            return Ok(Vec::new());
        }
        Ok(parse_inspected_tools(&val, "/data/tools"))
    }

    /// Poll `webmcp list` until tools appear or the settle deadline passes.
    /// Like the run-path polls, the settle deadline only decides whether
    /// to start another poll; the transport deadline comes from the
    /// remaining call budget so a poll finishing after settle still counts
    /// instead of closing the session. A timeout aborts the inspect.
    fn poll_all_tools(
        &self,
        base: &[String],
        stop_at: std::time::Instant,
        budgets: &CallBudget,
        session: &str,
    ) -> Result<Vec<InspectedTool>, CallError> {
        loop {
            let now = std::time::Instant::now();
            if now >= stop_at {
                return Ok(Vec::new());
            }
            let tools = self.list_all_tools(base, budgets.cap(15_000).max(1), session)?;
            if !tools.is_empty() {
                return Ok(tools);
            }
            let remaining = stop_at.saturating_duration_since(std::time::Instant::now());
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(500)));
        }
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
        // Credential boundary for custom tools, before any navigation: an
        // `http://` page must never open in a persistent session (its
        // cookies would travel in cleartext), while an `https://` page gets
        // a pre-navigation abort route so a server redirect hop to `http://`
        // cannot carry session cookies either. The guard lives for the whole
        // call and removes the route when it finishes (`let _x` at function
        // scope; bare `_` would drop immediately).
        if req.custom.is_some()
            && Self::is_http_url(&req.page)
            && self.uses_persistent_session(opts.fresh)
        {
            return Err(Self::http_session_refused(&req.page));
        }
        let _http_route_guard = if req.custom.is_some() && !Self::is_http_url(&req.page) {
            Some(self.block_http_hops(base, session, budgets)?)
        } else {
            None
        };
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

        let announced = tool_names_at(&opened, "/data/webmcp/tools");
        // Custom tools: blocked-page check, then native-first, else eval.
        if let Some(custom) = &req.custom {
            let settle_ms = opts.settle_ms.min(30_000);
            let (output, used_native) = self.run_custom(
                req, custom, base, session, budgets, &announced, title, page_url, settle_ms,
            )?;
            return Ok(RunOutcome {
                output,
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                reused,
                budget_ms: budgets.total_ms,
                used_native,
            });
        }
        let settle_ms = opts.settle_ms.min(30_000);
        let result = if announced.iter().any(|n| n == &req.tool) {
            map_invoke(
                self.invoke_once(req, base, session, budgets, None, None)?,
                &req.tool,
            )
        } else if is_blocked_page(title, page_url, "") {
            Err(blocked_page(page_url, title))
        } else if settle_ms == 0 {
            map_invoke(
                self.invoke_once(req, base, session, budgets, None, None)?,
                &req.tool,
            )
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
            used_native: false,
        })
    }

    /// Invoke the live native tool when the collision policy allows it;
    /// otherwise return the `native_denied` / `native_needs_confirmation`
    /// signal for `call.rs` (never fall back to custom code).
    fn invoke_native_or_signal(
        &self,
        req: &CallRequest,
        custom: &super::CustomExec,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<(serde_json::Value, bool), CallError> {
        let native_id = format!("{}/{}", custom.host, req.tool);
        match custom.native_policy {
            NativeCollisionPolicy::Allow => {
                // Bind the grant to the custom tool's expected origin, then
                // invoke on the verified frame: a redirect or an unverified
                // registration never runs native code.
                let frame_id = self.verify_native_frame(
                    base,
                    session,
                    budgets,
                    &custom.common.expected_origin,
                    &req.tool,
                )?;
                let out = map_invoke(
                    self.invoke_once(
                        req,
                        base,
                        session,
                        budgets,
                        Some(frame_id.as_str()),
                        Some(custom.common.expected_origin.as_str()),
                    )?,
                    &req.tool,
                )?;
                Ok((out, true))
            }
            NativeCollisionPolicy::Deny => Err(native_denied(&native_id)),
            NativeCollisionPolicy::Ask => Err(native_needs_confirmation(&native_id)),
        }
    }

    /// Verify a live native tool may stand in for the custom tool, and
    /// return the frame to bind the invocation to. The top-level page
    /// origin (`location.origin`) must equal `expected_origin`, so a
    /// redirect never runs native code. Every same-named `webmcp list`
    /// entry must then carry a verifiable origin (the CLI cannot address
    /// the top frame by ID, so origin equality against the verified
    /// top-level origin is the binding property); foreign origins and
    /// origin-less entries are refused. Exactly one same-origin
    /// registration must remain, since multiple matches would be ambiguous
    /// like the daemon's own unframed invoke. The caller invokes with
    /// `webmcp invoke --frame <id>` so the call lands on the verified frame
    /// even if the inventory shifts. Transport failures propagate.
    ///
    /// TOCTOU note: the daemon resolves the frame again at invoke time
    /// without comparing origins, so [`invoke_once`] re-checks the page
    /// origin immediately before invoking (same function, no awaited work
    /// in between beyond the check itself). A navigation in that residual
    /// window is still refused only when the daemon reports
    /// `webmcp_context_changed` (mapped to `driver_error`); operators who
    /// need atomic check-and-invoke must use the checked runtime path.
    fn verify_native_frame(
        &self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
        expected_origin: &str,
        tool: &str,
    ) -> Result<String, CallError> {
        // (1) Top-level page origin over stdin (never argv).
        let mut argv = base.to_vec();
        argv.extend(["eval".to_string(), "--stdin".to_string()]);
        let out = run_command_stdin(
            &self.command,
            &argv,
            b"location.origin",
            budgets.cap(15_000).max(1),
            session,
            "eval",
        )?;
        let envelope = parse_envelope(&out)?;
        if !is_success(&envelope) {
            return Err(CallError {
                code: "driver_error",
                message: if envelope_error(&envelope).is_empty() {
                    "agent-browser eval failed".to_string()
                } else {
                    envelope_error(&envelope).to_string()
                },
                data: Some(envelope),
            });
        }
        let actual = envelope
            .pointer("/data/result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if actual.to_lowercase() != expected_origin.to_lowercase() {
            return Err(CallError {
                code: "origin_mismatch",
                message: format!(
                    "live page origin {actual:?} does not match the custom tool host {expected_origin:?}; refusing native substitution"
                ),
                data: Some(serde_json::json!({
                    "actual": actual,
                    "expected": expected_origin,
                })),
            });
        }
        // (2) Every same-named registration must carry a verifiable
        // origin: foreign origins and origin-less entries are refused.
        let mut argv = base.to_vec();
        argv.extend(["webmcp".to_string(), "list".to_string(), tool.to_string()]);
        let out = run_command(
            &self.command,
            &argv,
            budgets.cap(15_000).max(1),
            session,
            "list",
        )?;
        let val = parse_envelope(&out)?;
        if !is_success(&val) {
            return Err(CallError {
                code: "driver_error",
                message: "agent-browser list failed".to_string(),
                data: Some(val),
            });
        }
        let entries: Vec<&serde_json::Value> = val
            .pointer("/data/tools")
            .and_then(serde_json::Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter(|t| t.get("name").and_then(serde_json::Value::as_str) == Some(tool))
                    .collect()
            })
            .unwrap_or_default();
        for entry in &entries {
            match entry.get("origin").and_then(serde_json::Value::as_str) {
                Some(origin) if origin.to_lowercase() == expected_origin.to_lowercase() => {}
                Some(origin) => {
                    return Err(CallError {
                        code: "origin_mismatch",
                        message: format!(
                            "live native tool is registered on {origin:?}, not {expected_origin:?}; refusing substitution"
                        ),
                        data: Some((*entry).clone()),
                    });
                }
                // No origin to verify against: fail closed.
                None => {
                    return Err(CallError {
                        code: "origin_mismatch",
                        message: format!(
                            "live native tool registration for {tool:?} carries no verifiable origin; refusing substitution"
                        ),
                        data: Some((*entry).clone()),
                    });
                }
            }
        }
        // (3) Exactly one same-origin registration may run, bound to its
        // frame — mirroring the daemon's own ambiguity rule for unframed
        // invokes. A missing or empty frameId refuses the substitution
        // instead of falling back to an unframed invoke, which would run
        // under the daemon's default resolution instead of the verified
        // frame.
        match entries.as_slice() {
            [] => Err(tool_not_found(tool)),
            [entry]
                if entry
                    .get("frameId")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| !id.is_empty()) =>
            {
                Ok(entry
                    .get("frameId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string())
            }
            [entry] => Err(CallError {
                code: "origin_mismatch",
                message: format!(
                    "live native tool registration for {tool:?} carries no frameId; refusing unframed substitution"
                ),
                data: Some((*entry).clone()),
            }),
            many => Err(CallError {
                code: "ambiguous_tool",
                message: format!(
                    "the page registers {tool} in {} same-origin frames; refusing to pick one for the custom shadow",
                    many.len(),
                ),
                data: Some(serde_json::json!({
                    "frames": many.iter().filter_map(|t| t.get("frameId")).collect::<Vec<_>>(),
                })),
            }),
        }
    }

    /// Re-check the top-level page origin immediately before a bound native
    /// invoke. Fails closed (origin_mismatch, or the eval failure) so the
    /// checked origin is verified in the document that is about to run the
    /// tool, not just the document checked earlier.
    fn recheck_frame_origin(
        &self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
        expected_origin: &str,
    ) -> Result<(), CallError> {
        let mut argv = base.to_vec();
        argv.extend(["eval".to_string(), "--stdin".to_string()]);
        let out = run_command_stdin(
            &self.command,
            &argv,
            b"location.origin",
            budgets.cap(15_000).max(1),
            session,
            "eval",
        )?;
        let envelope = parse_envelope(&out)?;
        if !is_success(&envelope) {
            return Err(CallError {
                code: "driver_error",
                message: if envelope_error(&envelope).is_empty() {
                    "agent-browser eval failed".to_string()
                } else {
                    envelope_error(&envelope).to_string()
                },
                data: Some(envelope),
            });
        }
        let actual = envelope
            .pointer("/data/result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if actual.to_lowercase() != expected_origin.to_lowercase() {
            return Err(CallError {
                code: "origin_mismatch",
                message: format!(
                    "live page origin {actual:?} does not match the custom tool host {expected_origin:?}; refusing native substitution"
                ),
                data: Some(serde_json::json!({
                    "actual": actual,
                    "expected": expected_origin,
                })),
            });
        }
        Ok(())
    }

    /// Custom-tool call: blocked-page check, native-first (honoring the
    /// settle window for late registrations), else `eval --stdin`.
    /// Returns the output and whether a live native tool ran instead.
    /// A transport failure or timeout from the native probe propagates
    /// instead of falling through to custom eval on a closed session.
    #[allow(clippy::too_many_arguments)]
    fn run_custom(
        &self,
        req: &CallRequest,
        custom: &super::CustomExec,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
        announced: &[String],
        title: &str,
        page_url: &str,
        settle_ms: u64,
    ) -> Result<(serde_json::Value, bool), CallError> {
        if announced.iter().any(|n| n == &req.tool) {
            return self.invoke_native_or_signal(req, custom, base, session, budgets);
        }
        if is_blocked_page(title, page_url, "") {
            return Err(blocked_page(page_url, title));
        }
        if settle_ms == 0 {
            // Single native check; failures propagate (finding: no custom
            // eval on a closed session).
            if self.list_contains_tool(base, &req.tool, budgets.cap(15_000), session)? {
                return self.invoke_native_or_signal(req, custom, base, session, budgets);
            }
        } else {
            if self.list_contains_tool(base, &req.tool, budgets.cap(15_000), session)? {
                return self.invoke_native_or_signal(req, custom, base, session, budgets);
            }
            if announced.is_empty() {
                // A timed-out or failed probe propagates (the wrapper
                // already tried to close the session): polling it would
                // lose the original timeout. Only a successful probe that
                // confirms the native API is absent permits custom eval.
                match self.probe_api_strict(base, session, budgets)? {
                    // Confirmed absent native API: no native collision is
                    // possible, so custom-only calls still run instead of
                    // failing with webmcp_unsupported.
                    Some(api) if api != "object" => {
                        let out = self.eval_custom(req, custom, base, session, budgets)?;
                        return Ok((out, false));
                    }
                    _ => {}
                }
            }
            let stop_at = std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(settle_ms))
                .unwrap_or(budgets.deadline)
                .min(budgets.deadline);
            // Reserve half the settle window for the post-reload look: the
            // first poll stops at the halfway mark, so a tool that
            // registers late still gets a real second listing instead of an
            // already-expired deadline that returns without polling.
            let first_stop_at = std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(settle_ms / 2))
                .unwrap_or(budgets.deadline)
                .min(budgets.deadline);
            if self.poll_native_present(base, &req.tool, first_stop_at, budgets, session)? {
                return self.invoke_native_or_signal(req, custom, base, session, budgets);
            }
            // A reload timeout stops the custom call: the wrapper closes
            // the session on timeout, so the second look below would run
            // against the closed session.
            self.reload_and_wait(base, session, budgets)?;
            if self.poll_native_present(base, &req.tool, stop_at, budgets, session)? {
                return self.invoke_native_or_signal(req, custom, base, session, budgets);
            }
            budgets.checkpoint("list")?;
            if self.list_contains_tool(base, &req.tool, budgets.cap(15_000), session)? {
                return self.invoke_native_or_signal(req, custom, base, session, budgets);
            }
        }
        let out = self.eval_custom(req, custom, base, session, budgets)?;
        Ok((out, false))
    }

    /// Poll a live `webmcp list` until the tool appears or the settle
    /// deadline passes. Transport failures propagate; an empty list is `Ok(false)`.
    /// The settle deadline only decides whether to start another poll: the
    /// transport deadline comes from the remaining call budget, so a poll
    /// may finish after the settle window (its result still counts) and a
    /// settle-expiry timeout never closes the session while call budget
    /// remains for the final list check below.
    fn poll_native_present(
        &self,
        base: &[String],
        tool: &str,
        stop_at: std::time::Instant,
        budgets: &CallBudget,
        session: &str,
    ) -> Result<bool, CallError> {
        loop {
            let now = std::time::Instant::now();
            if now >= stop_at {
                return Ok(false);
            }
            if self.list_contains_tool(base, tool, budgets.cap(15_000), session)? {
                return Ok(true);
            }
            let remaining = stop_at.saturating_duration_since(std::time::Instant::now());
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(500)));
        }
    }

    /// One live `webmcp list` probe distinguishing absence from failure:
    /// `Ok(true)` = registered, `Ok(false)` = successful empty list or a
    /// confirmed `webmcp_unsupported` response (no native discovery: no
    /// native tool can exist, so custom calls may proceed to custom
    /// evaluation), `Err` = transport failure, timeout, or other daemon
    /// error (propagated so the caller never runs custom code on a closed
    /// session).
    fn list_contains_tool(
        &self,
        base: &[String],
        tool: &str,
        per_poll_ms: u64,
        session: &str,
    ) -> Result<bool, CallError> {
        let mut argv = base.to_vec();
        argv.extend(["webmcp".to_string(), "list".to_string(), tool.to_string()]);
        let out = run_command(&self.command, &argv, per_poll_ms.max(1), session, "list")?;
        let val = parse_envelope(&out)?;
        if !is_success(&val) {
            // The named-list form reports an absent tool as
            // `webmcp_tool_not_found` instead of an empty list: that is
            // absence, not failure. A confirmed `webmcp_unsupported`
            // response (agent-browser without the CDP WebMCP domain) also
            // means no native tool can exist, so custom calls fall through
            // to custom evaluation instead of aborting. Anything else
            // (transport errors already propagated above) is a real failure.
            if envelope_code(&val) == "webmcp_tool_not_found"
                || envelope_code(&val) == "webmcp_unsupported"
            {
                return Ok(false);
            }
            return Err(CallError {
                code: "driver_error",
                message: if envelope_error(&val).is_empty() {
                    "agent-browser list failed".to_string()
                } else {
                    envelope_error(&val).to_string()
                },
                data: Some(val),
            });
        }
        Ok(tool_listed(&val, tool))
    }

    /// Run the custom IIFE via `eval --stdin` (piped, never in argv, since
    /// the input can carry secrets) and map its envelope. The input is
    /// validated against the custom schema here — only when no live native
    /// tool runs instead — so invalid input never reaches custom JS.
    /// The live page origin is re-verified on the host first: the in-page
    /// wrapper guard alone is spoofable (a page override of
    /// `String.prototype` methods), so custom code and its input never
    /// reach a page that fails this check.
    fn eval_custom(
        &self,
        req: &CallRequest,
        custom: &super::CustomExec,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<serde_json::Value, CallError> {
        crate::call::validate_custom_input(&custom.common.input_schema, &req.input)?;
        self.recheck_frame_origin(base, session, budgets, &custom.common.expected_origin)?;
        let iife = crate::tools::render_call_iife(
            &custom.common.expected_origin,
            &custom.common.js_source,
            &req.tool,
            &req.input,
        );
        let mut argv = base.to_vec();
        argv.extend(["eval".to_string(), "--stdin".to_string()]);
        let cap = budgets.timeout_ms.saturating_add(15_000);
        let out = run_command_stdin(
            &self.command,
            &argv,
            iife.as_bytes(),
            budgets.phase(cap, "eval")?,
            session,
            "eval",
        )?;
        let envelope = parse_envelope(&out)?;
        if !is_success(&envelope) {
            return Err(CallError {
                code: "driver_error",
                message: if envelope_error(&envelope).is_empty() {
                    "agent-browser eval failed".to_string()
                } else {
                    envelope_error(&envelope).to_string()
                },
                data: Some(envelope),
            });
        }
        let result_str = envelope
            .pointer("/data/result")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CallError {
                code: "driver_error",
                message: "agent-browser eval returned no result".to_string(),
                data: Some(envelope.clone()),
            })?;
        let inner: serde_json::Value = serde_json::from_str(result_str).map_err(|_| CallError {
            code: "driver_error",
            message: "agent-browser eval returned non-JSON".to_string(),
            data: Some(envelope),
        })?;
        map_custom_envelope(&inner, &req.tool)
    }

    /// Probe live availability for non-shadowed custom tools via one
    /// `eval --stdin` per host group. Never calls `execute()`. Timeouts
    /// propagate: `run_command_stdin` closes the session on timeout, so a
    /// per-tool mark would lie and later groups would run on a dead session.
    fn probe_custom_avail(
        &self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
        pending: &[&CustomInspect],
    ) -> Result<Vec<InspectedTool>, CallError> {
        // Group by origin+js so one eval covers every tool from a host.
        let groups = group_custom_inspects(pending.iter().copied());
        let mut out = Vec::new();
        for ((expected_origin, js_source), tools) in groups {
            // Host-side final-origin check before custom code reaches the
            // page: the in-page wrapper guard alone is spoofable. A
            // mismatch marks the group unavailable (like the wrapper's
            // `origin_mismatch` verdict) without evaluating anything; a
            // timeout aborts inspection (the session is closed); other
            // probe failures stay lenient `eval failed` marks.
            if let Err(e) = self.recheck_frame_origin(base, session, budgets, &expected_origin) {
                if e.code == "timeout" {
                    return Err(e);
                }
                let reason = if e.code == "origin_mismatch" {
                    "origin_mismatch".to_string()
                } else {
                    "eval failed".to_string()
                };
                for c in &tools {
                    out.push(custom_inspected(c, false, Some(reason.clone()), None));
                }
                continue;
            }
            let names: Vec<&str> = tools.iter().map(|c| c.name.as_str()).collect();
            let iife = crate::tools::render_avail_iife(&expected_origin, &js_source, &names);
            let mut argv = base.to_vec();
            argv.extend(["eval".to_string(), "--stdin".to_string()]);
            let eval_out = match run_command_stdin(
                &self.command,
                &argv,
                iife.as_bytes(),
                budgets.cap(15_000).max(1),
                session,
                "eval",
            ) {
                Ok(out) => out,
                Err(e) if e.code == "timeout" => return Err(e),
                Err(_) => {
                    for c in &tools {
                        out.push(custom_inspected(
                            c,
                            false,
                            Some("eval failed".to_string()),
                            None,
                        ));
                    }
                    continue;
                }
            };
            let envelope = match parse_envelope(&eval_out) {
                Ok(v) if is_success(&v) => v,
                _ => {
                    for c in &tools {
                        out.push(custom_inspected(
                            c,
                            false,
                            Some("eval failed".to_string()),
                            None,
                        ));
                    }
                    continue;
                }
            };
            let Some(result_str) = envelope
                .pointer("/data/result")
                .and_then(serde_json::Value::as_str)
            else {
                for c in &tools {
                    out.push(custom_inspected(
                        c,
                        false,
                        Some("eval failed".to_string()),
                        None,
                    ));
                }
                continue;
            };
            let inner: serde_json::Value = match serde_json::from_str(result_str) {
                Ok(v) => v,
                Err(_) => {
                    for c in &tools {
                        out.push(custom_inspected(
                            c,
                            false,
                            Some("eval failed".to_string()),
                            None,
                        ));
                    }
                    continue;
                }
            };
            // Origin mismatch or tool error marks every tool in the group.
            if !inner
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                let code = inner
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let reason = inner
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(code)
                    .to_string();
                for c in &tools {
                    out.push(custom_inspected(c, false, Some(reason.clone()), None));
                }
                continue;
            }
            let map = inner
                .pointer("/tools")
                .and_then(serde_json::Value::as_object)
                .cloned()
                .unwrap_or_default();
            for c in &tools {
                match map.get(&c.name) {
                    Some(entry)
                        if entry
                            .get("ok")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false) =>
                    {
                        out.push(custom_inspected(c, true, None, None));
                    }
                    Some(entry) => {
                        let reason = entry
                            .get("reason")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unavailable")
                            .to_string();
                        let remediation = entry
                            .get("remediation")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string);
                        out.push(custom_inspected(c, false, Some(reason), remediation));
                    }
                    None => {
                        out.push(custom_inspected(
                            c,
                            false,
                            Some("not registered".to_string()),
                            None,
                        ));
                    }
                }
            }
        }
        Ok(out)
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
    /// Bind the call to one frame when `frame` is `Some` (custom native
    /// substitution); `None` preserves the daemon's default resolution.
    /// When `expected_origin` is `Some`, the page origin is re-checked
    /// immediately before invoking and the call fails closed on mismatch,
    /// so a redirect after [`verify_native_frame`] never runs native code
    /// under the earlier authorization. Plain native calls pass `None`.
    fn invoke_once(
        &self,
        req: &CallRequest,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
        frame: Option<&str>,
        expected_origin: Option<&str>,
    ) -> Result<serde_json::Value, CallError> {
        if let Some(expected) = expected_origin.filter(|e| !e.is_empty()) {
            self.recheck_frame_origin(base, session, budgets, expected)?;
        }
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
        if let Some(frame_id) = frame.filter(|id| !id.is_empty()) {
            invoke_argv.extend(["--frame".to_string(), frame_id.to_string()]);
        }
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
        if self.poll_once(base, &req.tool, budgets.cap(15_000), session)? {
            return map_invoke(
                self.invoke_once(req, base, session, budgets, None, None)?,
                &req.tool,
            );
        }
        if announced_empty
            && let Some(api) = self.probe_api(base, session, budgets)?
            && api != "object"
        {
            return Err(webmcp_unsupported(&api));
        }
        if self.poll_until_present(base, &req.tool, stop_at, budgets, session)? {
            return map_invoke(
                self.invoke_once(req, base, session, budgets, None, None)?,
                &req.tool,
            );
        }
        // Native calls keep the second look on recoverable reload failures
        // (they return `Ok(())`); a reload or post-reload timeout closed
        // the session, so stop instead of polling a dead session.
        self.reload_and_wait(base, session, budgets)?;
        if self.poll_until_present(base, &req.tool, stop_at, budgets, session)? {
            return map_invoke(
                self.invoke_once(req, base, session, budgets, None, None)?,
                &req.tool,
            );
        }
        budgets.checkpoint("list")?;
        if self.poll_once(base, &req.tool, budgets.cap(15_000), session)? {
            return map_invoke(
                self.invoke_once(req, base, session, budgets, None, None)?,
                &req.tool,
            );
        }
        if is_blocked_page(title, page_url, "") {
            return Err(blocked_page(page_url, title));
        }
        Err(tool_not_found(&req.tool))
    }

    /// Poll `webmcp list <tool>` until it appears or the deadline passes.
    /// Failed polls count as absent; the budget bounds the waiting. Like
    /// [`poll_native_present`], the settle deadline only decides whether to
    /// start another poll while the transport deadline comes from the
    /// remaining call budget, so a poll finishing after settle still counts
    /// instead of closing the session. Timeouts propagate: the session is
    /// closed, so settling must stop instead of polling a dead session and
    /// reporting `tool_not_found`.
    fn poll_until_present(
        &self,
        base: &[String],
        tool: &str,
        stop_at: std::time::Instant,
        budgets: &CallBudget,
        session: &str,
    ) -> Result<bool, CallError> {
        loop {
            let now = std::time::Instant::now();
            if now >= stop_at {
                return Ok(false);
            }
            if self.poll_once(base, tool, budgets.cap(15_000).max(1), session)? {
                return Ok(true);
            }
            let remaining = stop_at.saturating_duration_since(std::time::Instant::now());
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(500)));
        }
    }

    /// One settle poll: lenient, so a busy page costs budget, not errors.
    /// Timeouts (transport or output-collection) are the exception: they
    /// close the session, so they propagate instead of degrading into an
    /// empty list that would poll a dead session and misreport
    /// `tool_not_found`.
    fn poll_once(
        &self,
        base: &[String],
        tool: &str,
        per_poll_ms: u64,
        session: &str,
    ) -> Result<bool, CallError> {
        let mut argv = base.to_vec();
        argv.extend(["webmcp".to_string(), "list".to_string(), tool.to_string()]);
        let out = match run_command(&self.command, &argv, per_poll_ms, session, "list") {
            Ok(out) => out,
            Err(e) if e.code == "timeout" => return Err(e),
            Err(_) => return Ok(false),
        };
        let val = match parse_envelope(&out) {
            Ok(val) => val,
            Err(_) => return Ok(false),
        };
        Ok(is_success(&val) && tool_listed(&val, tool))
    }

    /// The JS API shape, or None when the probe itself failed.
    fn probe_api(
        &self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<Option<String>, CallError> {
        self.probe_api_inner(base, session, budgets)
    }

    /// Fallible API probe for custom calls: transport errors (including
    /// timeouts, after the wrapper already tried to close the session)
    /// propagate so the custom call stops instead of polling the same
    /// session and losing the original timeout. Only a successful probe
    /// reporting a non-`"object"` shape confirms native absence and permits
    /// the custom fallback.
    fn probe_api_strict(
        &self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<Option<String>, CallError> {
        self.probe_api_inner(base, session, budgets)
    }

    /// Shared probe body: `Ok(None)` covers daemon-error envelopes and
    /// unparseable results (inconclusive); transport failures are `Err`.
    fn probe_api_inner(
        &self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<Option<String>, CallError> {
        let mut argv = base.to_vec();
        argv.extend([
            "eval".to_string(),
            "typeof document.modelContext".to_string(),
        ]);
        let out = run_command(&self.command, &argv, budgets.cap(15_000), session, "eval")?;
        let val = parse_envelope(&out)?;
        if !is_success(&val) {
            return Ok(None);
        }
        Ok(val
            .pointer("/data/result")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string))
    }

    /// Reload once and wait for load, like the weekly scanner. A failed
    /// reload still gets its second look, unless it timed out: the wrapper
    /// closes the session on timeout, so polling or evaluating afterwards
    /// would run against the closed session and lose the original error.
    /// Returns `Err` only for the timeout; recoverable failures yield `Ok`
    /// and the caller takes its second look.
    fn reload_and_wait(
        &self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<(), CallError> {
        let mut reload_argv = base.to_vec();
        reload_argv.push("reload".to_string());
        match run_step(
            &self.command,
            &reload_argv,
            budgets.phase(15_000, "reload").unwrap_or(0),
            session,
            "reload",
        ) {
            Ok(_) => {}
            Err(e) if e.code == "timeout" => return Err(e),
            Err(_) => return Ok(()),
        }
        let mut wait_argv = base.to_vec();
        wait_argv.extend([
            "wait".to_string(),
            "--load".to_string(),
            "load".to_string(),
            "--timeout".to_string(),
            "15000".to_string(),
        ]);
        // The post-reload wait runs against the same session: a timeout
        // closes it, so propagate like a reload timeout instead of polling
        // a dead session. Other failures stay lenient.
        if budgets.remaining_ms() > 0 {
            match run_command(
                &self.command,
                &wait_argv,
                budgets.cap(20_000),
                session,
                "wait",
            ) {
                Ok(_) => {}
                Err(e) if e.code == "timeout" => return Err(e),
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// Session credentials must never travel over cleartext HTTP: refuse a
    /// custom tool whose page is `http://` when the navigation would carry
    /// persistent session state — a configured profile or CDP endpoint, or
    /// any reused (non-fresh) session. A clean throwaway session (fresh,
    /// no profile, no CDP) carries no prior cookies and may still open it.
    fn http_session_refused(page: &str) -> CallError {
        CallError {
            code: "denied",
            message: format!(
                "refusing to open {page} over http:// in a persistent browser session; session cookies would travel in cleartext (use an https:// page or a clean --fresh session without --profile/--cdp)"
            ),
            data: None,
        }
    }

    /// Whether `page` uses plain HTTP.
    fn is_http_url(page: &str) -> bool {
        page.split_once("://")
            .map(|(scheme, _)| scheme.eq_ignore_ascii_case("http"))
            .unwrap_or(false)
    }

    /// Whether this navigation would carry persistent session state: a
    /// configured profile, a CDP endpoint, or a reused (non-fresh) session.
    fn uses_persistent_session(&self, fresh: bool) -> bool {
        !fresh || self.profile.is_some() || self.cdp.is_some()
    }

    /// Block cleartext HTTP requests in this session before navigating to
    /// an `https://` custom page: an `https://` start can still reach
    /// `http://` through a server redirect hop, which would send session
    /// cookies in cleartext. The abort route is session-scoped, installed
    /// pre-navigation (the documented staging pattern), and also covers
    /// later hops (reloads, in-page navigations). Fails closed: when the
    /// route cannot be installed, the call stops instead of navigating
    /// unprotected. The returned guard removes the route when the last
    /// overlapping operation on the session finishes (see
    /// [`UnblockHttpHops`]).
    ///
    /// The pattern is deliberately the guard-owned `http://***/***`, not
    /// `http://*`: it matches the same cleartext requests (verified live
    /// against agent-browser 0.43.0, including bare hosts and deep paths),
    /// but unroutes as its own exact-string entry, so removing it can never
    /// delete an operator-owned `http://*`, `http://**`, `http://**/*`, or
    /// `http://*/*` route. Why this spelling: the daemon collapses
    /// consecutive `*` before matching (`collapse_wildcards`), so
    /// `http://***/***`, `http://**/*`, and `http://*/*` all match
    /// identically — but `unroute` compares the stored pattern by exact
    /// string equality and removes every entry with that exact spelling.
    /// No operator would plausibly pre-register the triple-star spelling,
    /// so the guard's install/unroute pair is owned by this process; and
    /// even if an operator route with another spelling exists, our unroute
    /// leaves it installed (still blocking: fail-safe). Note `http://*/*`
    /// alone was re-verified to match in this engine (bare + deep), and
    /// `?` is not a wildcard, so neither is a substitute.
    fn block_http_hops<'a>(
        &'a self,
        base: &[String],
        session: &str,
        budgets: &CallBudget,
    ) -> Result<UnblockHttpHops<'a>, CallError> {
        // Per-session lock, not the global map lock: a slow `network
        // route` for one session must not consume another session's call
        // budget before that session can install its own route. The global
        // lock below is held only for the map lookup — never across a
        // subprocess and never while acquiring a session lock — so lock
        // ordering (global, then at most one session lock) cannot deadlock.
        let slot = {
            let mut routes = SESSION_HTTP_ROUTES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            routes.entry(session.to_string()).or_default().clone()
        };
        let mut state = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.count > 0 {
            // An overlapping guarded operation already installed the
            // route in this session: share it instead of appending a
            // duplicate the daemon would remove together.
            state.count += 1;
            return Ok(UnblockHttpHops {
                command: &self.command,
                base: base.to_vec(),
                session: session.to_string(),
            });
        }
        // First guard on this session: install while holding the
        // per-session lock so a concurrent drop on the SAME session cannot
        // unroute mid-install. Other sessions proceed independently.
        let mut argv = base.to_vec();
        argv.extend([
            "network".to_string(),
            "route".to_string(),
            "http://***/***".to_string(),
            "--abort".to_string(),
        ]);
        let out = run_command(
            &self.command,
            &argv,
            budgets.phase(15_000, "route")?,
            session,
            "route",
        )?;
        let val = match parse_envelope(&out) {
            Ok(val) => val,
            Err(e) => {
                // The install response is unparseable: the route may still
                // have been applied, so roll it back exactly like an
                // explicit failure instead of leaking it into the reusable
                // session — and never hand out a guard for it.
                let mut unroute_argv = base.to_vec();
                unroute_argv.extend([
                    "network".to_string(),
                    "unroute".to_string(),
                    "http://***/***".to_string(),
                ]);
                let _ = run_command_raw(&self.command, &unroute_argv, 15_000, "unroute");
                return Err(e);
            }
        };
        if !is_success(&val) {
            // The route install reported failure but may have partially
            // succeeded: roll the pattern back so a half-installed route
            // cannot linger in the reusable session (or be replayed by a
            // later guard that assumes a clean slate), then fail closed.
            let mut unroute_argv = base.to_vec();
            unroute_argv.extend([
                "network".to_string(),
                "unroute".to_string(),
                "http://***/***".to_string(),
            ]);
            let _ = run_command_raw(&self.command, &unroute_argv, 15_000, "unroute");
            return Err(CallError {
                code: "driver_error",
                message:
                    "agent-browser could not block cleartext HTTP requests; refusing to navigate"
                        .to_string(),
                data: Some(val),
            });
        }
        state.count = 1;
        Ok(UnblockHttpHops {
            command: &self.command,
            base: base.to_vec(),
            session: session.to_string(),
        })
    }
}

/// Per-session `http://***/***` abort-route state for
/// [`AgentBrowser::block_http_hops`]. Guarded operations on one reusable
/// session overlap, so the route must survive until the *last* of them
/// finishes: the first guard installs it, later guards share the count,
/// and only the drop that reaches zero unroutes. Each session has its own
/// lock so a slow route install/unroute on one session never stalls
/// another session's subprocess or budget.
#[derive(Default)]
struct SessionRoute {
    /// Active guard count; zero means no route installed.
    count: usize,
}

static SESSION_HTTP_ROUTES: std::sync::LazyLock<
    std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Arc<std::sync::Mutex<SessionRoute>>>,
    >,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Forget a session's route entry: the session was closed (timeout), so the
/// abort route died with it and a pending guard's `unroute` would only
/// relaunch a browser for the dead session. The guard's drop finds no slot
/// and skips the subprocess call.
fn evict_session_route(session: &str) {
    let mut routes = SESSION_HTTP_ROUTES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    routes.remove(session);
}

/// Removes the `http://***/***` abort route installed by
/// [`AgentBrowser::block_http_hops`] when the last overlapping guarded
/// operation on the session finishes, and releases the session's route
/// entry, so a restriction added for one operation never lingers in a
/// reusable named session and affects later calls (which did not request
/// it) — while an overlapping operation that still needs the block keeps
/// it. The pattern is operation-owned:
/// `unroute http://***/***` removes exactly this entry and leaves any
/// operator-configured routes (e.g. `http://*`) untouched. Best-effort:
/// failures are ignored because the guarded operation already finished
/// (e.g. the session was closed after a timeout). The cleanup deliberately
/// carries no session handle: it must never close the session as a side
/// effect.
struct UnblockHttpHops<'a> {
    command: &'a str,
    base: Vec<String>,
    session: String,
}

impl Drop for UnblockHttpHops<'_> {
    fn drop(&mut self) {
        // Decrement under the session's own lock, and only unroute at zero:
        // dropping the first of two overlapping guards must not remove the
        // route the second one still needs. The session lock is held from
        // here through the unroute, so a concurrent install on this session
        // blocks until the route is gone and then installs fresh — it can
        // never acquire the slot between our final check and the unroute
        // and lose its route to us.
        let slot = {
            let routes = SESSION_HTTP_ROUTES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match routes.get(&self.session).cloned() {
                Some(slot) => slot,
                // Never installed (or already cleaned up): nothing to do.
                None => return,
            }
        };
        let mut state = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.count = state.count.saturating_sub(1);
        if state.count > 0 {
            return;
        }
        // Last user: release the map entry before unrouting. The global
        // lock is taken with try_lock — never block while holding the
        // session lock, since install takes global-then-session and
        // blocking here would deadlock. If the global is contended the
        // slot stays for a later drop to release.
        let mut removed = false;
        if let Ok(mut routes) = SESSION_HTTP_ROUTES.try_lock()
            && routes
                .get(&self.session)
                .is_some_and(|current| std::sync::Arc::ptr_eq(current, &slot))
        {
            routes.remove(&self.session);
            removed = true;
        }
        // A missing entry we did not remove means a timeout evicted the
        // slot after closing the session: unrouting now would implicitly
        // relaunch a browser for the dead session, so skip it.
        if !removed
            && SESSION_HTTP_ROUTES
                .try_lock()
                .is_ok_and(|routes| !routes.contains_key(&self.session))
        {
            return;
        }
        let mut argv = self.base.clone();
        argv.extend([
            "network".to_string(),
            "unroute".to_string(),
            "http://***/***".to_string(),
        ]);
        // No session close on failure: cleanup must not disturb whatever the
        // operation left behind.
        let _ = run_command_raw(self.command, &argv, 15_000, "unroute");
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

/// Write the input JSON to a 0600 temp file with a unique name.
fn write_params_file(input_json: &str) -> Result<TempFile, CallError> {
    write_temp("params", input_json)
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
            // The session is closed on timeout: evict its route entry so a
            // pending `UnblockHttpHops` guard skips its `unroute` instead of
            // implicitly launching a new browser for a dead session. Evict
            // only when the close actually succeeded (a collected-output
            // `Ok` is not confirmation): otherwise the route may still be
            // installed and the guard must still unroute it.
            let closed = run_command_raw(
                command,
                &[
                    "--session".to_string(),
                    session.to_string(),
                    "close".to_string(),
                ],
                15_000,
                "close",
            )
            .ok()
            .and_then(|out| parse_envelope(&out).ok())
            .is_some_and(|val| is_success(&val));
            if closed {
                evict_session_route(session);
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// Run a child with piped stdin, captured output, and a hard deadline.
/// Secrets travel over the pipe, never in argv. Timeouts close the session
/// like [`run_command`].
fn run_command_stdin(
    command: &str,
    args: &[String],
    stdin_data: &[u8],
    deadline_ms: u64,
    session: &str,
    phase: &str,
) -> Result<ChildOutput, CallError> {
    match run_command_raw_with_stdin(command, args, stdin_data, deadline_ms, phase) {
        Ok(out) => Ok(out),
        Err(e) if e.code == "timeout" => {
            // See `run_command`: a closed session must not be reopened by a
            // pending route guard's `unroute` — but only evict on confirmed
            // closure, so a failed close keeps its guard-mandated unroute.
            let closed = run_command_raw(
                command,
                &[
                    "--session".to_string(),
                    session.to_string(),
                    "close".to_string(),
                ],
                15_000,
                "close",
            )
            .ok()
            .and_then(|out| parse_envelope(&out).ok())
            .is_some_and(|val| is_success(&val));
            if closed {
                evict_session_route(session);
            }
            Err(e)
        }
        Err(e) => Err(e),
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
        Some("timed_out") => return Err(super::exec::timed_out("agent-browser", "invoke")),
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
pub(super) fn tool_error_text(output: &serde_json::Value) -> String {
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Stub `agent-browser` binary: logs argv lines and always answers a
    /// success envelope, so route install/unroute calls can be counted.
    fn stub_command(dir: &tempfile::TempDir, name: &str) -> (String, std::path::PathBuf) {
        let log = dir.path().join(format!("{name}.argv"));
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nprintf '%s\\n' '{{\"success\":true,\"data\":{{}}}}'\n",
            log.display()
        );
        let path = dir.path().join(name);
        std::fs::write(&path, &script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Warmup: refuse ETXTBSY flakes like the integration fakes do.
        let _ = std::fs::read(&path);
        for _ in 0..100 {
            match std::process::Command::new(&path).output() {
                Ok(_) => break,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        let _ = std::fs::write(&log, "");
        (path.display().to_string(), log)
    }

    fn test_driver(command: String) -> AgentBrowser {
        AgentBrowser {
            command,
            session: "route-test".to_string(),
            headed: false,
            profile: None,
            cdp: None,
            extra_args: Vec::new(),
        }
    }

    fn logged(log: &std::path::Path) -> String {
        std::fs::read_to_string(log).unwrap_or_default()
    }

    /// Two overlapping guards on one session share a single route install,
    /// and dropping the first must not unroute while the second is active.
    /// Dropping the last guard removes the route.
    #[test]
    fn http_route_guard_is_session_refcounted() {
        let dir = tempfile::tempdir().unwrap();
        let (cmd, log) = stub_command(&dir, "ab-route");
        let driver = test_driver(cmd);
        let session = "overlap-route-sess";
        let base = vec![
            "--session".to_string(),
            session.to_string(),
            "--json".to_string(),
        ];
        let budgets = CallBudget::new(10_000, 0, std::time::Instant::now());
        let g1 = driver
            .block_http_hops(&base, session, &budgets)
            .expect("first guard installs");
        let g2 = driver
            .block_http_hops(&base, session, &budgets)
            .expect("second guard shares");
        assert_eq!(
            logged(&log).lines().filter(|l| *l == "route").count(),
            1,
            "one install for two overlapping guards:\n{}",
            logged(&log)
        );
        drop(g1);
        assert!(
            !logged(&log).lines().any(|l| l == "unroute"),
            "first drop must not unroute while the second guard is active:\n{}",
            logged(&log)
        );
        drop(g2);
        assert!(
            logged(&log).lines().any(|l| l == "unroute"),
            "last drop removes the route:\n{}",
            logged(&log)
        );
    }
}
