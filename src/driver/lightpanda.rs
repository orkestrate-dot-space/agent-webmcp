//! Lightpanda: the lighter run driver, over `lightpanda run` PandaScript.
//!
//! Verified against the real binary: `page.goto`, `page.evaluate` (promise
//! results are awaited and come back stringified) and
//! `page.callPageTool({name, arguments})` invoke a tool. `page.pageTools()`
//! is not a script method, so the script lists tools through
//! `navigator.modelContext.getTools()` instead; sleeps use an in-page
//! promise because PandaScript has no `setTimeout`.

use super::exec::{
    parse_version, run_command_raw, run_version_arg, tool_not_found, webmcp_unsupported, write_temp,
};
use super::{
    CallRequest, CustomInspect, Detection, Driver, InspectOutcome, Kind, NativeCollisionPolicy,
    Plan, RunOptions, RunOutcome, Step, custom_inspected, group_custom_inspects, native_denied,
    native_needs_confirmation, parse_inspected_tools,
};
use crate::call::CallError;
use crate::config::Config;

/// Install line when the binary is missing, from the one-liner docs.
const INSTALL: &str = "curl -fsSL https://pkg.lightpanda.io/install.sh | bash -s \"1.0.4\"";

/// Run driver: opens the page and invokes the tool with one PandaScript.
pub struct Lightpanda {
    command: String,
}

impl Lightpanda {
    /// Build from config; the registry calls this so plans stay in sync.
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            command: cfg.drivers.lightpanda.command.clone(),
        }
    }

    fn run_inner(&self, req: &CallRequest, opts: &RunOptions) -> Result<RunOutcome, CallError> {
        let started = std::time::Instant::now();
        let settle_ms = opts.settle_ms.min(30_000);
        let total_ms = opts
            .timeout_ms
            .saturating_add(settle_ms)
            .saturating_add(30_000);
        if let Some(custom) = &req.custom {
            return self.run_custom(req, custom, opts, settle_ms, started, total_ms);
        }
        let script = render_script(req, opts.timeout_ms, settle_ms);
        let file = write_temp("lightpanda", &script)?;
        let argv = vec!["run".to_string(), file.path.display().to_string()];
        let out = run_command_raw(&self.command, &argv, total_ms, "run")?;
        map_output(&out.stdout, &out.stderr, req, started, total_ms)
    }

    /// Custom-tool call in a single document (`page.goto` runs once) so
    /// discovery and dispatch share what the page actually registered:
    /// settle for late native registrations, then invoke the shadowing
    /// native tool under `native_policy`, or evaluate the custom tool when
    /// none runs. Rust validates the input up front but only enforces the
    /// result when the document confirms no native tool handles the call —
    /// a shadowing native tool accepts its own input, and custom JS never
    /// evaluates invalid input (the script gates on the embedded verdict).
    fn run_custom(
        &self,
        req: &CallRequest,
        custom: &super::CustomExec,
        opts: &RunOptions,
        settle_ms: u64,
        started: std::time::Instant,
        total_ms: u64,
    ) -> Result<RunOutcome, CallError> {
        // Session-authenticated tools need the driver's logged-in session,
        // and their navigation must never send session credentials over a
        // cleartext redirect hop. Lightpanda runs a fresh sessionless
        // browser with no request interception, so it can neither supply
        // the session nor guard the redirect chain: refuse before any
        // navigation instead of running logged-out over an unguarded load.
        if custom.auth == crate::tools::AuthMode::Session {
            return Err(CallError {
                code: "needs_session",
                message: format!(
                    "tool {} needs a logged-in session; lightpanda runs a fresh sessionless browser and cannot guard its redirect chain — use --driver agent-browser or --driver browse",
                    req.tool
                ),
                data: None,
            });
        }
        let native_id = format!("{}/{}", custom.host, req.tool);
        let input_check =
            crate::call::validate_custom_input(&custom.common.input_schema, &req.input);
        let script = render_custom_script(
            req,
            custom,
            opts,
            settle_ms,
            input_check.is_ok(),
            &native_id,
        );
        let file = write_temp("lightpanda", &script)?;
        let argv = vec!["run".to_string(), file.path.display().to_string()];
        let out = run_command_raw(&self.command, &argv, total_ms, "run")?;
        // Deferred local schema error: the execution document confirmed no
        // native tool handles the call, so the stored validation error
        // fails it without any custom code having run.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(out.stdout.trim())
            && val.get("code").and_then(serde_json::Value::as_str) == Some("input_invalid")
        {
            return match input_check {
                Err(e) => Err(e),
                // Unreachable: the script reports `input_invalid` only when
                // the embedded validity flag is false.
                Ok(()) => Err(CallError {
                    code: "invalid_input",
                    message: "custom input failed validation".to_string(),
                    data: None,
                }),
            };
        }
        map_output(&out.stdout, &out.stderr, req, started, total_ms)
    }

    fn inspect_inner(
        &self,
        url: &str,
        custom: &[CustomInspect],
        opts: &RunOptions,
    ) -> Result<InspectOutcome, CallError> {
        let started = std::time::Instant::now();
        let settle_ms = opts.settle_ms.min(30_000);
        let total_ms = opts
            .timeout_ms
            .saturating_add(settle_ms)
            .saturating_add(30_000);
        // Headed and session-auth custom tools never run on lightpanda:
        // probe the rest. Session tools need a logged-in session lightpanda
        // cannot supply, and their navigation cannot guard redirect hops.
        let runnable: Vec<CustomInspect> = custom
            .iter()
            .filter(|c| !c.common.headed && c.auth != crate::tools::AuthMode::Session)
            .cloned()
            .collect();
        let script = render_custom_inspect_script(
            url,
            &runnable,
            opts.timeout_ms,
            settle_ms,
            !custom.is_empty(),
        );
        let file = write_temp("lightpanda-inspect", &script)?;
        let argv = vec!["run".to_string(), file.path.display().to_string()];
        let out = run_command_raw(&self.command, &argv, total_ms, "inspect")?;
        let mut outcome = map_inspect_output(&out.stdout, &out.stderr, url, started, total_ms)?;
        // Merge custom availability returned by the script.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(out.stdout.trim())
            && val
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        {
            let avail_map = val
                .pointer("/custom")
                .and_then(serde_json::Value::as_object)
                .cloned()
                .unwrap_or_default();
            for c in custom {
                if outcome.tools.iter().any(|t| t.name == c.name) {
                    continue;
                }
                if c.common.headed {
                    outcome.tools.push(custom_inspected(
                        c,
                        false,
                        Some("needs_headed".to_string()),
                        None,
                    ));
                    continue;
                }
                if c.auth == crate::tools::AuthMode::Session {
                    outcome.tools.push(custom_inspected(
                        c,
                        false,
                        Some("needs_session".to_string()),
                        Some(
                            "session-authenticated tools need a logged-in driver; use --driver agent-browser or --driver browse"
                                .to_string(),
                        ),
                    ));
                    continue;
                }
                match avail_map.get(&c.name) {
                    Some(entry)
                        if entry
                            .get("ok")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false) =>
                    {
                        outcome.tools.push(custom_inspected(c, true, None, None));
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
                        outcome
                            .tools
                            .push(custom_inspected(c, false, Some(reason), remediation));
                    }
                    None => {
                        // No entry: the page redirected or the tool is missing.
                        // Only report when the script also saw no native tools;
                        // otherwise leave it to the avail map (absent = skip).
                        // For determinism, mark as unavailable when the script
                        // returned an origin mismatch.
                        if val
                            .pointer("/customOriginMismatch")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false)
                        {
                            outcome.tools.push(custom_inspected(
                                c,
                                false,
                                Some("origin_mismatch".to_string()),
                                None,
                            ));
                        }
                    }
                }
            }
        } else {
            // Script failed to return native tools; still report headed blocks.
            for c in custom {
                if c.common.headed && !outcome.tools.iter().any(|t| t.name == c.name) {
                    outcome.tools.push(custom_inspected(
                        c,
                        false,
                        Some("needs_headed".to_string()),
                        None,
                    ));
                }
            }
        }
        Ok(outcome)
    }
}

