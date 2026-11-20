//! OpenCode Browse: drives the user's real Chrome through the `browse` CLI.

use super::agent_browser::is_blocked_page;
use super::exec::{
    CallBudget, ChildOutput, parse_version, run_command_raw, run_probe, tool_not_found,
    webmcp_unsupported,
};
use super::{
    CallRequest, Detection, Driver, InspectOutcome, InspectedTool, Kind, Plan, RunOptions,
    RunOutcome, Step, parse_inspected_tools,
};
use crate::call::CallError;
use crate::config::Config;

const INSTALL: &str = "install browse and pair your browser extension (browse install)";

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

    fn run_inner(&self, req: &CallRequest, opts: &RunOptions) -> Result<RunOutcome, CallError> {
        let started = std::time::Instant::now();
        let settle_ms = opts.settle_ms.min(30_000);
        let budgets = CallBudget::new(opts.timeout_ms, settle_ms, started);

        let nav_args = serde_json::json!({
            "url": req.page,
            "group": "agent-webmcp",
            "timeout": budgets.timeout_ms,
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

        let outcome = self.run_on_tab(req, opts, &tab, &nav_out, &budgets, settle_ms, started);
        let timed_out = matches!(&outcome, Err(e) if e.code == "timeout");
        if !timed_out
            && opts.fresh
            && let Ok(ms) = budgets.phase(10_000, "close")
        {
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

        if settle_ms > 0 {
            self.wait_for_tool(req, tab, budgets, settle_ms)?;
        }

        let call_args = serde_json::json!({
            "action": "call",
            "tab": tab,
            "name": req.tool,
            "input": req.input,
            "timeout": budgets.timeout_ms,
        });
        let call_cap = budgets.timeout_ms.saturating_add(5_000);
        let res = run_browse_json(
            &self.command,
            "webmcp",
            &call_args,
            budgets.phase(call_cap, "invoke")?,
            "invoke",
        )?;
        let output = map_call_result(res, &req.tool)?;
        Ok(RunOutcome {
            output,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reused: false,
            budget_ms: budgets.total_ms,
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

    fn inspect_inner(&self, url: &str, opts: &RunOptions) -> Result<InspectOutcome, CallError> {
        let started = std::time::Instant::now();
        let settle_ms = opts.settle_ms.min(30_000);
        let budgets = CallBudget::new(opts.timeout_ms, settle_ms, started);

        let nav_args = serde_json::json!({
            "url": url,
            "group": "agent-webmcp",
            "timeout": budgets.timeout_ms,
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

        let outcome = self.inspect_on_tab(url, &tab, &nav_out, &budgets, settle_ms, started);
        let timed_out = matches!(&outcome, Err(e) if e.code == "timeout");
        if !timed_out
            && opts.fresh
            && let Ok(ms) = budgets.phase(10_000, "close")
        {
            let close_args = serde_json::json!({"action": "close", "tab": tab});
            let _ = run_browse_json(&self.command, "tabs", &close_args, ms, "close");
        }
        outcome
    }

    fn inspect_on_tab(
        &self,
        url: &str,
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

        let tools = self.poll_tools(tab, budgets, settle_ms)?;
        Ok(InspectOutcome {
            page: page_url,
            tools,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reused: false,
            budget_ms: budgets.total_ms,
        })
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

    fn inspect(&self, url: &str, opts: &RunOptions) -> Result<InspectOutcome, CallError> {
        self.inspect_inner(url, opts)
    }

    fn plan(&self, req: &CallRequest) -> Plan {
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
    if let Some(result) = res.get("result") {
        return Ok(result.clone());
    }
    Ok(res)
}
