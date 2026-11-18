//! OpenCode Browse: plan steps in its own tool names.

use super::{CallRequest, Detection, Driver, Kind, Plan, Step};
use crate::config::Config;

/// Plan driver: the agent drives its own Chrome through OpenCode Browse.
pub struct Browse;

impl Driver for Browse {
    fn id(&self) -> &'static str {
        "browse"
    }

    fn kind(&self) -> Kind {
        Kind::Plan
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        Detection::Unknown
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