impl Driver for Lightpanda {
    fn id(&self) -> &'static str {
        "lightpanda"
    }

    fn kind(&self) -> Kind {
        Kind::Run
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        detect_with(&self.command, &|cmd, arg| run_version_arg(cmd, arg))
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
            // Never emit the raw `<render_call_promise_expr>` evaluation:
            // it cannot run the tool as written, and hand-rendering it
            // would skip the runtime's native collision policy and
            // headed-tool gate. Route through the guarded CLI instead;
            // `--params @-` keeps the input out of process arguments.
            let mut why = format!(
                "run custom tool {} from {} (sha {}) through the guarded CLI, which checks for a shadowing native tool and enforces the headed-browser gate before evaluating custom code; pipe the input JSON on standard input",
                req.tool,
                c.dir.display(),
                short
            );
            if c.common.headed {
                why.push_str("; note: this tool needs a headed browser and lightpanda is headless-only — expect needs_headed, or re-run with --driver agent-browser --headed");
            }
            return Plan {
                driver: self.id(),
                note: Some(
                    "tools register after load; call pageTools to list them if callPageTool says the tool is missing"
                        .to_string(),
                ),
                steps: vec![Step {
                    tool: "shell".to_string(),
                    args: serde_json::json!({"command": ["agent-webmcp", "call", "--driver", "lightpanda", target, "--params", "@-"]}),
                    why,
                }],
            };
        }
        Plan {
            driver: self.id(),
            note: Some(
                "tools register after load; call pageTools to list them if callPageTool says the tool is missing"
                    .to_string(),
            ),
            steps: vec![
                Step {
                    tool: "goto".to_string(),
                    args: serde_json::json!({"url": req.page}),
                    why: "open the page".to_string(),
                },
                Step {
                    tool: "callPageTool".to_string(),
                    args: serde_json::json!({"name": req.tool, "arguments": req.input}),
                    why: "call the tool with the JSON input".to_string(),
                },
            ],
        }
    }

    fn note(&self) -> Option<&'static str> {
        Some("experimental, lightest")
    }

    fn install(&self) -> Option<&'static str> {
        Some(INSTALL)
    }
}

/// Detect lightpanda by running `<command> version` (bare `--version` is a
/// fatal error there); the runner is injectable so tests can fake it.
pub fn detect_with(
    command: &str,
    run: &dyn Fn(&str, &str) -> std::io::Result<std::process::Output>,
) -> Detection {
    match run(command, "version") {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Detection::Missing {
            install: INSTALL.to_string(),
            detail: None,
        },
        Err(e) => Detection::Missing {
            install: INSTALL.to_string(),
            detail: Some(format!("could not run {command} version: {e}")),
        },
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            match parse_version(&text) {
                Some(version) => Detection::Ready { version },
                None => Detection::Missing {
                    install: INSTALL.to_string(),
                    detail: Some(format!("could not parse a version from {command} version")),
                },
            }
        }
    }
}

/// PandaScript lines that return `{ok:false,code:"blocked"}` when the page
/// looks like an unsolved bot check. Blocking needs corroborating challenge
/// evidence, never a generic phrase alone. A challenge title, or a short
/// interstitial body mentioning bot checks, each suffice; a challenge
/// widget iframe counts only on an interstitial-shaped page (short body),
/// never embedded in an otherwise ordinary page. A long article body
/// discussing e.g. "verify you are human", or a URL containing such a
/// phrase, never blocks on its own.
fn blocked_check_lines() -> Vec<String> {
    vec![
        "const __bTitle = await page.evaluate(\"document.title\");".to_string(),
        "const __bBody = await page.evaluate(\"document.body ? document.body.innerText.slice(0, 4000) : ''\");".to_string(),
        "const __bDom = await page.evaluate(\"document.querySelector('#challenge-form,#cf-challenge-running,iframe[src*=\\\"challenges.cloudflare.com\\\"]') ? '1' : ''\");".to_string(),
        "const __bT = ((__bTitle || '') + '').toLowerCase();".to_string(),
        "const __bB = ((__bBody || '') + '').toLowerCase();".to_string(),
        "const __bTitleHit = (__bT.indexOf(\"just a moment\") !== -1 || __bT.indexOf(\"attention required\") !== -1 || __bT.indexOf(\"access denied\") !== -1 || __bT.indexOf(\"robot or human\") !== -1);".to_string(),
        "const __bBodyHit = (__bB.indexOf(\"captcha\") !== -1 || __bB.indexOf(\"verify you are human\") !== -1 || __bB.indexOf(\"are you a robot\") !== -1 || __bB.indexOf(\"bot challenge\") !== -1);".to_string(),
        "const __bShort = ((__bBody || '').length < 500);".to_string(),
        "if (__bTitleHit || (__bShort && __bBodyHit) || (__bDom !== '' && __bShort)) return JSON.stringify({ok: false, code: \"blocked\"});".to_string(),
    ]
}

