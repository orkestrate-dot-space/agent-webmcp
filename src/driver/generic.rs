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
