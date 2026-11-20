//! `agent-webmcp inspect`: open a live URL and list the WebMCP tools it registers.

use serde_json::{Value, json};

use crate::call::{CallError, ToolKind, example_input, inferred_read, no_driver};
use crate::config::Config;
use crate::driver::{
    self, Choice, InspectOutcome as DriverInspectOutcome, InspectedTool, RunOptions,
};

/// Options for `run_inspect`.
#[derive(Debug, Clone, Default)]
pub struct InspectOptions {
    pub fresh: bool,
}

/// Successful `inspect` output ready for human or JSON formatting.
#[derive(Debug, Clone)]
pub struct InspectResult {
    pub driver: &'static str,
    pub page: String,
    pub tools: Vec<InspectedTool>,
    pub duration_ms: u64,
    pub reused: bool,
    pub budget_ms: u64,
}

impl InspectResult {
    fn from_driver(driver: &'static str, out: DriverInspectOutcome) -> Self {
        Self {
            driver,
            page: out.page,
            tools: out.tools,
            duration_ms: out.duration_ms,
            reused: out.reused,
            budget_ms: out.budget_ms,
        }
    }

    pub fn envelope(&self) -> Value {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|t| {
                let (kind, kind_inferred) = tool_kind(t);
                let kind_str = match kind {
                    ToolKind::Read => "read",
                    ToolKind::Consequential => "consequential",
                    ToolKind::Unspecified => "unspecified",
                };
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                    "kind": kind_str,
                    "kind_inferred": kind_inferred,
                    "call": {
                        "target": format!("{}#{}", self.page, t.name),
                        "input": example_input(&t.input_schema),
                    }
                })
            })
            .collect();
        json!({
            "success": true,
            "data": {
                "driver": self.driver,
                "page": self.page,
                "durationMs": self.duration_ms,
                "reused": self.reused,
                "budgetMs": self.budget_ms,
                "tools": tools,
            }
        })
    }
}

/// Normalize a user-supplied URL or bare host (`cal.com` -> `https://cal.com/`).
pub fn normalize_url(raw: &str) -> Result<String, CallError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CallError::usage("expected a URL or host to inspect"));
    }
    if trimmed.contains(char::is_whitespace) {
        return Err(CallError::usage(format!("invalid URL {raw:?}")));
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        let host_prefix = trimmed.split(['/', '?', '#']).next().unwrap_or("");
        if let Some((_, after_colon)) = host_prefix.split_once(':')
            && !after_colon.chars().all(|c| c.is_ascii_digit())
        {
            return Err(CallError::usage(format!(
                "unsupported URL scheme in {raw:?}; expected http or https"
            )));
        }
        let host_only = host_prefix.split(':').next().unwrap_or("");
        if host_only.eq_ignore_ascii_case("localhost") {
            format!("http://{trimmed}")
        } else {
            format!("https://{trimmed}")
        }
    };
    let Some((scheme, rest)) = with_scheme.split_once("://") else {
        return Err(CallError::usage(format!("invalid URL {raw:?}")));
    };
    if scheme != "http" && scheme != "https" {
        return Err(CallError::usage(format!(
            "unsupported URL scheme {scheme:?}; expected http or https"
        )));
    }
    let before_query = rest.split(['?', '#']).next().unwrap_or("");
    let host_part = before_query.split('/').next().unwrap_or("");
    if host_part.is_empty() {
        return Err(CallError::usage(format!("missing host in URL {raw:?}")));
    }
    if !before_query.contains('/') {
        let split_at = scheme.len() + 3 + before_query.len();
        let (head, tail) = with_scheme.split_at(split_at);
        Ok(format!("{head}/{tail}"))
    } else {
        Ok(with_scheme)
    }
}

/// Open `raw_url` with the configured run driver and list registered WebMCP tools.
pub fn run_inspect(
    raw_url: &str,
    opts: &InspectOptions,
    cfg: &Config,
) -> Result<InspectResult, CallError> {
    let url = normalize_url(raw_url)?;
    match driver::choose(cfg) {
        Choice::Run(driver, _) => {
            let out = driver.inspect(
                &url,
                &RunOptions {
                    timeout_ms: cfg.call.timeout,
                    settle_ms: cfg.call.settle_ms.min(30_000),
                    fresh: opts.fresh,
                },
            )?;
            Ok(InspectResult::from_driver(driver.id(), out))
        }
        Choice::Plan(_) | Choice::Unusable { .. } => Err(no_driver(cfg)),
    }
}

/// Print human-readable `inspect` output.
pub fn print_human(res: &InspectResult) {
    let page = crate::text::clean(&res.page);
    if res.tools.is_empty() {
        println!(
            "no WebMCP tools registered on {page} (via {}, {} ms)",
            res.driver, res.duration_ms
        );
        return;
    }
    println!(
        "{page} ({} {}, via {}, {} ms)",
        res.tools.len(),
        if res.tools.len() == 1 {
            "tool"
        } else {
            "tools"
        },
        res.driver,
        res.duration_ms
    );
    for tool in &res.tools {
        let (kind, inferred) = tool_kind(tool);
        let tag = match (kind, inferred) {
            (ToolKind::Read, true) => "[read, inferred]",
            (ToolKind::Read, false) => "[read]",
            (ToolKind::Consequential, _) => "[consequential]",
            (ToolKind::Unspecified, _) => "[unspecified]",
        };
        let name = crate::text::clean(&tool.name);
        println!("{name}  {tag}");
        if let Some(desc) = &tool.description {
            println!("    {}", crate::text::clean(desc));
        }
        let ex = serde_json::to_string(&example_input(&tool.input_schema))
            .unwrap_or_else(|_| "{}".to_string());
        println!(
            "    call agent-webmcp call {page}#{name} '{}'",
            crate::text::clean(&ex)
        );
    }
}

fn tool_kind(tool: &InspectedTool) -> (ToolKind, bool) {
    match tool.read_only {
        Some(true) => (ToolKind::Read, false),
        Some(false) => (ToolKind::Consequential, false),
        None if inferred_read(&tool.name) => (ToolKind::Read, true),
        None => (ToolKind::Unspecified, false),
    }
}