/// [`blocked_check_lines`] gated on an empty discovery result: emits
/// `if (<empty>) { <probe lines> }` so the tool inventory is consulted
/// first and a working page that embeds a challenge widget but still
/// registers tools is never rejected. `<empty>` is a JS length check like
/// `__names.length === 0`.
fn blocked_check_unless(tools_empty: &str) -> Vec<String> {
    let mut lines = vec![format!("if ({tools_empty}) {{")];
    lines.extend(blocked_check_lines());
    lines.push("}".to_string());
    lines
}

/// The PandaScript: goto, settle-poll for the tool, call it. URL, tool, and
/// input go in as JSON data (double-encoded for `JSON.parse`), never raw
/// string concatenation. `evaluate`
/// cannot run top-level `await`, so async page calls use `.then()` chains.
fn render_script(req: &CallRequest, timeout_ms: u64, settle_ms: u64) -> String {
    let url = serde_json::to_string(&req.page).unwrap_or_default();
    let tool = serde_json::to_string(&req.tool).unwrap_or_default();
    let input = serde_json::to_string(&req.input).unwrap_or_else(|_| "{}".to_string());
    // Arguments cross into PandaScript as data: double-encode so
    // `JSON.parse` rebuilds them. An object literal would silently drop
    // `{"__proto__": ...}` keys (prototype assignment, not own properties).
    let input_str = serde_json::to_string(&input).unwrap_or_else(|_| "\"{}\"".to_string());
    let mut lines = vec![
        "const page = new Page();".to_string(),
        format!("await page.goto({url}, {{waitUntil: \"load\", timeout: {timeout_ms}}});"),
    ];
    lines.extend(vec![
        "const __t0 = Date.now();".to_string(),
        "let __names = [];".to_string(),
        "let __mcMissing = false;".to_string(),
        "for (;;) {".to_string(),
        "  const __mc = await page.evaluate(\"typeof navigator.modelContext\");".to_string(),
        "  if (__mc === \"undefined\") { __mcMissing = true; __names = []; break; }".to_string(),
        // evaluate hands arrays back JSON-encoded, so parse instead of
        // joining: a "\n" here would land as a raw newline in page code.
        "  const __list = await page.evaluate(\"navigator.modelContext.getTools().then(function(ts){return ts.map(function(t){return t.name})})\");"
            .to_string(),
        "  __names = JSON.parse(__list || \"[]\");".to_string(),
        format!("  if (__names.indexOf({tool}) !== -1) break;"),
        format!("  if (Date.now() - __t0 >= {settle_ms}) break;"),
        "  await page.evaluate(\"new Promise(function(__r){setTimeout(__r, 300)})\");".to_string(),
        "}".to_string(),
        // Tools first: a working page that embeds a challenge widget but
        // still registers tools is never rejected as blocked.
        "if (__names.length === 0) {".to_string(),
    ]);
    lines.extend(blocked_check_lines());
    lines.extend(vec![
        "}".to_string(),
        "if (__mcMissing) return JSON.stringify({ok: false, code: \"unsupported\"});".to_string(),
        format!(
            "if (__names.indexOf({tool}) === -1) return JSON.stringify({{ok: false, code: \"not_found\"}});"
        ),
        "try {".to_string(),
        format!(
            "  const __out = await page.callPageTool({{name: {tool}, arguments: JSON.parse({input_str})}});"
        ),
        "  return JSON.stringify({ok: true, output: (typeof __out === \"undefined\") ? null : __out});"
            .to_string(),
        "} catch (__e) {".to_string(),
        "  return JSON.stringify({ok: false, error: String((__e && __e.message) || __e)});"
            .to_string(),
        "}".to_string(),
    ]);
    lines.join("\n") + "\n"
}

