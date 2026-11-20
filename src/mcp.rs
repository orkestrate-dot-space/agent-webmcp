//! MCP server exposing `find`, `inspect`, and `call`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ElicitRequest,
    ElicitRequestParams, ElicitationAction, Implementation, InitializeResult, InputRequest,
    InputRequests, InputRequiredResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::index::Search;

const LIMIT_DEFAULT: usize = 5;
const LIMIT_MAX: usize = 20;

pub const INSTRUCTIONS: &str = "Check whether a website offers WebMCP tools before opening it, \
or search tools by task. `find` searches the public directory and returns the page to open \
and the tool to call there. Use `inspect` to check any live page (unindexed sites, localhost, \
or logged-in dashboards) and retrieve live input schemas without calling anything. Then `call` \
runs the tool through your configured run driver (`agent-browser`, `browse`, or `lightpanda`), \
or returns generic steps when `driver` is `none`.";

pub struct Server {
    search: Arc<Mutex<Search>>,
    fixed_config: Option<crate::config::Config>,
}

impl Server {
    pub fn new(search: Search) -> Self {
        Self {
            search: Arc::new(Mutex::new(search)),
            fixed_config: None,
        }
    }

    /// Pinned config for tests; the handler uses it instead of loading per call.
    pub fn with_config(search: Search, config: crate::config::Config) -> Self {
        Self {
            search: Arc::new(Mutex::new(search)),
            fixed_config: Some(config),
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
                            *shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = search;
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

    async fn call(
        &self,
        request: &CallToolRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> CallToolResponse {
        let args = request.arguments.clone().unwrap_or_default();
        let Some(target) = args.get("target").and_then(Value::as_str) else {
            return complete_error(
                "`target` is required: site/tool from find, or https://page#tool",
            );
        };
        let target = target.to_string();
        let input = args.get("input").cloned().unwrap_or(json!(null));
        let input = if input.is_null() { json!({}) } else { input };
        let confirm = args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let shared = self.shared();
        // Config is cheap; the pipeline below may block on child processes.
        // Tests pin a config so they never depend on the machine's drivers.
        let cfg = match self.fixed_config.clone() {
            Some(cfg) => cfg,
            None => match crate::config::load() {
                Ok(loaded) => loaded.config,
                Err(e) => return complete_error(format!("could not load config: {e:#}")),
            },
        };
        let prepared = {
            let search = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match crate::call::prepare(&target, input.clone(), &cfg, &search) {
                Ok(prepared) => prepared,
                Err(e) => return complete_envelope(e.envelope()),
            }
        };
        if !matches!(prepared.decision, crate::call::Decision::Ask) {
            return self.run_prepared(prepared, mcp_opts(confirm), cfg).await;
        }
        // The model asked for an `ask` tool: with form elicitation the human
        // decides, and the model's `confirm` is ignored entirely.
        if !client_supports_form(context) {
            let (mut envelope, _) = self
                .run_prepared_outcome(prepared, mcp_opts(confirm), cfg)
                .await;
            if !confirm {
                note_no_elicitation(&mut envelope);
            }
            return complete_envelope(envelope);
        }
        let id = prepared.resolved.id();
        let message = approval_message(&prepared);
        let state = crate::call::approval_state(&target, &input);
        if let Some(responses) = request.input_responses.as_ref() {
            return self
                .retry_after_approval(
                    prepared,
                    &id,
                    request.request_state.as_deref(),
                    &state,
                    responses,
                    cfg,
                )
                .await;
        }
        if negotiated_version(context).is_some_and(|v| v >= ProtocolVersion::V_2026_07_28) {
            let mut requests = InputRequests::new();
            requests.insert("approve".to_string(), elicitation_request(&message));
            return InputRequiredResult::new(Some(requests), Some(state)).into();
        }
        self.elicit_old(prepared, &message, &id, cfg, context).await
    }

    /// Run a prepared call off the executor; child processes block.
    async fn run_prepared(
        &self,
        prepared: crate::call::Prepared,
        opts: crate::call::CallOptions,
        cfg: crate::config::Config,
    ) -> CallToolResponse {
        let (envelope, is_error) = self.run_prepared_outcome(prepared, opts, cfg).await;
        complete_envelope_with(envelope, is_error)
    }

    /// The blocking half of `run_prepared`, shared with the approval retry.
    /// rmcp carries text and structured content side by side, so results
    /// carry the envelope as text AND as structured content.
    async fn run_prepared_outcome(
        &self,
        prepared: crate::call::Prepared,
        opts: crate::call::CallOptions,
        cfg: crate::config::Config,
    ) -> (Value, bool) {
        let joined =
            tokio::task::spawn_blocking(move || crate::call::finish(prepared, &opts, &cfg)).await;
        match joined {
            Ok(Ok(done)) => (done.envelope(), false),
            Ok(Err(e)) => (e.envelope(), true),
            Err(e) => {
                let envelope = crate::call::CallError {
                    code: "driver_error",
                    message: format!("call task failed: {e}"),
                    data: None,
                }
                .envelope();
                (envelope, true)
            }
        }
    }

    /// A retried `call` carrying the user's answer: verify it, then run.
    async fn retry_after_approval(
        &self,
        prepared: crate::call::Prepared,
        id: &str,
        echoed: Option<&str>,
        expected: &str,
        responses: &rmcp::model::InputResponses,
        cfg: crate::config::Config,
    ) -> CallToolResponse {
        if echoed != Some(expected) {
            return complete_envelope(
                crate::call::CallError {
                    code: "invalid_input",
                    message: "the approval does not match this call".to_string(),
                    data: None,
                }
                .envelope(),
            );
        }
        if !is_approved(responses) {
            return complete_envelope(
                crate::call::CallError {
                    code: "denied",
                    message: format!("the user declined {id}"),
                    data: None,
                }
                .envelope(),
            );
        }
        // The user approved; their answer stands in for `confirm: true`.
        self.run_prepared(
            prepared,
            crate::call::CallOptions {
                confirm: true,
                plan_only: false,
                fresh: false,
                retry_hint: crate::call::RETRY_MCP,
            },
            cfg,
        )
        .await
    }

    /// Old protocols: ask over a server-to-client request, 120 s to answer.
    async fn elicit_old(
        &self,
        prepared: crate::call::Prepared,
        message: &str,
        id: &str,
        cfg: crate::config::Config,
        context: &RequestContext<RoleServer>,
    ) -> CallToolResponse {
        let params = approval_params(message);
        match context
            .peer
            .create_elicitation_with_timeout(params, Some(std::time::Duration::from_secs(120)))
            .await
        {
            Ok(result)
                if result.action == ElicitationAction::Accept
                    && is_approved_content(&result.content) =>
            {
                self.run_prepared(
                    prepared,
                    crate::call::CallOptions {
                        confirm: true,
                        plan_only: false,
                        fresh: false,
                        retry_hint: crate::call::RETRY_MCP,
                    },
                    cfg,
                )
                .await
            }
            Ok(_) => complete_envelope(
                crate::call::CallError {
                    code: "denied",
                    message: format!("the user declined {id}"),
                    data: None,
                }
                .envelope(),
            ),
            Err(rmcp::service::ServiceError::Timeout { .. }) => complete_envelope(
                crate::call::CallError {
                    code: "needs_confirmation",
                    message: "no answer from the user".to_string(),
                    data: None,
                }
                .envelope(),
            ),
            Err(e) => complete_envelope(
                crate::call::CallError {
                    code: "driver_error",
                    message: format!("could not ask the user: {e}"),
                    data: None,
                }
                .envelope(),
            ),
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
        let result = self
            .search
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .find(query, limit);
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
            inspect_tool(),
            call_tool(),
        ]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        match request.name.as_ref() {
            "find" => Ok(self.find(&request.arguments.unwrap_or_default()).into()),
            "inspect" => Ok(self.inspect(&request.arguments.unwrap_or_default()).await),
            "call" => Ok(self.call(&request, &context).await),
            other => Err(McpError::invalid_params(
                format!("unknown tool {other:?}"),
                None,
            )),
        }
    }
}

impl Server {
    async fn inspect(&self, args: &serde_json::Map<String, Value>) -> CallToolResponse {
        let Some(url) = args.get("url").and_then(Value::as_str) else {
            return complete_error("`url` is required: https://page or host to inspect");
        };
        let url = url.to_string();
        let fresh = args.get("fresh").and_then(Value::as_bool).unwrap_or(false);
        let cfg = match self.fixed_config.clone() {
            Some(cfg) => cfg,
            None => match crate::config::load() {
                Ok(loaded) => loaded.config,
                Err(e) => return complete_error(format!("could not load config: {e:#}")),
            },
        };
        let opts = crate::inspect::InspectOptions { fresh };
        let res =
            tokio::task::spawn_blocking(move || crate::inspect::run_inspect(&url, &opts, &cfg))
                .await;
        match res {
            Ok(Ok(done)) => complete_envelope_with(done.envelope(), false),
            Ok(Err(e)) => complete_envelope(e.envelope()),
            Err(e) => complete_error(format!("inspect task failed: {e}")),
        }
    }
}

/// The `inspect` tool definition, as `list_tools` advertises it.
pub fn inspect_tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "description": "Page URL or host to inspect (e.g. https://cal.com/ or cal.com)" },
            "fresh": { "type": "boolean", "description": "Use a throwaway session or tab, closed afterwards" }
        },
        "required": ["url"],
        "additionalProperties": false
    });
    let Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "inspect",
        "Open a live URL in the browser and list the WebMCP tools it registers (names, descriptions, \
         and live input schemas) without calling any of them.",
        Arc::new(schema),
    )
    .annotate(ToolAnnotations::new().read_only(true).open_world(true))
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
        "Call a WebMCP tool: if agent-webmcp can drive a browser (agent-browser, browse, or lightpanda) it returns \
         the tool's output, otherwise it returns generic steps when `driver` is `none`. \
         When the client supports it, agent-webmcp asks the user itself before calls that \
         need approval; `confirm` only applies to clients that cannot ask.",
        Arc::new(schema),
    )
    .annotate(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .open_world(true),
    )
}

