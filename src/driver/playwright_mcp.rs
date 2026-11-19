//! Playwright MCP: plan steps in its own tool names.

use super::{CallRequest, Detection, Driver, Kind, Plan, Step};
use crate::config::Config;

/// Plan driver: the agent drives the page through Playwright MCP.
pub struct PlaywrightMcp;

impl Driver for PlaywrightMcp {
    fn id(&self) -> &'static str {
        "playwright-mcp"
    }

    fn kind(&self) -> Kind {
        Kind::Plan
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        Detection::Unknown
    }

    fn note(&self) -> Option<&'static str> {
        Some(
            "needs --enable-features=WebMCP in browser.launchOptions.args in the Playwright MCP config file",
        )
    }

    fn plan(&self, req: &CallRequest) -> Plan {
        Plan {
            driver: self.id(),
            note: self.note().map(str::to_string),
            steps: vec![
                Step {
                    tool: "browser_navigate".to_string(),
                    args: serde_json::json!({"url": req.page}),
                    why: "open the page; the response lists the registered WebMCP tools".to_string(),
                },
                Step {
                    tool: format!("webmcp_{}", req.tool),
                    args: req.input.clone(),
                    why: "use the exact webmcp_* name from the navigate listing; Playwright rewrites characters outside A-Za-z0-9_- to _ and cuts names at 64".to_string(),
                },
            ],
        }
    }
}