/// Single-document custom PandaScript: one `page.goto`, blocked check,
/// settle for late native registrations in THIS document, then either the
/// shadowing native tool (under the embedded collision policy, origin
/// bound like the daemon's own call) or the custom evaluation. Because
/// discovery and dispatch share the document, a tool that registers late
/// is invoked where it registered instead of being missed by a second
/// load, and a same-document re-check can never disagree with the probe.
/// The custom branch runs only when no native tool registered, the tool is
/// headless-safe, and Rust's up-front input validation passed (the script
/// carries that verdict and reports `input_invalid` instead of evaluating
/// otherwise); the caller maps it back to the stored validation error.
fn render_custom_script(
    req: &CallRequest,
    custom: &super::CustomExec,
    opts: &RunOptions,
    settle_ms: u64,
    input_valid: bool,
    native_id: &str,
) -> String {
    let url = serde_json::to_string(&req.page).unwrap_or_default();
    let tool = serde_json::to_string(&req.tool).unwrap_or_default();
    let input = serde_json::to_string(&req.input).unwrap_or_else(|_| "{}".to_string());
    // Same double-encoding as `render_script`: native arguments rebuild
    // via `JSON.parse` so `{"__proto__": ...}` survives as data.
    let input_str = serde_json::to_string(&input).unwrap_or_else(|_| "\"{}\"".to_string());
    let expected_origin_json = serde_json::to_string(&*custom.common.expected_origin)
        .unwrap_or_else(|_| "\"\"".to_string());
    let native_id_json = serde_json::to_string(native_id).unwrap_or_default();
    let expr = crate::tools::render_call_promise_expr(
        &custom.common.expected_origin,
        &custom.common.js_source,
        &req.tool,
        &req.input,
    );
    let expr_json = serde_json::to_string(&expr).unwrap_or_default();
    let mut lines = vec![
        "const page = new Page();".to_string(),
        format!(
            "await page.goto({url}, {{waitUntil: \"load\", timeout: {}}});",
            opts.timeout_ms
        ),
        // Host-side final-origin check before custom code or its input
        // reaches the page: `location.origin` is read in the outer
        // PandaScript context and compared here with `!==` (no page-mutable
        // methods — a page override of `String.prototype.toLowerCase` cannot
        // spoof an operator). The in-page wrapper guard remains as defense
        // in depth.
        "const __cOrigin = await page.evaluate(\"location.origin\");".to_string(),
        format!(
            "if (__cOrigin !== {expected_origin_json}) return JSON.stringify({{ok: false, code: \"origin_mismatch\", error: \"page origin does not match \" + {expected_origin_json}}});"
        ),
    ];
    lines.extend(vec![
        "const __t0 = Date.now();".to_string(),
        "let __names = [];".to_string(),
        "for (;;) {".to_string(),
        "  const __mc = await page.evaluate(\"typeof navigator.modelContext\");".to_string(),
        "  if (__mc === \"undefined\") { __names = []; break; }".to_string(),
        "  const __list = await page.evaluate(\"navigator.modelContext.getTools().then(function(ts){return ts.map(function(t){return t.name})})\");"
            .to_string(),
        "  __names = JSON.parse(__list || \"[]\");".to_string(),
        format!("  if (__names.indexOf({tool}) !== -1) break;"),
        format!("  if (Date.now() - __t0 >= {settle_ms}) break;"),
        "  await page.evaluate(\"new Promise(function(__r){setTimeout(__r, 300)})\");".to_string(),
        "}".to_string(),
        format!("const __hasNative = (__names.indexOf({tool}) !== -1);"),
    ]);
    // Tools first: the interstitial verdict only applies when nothing
    // registered, so an embedded widget on a working page never blocks.
    lines.extend(blocked_check_unless("__names.length === 0"));
    if custom.common.headed {
        lines.push(
            "if (!__hasNative) return JSON.stringify({ok: false, code: \"needs_headed\"});"
                .to_string(),
        );
    }
    if !input_valid {
        lines.push(
            "if (!__hasNative) return JSON.stringify({ok: false, code: \"input_invalid\"});"
                .to_string(),
        );
    }
    // Final document binding immediately before dispatch: re-verify the
    // execution document's origin with a strict host-side comparison.
    // Only host-side policy checks follow — no page interaction — so the
    // native invoke and the custom evaluation below are the very next page
    // contact. A navigation in any earlier window lands here instead of in
    // custom code, and the fused in-page guard re-verifies synchronously
    // inside the delivered expression itself.
    lines.extend(vec![
        "const __cOriginF = await page.evaluate(\"location.origin\");".to_string(),
        format!("if (__cOriginF !== {expected_origin_json}) return JSON.stringify({{ok: false, code: \"origin_mismatch\", error: \"page origin does not match \" + {expected_origin_json}}});"),
    ]);
    // Frame binding: unlike the daemon drivers (which verify the native
    // registration's frame and refuse on foreign/ambiguous frames),
    // lightpanda cannot attribute a native registration to its frame, so a
    // shadowing native tool on a page with subframes could divert the
    // approved input to a cross-origin frame. Refuse substitution then;
    // custom evaluation below still runs when no native tool shadows it.
    lines.extend(vec![
        "let __frames = 0;".to_string(),
        "try { __frames = JSON.parse(await page.evaluate(\"window.length\") || \"0\"); } catch (__e) { __frames = 0; }".to_string(),
        "if (typeof __frames !== \"number\" || !(__frames >= 0)) __frames = 0;".to_string(),
        "if (__hasNative && __frames > 0) return JSON.stringify({ok: false, code: \"ambiguous_tool\", frames: __frames});".to_string(),
    ]);
    match custom.native_policy {
        NativeCollisionPolicy::Allow => {
            lines.extend(vec![
                "if (__hasNative) {".to_string(),
                "  try {".to_string(),
                format!(
                    "    const __out = await page.callPageTool({{name: {tool}, arguments: JSON.parse({input_str})}});"
                ),
                "    return JSON.stringify({ok: true, output: (typeof __out === \"undefined\") ? null : __out, used_native: true});".to_string(),
                "  } catch (__e) {".to_string(),
                "    return JSON.stringify({ok: false, error: String((__e && __e.message) || __e)});".to_string(),
                "  }".to_string(),
                "}".to_string(),
            ]);
        }
        NativeCollisionPolicy::Deny => {
            lines.push(format!(
                "if (__hasNative) return JSON.stringify({{ok: false, code: \"native_denied\", native: {native_id_json}}});"
            ));
        }
        NativeCollisionPolicy::Ask => {
            lines.push(format!(
                "if (__hasNative) return JSON.stringify({{ok: false, code: \"native_needs_confirmation\", native: {native_id_json}}});"
            ));
        }
    }
    lines.extend(vec![
        "try {".to_string(),
        format!("  const __customStr = await page.evaluate({expr_json});"),
        "  const __inner = JSON.parse(__customStr || \"{}\");".to_string(),
        "  if (__inner.ok) return JSON.stringify({ok: true, output: (__inner.output === undefined ? null : __inner.output), used_native: false});".to_string(),
        "  return JSON.stringify({ok: false, code: (__inner.code || \"tool_error\"), reason: (__inner.reason || null), remediation: (__inner.remediation || null), error: (__inner.error || null)});".to_string(),
        "} catch (__e) {".to_string(),
        "  return JSON.stringify({ok: false, error: String((__e && __e.message) || __e)});".to_string(),
        "}".to_string(),
    ]);
    lines.join("\n") + "\n"
}

