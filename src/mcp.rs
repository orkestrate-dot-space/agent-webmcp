//! MCP server exposing `find` and `call`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::index::Search;

const LIMIT_DEFAULT: usize = 5;
const LIMIT_MAX: usize = 20;

pub const INSTRUCTIONS: &str = "Check whether a website offers WebMCP tools before opening it, \
or search tools by task. `find` returns the page to open and the tool to call there. \
Then `call` runs the tool when agent-webmcp can drive a browser, or returns exact \
steps for your own browser driver. Open the page with your browser driver; the tool \
is registered on the page and appears in your driver's WebMCP tools.";

pub struct Server {
    search: Arc<Mutex<Search>>,
}

impl Server {
    pub fn new(search: Search) -> Self {
        Self {
            search: Arc::new(Mutex::new(search)),
        }
    }

    /// The shared search, so a background task can swap in a newer index.
    pub fn shared(&self) -> Arc<Mutex<Search>> {
        self.search.clone()
    }

    /// Check for a newer index every hour, with a little jitter so many
    /// clients do not ask at the same moment. A failure is reported once and
    /// the current index is kept. `AGENT_WEBMCP_INDEX` pins a file, so
    /// nothing is fetched then.
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

    async fn call(&self, args: &serde_json::Map<String, Value>) -> CallToolResult {
        let Some(target) = args.get("target").and_then(Value::as_str) else {
            return CallToolResult::error(vec![ContentBlock::text(
                "`target` is required: site/tool from find, or https://page#tool",
            )]);
        };
        let target = target.to_string();
        let input = args.get("input").cloned().unwrap_or(json!(null));
        let input = if input.is_null() { json!({}) } else { input };
        let confirm = args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let shared = self.shared();
        // The pipeline blocks on child processes; keep it off the executor.
        // rmcp carries text and structured content side by side, so failures
        // use error() with the envelope as text AND as structured content.
        let joined = tokio::task::spawn_blocking(move || {
            let search = shared.lock().expect("search lock");
            let cfg = match crate::config::load() {
                Ok(loaded) => loaded.config,
                Err(e) => {
                    let envelope = crate::call::CallError {
                        code: "driver_error",
                        message: format!("could not load config: {e:#}"),
                        data: None,
                    }
                    .envelope();
                    return (envelope, true);
                }
            };
            crate::call::run_mcp(&target, input, confirm, &cfg, &search)
        })
        .await;
        match joined {
            Ok((envelope, is_error)) => {
                let text = serde_json::to_string_pretty(&envelope).unwrap_or_default();
                let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
                result.structured_content = Some(envelope);
                result.is_error = Some(is_error);
                result
            }
            Err(e) => {
                CallToolResult::error(vec![ContentBlock::text(format!("call task failed: {e}"))])
            }
        }
    }

    fn find(&self, args: &serde_json::Map<String, Value>) -> CallToolResult {
        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return CallToolResult::error(vec![ContentBlock::text(
                "`query` is required: say what you want to do",
            )]);
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
            Err(e) => {
                CallToolResult::error(vec![ContentBlock::text(format!("search failed: {e:#}"))])
            }
        }
    }
}

fn find_tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "A site (cal.com), a URL, or what you want to do, in plain words. Example: search flights from SFO to JFK." },
            "limit": { "type": "integer", "minimum": 1, "maximum": LIMIT_MAX, "description": "How many matches. Default 5." }
        },
        "required": ["query"],
        "additionalProperties": false
    });
    let Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "find",
        "Check whether a site offers WebMCP tools, or find a tool for a task. Pass a site, a URL, or what \
         you need in plain words. Returns, for each match, the page to open and the tool name and input \
         schema. Open the page with your browser driver, then call the tool through the driver's WebMCP support.",
        Arc::new(schema),
    )
    .annotate(ToolAnnotations::new().read_only(true).open_world(false))
}

impl ServerHandler for Server {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "agent-webmcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(vec![
            find_tool(),
            call_tool(),
        ]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        match request.name.as_ref() {
            "find" => Ok(self.find(&request.arguments.unwrap_or_default()).into()),
            "call" => Ok(self
                .call(&request.arguments.unwrap_or_default())
                .await
                .into()),
            other => Err(McpError::invalid_params(
                format!("unknown tool {other:?}"),
                None,
            )),
        }
    }
}

/// The `call` tool definition, as `list_tools` advertises it.
pub fn call_tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "target": { "type": "string", "description": "site/tool from find, or https://page#tool" },
            "input": { "type": "object", "description": "Tool input, matching the input_schema from find" },
            "confirm": { "type": "boolean", "description": "Set true to approve a tool that needs confirmation" }
        },
        "required": ["target"],
        "additionalProperties": false
    });
    let Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "call",
        "Call a WebMCP tool: if agent-webmcp can drive a browser (agent-browser) it returns \
         the tool's output, otherwise it returns exact steps for your own browser driver.",
        Arc::new(schema),
    )
    .annotate(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .open_world(true),
    )
}