/// `confirm: true` only matters where the client cannot ask the user.
fn mcp_opts(confirm: bool) -> crate::call::CallOptions {
    crate::call::CallOptions {
        confirm,
        plan_only: false,
        fresh: false,
        retry_hint: crate::call::RETRY_MCP,
    }
}

/// A failure envelope as a complete tool result.
fn complete_error(message: impl Into<String>) -> CallToolResponse {
    complete_envelope(
        crate::call::CallError {
            code: "driver_error",
            message: message.into(),
            data: None,
        }
        .envelope(),
    )
}

/// An envelope as a complete tool result.
fn complete_envelope(envelope: Value) -> CallToolResponse {
    complete_envelope_with(envelope, true)
}

/// An envelope as a complete tool result, with an explicit error flag.
fn complete_envelope_with(envelope: Value, is_error: bool) -> CallToolResponse {
    let text = serde_json::to_string_pretty(&envelope).unwrap_or_default();
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(envelope);
    result.is_error = Some(is_error);
    result.into()
}

/// Whether the client renders a form the user can answer.
fn client_supports_form(context: &RequestContext<RoleServer>) -> bool {
    // New-protocol requests carry capabilities per call, but may arrive
    // empty; the handshake stays authoritative for what it names.
    let from_meta = context.client_capabilities().and_then(|c| c.elicitation);
    let from_peer = context
        .peer
        .peer_info()
        .and_then(|info| info.capabilities.elicitation.clone());
    [from_meta, from_peer].into_iter().flatten().any(|e| {
        // No mode listed means form, matching the SDK's own fallback.
        e.form.is_some() || e.url.is_none()
    })
}