/// Map the script envelope to a run outcome or a typed error, using the
/// same codes agent-browser reports. A failing child leaves no JSON: its
/// stderr names the failure (`navigation failed` is unreachable).
fn map_output(
    stdout: &str,
    stderr: &str,
    req: &CallRequest,
    started: std::time::Instant,
    total_ms: u64,
) -> Result<RunOutcome, CallError> {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        if val
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            let raw_output = val.get("output").cloned().unwrap_or_default();
            if raw_output
                .get("isError")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                return Err(CallError {
                    code: "tool_error",
                    message: super::agent_browser::tool_error_text(&raw_output),
                    data: Some(raw_output),
                });
            }
            let output = wrap_output(raw_output);
            let used_native = val
                .get("used_native")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            return Ok(RunOutcome {
                output,
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                reused: false,
                budget_ms: total_ms,
                used_native,
            });
        }
        match val.get("code").and_then(serde_json::Value::as_str) {
            Some("unsupported") => return Err(webmcp_unsupported("undefined")),
            Some("not_found") => return Err(tool_not_found(&req.tool)),
            Some("needs_headed") => {
                return Err(CallError {
                    code: "needs_headed",
                    message: "tool requires a headed browser; lightpanda is headless-only"
                        .to_string(),
                    data: None,
                });
            }
            Some("native_denied") => {
                let native = val
                    .get("native")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                return Err(native_denied(native));
            }
            Some("native_needs_confirmation") => {
                let native = val
                    .get("native")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                return Err(native_needs_confirmation(native));
            }
            Some("ambiguous_tool") => {
                let frames = val.get("frames").and_then(serde_json::Value::as_u64);
                return Err(CallError {
                    code: "ambiguous_tool",
                    message: match frames {
                        Some(n) => format!(
                            "more than one frame may provide {} ({} subframes); refusing to substitute the native tool",
                            req.tool, n
                        ),
                        None => format!(
                            "more than one frame may provide {}; refusing to substitute the native tool",
                            req.tool
                        ),
                    },
                    data: Some(val.clone()),
                });
            }
            Some("blocked") => {
                return Err(CallError {
                    code: "page_unreachable",
                    message: "the page shows a bot check; it was not solved".to_string(),
                    data: None,
                });
            }
            Some("origin_mismatch") => {
                let error = val
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("page origin does not match the custom tool host");
                return Err(CallError {
                    code: "origin_mismatch",
                    message: error.to_string(),
                    data: Some(val.clone()),
                });
            }
            Some("unavailable") => {
                let reason = val
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unavailable");
                let remediation = val
                    .get("remediation")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                return Err(CallError {
                    code: "unavailable",
                    message: format!("{} is unavailable: {reason}", req.tool),
                    data: Some(serde_json::json!({
                        "reason": reason,
                        "remediation": remediation,
                    })),
                });
            }
            _ => {}
        }
        let error = val
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if error.contains("No WebMCP tool named") {
            return Err(tool_not_found(&req.tool));
        }
        return Err(CallError {
            code: "tool_error",
            message: if error.is_empty() {
                "the tool reported an error".to_string()
            } else {
                error.to_string()
            },
            data: None,
        });
    }
    let haystack = format!("{stdout}\n{stderr}").to_lowercase();
    if haystack.contains("navigation failed") || haystack.contains("navigation timed out") {
        return Err(CallError {
            code: "page_unreachable",
            message: "could not open the page".to_string(),
            data: None,
        });
    }
    Err(CallError {
        code: "driver_error",
        message: "lightpanda printed something that is not JSON".to_string(),
        data: Some(serde_json::json!({
            "stdout": stdout.chars().take(200).collect::<String>(),
            "stderr": stderr.chars().take(500).collect::<String>(),
        })),
    })
}

/// Tool output already shaped like MCP content passes through; a plain
/// string becomes one text block so envelopes match agent-browser.
fn wrap_output(output: serde_json::Value) -> serde_json::Value {
    if output
        .get("content")
        .and_then(serde_json::Value::as_array)
        .is_some()
    {
        return output;
    }
    match output {
        serde_json::Value::String(text) => {
            serde_json::json!({"content": [{"type": "text", "text": text}]})
        }
        other => other,
    }
}

fn render_inspect_script(url: &str, timeout_ms: u64, settle_ms: u64) -> String {
    let url = serde_json::to_string(url).unwrap_or_default();
    let mut lines = vec![
        "const page = new Page();".to_string(),
        format!("await page.goto({url}, {{waitUntil: \"load\", timeout: {timeout_ms}}});"),
    ];
    lines.extend(vec![
        "const __t0 = Date.now();".to_string(),
        "let __tools = [];".to_string(),
        "let __mcMissing = false;".to_string(),
        "for (;;) {".to_string(),
        "  const __mc = await page.evaluate(\"typeof navigator.modelContext\");".to_string(),
        "  if (__mc === \"undefined\") { __mcMissing = true; __tools = []; break; }".to_string(),
        concat!(
            "  const __list = await page.evaluate(\"",
            "navigator.modelContext.getTools().then(function(ts){",
            "return ts.map(function(t){",
            "return {name:t.name,description:t.description,inputSchema:t.inputSchema,annotations:t.annotations}",
            "})",
            "})",
            "\");"
        )
            .to_string(),
        "  __tools = JSON.parse(__list || \"[]\");".to_string(),
        "  if (__tools.length > 0) break;".to_string(),
        format!("  if (Date.now() - __t0 >= {settle_ms}) break;"),
        "  await page.evaluate(\"new Promise(function(__r){setTimeout(__r, 300)})\");".to_string(),
        "}".to_string(),
    ]);
    // Tools first: report a genuine interstitial as blocked, but never
    // reject a page that registered tools; without a modelContext API the
    // verdict is `unsupported` as before.
    lines.extend(blocked_check_unless("__tools.length === 0"));
    lines.extend(vec![
        "if (__mcMissing) return JSON.stringify({ok: false, code: \"unsupported\"});".to_string(),
        "return JSON.stringify({ok: true, tools: __tools});".to_string(),
    ]);
    lines.join("\n") + "\n"
}

