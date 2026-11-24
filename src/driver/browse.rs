//! OpenCode Browse: drives the user's real Chrome through the `browse` CLI.

use super::agent_browser::is_blocked_page;
use super::exec::{
    CallBudget, ChildOutput, parse_version, run_command_raw, run_command_raw_with_stdin, run_probe,
    tool_not_found, webmcp_unsupported,
};
use super::{
    CallRequest, CustomInspect, Detection, Driver, InspectOutcome, InspectedTool, Kind,
    NativeCollisionPolicy, Plan, RunOptions, RunOutcome, Step, custom_inspected,
    group_custom_inspects, map_custom_envelope, native_denied, native_needs_confirmation,
    parse_inspected_tools, unshadowed,
};
use crate::call::CallError;
use crate::config::Config;

const INSTALL: &str = "install browse and pair your browser extension (browse install)";

/// Browse 0.1.5 clips `execute` display output at 100,000 characters; a
/// marked result at or past that size may be cut off.
const BROWSE_DISPLAY_LIMIT: usize = 100_000;

/// The tool may already have run: its result was cut off by the display
/// limit before the marked JSON could be parsed.
fn truncated_output(out: &super::exec::ChildOutput) -> CallError {
    CallError {
        code: "truncated",
        message: "browse clipped the execute output at its display limit; the tool may already have completed, but its full result could not be retrieved"
            .to_string(),
        data: Some(serde_json::json!({
            "stdout": out.stdout.chars().take(200).collect::<String>(),
            "stderr": out.stderr.chars().take(500).collect::<String>(),
        })),
    }
}

/// Meaningful error when `browse execute` aborts before emitting the marked
/// result line (e.g. `TimeoutError: eval timed out after 60000 ms`): surface
/// the real detail with the right code instead of hiding it behind a generic
/// "no marked result". Timeouts keep `code: "timeout"` so tab cleanup,
/// retry, and fresh-session handling treat them correctly.
fn unmarked_execute_error(out: &super::exec::ChildOutput) -> CallError {
    // Runtime errors land on stderr; the relay's own chatter
    // (`tab: <id>` line, blank lines) lives on stdout.
    let mut detail = out.stderr.trim().to_string();
    if detail.is_empty() {
        detail = out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("tab:"))
            .collect::<Vec<_>>()
            .join(" ");
    }
    // Flatten/sanitize to a single line, capped.
    let detail: String = detail
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect();
    let data = || {
        Some(serde_json::json!({
            "stdout": out.stdout.chars().take(200).collect::<String>(),
            "stderr": out.stderr.chars().take(500).collect::<String>(),
        }))
    };
    if detail.is_empty() {
        return CallError {
            code: "driver_error",
            message: "browse execute returned no marked result".to_string(),
            data: data(),
        };
    }
    let lower = detail.to_lowercase();
    if lower.contains("timeouterror") || lower.contains("timed out") {
        return CallError {
            code: "timeout",
            message: format!("browse execute timed out: {detail}"),
            data: data(),
        };
    }
    CallError {
        code: "driver_error",
        message: format!("browse execute failed: {detail}"),
        data: data(),
    }
}

/// Run driver: opens a background tab in the user's Chrome and invokes the tool.
pub struct Browse {
    command: String,
}

