//! Chrome DevTools MCP: plan steps in its own tool names.

use super::{CallRequest, Detection, Driver, Kind, Plan, Step};
use crate::config::Config;

/// Plan driver: the agent drives Chrome through Chrome DevTools MCP.
pub struct DevtoolsMcp;

impl Driver for DevtoolsMcp {
    fn id(&self) -> &'static str {
        "chrome-devtools-mcp"
    }

    fn kind(&self) -> Kind {
        Kind::Plan
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        Detection::Unknown
    }

    fn note(&self) -> Option<&'static str> {
        Some("needs --categoryExperimentalWebmcp, and Chrome with WebMCP enabled")
    }

    fn plan(&self, req: &CallRequest) -> Plan {
        let input = serde_json::to_string(&req.input).unwrap_or_default();
        Plan {
            driver: self.id(),
            note: self.note().map(str::to_string),
            steps: vec![
                Step {
                    tool: "navigate_page".to_string(),
                    args: serde_json::json!({"url": req.page}),
                    why: "open the page".to_string(),
                },
                Step {
                    tool: "list_webmcp_tools".to_string(),
                    args: serde_json::json!({}),
                    why: "confirm the tool registered; note the pageId if your client needs it"
                        .to_string(),
                },
                Step {
                    tool: "execute_webmcp_tool".to_string(),
                    args: serde_json::json!({"toolName": req.tool, "input": input}),
                    why: "call the tool; input is a JSON string".to_string(),
                },
            ],
        }
    }
}