/// Inspect script that also probes headless custom tools via
/// `render_avail_promise_expr`. Returns
/// `{ok:true, tools:[...native...], custom:{name:{ok,...}}, customOriginMismatch:bool}`.
fn render_custom_inspect_script(
    url: &str,
    runnable: &[CustomInspect],
    timeout_ms: u64,
    settle_ms: u64,
    custom_configured: bool,
) -> String {
    if runnable.is_empty() {
        if custom_configured {
            // Customs are configured but none can run on this driver (all
            // headed or session-auth). Report an empty native catalog so
            // the merge reports needs_headed/needs_session instead of
            // failing with `unsupported` on pages without a native API.
            return "return JSON.stringify({ok: true, tools: [], custom: {}, customOriginMismatch: false});"
                .to_string();
        }
        return render_inspect_script(url, timeout_ms, settle_ms);
    }
    let groups = group_custom_inspects(runnable.iter());
    let url_json = serde_json::to_string(url).unwrap_or_default();
    let mut lines = vec![
        "const page = new Page();".to_string(),
        format!("await page.goto({url_json}, {{waitUntil: \"load\", timeout: {timeout_ms}}});"),
    ];
    lines.extend(vec![
        "const __t0 = Date.now();".to_string(),
        "let __tools = [];".to_string(),
        "for (;;) {".to_string(),
        "  const __mc = await page.evaluate(\"typeof navigator.modelContext\");".to_string(),
        "  if (__mc === \"undefined\") { __tools = []; break; }".to_string(),
        concat!(
            "  const __list = await page.evaluate(\"",
            "navigator.modelContext.getTools().then(function(ts){",
            "return ts.map(function(t){",
            "return {name:t.name,description:t.description,inputSchema:t.inputSchema,annotations:t.annotations}",
            "})",
            "})",
            "\");"
        )
        .to_string(),
        "  __tools = JSON.parse(__list || \"[]\");".to_string(),
        "  if (__tools.length > 0) break;".to_string(),
        format!("  if (Date.now() - __t0 >= {settle_ms}) break;"),
        "  await page.evaluate(\"new Promise(function(__r){setTimeout(__r, 300)})\");".to_string(),
        "}".to_string(),
    ]);
    // Tools first: the interstitial verdict only applies when no native
    // tool registered, so custom availability probes still run on working
    // pages that embed a challenge widget.
    lines.extend(blocked_check_unless("__tools.length === 0"));
    // Native names discovered above: a group whose every tool is shadowed
    // by a live native registration is skipped outright, and mixed groups
    // probe only the unshadowed names — a shadowed tool's `available()`
    // never runs for a verdict Rust would drop anyway. The probe runs via
    // `page.evaluate`, i.e. in the page, where PandaScript locals do not
    // exist: so the filtered names cross as data, not as identifiers. The
    // template carries a placeholder literal that PandaScript replaces with
    // the serialized filtered array before evaluating.
    lines.push("const __nativeNames = __tools.map(function(__t){ return __t.name; });".to_string());
    // Host-side origin read for the per-group gates below: compared with
    // `!==` in this outer context (no page-mutable methods), so a mismatched
    // page never receives a custom availability probe. The in-page wrapper
    // guard remains as defense in depth.
    lines.push("const __cOrigin = await page.evaluate(\"location.origin\");".to_string());
    lines.extend(vec![
        "let __custom = {};".to_string(),
        "let __customOriginMismatch = false;".to_string(),
    ]);
    for (group_idx, ((expected_origin, js_source), tools)) in groups.into_iter().enumerate() {
        let refs: Vec<&str> = tools.iter().map(|c| c.name.as_str()).collect();
        let names_json = serde_json::to_string(&refs).unwrap_or_default();
        // Placeholder literal replaced in PandaScript (below) with the
        // serialized filtered array. It must not occur in the tool script
        // itself, or the replace would corrupt custom code: in that
        // vanishingly unlikely case, probe the baked full list instead
        // (evaluating a shadowed tool beats running corrupted code).
        let token = format!("__awm_names_{group_idx}__");
        let (probe_names_js, literal_probe) = if js_source.contains(&token) {
            (names_json.clone(), true)
        } else {
            (format!("\"{token}\""), false)
        };
        let expr = crate::tools::render_avail_promise_expr_names_js(
            &expected_origin,
            &js_source,
            &probe_names_js,
        );
        let expr_json = serde_json::to_string(&expr).unwrap_or_default();
        let group_expected_json = serde_json::to_string(&*expected_origin).unwrap_or_default();
        let unfiltered = format!("__un{group_idx}");
        lines.push(format!(
            "const {unfiltered} = {names_json}.filter(function(__n){{ return __nativeNames.indexOf(__n) < 0; }});"
        ));
        lines.push(format!(
            "if (__cOrigin !== {group_expected_json}) {{ __customOriginMismatch = true; }}"
        ));
        if literal_probe {
            lines.push(format!("else if ({unfiltered}.length) {{ try {{"));
        } else {
            // The template's placeholder is a quoted literal; swap the whole
            // quoted token for the serialized array so the page evaluates
            // `const __names = ["a", ...];` with no free identifiers.
            lines.push(format!("else if ({unfiltered}.length) {{ try {{"));
            lines.push(format!(
                "  const __probe{group_idx} = {expr_json}.replace('\"{token}\"', JSON.stringify({unfiltered}));"
            ));
        }
        lines.push(format!(
            "  const __s = await page.evaluate({});",
            if literal_probe {
                expr_json.clone()
            } else {
                format!("__probe{group_idx}")
            }
        ));
        lines.push("  const __inner = JSON.parse(__s || \"{}\");".to_string());
        lines.push("  if (__inner.ok && __inner.tools) {".to_string());
        lines.push(
            "    for (const __k in __inner.tools) { __custom[__k] = __inner.tools[__k]; }"
                .to_string(),
        );
        lines.push(
            "  } else if (!__inner.ok && __inner.code === \"origin_mismatch\") {".to_string(),
        );
        lines.push("    __customOriginMismatch = true;".to_string());
        // Any other wrapper failure (tool_error, unknown_tool, malformed
        // envelope) emits an explicit unavailable entry per probed tool:
        // without this the group vanishes from __custom and a *different*
        // group's origin mismatch could misattribute the failure.
        lines.push("  } else if (!__inner.ok) {".to_string());
        lines.push(format!(
            "    for (const __k of {unfiltered}) {{ __custom[__k] = {{ok: false, reason: (__inner.code || \"eval failed\"), remediation: (__inner.error || null)}}; }}"
        ));
        lines.push("  }".to_string());
        lines.push(format!(
            "}} catch (__e) {{ for (const __k of {unfiltered}) {{ __custom[__k] = {{ok: false, reason: \"eval failed\", remediation: String((__e && __e.message) || __e)}}; }} }}"
        ));
        lines.push("}".to_string());
    }
    lines.push(
        "return JSON.stringify({ok: true, tools: __tools, custom: __custom, customOriginMismatch: __customOriginMismatch});"
            .to_string(),
    );
    lines.join("\n") + "\n"
}