impl Browse {
    /// Build from config so `status`, `call`, and `plan` share the configured binary.
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            command: cfg.drivers.browse.command.clone(),
        }
    }

    fn browse_not_allowed() -> CallError {
        CallError {
            code: "browse_not_allowed",
            message: "custom tool requires allowBrowse: true in manifest.json to run in personal Chrome (browse)".to_string(),
            data: None,
        }
    }

    fn http_page_refused(page: &str) -> CallError {
        CallError {
            code: "denied",
            message: format!(
                "refusing to open {page} over http:// in personal Chrome (browse); the profile's cookies would travel in cleartext (use an https:// page)"
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

    fn run_inner(&self, req: &CallRequest, opts: &RunOptions) -> Result<RunOutcome, CallError> {
        // No fail-fast here: a live native tool takes precedence over a
        // same-named custom tool, so the native check in `run_on_tab` runs
        // first and `browse_not_allowed` only fires when no live native
        // tool exists to invoke.
        //
        // Credential boundary first: personal Chrome always carries the
        // user's profile cookies, so a custom tool page over cleartext HTTP
        // never opens here at all (session tools are already https-only by
        // manifest validation; this also covers `auth: none` tools).
        if req.custom.is_some() && Self::is_http_url(&req.page) {
            return Err(Self::http_page_refused(&req.page));
        }
        let started = std::time::Instant::now();
        let settle_ms = opts.settle_ms.min(30_000);
        let budgets = CallBudget::new(opts.timeout_ms, settle_ms, started);

        // Custom tools navigate inside an origin allowlist: the tool is
        // bound to its expected origin, so top-level navigations (including
        // server redirect hops) outside it are refused before any cookies
        // travel. Native calls keep unrestricted navigation.
        let mut nav_args = serde_json::json!({
            "url": req.page,
            "group": "agent-webmcp",
            "timeout": budgets.timeout_ms,
        });
        if let Some(custom) = &req.custom {
            nav_args["allowedOrigins"] =
                serde_json::json!([custom.common.expected_origin.as_str()]);
        }
        let nav_cap = budgets.timeout_ms.saturating_add(15_000);
        let nav_out = run_browse_json(
            &self.command,
            "navigate",
            &nav_args,
            budgets.phase(nav_cap, "open")?,
            "open",
        )?;
        let tab = nav_out.get("tab").cloned().ok_or_else(|| CallError {
            code: "driver_error",
            message: "browse navigate did not return a tab id".to_string(),
            data: Some(nav_out.clone()),
        })?;

        let outcome = self.run_on_tab(req, opts, &tab, &nav_out, &budgets, settle_ms, started);
        let timed_out = matches!(&outcome, Err(e) if e.code == "timeout");
        // Every call navigates a brand-new background tab: close it when
        // done so tabs never leak across calls (Chrome's cookies and
        // session persist across tabs regardless). Only a timeout leaves
        // the tab open, so an in-flight action is not interrupted.
        if !timed_out && let Ok(ms) = budgets.phase(10_000, "close") {
            let close_args = serde_json::json!({"action": "close", "tab": tab});
            let _ = run_browse_json(&self.command, "tabs", &close_args, ms, "close");
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn run_on_tab(
        &self,
        req: &CallRequest,
        _opts: &RunOptions,
        tab: &serde_json::Value,
        nav_out: &serde_json::Value,
        budgets: &CallBudget,
        settle_ms: u64,
        started: std::time::Instant,
    ) -> Result<RunOutcome, CallError> {
        let page_url = nav_out
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&req.page);
        let title = nav_out
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if nav_out
            .get("challenge")
            .is_some_and(|v| !v.is_null() && v != &serde_json::Value::Bool(false))
            || is_blocked_page(title, page_url, "")
        {
            return Err(CallError {
                code: "page_unreachable",
                message: "the page shows a bot check; it was not solved".to_string(),
                data: Some(serde_json::json!({ "url": page_url, "title": title })),
            });
        }

        // Custom tools: native-first, else eval in the main world.
        if let Some(custom) = &req.custom {
            let (output, used_native) =
                self.run_custom(req, custom, tab, nav_out, budgets, settle_ms)?;
            return Ok(RunOutcome {
                output,
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                reused: false,
                budget_ms: budgets.total_ms,
                used_native,
            });
        }
        if settle_ms > 0 {
            self.wait_for_tool(req, tab, budgets, settle_ms)?;
        }

        let res = self.browse_webmcp_call(tab, &req.tool, &req.input, None, budgets)?;
        let output = map_call_result(res, &req.tool)?;
        Ok(RunOutcome {
            output,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reused: false,
            budget_ms: budgets.total_ms,
            used_native: false,
        })
    }

    fn wait_for_tool(
        &self,
        req: &CallRequest,
        tab: &serde_json::Value,
        budgets: &CallBudget,
        settle_ms: u64,
    ) -> Result<(), CallError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(settle_ms);
        let list_args = serde_json::json!({"action": "list", "tab": tab});
        loop {
            let cap = budgets.cap(5_000).max(1);
            let list = run_browse_json(&self.command, "webmcp", &list_args, cap, "settle")?;
            if list.get("available").and_then(serde_json::Value::as_bool) == Some(false) {
                return Err(webmcp_unsupported("navigator.modelContext"));
            }
            let found = list
                .get("tools")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tools| {
                    tools.iter().any(|t| {
                        t.get("name").and_then(serde_json::Value::as_str) == Some(req.tool.as_str())
                    })
                });
            if found {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(tool_not_found(&req.tool));
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }

    /// Invoke the live native tool when the collision policy allows it;
    /// otherwise return the `native_denied` / `native_needs_confirmation`
    /// signal for `call.rs` (never fall back to custom code).
    fn invoke_native_or_signal(
        &self,
        req: &CallRequest,
        custom: &super::CustomExec,
        tab: &serde_json::Value,
        budgets: &CallBudget,
    ) -> Result<(serde_json::Value, bool), CallError> {
        let native_id = format!("{}/{}", custom.host, req.tool);
        match custom.native_policy {
            NativeCollisionPolicy::Allow => {
                // Bind the grant to the custom tool's expected origin: a
                // redirect since navigation must never run native code.
                let actual = self.browse_page_origin(tab, budgets)?;
                if actual.to_lowercase() != custom.common.expected_origin.to_lowercase() {
                    let expected: &str = &custom.common.expected_origin;
                    return Err(CallError {
                        code: "origin_mismatch",
                        message: format!(
                            "live page origin {actual:?} does not match the custom tool host {expected:?}; refusing native substitution"
                        ),
                        data: Some(serde_json::json!({
                            "actual": actual,
                            "expected": expected,
                        })),
                    });
                }
                // Bind the input to a same-origin frame: a same-named tool
                // in a foreign frame (e.g. a cross-origin iframe) must never
                // receive it, even when it is the sole match.
                let frame = self.verify_native_frame(
                    tab,
                    budgets,
                    &custom.common.expected_origin,
                    &req.tool,
                )?;
                let res =
                    self.browse_webmcp_call(tab, &req.tool, &req.input, Some(&frame), budgets)?;
                Ok((map_call_result(res, &req.tool)?, true))
            }
            NativeCollisionPolicy::Deny => Err(native_denied(&native_id)),
            NativeCollisionPolicy::Ask => Err(native_needs_confirmation(&native_id)),
        }
    }

    /// Origin of a `webmcp list` frame value when it is a URL
    /// (`scheme://host[:port][/...]`), normalized like [`expected_origin`]:
    /// lowercased, explicit scheme-default ports stripped. Labels and
    /// anything unparseable yield `None` (unverifiable).
    fn frame_origin(frame: &str) -> Option<String> {
        let (scheme, rest) = frame.split_once("://")?;
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return None;
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.is_empty() {
            return None;
        }
        let (name, port) = match authority.split_once(':') {
            Some((n, p)) => {
                let number: u32 = p.parse().ok()?;
                if number == 0 || number > 65535 {
                    return None;
                }
                (n, Some(number))
            }
            None => (authority, None),
        };
        if name.is_empty() {
            return None;
        }
        let lower_scheme = scheme.to_ascii_lowercase();
        let default_port = if lower_scheme == "http" { 80 } else { 443 };
        match port {
            Some(number) if number == default_port => {
                Some(format!("{lower_scheme}://{}", name.to_ascii_lowercase()))
            }
            Some(number) => Some(format!(
                "{lower_scheme}://{}:{number}",
                name.to_ascii_lowercase()
            )),
            None => Some(format!("{lower_scheme}://{}", name.to_ascii_lowercase())),
        }
    }

    /// Verify the shadowing native registration lives in a frame on the
    /// expected origin, and return that frame for a filtered invoke.
    /// Authorization comes only from browser-derived frame data: every
    /// same-named entry must carry a `frame` whose URL parses to the
    /// expected origin. The page-supplied `origin` field is advisory only
    /// (page content can claim any origin) and never authorizes the call;
    /// entries on foreign origins, entries with missing or unparsable
    /// frames, and label-only frames are all refused fail-closed. The call
    /// then requires exactly one matching frame (mirroring the daemon's own
    /// ambiguity rule). Input never reaches any other frame.
    fn verify_native_frame(
        &self,
        tab: &serde_json::Value,
        budgets: &CallBudget,
        expected_origin: &str,
        tool: &str,
    ) -> Result<String, CallError> {
        let expected = expected_origin.to_lowercase();
        let list_args = serde_json::json!({"action": "list", "tab": tab});
        let list = run_browse_json(
            &self.command,
            "webmcp",
            &list_args,
            budgets.cap(15_000).max(1),
            "settle",
        )?;
        if list.get("available").and_then(serde_json::Value::as_bool) == Some(false) {
            return Err(tool_not_found(tool));
        }
        let entries: Vec<&serde_json::Value> = list
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter(|t| t.get("name").and_then(serde_json::Value::as_str) == Some(tool))
                    .collect()
            })
            .unwrap_or_default();
        let mut matched: Vec<String> = Vec::new();
        for entry in &entries {
            // The receiving frame must prove its own origin: parse the
            // browser-reported `frame` URL and compare. The `origin` field
            // is page-supplied and untrusted, so it is never consulted for
            // authorization (a foreign iframe can claim the top-level
            // origin). Missing, label-only, or foreign frames are refused.
            match entry
                .get("frame")
                .and_then(serde_json::Value::as_str)
                .and_then(Self::frame_origin)
            {
                Some(origin) if origin == expected => {
                    matched.push(
                        entry
                            .get("frame")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    );
                }
                _ => {
                    return Err(CallError {
                        code: "origin_mismatch",
                        message: format!(
                            "live native tool registration for {tool:?} carries no verifiable same-origin frame; refusing substitution"
                        ),
                        data: Some((*entry).clone()),
                    });
                }
            }
        }
        match matched.as_slice() {
            [] => Err(tool_not_found(tool)),
            [frame] => Ok(frame.clone()),
            many => Err(CallError {
                code: "ambiguous_tool",
                message: format!(
                    "the page registers {tool} in {} same-origin frames; refusing to pick one for the custom shadow",
                    many.len(),
                ),
                data: Some(serde_json::json!({
                    "frames": many,
                })),
            }),
        }
    }

    fn run_custom(
        &self,
        req: &CallRequest,
        custom: &super::CustomExec,
        tab: &serde_json::Value,
        nav_out: &serde_json::Value,
        budgets: &CallBudget,
        settle_ms: u64,
    ) -> Result<(serde_json::Value, bool), CallError> {
        let _ = nav_out;
        // Native-first: an eligible live native tool wins even when the
        // same-named custom tool opts out of browse. Only when no live
        // native tool exists does `allowBrowse: false` fail fast, before
        // any custom JS runs.
        match self.native_probe(tab, &req.tool, budgets)? {
            Some(true) => {
                if settle_ms > 0 {
                    self.wait_for_tool(req, tab, budgets, settle_ms)?;
                }
                return self.invoke_native_or_signal(req, custom, tab, budgets);
            }
            Some(false) if settle_ms > 0 => {
                // Settle window for late native registration.
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_millis(settle_ms);
                loop {
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    let cap = budgets.cap(5_000).max(1);
                    match self.native_probe_cap(tab, &req.tool, cap)? {
                        Some(true) => {
                            return self.invoke_native_or_signal(req, custom, tab, budgets);
                        }
                        // Unsupported mid-settle: no native tool can appear.
                        None => break,
                        Some(false) => {}
                    }
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            }
            _ => {}
        }
        if !custom.common.allow_browse {
            return Err(Self::browse_not_allowed());
        }
        // Validate against the custom schema only now that no live native
        // tool runs: a native tool with a different schema must not be
        // blocked, and invalid input must never reach custom JS.
        crate::call::validate_custom_input(&custom.common.input_schema, &req.input)?;
        // Host-side final-origin check before custom code or its input
        // reaches the page: the in-page wrapper guard alone is spoofable
        // (a page override of `String.prototype` methods).
        let actual = self.browse_page_origin(tab, budgets)?;
        if actual.to_lowercase() != custom.common.expected_origin.to_lowercase() {
            let expected: &str = &custom.common.expected_origin;
            return Err(CallError {
                code: "origin_mismatch",
                message: format!(
                    "live page origin {actual:?} does not match the custom tool host {expected:?}; refusing custom evaluation"
                ),
                data: Some(serde_json::json!({
                    "actual": actual,
                    "expected": expected,
                })),
            });
        }
        let iife = crate::tools::render_call_iife(
            &custom.common.expected_origin,
            &custom.common.js_source,
            &req.tool,
            &req.input,
        );
        let inner = self.browse_eval(tab, &iife, budgets)?;
        Ok((map_custom_envelope(&inner, &req.tool)?, false))
    }

    /// Live `webmcp list` probe for native-first shadowing.
    /// `Ok(Some(true))` = registered, `Ok(Some(false))` = supported but
    /// absent, `Ok(None)` = WebMCP unsupported (treated as no native tool,
    /// never an error, on custom paths). Transport failures and timeouts
    /// propagate so custom code never runs on a dead tab.
    fn native_probe(
        &self,
        tab: &serde_json::Value,
        tool: &str,
        budgets: &CallBudget,
    ) -> Result<Option<bool>, CallError> {
        self.native_probe_cap(tab, tool, budgets.cap(5_000).max(1))
    }

    /// [`native_probe`] with an explicit per-poll budget (settle loops).
    fn native_probe_cap(
        &self,
        tab: &serde_json::Value,
        tool: &str,
        cap_ms: u64,
    ) -> Result<Option<bool>, CallError> {
        let list_args = serde_json::json!({"action": "list", "tab": tab});
        // A `webmcp_unsupported` list failure means no native tool can
        // exist: fall back to custom cleanly. Timeouts and all other
        // errors propagate so custom code never runs on a dead tab.
        let list = match run_browse_json(&self.command, "webmcp", &list_args, cap_ms, "settle") {
            Ok(list) => list,
            Err(e) if e.code == "webmcp_unsupported" => return Ok(None),
            Err(e) => return Err(e),
        };
        if list.get("available").and_then(serde_json::Value::as_bool) == Some(false) {
            return Ok(None);
        }
        Ok(Some(
            list.get("tools")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tools| {
                    tools
                        .iter()
                        .any(|t| t.get("name").and_then(serde_json::Value::as_str) == Some(tool))
                }),
        ))
    }

    /// Per-call unique marker prefixing `browse execute` results, so page
    /// output and warnings can never collide with or spoof the result line.
    fn execute_marker() -> String {
        static MARKER_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        format!(
            "__AWM_{}_{}__",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            MARKER_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }

    /// Run a script through `browse execute --file -` (piped over stdin,
    /// never in argv) and return its parsed result value.
    fn browse_execute(
        &self,
        tab: &serde_json::Value,
        script: &str,
        marker: &str,
        budgets: &CallBudget,
    ) -> Result<serde_json::Value, CallError> {
        let tab_str = match tab {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => {
                return Err(CallError {
                    code: "driver_error",
                    message: "browse navigate did not return a tab id".to_string(),
                    data: Some(tab.clone()),
                });
            }
        };
        // Flags verified live against Browse 0.1.5: `--file -` reads the
        // script from stdin, `--tab` continues the navigated tab exactly,
        // and `--timeout-ms=<ms>` (equals form) bounds the run.
        let args = vec![
            "execute".to_string(),
            "--file".to_string(),
            "-".to_string(),
            "--tab".to_string(),
            tab_str,
            format!("--timeout-ms={}", budgets.timeout_ms),
        ];
        let cap = budgets.timeout_ms.saturating_add(5_000);
        // Transport failures and timeouts propagate so custom code never
        // runs on a dead tab; timeouts keep their code for fresh cleanup.
        let out = run_command_raw_with_stdin(
            &self.command,
            &args,
            script.as_bytes(),
            budgets.phase(cap, "eval")?,
            "eval",
        )?;
        // Browse clips `execute` display output at 100,000 characters: a
        // large tool result can lose the marked line (or cut the marked
        // JSON mid-value) after the tool has already run. Detect the clip
        // and report truncation — making clear the invocation may already
        // have completed — instead of a misleading `driver_error`.
        let clipped = out.truncated || out.stdout.len() >= BROWSE_DISPLAY_LIMIT;
        let line = out
            .stdout
            .lines()
            .find(|l| l.contains(marker))
            .ok_or_else(|| {
                if clipped {
                    truncated_output(&out)
                } else {
                    unmarked_execute_error(&out)
                }
            })?;
        let json_text = line
            .split_once(marker)
            .map(|(_, rest)| rest.trim())
            .unwrap_or("");
        serde_json::from_str(json_text).map_err(|_| {
            if clipped {
                truncated_output(&out)
            } else {
                CallError {
                    code: "driver_error",
                    message: "browse eval returned non-JSON".to_string(),
                    data: Some(serde_json::json!({
                        "stdout": out.stdout.chars().take(200).collect::<String>(),
                    })),
                }
            }
        })
    }

    /// The live page origin via `browse.eval` in the main world (the
    /// `browse execute` runtime has no tab-backed `location` binding, so a
    /// bare `location.origin` script throws `ReferenceError` and yields "no
    /// marked result"). The script is piped over stdin, never in argv.
    fn browse_page_origin(
        &self,
        tab: &serde_json::Value,
        budgets: &CallBudget,
    ) -> Result<String, CallError> {
        let marker = Self::execute_marker();
        let eval_call = serde_json::json!({
            "expression": "location.origin",
            "world": "main",
            "timeout": budgets.timeout_ms,
        });
        let eval_json = serde_json::to_string(&eval_call).unwrap_or_else(|_| "{}".to_string());
        let script = format!(
            "const res = await browse.eval({eval_json});\nreturn {marker_json} + JSON.stringify(res);",
            marker_json = serde_json::to_string(&marker).unwrap_or_else(|_| "\"\"".to_string()),
        );
        let res = self.browse_execute(tab, &script, &marker, budgets)?;
        // `browse.eval` wraps the value as `{result: <origin>}` (or
        // `{data:{result:...}}` on some relays); accept the envelope or the
        // raw string so the origin comparison sees the page's actual origin.
        let candidate = res
            .get("result")
            .or_else(|| res.pointer("/data/result"))
            .unwrap_or(&res);
        if let Some(origin) = candidate.as_str() {
            return Ok(origin.to_string());
        }
        Err(CallError {
            code: "driver_error",
            message: "browse eval returned a non-string origin".to_string(),
            data: Some(res),
        })
    }

    /// Run the custom IIFE in the main world through `browse execute
    /// --file -` (script piped over stdin, never in argv: the input can
    /// carry secrets and long scripts exceed `MAX_ARG_STRLEN`). The wrapper
    /// awaits `browse.eval` and prefixes a per-call unique marker, so page
    /// output can never collide with or spoof the result line.
    fn browse_eval(
        &self,
        tab: &serde_json::Value,
        expression: &str,
        budgets: &CallBudget,
    ) -> Result<serde_json::Value, CallError> {
        let marker = Self::execute_marker();
        let eval_call = serde_json::json!({
            "expression": expression,
            "world": "main",
            "timeout": budgets.timeout_ms,
        });
        let eval_json = serde_json::to_string(&eval_call).unwrap_or_else(|_| "{}".to_string());
        let script = format!(
            "const res = await browse.eval({eval_json});\nreturn {marker_json} + JSON.stringify(res);",
            marker_json = serde_json::to_string(&marker).unwrap_or_else(|_| "\"\"".to_string()),
        );
        let res = self.browse_execute(tab, &script, &marker, budgets)?;
        // Accept several shapes: {result: "<json>"}, {result: {...}},
        // {data: {result: ...}}, or the envelope itself.
        let candidate = res
            .get("result")
            .or_else(|| res.pointer("/data/result"))
            .unwrap_or(&res);
        if let Some(s) = candidate.as_str() {
            return serde_json::from_str(s).map_err(|_| CallError {
                code: "driver_error",
                message: "browse eval returned non-JSON".to_string(),
                data: Some(res),
            });
        }
        if candidate.is_object() {
            // Already an envelope object (or {ok,...} directly).
            if candidate.get("ok").is_some() {
                return Ok(candidate.clone());
            }
            return Ok(candidate.clone());
        }
        Err(CallError {
            code: "driver_error",
            message: "browse eval returned no result".to_string(),
            data: Some(res),
        })
    }

    /// Invoke a live native tool through `browse execute --file -` (script
    /// piped over stdin, never in argv) so secret-bearing `input` never
    /// appears in child argv where a local process observer could read it.
    /// Both the shadowing-native route and the plain native route use this;
    /// list probes (no input) keep the `--json` argv transport. A `frame`
    /// filter (from [`verify_native_frame`]) constrains the call to the
    /// verified same-origin frame; `None` preserves unfiltered dispatch for
    /// plain native calls, which have no expected origin to bind.
    fn browse_webmcp_call(
        &self,
        tab: &serde_json::Value,
        name: &str,
        input: &serde_json::Value,
        frame: Option<&str>,
        budgets: &CallBudget,
    ) -> Result<serde_json::Value, CallError> {
        let marker = Self::execute_marker();
        let name_json = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".to_string());
        // Input crosses into page code as data, not syntax: double-encode
        // so `JSON.parse` rebuilds it. An object literal would silently
        // drop `{"__proto__": ...}` (prototype assignment, not an own
        // property) and mis-deliver user input to the tool.
        let input_str = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
        let input_json = serde_json::to_string(&input_str).unwrap_or_else(|_| "\"{}\"".to_string());
        let script = match frame.filter(|f| !f.is_empty()) {
            Some(frame) => {
                let frame_json =
                    serde_json::to_string(frame).unwrap_or_else(|_| "\"\"".to_string());
                format!(
                    "const __res = await browse.webmcp({{action: \"call\", name: {name_json}, input: JSON.parse({input_json}), frame: {frame_json}, timeout: {timeout}}});\nreturn {marker_json} + JSON.stringify(__res);",
                    timeout = budgets.timeout_ms,
                    marker_json =
                        serde_json::to_string(&marker).unwrap_or_else(|_| "\"\"".to_string()),
                )
            }
            None => format!(
                "const __res = await webmcp.call({name_json}, JSON.parse({input_json}));\nreturn {marker_json} + JSON.stringify(__res);",
                marker_json = serde_json::to_string(&marker).unwrap_or_else(|_| "\"\"".to_string()),
            ),
        };
        self.browse_execute(tab, &script, &marker, budgets)
    }

    fn inspect_inner(
        &self,
        url: &str,
        custom: &[CustomInspect],
        opts: &RunOptions,
    ) -> Result<InspectOutcome, CallError> {
        // Personal Chrome always carries the user's profile cookies, so a
        // cleartext navigation never opens here at all — even with no local
        // custom tools. (The inspect gate above refuses these too; this
        // covers direct `Driver::inspect` callers.)
        if Self::is_http_url(url) {
            return Err(Self::http_page_refused(url));
        }
        let started = std::time::Instant::now();
        let settle_ms = opts.settle_ms.min(30_000);
        let budgets = CallBudget::new(opts.timeout_ms, settle_ms, started);

        // Every navigation runs inside an origin allowlist — the inspected
        // URL's own origin plus every declared *HTTPS* custom origin — so
        // redirect hops outside them (including HTTPS -> HTTP downgrades)
        // are refused before any cookies travel. Custom tools declared on
        // `http://` are filtered out of the allowlist: an `http://` entry
        // would permit exactly the cleartext hop this blocks (they stay
        // listed, and their availability probes still report
        // `origin_mismatch` without running). Fail closed when the
        // inspected URL has no derivable origin.
        let mut origins = vec![crate::tools::expected_origin(url).map_err(|e| CallError {
            code: "denied",
            message: format!(
                "refusing to inspect {url}: cannot derive a safe origin allowlist ({e})"
            ),
            data: None,
        })?];
        origins.extend(
            custom
                .iter()
                .map(|c| c.common.expected_origin.clone())
                .filter(|o| o.starts_with("https://")),
        );
        origins.sort();
        origins.dedup();
        let nav_args = serde_json::json!({
            "url": url,
            "group": "agent-webmcp",
            "timeout": budgets.timeout_ms,
            "allowedOrigins": origins,
        });
        let nav_cap = budgets.timeout_ms.saturating_add(15_000);
        let nav_out = run_browse_json(
            &self.command,
            "navigate",
            &nav_args,
            budgets.phase(nav_cap, "open")?,
            "open",
        )?;
        let tab = nav_out.get("tab").cloned().ok_or_else(|| CallError {
            code: "driver_error",
            message: "browse navigate did not return a tab id".to_string(),
            data: Some(nav_out.clone()),
        })?;

        let outcome =
            self.inspect_on_tab(url, custom, &tab, &nav_out, &budgets, settle_ms, started);
        let timed_out = matches!(&outcome, Err(e) if e.code == "timeout");
        // Every inspect navigates a brand-new background tab: close it when
        // done so tabs never leak across inspects (Chrome's cookies and
        // session persist across tabs regardless). Only a timeout leaves
        // the tab open, so an in-flight action is not interrupted.
        if !timed_out && let Ok(ms) = budgets.phase(10_000, "close") {
            let close_args = serde_json::json!({"action": "close", "tab": tab});
            let _ = run_browse_json(&self.command, "tabs", &close_args, ms, "close");
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn inspect_on_tab(
        &self,
        url: &str,
        custom: &[CustomInspect],
        tab: &serde_json::Value,
        nav_out: &serde_json::Value,
        budgets: &CallBudget,
        settle_ms: u64,
        started: std::time::Instant,
    ) -> Result<InspectOutcome, CallError> {
        let page_url = nav_out
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(url)
            .to_string();
        let title = nav_out
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if nav_out
            .get("challenge")
            .is_some_and(|v| !v.is_null() && v != &serde_json::Value::Bool(false))
            || is_blocked_page(title, &page_url, "")
        {
            return Err(CallError {
                code: "page_unreachable",
                message: "the page shows a bot check; it was not solved".to_string(),
                data: Some(serde_json::json!({ "url": page_url, "title": title })),
            });
        }

        // A missing native WebMCP API is an empty native list when local
        // custom tools still deserve availability probes; native-only
        // inspection keeps propagating the error.
        let mut tools = match self.poll_tools(tab, budgets, settle_ms) {
            Ok(tools) => tools,
            Err(e) if e.code == "webmcp_unsupported" && !custom.is_empty() => Vec::new(),
            Err(e) => return Err(e),
        };
        // Custom tools: opt-outs report unavailable without running JS.
        let pending = unshadowed(custom, &tools);
        let mut denied = Vec::new();
        let mut opted_in = Vec::new();
        for c in pending {
            if c.common.allow_browse {
                opted_in.push(c);
            } else {
                denied.push(c);
            }
        }
        for c in denied {
            tools.push(custom_inspected(
                c,
                false,
                Some("browse_not_allowed".to_string()),
                Some(
                    "set allowBrowse: true in manifest.json or use --driver agent-browser"
                        .to_string(),
                ),
            ));
        }
        if !opted_in.is_empty() {
            tools.extend(self.probe_custom_avail(tab, budgets, &opted_in)?);
        }
        Ok(InspectOutcome {
            page: page_url,
            tools,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reused: false,
            budget_ms: budgets.total_ms,
        })
    }

    /// Probe live availability for opted-in custom tools via `browse eval`.
    /// Timeouts propagate: `run_command_raw` closes nothing here, but a
    /// timed-out tab is unusable and later groups must not run against it —
    /// the call site aborts inspection on `timeout` instead of reporting a
    /// false empty catalog with per-tool marks.
    fn probe_custom_avail(
        &self,
        tab: &serde_json::Value,
        budgets: &CallBudget,
        pending: &[&CustomInspect],
    ) -> Result<Vec<InspectedTool>, CallError> {
        let groups = group_custom_inspects(pending.iter().copied());
        let mut out = Vec::new();
        for ((expected_origin, js_source), tools) in groups {
            // Host-side final-origin check before the custom availability
            // probe reaches the page: the in-page wrapper guard alone is
            // spoofable. A mismatch marks the group unavailable (like the
            // wrapper's `origin_mismatch` verdict) without evaluating
            // anything; a timeout aborts inspection; other probe failures
            // stay lenient `eval failed` marks with the detail as
            // remediation so authors can diagnose them.
            match self.browse_page_origin(tab, budgets) {
                Ok(actual) if actual.to_lowercase() == expected_origin.to_lowercase() => {}
                Ok(_) => {
                    for c in &tools {
                        out.push(custom_inspected(
                            c,
                            false,
                            Some("origin_mismatch".to_string()),
                            None,
                        ));
                    }
                    continue;
                }
                Err(e) if e.code == "timeout" => return Err(e),
                Err(e) => {
                    for c in &tools {
                        out.push(custom_inspected(
                            c,
                            false,
                            Some("eval failed".to_string()),
                            Some(e.message.clone()),
                        ));
                    }
                    continue;
                }
            }
            let names: Vec<&str> = tools.iter().map(|c| c.name.as_str()).collect();
            let iife = crate::tools::render_avail_iife(&expected_origin, &js_source, &names);
            let inner = match self.browse_eval(tab, &iife, budgets) {
                Ok(v) => v,
                Err(e) if e.code == "timeout" => return Err(e),
                Err(e) => {
                    for c in &tools {
                        out.push(custom_inspected(
                            c,
                            false,
                            Some("eval failed".to_string()),
                            Some(e.message.clone()),
                        ));
                    }
                    continue;
                }
            };
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

    fn poll_tools(
        &self,
        tab: &serde_json::Value,
        budgets: &CallBudget,
        settle_ms: u64,
    ) -> Result<Vec<InspectedTool>, CallError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(settle_ms);
        let list_args = serde_json::json!({"action": "list", "tab": tab});
        loop {
            let cap = budgets.cap(5_000).max(1);
            let list = run_browse_json(&self.command, "webmcp", &list_args, cap, "settle")?;
            if list.get("available").and_then(serde_json::Value::as_bool) == Some(false) {
                return Err(webmcp_unsupported("navigator.modelContext"));
            }
            let tools = parse_inspected_tools(&list, "/tools");
            if !tools.is_empty() || settle_ms == 0 || std::time::Instant::now() >= deadline {
                return Ok(tools);
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }

    /// Navigate step for custom plans: refuses cleartext HTTP pages (the
    /// runtime never opens custom tool pages in personal Chrome, so no
    /// navigation step is emitted) and allowlists the expected origin on
    /// HTTPS pages, mirroring the runtime `navigate` call so redirect hops
    /// outside it are refused.
    fn custom_navigate_step(
        &self,
        req: &CallRequest,
        custom: &super::CustomExec,
    ) -> Result<Step, Plan> {
        if Self::is_http_url(&req.page) {
            return Err(Plan {
                driver: self.id(),
                note: Some(
                    "refusing to plan a personal-Chrome navigation to an http:// page: the runtime never opens custom tool pages in cleartext; run it with the guarded CLI workflow instead (`agent-webmcp call --driver agent-browser ...`)".to_string(),
                ),
                steps: Vec::new(),
            });
        }
        Ok(Step {
            tool: "navigate".to_string(),
            args: serde_json::json!({
                "url": req.page,
                "allowedOrigins": [custom.common.expected_origin.as_str()],
            }),
            why: "keep the returned tab id and pass it as tab; navigations outside the allowlist are refused".to_string(),
        })
    }
}

impl Driver for Browse {
    fn id(&self) -> &'static str {
        "browse"
    }

    fn kind(&self) -> Kind {
        Kind::Run
    }

    fn install(&self) -> Option<&'static str> {
        Some(INSTALL)
    }

    fn note(&self) -> Option<&'static str> {
        Some("uses your connected Chrome via the browse extension")
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        detect_with(&self.command, &run_status_json)
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
        if let Some(c) = &req.custom {
            let short = c.common.tool_hash.chars().take(12).collect::<String>();
            let target = format!("{}#{}", req.page, req.tool);
            if !c.common.allow_browse {
                // Never plan custom script evaluation in personal Chrome
                // for an opted-out tool. The native tool, if any, must go
                // through the guarded CLI `call` path too: it enforces the
                // native collision policy (Ask/Deny), the origin match, and
                // the same-origin frame binding. An agent-side `webmcp`
                // call would skip all three (CWE-863), so no such step is
                // emitted — custom code never runs in personal Chrome.
                let navigate = match self.custom_navigate_step(req, c) {
                    Ok(step) => step,
                    Err(refusal) => return refusal,
                };
                return Plan {
                    driver: self.id(),
                    note: Some(
                        "this custom tool opts out of personal Chrome (allowBrowse: false): use --driver agent-browser, or set \"allowBrowse\": true in manifest.json to run its script here".to_string(),
                    ),
                    steps: vec![
                        navigate,
                        Step {
                            tool: "shell".to_string(),
                            args: serde_json::json!({"command": ["agent-webmcp", "call", "--driver", "browse", target, "--params", "@-"]}),
                            why: "if a live native tool with this name exists, call it only through the guarded CLI, which enforces the native Ask/Deny policy, the page-origin match, and the same-origin frame check (never invoke page tools directly); pipe the input JSON on standard input".to_string(),
                        },
                    ],
                };
            }
            let navigate = match self.custom_navigate_step(req, c) {
                Ok(step) => step,
                Err(refusal) => return refusal,
            };
            return Plan {
                driver: self.id(),
                note: self.note().map(str::to_string),
                steps: vec![
                    navigate,
                    Step {
                        // One guarded call: the CLI itself checks for a live
                        // native tool first (native wins under its policy and
                        // input-schema validation) and only then evaluates
                        // custom code — a separate `webmcp list` step would
                        // duplicate that check and race it. `--params @-`
                        // keeps the input out of process arguments.
                        tool: "shell".to_string(),
                        args: serde_json::json!({"command": ["agent-webmcp", "call", "--driver", "browse", target, "--params", "@-"]}),
                        why: format!(
                            "when no live native tool shadows it, run custom tool {} from {} (sha {}) through the guarded CLI; pipe the input JSON on standard input",
                            req.tool,
                            c.dir.display(),
                            short
                        ),
                    },
                ],
            };
        }
        Plan {
            driver: self.id(),
            note: self.note().map(str::to_string),
            steps: vec![
                Step {
                    tool: "navigate".to_string(),
                    args: serde_json::json!({"url": req.page}),
                    why: "keep the returned tab id and pass it as tab".to_string(),
                },
                Step {
                    tool: "webmcp".to_string(),
                    args: serde_json::json!({"action": "call", "name": req.tool, "input": req.input}),
                    why: "pass the tab from step 1".to_string(),
                },
            ],
        }
    }
}

/// Detect browse via `<command> status --json`; injectable for tests.
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
            detail: Some(format!("could not run {command} status --json: {e}")),
        },
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                let version = json
                    .pointer("/relay/version")
                    .or_else(|| json.pointer("/setup/version"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .or_else(|| parse_version(&text))
                    .unwrap_or_else(|| "0.0.0".to_string());
                if json.get("connected").and_then(serde_json::Value::as_bool) == Some(true) {
                    Detection::Ready { version }
                } else {
                    Detection::Missing {
                        install: INSTALL.to_string(),
                        detail: Some(
                            "browse is installed, but no browser extension is connected; run `browse status` or `browse pair`"
                                .to_string(),
                        ),
                    }
                }
            } else {
                Detection::Missing {
                    install: INSTALL.to_string(),
                    detail: Some(format!("could not parse JSON from {command} status --json")),
                }
            }
        }
    }
}

fn run_status_json(command: &str) -> std::io::Result<std::process::Output> {
    run_probe(command, &["status", "--json"])
}

fn run_browse_json(
    command: &str,
    subcommand: &str,
    payload: &serde_json::Value,
    deadline_ms: u64,
    phase: &str,
) -> Result<serde_json::Value, CallError> {
    let args = vec![
        subcommand.to_string(),
        "--json".to_string(),
        payload.to_string(),
    ];
    let out = run_command_raw(command, &args, deadline_ms, phase)?;
    parse_browse_output(&out, subcommand, phase)
}

fn parse_browse_output(
    out: &ChildOutput,
    subcommand: &str,
    phase: &str,
) -> Result<serde_json::Value, CallError> {
    let trimmed = out.stdout.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(err) = value.get("error").and_then(serde_json::Value::as_str) {
            return Err(map_browse_error(err, phase));
        }
        return Ok(value);
    }
    let detail = if out.stderr.trim().is_empty() {
        trimmed
    } else {
        out.stderr.trim()
    };
    if !detail.is_empty() {
        return Err(map_browse_error(detail, phase));
    }
    Err(CallError {
        code: "driver_error",
        message: format!("browse {subcommand} returned non-JSON output during {phase}"),
        data: None,
    })
}

