//! Generic fallback: plain-language steps for whatever driver the agent has.

use super::{CallRequest, Detection, Driver, Kind, Plan, Step};
use crate::config::Config;

/// Plan driver: used for `driver: none` or when nothing usable is installed.
pub struct Generic;

impl Driver for Generic {
    fn id(&self) -> &'static str {
        "generic"
    }

    fn kind(&self) -> Kind {
        Kind::Plan
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        Detection::Unknown
    }

    fn note(&self) -> Option<&'static str> {
        Some(
            "no browser driver configured; set driver in agent-webmcp.json or install agent-browser",
        )
    }

    fn plan(&self, req: &CallRequest) -> Plan {
        if let Some(c) = &req.custom {
            let short = c.common.tool_hash.chars().take(12).collect::<String>();
            let target = format!("{}#{}", req.page, req.tool);
            // No run driver is configured, so nothing here can register
            // the local script: a manual `call` step would fail against
            // the unregistered name. Point at a run driver and hand the
            // guarded CLI workflow instead; `--params @-` keeps the input
            // out of process arguments. An `http://` page needs `--fresh`
            // (the runtime refuses cleartext custom pages in persistent
            // sessions), so recommend a command agent-browser accepts.
            let mut command = vec![
                "agent-webmcp".to_string(),
                "call".to_string(),
                "--driver".to_string(),
                "agent-browser".to_string(),
            ];
            if is_http_url(&req.page) {
                command.push("--fresh".to_string());
            }
            command.extend([target, "--params".to_string(), "@-".to_string()]);
            return Plan {
                driver: self.id(),
                note: self.note().map(str::to_string),
                steps: vec![Step {
                    tool: "shell".to_string(),
                    args: serde_json::json!({"command": command}),
                    why: format!(
                        "no run driver is configured, so install one first (agent-browser: `npm i -g agent-browser && agent-browser install`), then run custom tool {} from {} (sha {}) through the guarded CLI; pipe the input JSON on standard input",
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
                    tool: "open".to_string(),
                    args: serde_json::json!({"url": req.page}),
                    why: "open this page with your browser driver".to_string(),
                },
                Step {
                    tool: "call".to_string(),
                    args: serde_json::json!({"tool": req.tool, "input": req.input}),
                    why: "call this WebMCP tool on that page".to_string(),
                },
            ],
        }
    }
}

/// Whether `page` uses plain HTTP.
fn is_http_url(page: &str) -> bool {
    page.split_once("://")
        .map(|(scheme, _)| scheme.eq_ignore_ascii_case("http"))
        .unwrap_or(false)
}