fn map_inspect_output(
    stdout: &str,
    stderr: &str,
    url: &str,
    started: std::time::Instant,
    total_ms: u64,
) -> Result<InspectOutcome, CallError> {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        if val
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            let tools = parse_inspected_tools(&val, "/tools");
            return Ok(InspectOutcome {
                page: url.to_string(),
                tools,
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                reused: false,
                budget_ms: total_ms,
            });
        }
        if val.get("code").and_then(serde_json::Value::as_str) == Some("unsupported") {
            return Err(webmcp_unsupported("undefined"));
        }
        if val.get("code").and_then(serde_json::Value::as_str) == Some("blocked") {
            return Err(CallError {
                code: "page_unreachable",
                message: "the page shows a bot check; it was not solved".to_string(),
                data: None,
            });
        }
        let error = val
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("lightpanda inspect failed");
        return Err(CallError {
            code: "driver_error",
            message: error.to_string(),
            data: None,
        });
    }
    let haystack = format!("{stdout}\n{stderr}").to_lowercase();
    if haystack.contains("navigation failed") || haystack.contains("navigation timed out") {
        return Err(CallError {
            code: "page_unreachable",
            message: "could not open the page".to_string(),
            data: None,
        });
    }
    Err(CallError {
        code: "driver_error",
        message: "lightpanda printed something that is not JSON".to_string(),
        data: Some(serde_json::json!({
            "stdout": stdout.chars().take(200).collect::<String>(),
            "stderr": stderr.chars().take(500).collect::<String>(),
        })),
    })
}

#[cfg(test)]
mod tests {
    use super::blocked_check_lines;
    use super::render_custom_script;

    /// Challenge DOM markers gate the blocked verdict.
    #[test]
    fn blocked_check_probes_challenge_dom_markers() {
        let script = blocked_check_lines().join("\n");
        for marker in [
            "#challenge-form",
            "#cf-challenge-running",
            "challenges.cloudflare.com",
        ] {
            assert!(script.contains(marker), "{script}");
        }
    }

    /// Body phrases only count in a short interstitial body: an ordinary
    /// long article discussing e.g. "verify you are human" must not match.
    #[test]
    fn blocked_check_gates_body_phrases_behind_short_body() {
        let script = blocked_check_lines().join("\n");
        assert!(script.contains("length < 500"), "{script}");
        assert!(
            script.contains("__bShort && __bBodyHit"),
            "body phrases must require the short-body gate: {script}"
        );
        for phrase in [
            "verify you are human",
            "are you a robot",
            "bot challenge",
            "captcha",
        ] {
            assert!(script.contains(phrase), "{script}");
        }
    }

    /// Challenge titles alone suffice; URLs never participate: a phrase in
    /// the URL (or a long body) alone must not block.
    #[test]
    fn blocked_check_uses_titles_not_urls() {
        let script = blocked_check_lines().join("\n");
        assert!(!script.contains("location.href"), "{script}");
        assert!(!script.contains("__bHref"), "{script}");
        for title in [
            "just a moment",
            "attention required",
            "access denied",
            "robot or human",
        ] {
            assert!(script.contains(title), "{script}");
        }
    }

    /// A challenge widget iframe alone never blocks: the verdict needs a
    /// challenge title or a short interstitial body mentioning checks.
    #[test]
    fn blocked_check_widget_alone_does_not_block() {
        let script = blocked_check_lines().join("\n");
        let verdict = script
            .lines()
            .find(|l| l.contains("code: \"blocked\""))
            .expect("blocked verdict line");
        assert!(
            !verdict.contains("__bDom !== '' ||"),
            "bare DOM signal must not block: {verdict}"
        );
        assert!(
            verdict.contains("__bDom !== '' && __bShort"),
            "widget counts only on an interstitial-shaped page: {verdict}"
        );
    }

    /// Inspect probes skip native-shadowed tools: the script learns the
    /// live native names first, then evaluates a group only for its
    /// unshadowed members — a shadowed tool's `available()` never runs for
    /// a verdict Rust would drop anyway. The filtered names cross into the
    /// page as data (a quoted placeholder swapped for the serialized
    /// array): a bare PandaScript identifier must never reach
    /// page-evaluated code, where it would be a `ReferenceError`.
    #[test]
    fn custom_inspect_script_skips_shadowed_tools() {
        use super::render_custom_inspect_script;
        let mk = |name: &str| super::super::CustomInspect {
            name: name.to_string(),
            host: "example.com".to_string(),
            common: super::super::CustomCommon {
                expected_origin: "https://example.com".to_string(),
                js_source: "__webmcpCustom({});".into(),
                tool_hash: "abc".to_string(),
                headed: false,
                allow_browse: true,
                input_schema: serde_json::json!({}),
            },
            auth: crate::tools::AuthMode::None,
            description: "d".to_string(),
        };
        let runnable = vec![mk("get_item"), mk("other")];
        let script =
            render_custom_inspect_script("https://example.com/", &runnable, 1000, 500, true);
        assert!(script.contains("__nativeNames"), "{script}");
        assert!(script.contains("__un0"), "{script}");
        assert!(
            script.contains(".replace('\"__awm_names_0__\"', JSON.stringify(__un0))"),
            "{script}"
        );
        for line in script.lines().filter(|l| l.contains("page.evaluate(")) {
            assert!(
                !line.contains("__un0"),
                "page-evaluated code must not reference PandaScript locals: {line}"
            );
        }
    }