fn map_browse_error(message: &str, phase: &str) -> CallError {
    let lower = message.to_lowercase();
    if phase == "open"
        && (lower.contains("err_name_not_resolved")
            || lower.contains("err_connection")
            || lower.contains("navigation")
            || lower.contains("net::"))
    {
        return CallError {
            code: "page_unreachable",
            message: message.to_string(),
            data: None,
        };
    }
    if lower.contains("modelcontext") && lower.contains("not") {
        return webmcp_unsupported("navigator.modelContext");
    }
    CallError {
        code: "driver_error",
        message: message.to_string(),
        data: None,
    }
}

fn map_call_result(res: serde_json::Value, tool: &str) -> Result<serde_json::Value, CallError> {
    if res.get("isError").and_then(serde_json::Value::as_bool) == Some(true) {
        let msg = res
            .get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("text"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("tool returned an error");
        return Err(CallError {
            code: "tool_error",
            message: format!("{tool}: {msg}"),
            data: Some(res),
        });
    }
    // A top-level string `error` field is a tool failure surfacing through
    // the call transport, not successful output: reject it before the
    // result unwrap so a failed native call is never reported as success.
    if let Some(msg) = res.get("error").and_then(serde_json::Value::as_str) {
        let msg = if msg.is_empty() {
            "tool returned an error"
        } else {
            msg
        };
        return Err(CallError {
            code: "tool_error",
            message: format!("{tool}: {msg}"),
            data: Some(res),
        });
    }
    if let Some(result) = res.get("result") {
        return Ok(result.clone());
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::super::exec::ChildOutput;
    use super::unmarked_execute_error;

    fn child_out(stdout: &str, stderr: &str) -> ChildOutput {
        ChildOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            truncated: false,
        }
    }

    #[test]
    fn unmarked_timeout_error_on_stderr_is_timeout() {
        let err = unmarked_execute_error(&child_out(
            "tab: 7\n",
            "TimeoutError: eval timed out after 60000 ms (page may be busy)",
        ));
        assert_eq!(err.code, "timeout");
        assert!(err.message.contains("browse execute timed out"), "{err:?}");
        assert!(err.message.contains("TimeoutError"), "{err:?}");
    }

    #[test]
    fn unmarked_timeout_error_on_stdout_skips_tab_line() {
        let err = unmarked_execute_error(&child_out(
            "tab: 7\n\nTimeoutError: eval timed out after 60000 ms\n",
            "",
        ));
        assert_eq!(err.code, "timeout");
        assert!(err.message.contains("timed out after 60000"), "{err:?}");
        assert!(!err.message.contains("tab:"), "{err:?}");
    }

    #[test]
    fn unmarked_non_timeout_error_is_driver_error_with_detail() {
        let err = unmarked_execute_error(&child_out("", "Error: tab 7 not found"));
        assert_eq!(err.code, "driver_error");
        assert!(err.message.contains("browse execute failed"), "{err:?}");
        assert!(err.message.contains("tab 7 not found"), "{err:?}");
    }

    #[test]
    fn unmarked_empty_output_keeps_generic_message() {
        let err = unmarked_execute_error(&child_out("tab: 7\n", ""));
        assert_eq!(err.code, "driver_error");
        assert!(err.message.contains("no marked result"), "{err:?}");
    }
}
