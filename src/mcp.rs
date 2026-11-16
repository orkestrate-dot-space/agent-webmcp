use std::borrow::Cow;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities,
    Tool, ToolAnnotations,
};
use rmcp::service::{ElicitationError, ElicitationMode, RequestContext};
use rmcp::{ErrorData as McpError, Peer, RoleServer, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::approval::{Approver, Decision, Request};
use crate::backend::Backend;
use crate::catalog::{Catalog, Entry, Kind, Source};
use crate::engine::{self, CallError};
use crate::index::Index;
use crate::ledger::Ledger;

const FIND_LIMIT_DEFAULT: usize = 5;
const FIND_LIMIT_MAX: usize = 20;
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

const INSTRUCTIONS: &str = "Tools for using websites in the user's own browser. \
Use `find` to look for a tool by what you want to do, then `call` it with the id and the arguments \
from its input_schema. Tools that change things on a site ask the user for approval first.";

/// Gives the server a browser for each call, and takes it back afterwards.
pub trait Provider: Send + Sync + 'static {
    type B: Backend + Send + Sync;

    fn connect(&self) -> impl Future<Output = Result<Self::B>> + Send;

    fn finish(&self, backend: Self::B) -> impl Future<Output = ()> + Send;
}

pub struct Server<P: Provider> {
    catalog: Arc<Catalog>,
    index: Arc<Mutex<Index>>,
    provider: Arc<P>,
    ledger: Ledger,
    options: engine::Options,
}

impl<P: Provider> Server<P> {
    pub fn new(catalog: Catalog, provider: P, ledger: Ledger) -> Result<Self> {
        let index = Index::build(&catalog.entries)?;
        Ok(Self {
            catalog: Arc::new(catalog),
            index: Arc::new(Mutex::new(index)),
            provider: Arc::new(provider),
            ledger,
            options: engine::Options::default(),
        })
    }

    pub fn with_options(mut self, options: engine::Options) -> Self {
        self.options = options;
        self
    }

    fn find(&self, args: &serde_json::Map<String, Value>) -> CallToolResult {
        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return failure("`query` is required: say what you want to do, in plain words");
        };
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| (n as usize).clamp(1, FIND_LIMIT_MAX))
            .unwrap_or(FIND_LIMIT_DEFAULT);
        let hits = match self.index.lock().expect("index lock").search(query, limit) {
            Ok(h) => h,
            Err(e) => return failure(&format!("search failed: {e:#}")),
        };
        let matches: Vec<Value> = hits.iter().map(|(pos, _)| describe(&self.catalog.entries[*pos])).collect();
        let note = if matches.is_empty() {
            Some("No tool matches. Try different words, or use the browser directly.")
        } else {
            None
        };
        let mut result = json!({ "matches": matches });
        if let Some(note) = note {
            result["note"] = json!(note);
        }
        CallToolResult::structured(result)
    }

    async fn call(&self, args: &serde_json::Map<String, Value>, peer: Peer<RoleServer>) -> CallToolResult {
        let Some(id) = args.get("id").and_then(Value::as_str) else {
            return failure("`id` is required: use an id returned by `find`");
        };
        let tool_args = args.get("args").cloned().unwrap_or_else(|| json!({}));

        let backend = match self.provider.connect().await {
            Ok(b) => b,
            Err(e) => return failure(&format!("could not start the browser: {e:#}")),
        };
        let approver = McpApprover { peer };
        let result = engine::call(&backend, &approver, &self.ledger, &self.catalog, id, tool_args, &self.options).await;
        self.provider.finish(backend).await;

        match result {
            Ok(outcome) => match serde_json::to_value(&outcome) {
                Ok(v) => CallToolResult::structured(v),
                Err(e) => failure(&format!("could not encode the result: {e}")),
            },
            Err(e) => failure_for(&e),
        }
    }
}

fn describe(e: &Entry) -> Value {
    json!({
        "id": e.id,
        "site": e.site,
        "description": e.description,
        "kind": match e.kind { Kind::Read => "read", Kind::Write => "write" },
        "source": match e.source { Source::Native => "native", Source::Community => "community" },
        "input_schema": e.input_schema,
    })
}

fn failure(message: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

fn failure_for(e: &CallError) -> CallToolResult {
    failure(&e.to_string())
}

fn tool_definitions() -> Vec<Tool> {
    let find_schema = json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "What you want to do, in plain words. For example: search flights from SFO to JFK." },
            "limit": { "type": "integer", "minimum": 1, "maximum": FIND_LIMIT_MAX, "description": "How many matches to return. Default 5." }
        },
        "required": ["query"],
        "additionalProperties": false
    });
    let call_schema = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "A tool id returned by `find`, such as example.com/say_hello." },
            "args": { "type": "object", "description": "Arguments for the tool. Must match the input_schema that `find` returned." }
        },
        "required": ["id"],
        "additionalProperties": false
    });
    vec![
        Tool::new(
            "find",
            "Find website tools that can do what you need. Describe the capability in plain words. \
             Returns matching tools with their ids and input schemas. Pass an id to `call`.",
            Arc::new(object(find_schema)),
        )
        .annotate(ToolAnnotations::new().read_only(true).open_world(false)),
        Tool::new(
            "call",
            "Run a tool returned by `find`, in the user's own browser. Tools that change things on a site \
             ask the user for approval first; if they decline, nothing runs.",
            Arc::new(object(call_schema)),
        )
        .annotate(ToolAnnotations::new().read_only(false).open_world(true)),
    ]
}

fn object(v: Value) -> serde_json::Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => unreachable!("tool schemas are objects"),
    }
}

impl<P: Provider> ServerHandler for Server<P> {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("agent-webmcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
    }

    // Approvals use server-to-client elicitation, which spec 2026-07-28 removed in favour of
    // multi-round-trip results. Until that is built, negotiate down to 2025-11-25.
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(ProtocolVersion::known_up_to(&ProtocolVersion::V_2025_11_25))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tool_definitions()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        let result = match request.name.as_ref() {
            "find" => self.find(&args),
            "call" => self.call(&args, context.peer.clone()).await,
            other => return Err(McpError::invalid_params(format!("unknown tool {other:?}"), None)),
        };
        Ok(result.into())
    }
}

/// Asks the user through the MCP client's own approval prompt.
struct McpApprover {
    peer: Peer<RoleServer>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct Confirm {
    /// Tick to let the tool run.
    approve: bool,
}
rmcp::elicit_safe!(Confirm);

impl Approver for McpApprover {
    async fn ask(&self, request: &Request<'_>) -> Decision {
        if !self.peer.supported_elicitation_modes().contains(&ElicitationMode::Form) {
            return Decision::Unavailable;
        }
        match self
            .peer
            .elicit_with_timeout::<Confirm>(request.message(), Some(APPROVAL_TIMEOUT))
            .await
        {
            Ok(Some(c)) if c.approve => Decision::Granted,
            Ok(_) => Decision::Declined,
            Err(ElicitationError::UserDeclined | ElicitationError::UserCancelled) => Decision::Declined,
            Err(_) => Decision::Unavailable,
        }
    }
}