    /// Discovery and dispatch share one document: a single `page.goto`,
    /// with the settle poll, policy branches, and custom evaluation inline.
    /// Invalid input and headed tools are gated before the custom script
    /// runs, and the native policy decides before any invoke.
    #[test]
    fn custom_script_unifies_discovery_and_dispatch() {
        let req = super::CallRequest {
            page: "https://example.com/app".to_string(),
            tool: "get_item".to_string(),
            input: serde_json::json!({}),
            custom: None,
        };
        let custom = || super::super::CustomExec {
            host: "example.com".to_string(),
            common: super::super::CustomCommon {
                expected_origin: "https://example.com".to_string(),
                js_source: "__webmcpCustom({});".into(),
                tool_hash: "abc".to_string(),
                input_schema: serde_json::json!({}),
                headed: false,
                allow_browse: true,
            },
            auth: crate::tools::AuthMode::None,
            dir: std::path::PathBuf::from("/tmp/x"),
            native_policy: super::NativeCollisionPolicy::Allow,
        };
        let opts = || super::RunOptions {
            timeout_ms: 1000,
            settle_ms: 500,
            fresh: false,
        };
        // Allow: one goto, host-side origin gate, settle poll, origin-bound
        // invoke, custom eval.
        let script =
            render_custom_script(&req, &custom(), &opts(), 500, true, "example.com/get_item");
        assert_eq!(script.matches("page.goto").count(), 1, "{script}");
        assert_eq!(script.matches("new Page()").count(), 1, "{script}");
        assert!(script.contains("navigator.modelContext"), "{script}");
        assert!(script.contains("callPageTool"), "{script}");
        assert!(script.contains("origin_mismatch"), "{script}");
        assert!(script.contains("__webmcpCustom"), "{script}");
        assert!(!script.contains("native_denied"), "{script}");
        assert!(!script.contains("input_invalid"), "{script}");
        // Host-side final-origin check before anything page-bound runs:
        // strict comparison, no page-mutable methods.
        let gate = script
            .find("const __cOrigin = await page.evaluate(\"location.origin\");")
            .expect("host-side origin read");
        let goto = script.find("page.goto").unwrap();
        let settle = script.find("Date.now() - __t0").unwrap();
        assert!(goto < gate && gate < settle, "{script}");
        let cmp = script.find("__cOrigin !==").expect("strict origin compare");
        assert!(cmp > gate, "{script}");
        assert!(
            !script.contains("__cOrigin ||"),
            "no method-based origin fallback chain: {script}"
        );
        // Final binding immediately before dispatch: the last origin
        // re-verification precedes the native invoke and custom
        // evaluation, with no page contact in between.
        let final_gate = script
            .find("const __cOriginF = await page.evaluate(\"location.origin\");")
            .expect("pre-dispatch origin recheck");
        let call_pos = script.find("callPageTool").unwrap();
        let eval_pos = script.find("const __customStr").unwrap();
        assert!(final_gate < call_pos, "{script}");
        assert!(final_gate < eval_pos, "{script}");
        assert!(
            script[final_gate..eval_pos].contains("__cOriginF !=="),
            "strict recheck before delivery: {script}"
        );
        // Deny: signal when native is present, never invoke.
        let mut deny = custom();
        deny.native_policy = super::NativeCollisionPolicy::Deny;
        let script = render_custom_script(&req, &deny, &opts(), 500, true, "example.com/get_item");
        assert!(script.contains("native_denied"), "{script}");
        assert!(!script.contains("callPageTool"), "{script}");
        // Ask: confirmation signal instead of an invoke.
        let mut ask = custom();
        ask.native_policy = super::NativeCollisionPolicy::Ask;
        let script = render_custom_script(&req, &ask, &opts(), 500, true, "example.com/get_item");
        assert!(script.contains("native_needs_confirmation"), "{script}");
        assert!(!script.contains("callPageTool"), "{script}");
        // Invalid input gates the custom evaluation behind the native check.
        let script =
            render_custom_script(&req, &custom(), &opts(), 500, false, "example.com/get_item");
        assert!(script.contains("input_invalid"), "{script}");
        let gate = script.find("input_invalid").unwrap();
        let eval_pos = script.find("__webmcpCustom").unwrap();
        assert!(gate < eval_pos, "{script}");
        // Headed tools report before evaluating.
        let mut headed = custom();
        headed.common.headed = true;
        let script =
            render_custom_script(&req, &headed, &opts(), 500, true, "example.com/get_item");
        assert!(script.contains("needs_headed"), "{script}");
        let gate = script.find("needs_headed").unwrap();
        let eval_pos = script.find("__webmcpCustom").unwrap();
        assert!(gate < eval_pos, "{script}");
        // The interstitial verdict runs after discovery and only when
        // nothing registered, so an embedded widget on a working page
        // never rejects.
        let settle = script.find("Date.now() - __t0").unwrap();
        let blocked = script.find("code: \"blocked\"").unwrap();
        assert!(settle < blocked, "{script}");
        let gate = script.find("if (__names.length === 0)").unwrap();
        assert!(gate < blocked, "{script}");
    }

    /// Session-authenticated tools never navigate on lightpanda: the driver
    /// runs a fresh sessionless browser with no request interception, so it
    /// can neither supply the session nor guard redirect hops (CWE-319).
    /// The refusal happens before any PandaScript is built or run.
    #[test]
    fn session_auth_custom_refuses_before_navigation() {
        let driver = super::Lightpanda {
            command: "lightpanda-test-unreachable".to_string(),
        };
        let req = super::CallRequest {
            page: "https://example.com/app".to_string(),
            tool: "get_item".to_string(),
            input: serde_json::json!({}),
            custom: None,
        };
        let custom = super::super::CustomExec {
            host: "example.com".to_string(),
            common: super::super::CustomCommon {
                expected_origin: "https://example.com".to_string(),
                js_source: "__webmcpCustom({});".into(),
                tool_hash: "abc".to_string(),
                input_schema: serde_json::json!({}),
                headed: false,
                allow_browse: true,
            },
            auth: crate::tools::AuthMode::Session,
            dir: std::path::PathBuf::from("/tmp/x"),
            native_policy: super::NativeCollisionPolicy::Allow,
        };
        let opts = super::RunOptions {
            timeout_ms: 1000,
            settle_ms: 500,
            fresh: false,
        };
        let started = std::time::Instant::now();
        let err = driver
            .run_custom(&req, &custom, &opts, 500, started, 10_000)
            .unwrap_err();
        assert_eq!(err.code, "needs_session", "{err:?}");
        assert!(err.message.contains("agent-browser"), "{err:?}");
    }
}
