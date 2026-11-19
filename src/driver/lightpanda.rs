//! Lightpanda: the lighter run driver, over `lightpanda run` PandaScript.
//!
//! Verified against the real binary: `page.goto`, `page.evaluate` (promise
//! results are awaited and come back stringified) and
//! `page.callPageTool({name, arguments})` invoke a tool. `page.pageTools()`
//! is not a script method, so the script lists tools through
//! `navigator.modelContext.getTools()` instead; sleeps use an in-page
//! promise because PandaScript has no `setTimeout`.

use super::agent_browser::{
    parse_version, run_command_raw, run_version_arg, tool_not_found, webmcp_unsupported, write_temp,
};
use super::{CallRequest, Detection, Driver, Kind, Plan, RunOptions, RunOutcome, Step};
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
        let file = write_temp(
            "lightpanda",
            &render_script(req, opts.timeout_ms, settle_ms),
        )?;
        let argv = vec!["run".to_string(), file.path.display().to_string()];
        let out = run_command_raw(&self.command, &argv, total_ms, "run")?;
        map_output(&out.stdout, &out.stderr, req, started, total_ms)
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

    fn plan(&self, req: &CallRequest) -> Plan {
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

/// The PandaScript: goto, settle-poll for the tool, call it. URL, tool, and
/// input go in as JSON literals, never raw string concatenation. `evaluate`
/// cannot run top-level `await`, so async page calls use `.then()` chains.
fn render_script(req: &CallRequest, timeout_ms: u64, settle_ms: u64) -> String {
    let url = serde_json::to_string(&req.page).unwrap_or_default();
    let tool = serde_json::to_string(&req.tool).unwrap_or_default();
    let input = serde_json::to_string(&req.input).unwrap_or_else(|_| "{}".to_string());
    [
        "const page = new Page();".to_string(),
        format!("await page.goto({url}, {{waitUntil: \"load\", timeout: {timeout_ms}}});"),
        "const __t0 = Date.now();".to_string(),
        "let __names = [];".to_string(),
        "for (;;) {".to_string(),
        "  const __mc = await page.evaluate(\"typeof navigator.modelContext\");".to_string(),
        "  if (__mc === \"undefined\") return JSON.stringify({ok: false, code: \"unsupported\"});"
            .to_string(),
        // evaluate hands arrays back JSON-encoded, so parse instead of
        // joining: a "\n" here would land as a raw newline in page code.
        "  const __list = await page.evaluate(\"navigator.modelContext.getTools().then(function(ts){return ts.map(function(t){return t.name})})\");"
            .to_string(),
        "  __names = JSON.parse(__list || \"[]\");".to_string(),
        format!("  if (__names.indexOf({tool}) !== -1) break;"),
        format!("  if (Date.now() - __t0 >= {settle_ms}) break;"),
        "  await page.evaluate(\"new Promise(function(__r){setTimeout(__r, 300)})\");".to_string(),
        "}".to_string(),
        format!(
            "if (__names.indexOf({tool}) === -1) return JSON.stringify({{ok: false, code: \"not_found\"}});"
        ),
        "try {".to_string(),
        format!(
            "  const __out = await page.callPageTool({{name: {tool}, arguments: {input}}});"
        ),
        "  return JSON.stringify({ok: true, output: (typeof __out === \"undefined\") ? null : __out});"
            .to_string(),
        "} catch (__e) {".to_string(),
        "  return JSON.stringify({ok: false, error: String((__e && __e.message) || __e)});"
            .to_string(),
        "}".to_string(),
    ]
    .join("\n")
        + "\n"
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
            let output = wrap_output(val.get("output").cloned().unwrap_or_default());
            return Ok(RunOutcome {
                output,
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                reused: false,
                budget_ms: total_ms,
            });
        }
        match val.get("code").and_then(serde_json::Value::as_str) {
            Some("unsupported") => return Err(webmcp_unsupported("undefined")),
            Some("not_found") => return Err(tool_not_found(&req.tool)),
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