/// Negotiated protocol version from request meta or the handshake.
fn negotiated_version(context: &RequestContext<RoleServer>) -> Option<ProtocolVersion> {
    context.protocol_version().or_else(|| {
        context
            .peer
            .peer_info()
            .map(|info| info.protocol_version.clone())
    })
}

/// The question the user answers, with the call it approves.
fn approval_message(prepared: &crate::call::Prepared) -> String {
    let id = prepared.resolved.id();
    let mut input = serde_json::to_string(&prepared.input).unwrap_or_default();
    if input.chars().count() > 300 {
        input = input.chars().take(300).collect();
    }
    format!(
        "Allow agent-webmcp to call {id} on {}? It is {}; input: {input}",
        prepared.resolved.page,
        crate::call::kind_str(prepared.resolved.kind),
    )
}

/// Human approval form: one boolean, required.
fn approval_params(message: &str) -> ElicitRequestParams {
    let requested_schema = serde_json::from_value(json!({
        "type": "object",
        "properties": {"approve": {"type": "boolean", "title": "Approve this call"}},
        "required": ["approve"]
    }))
    .expect("approval schema is fixed");
    ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: message.to_string(),
        requested_schema,
    }
}

/// The `approve` elicitation request, keyed `"approve"` per the MRTR pattern.
fn elicitation_request(message: &str) -> InputRequest {
    InputRequest::Elicitation(ElicitRequest::new(approval_params(message)))
}

/// Approval only counts from the user's answer: accept plus approve true.
fn is_approved(responses: &rmcp::model::InputResponses) -> bool {
    responses
        .get("approve")
        .filter(|r| r.get("action").and_then(Value::as_str) == Some("accept"))
        .and_then(|r| {
            r.get("content")
                .and_then(|c| c.get("approve"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

/// Same check for a direct elicitation result on old protocols.
fn is_approved_content(content: &Option<Value>) -> bool {
    content
        .as_ref()
        .and_then(|c| c.get("approve"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Clients without elicitation get a hint to configure approval themselves.
fn note_no_elicitation(envelope: &mut Value) {
    envelope["data"]["note"] = json!(
        "this client cannot ask you directly; set the agent-webmcp call tool to ask in your agent's permission settings"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_lock_still_answers_find() {
        let index = crate::model::Index {
            schema_version: 1,
            generated_at: String::new(),
            sites: vec![],
        };
        let search = Search::build(index).unwrap();
        let server = Server::new(search);
        let shared = server.shared();
        let _ = std::thread::spawn(move || {
            let _guard = shared.lock().unwrap();
            panic!("poison the search lock");
        })
        .join();
        let mut args = serde_json::Map::new();
        args.insert("query".to_string(), json!("cal.com"));
        let _ = server.find(&args);
    }
}
