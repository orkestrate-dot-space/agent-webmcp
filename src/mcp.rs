//! MCP server exposing one tool, `find`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, InitializeResult,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::index::Search;

const LIMIT_DEFAULT: usize = 5;
const LIMIT_MAX: usize = 20;

pub const INSTRUCTIONS: &str = "Search websites' WebMCP tools by what you want to do. \
`find` returns the page to open and the tool to call there. Open the page with your browser \
driver; the tool is registered on the page and appears in your driver's WebMCP tools. \
Call it with your driver.";

pub struct Server {
    search: Arc<Mutex<Search>>,
}

impl Server {
    pub fn new(search: Search) -> Self {
        Self { search: Arc::new(Mutex::new(search)) }
    }

    /// The shared search, so a background task can swap in a newer index.
    pub fn shared(&self) -> Arc<Mutex<Search>> {
        self.search.clone()
    }

    /// Check for a newer index every hour, with a little jitter so many
    /// clients do not ask at the same moment. A failure is reported once and
    /// the current index is kept. `--index` and `AGENT_WEBMCP_INDEX` pin a
    /// file, so nothing is fetched then.
    pub fn spawn_refresh(&self) {
        if std::env::var_os("AGENT_WEBMCP_INDEX").is_some() {
            return;
        }
        let shared = self.shared();
        let jitter = Duration::from_secs(u64::from(std::process::id()) % 300);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60 * 60) + jitter).await;
            loop {
                let fresh = tokio::task::spawn_blocking(crate::store::refresh).await;
                match fresh {
                    Ok(Ok(index)) => match Search::build(index) {
                        Ok(search) => {
                            *shared.lock().expect("search lock") = search;
                        }
                        Err(e) => eprintln!("agent-webmcp: could not build the new index: {e:#}"),
                    },
                    Ok(Err(e)) => eprintln!("agent-webmcp: keeping the current index ({e:#})"),
                    Err(e) => eprintln!("agent-webmcp: index refresh task failed: {e:#}"),
                }
                tokio::time::sleep(Duration::from_secs(60 * 60)).await;
            }
        });
    }

    fn find(&self, args: &serde_json::Map<String, Value>) -> CallToolResult {
        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return CallToolResult::error(vec![ContentBlock::text("`query` is required: say what you want to do")]);
        };
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| (n as usize).clamp(1, LIMIT_MAX))
            .unwrap_or(LIMIT_DEFAULT);
        let result = self.search.lock().expect("search lock").find(query, limit);
        match result {
            Ok(hits) if hits.is_empty() => CallToolResult::structured(json!({
                "matches": [],
                "note": "No indexed site has a tool for this. Use your browser driver as usual."
            })),
            Ok(hits) => CallToolResult::structured(json!({ "matches": hits })),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(format!("search failed: {e:#}"))]),
        }
    }
}

fn find_tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "What you want to do, in plain words. Example: search flights from SFO to JFK." },
            "limit": { "type": "integer", "minimum": 1, "maximum": LIMIT_MAX, "description": "How many matches. Default 5." }
        },
        "required": ["query"],
        "additionalProperties": false
    });
    let Value::Object(schema) = schema else { unreachable!() };
    Tool::new(
        "find",
        "Find websites that offer a WebMCP tool for what you need. Returns, for each match, the page \
         to open and the tool name and input schema. Open the page with your browser driver, then call \
         the tool through the driver's WebMCP support.",
        Arc::new(schema),
    )
    .annotate(ToolAnnotations::new().read_only(true).open_world(false))
}

impl ServerHandler for Server {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("agent-webmcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(vec![find_tool()]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        match request.name.as_ref() {
            "find" => Ok(self.find(&request.arguments.unwrap_or_default()).into()),
            other => Err(McpError::invalid_params(format!("unknown tool {other:?}"), None)),
        }
    }
}
