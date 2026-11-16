use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_webmcp::backend::{Backend, Invocation, PageTool, Tab};
use agent_webmcp::catalog::Catalog;
use agent_webmcp::engine::Options;
use agent_webmcp::ledger::{Ledger, Outcome};
use agent_webmcp::mcp::{Provider, Server};
use anyhow::Result;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ContentBlock, ElicitRequestParams,
    ElicitResult, ElicitationAction, Implementation,
};
use rmcp::service::{RequestContext, RunningService};
use rmcp::{ClientHandler, ErrorData as McpError, RoleClient, ServiceExt};
use serde_json::{Value, json};

type Log = Arc<Mutex<Vec<String>>>;

struct FakeBrowser {
    log: Log,
    tools: Vec<PageTool>,
}

impl Backend for FakeBrowser {
    async fn open(&self, url: &str) -> Result<Tab> {
        self.log.lock().unwrap().push(format!("open {url}"));
        Ok(Tab { session: "s".into(), url: url.into() })
    }
    async fn inject(&self, _: &Tab, _: &str) -> Result<()> {
        self.log.lock().unwrap().push("inject".into());
        Ok(())
    }
    async fn list_tools(&self, _: &Tab) -> Result<Vec<PageTool>> {
        Ok(self.tools.clone())
    }
    async fn invoke(&self, _: &Tab, t: &PageTool, args: Value) -> Result<Invocation> {
        self.log.lock().unwrap().push(format!("invoke {}", t.name));
        Ok(Invocation { output: json!({ "echo": args }), origin: t.origin.clone(), duration_ms: Some(1) })
    }
}

struct FakeProvider {
    log: Log,
    closed: Arc<Mutex<u32>>,
}

impl Provider for FakeProvider {
    type B = FakeBrowser;
    async fn connect(&self) -> Result<FakeBrowser> {
        let tool = |name: &str| PageTool {
            name: name.into(),
            description: String::new(),
            frame_id: "f".into(),
            origin: Some("https://example.com".into()),
            read_only: true,
            consequential: false,
            input_schema: Value::Null,
        };
        Ok(FakeBrowser { log: self.log.clone(), tools: vec![tool("say_hello"), tool("leave_note")] })
    }
    async fn finish(&self, _: FakeBrowser) {
        *self.closed.lock().unwrap() += 1;
    }
}

/// An MCP client that answers approval prompts the way the test says.
struct Client {
    capabilities: ClientCapabilities,
    answer: Option<ElicitationAction>,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(self.capabilities.clone(), Implementation::new("test-client", "0"))
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, McpError> {
        if let ElicitRequestParams::FormElicitationParams { message, .. } = request {
            self.prompts.lock().unwrap().push(message);
        }
        match self.answer.clone() {
            Some(ElicitationAction::Accept) => {
                Ok(ElicitResult::new(ElicitationAction::Accept).with_content(json!({ "approve": true })))
            }
            Some(action) => Ok(ElicitResult::new(action)),
            None => Err(McpError::internal_error("no answer scripted", None)),
        }
    }
}

struct Harness {
    client: RunningService<RoleClient, Client>,
    log: Log,
    closed: Arc<Mutex<u32>>,
    prompts: Arc<Mutex<Vec<String>>>,
    ledger: Ledger,
    _dir: tempfile::TempDir,
}

async fn start(answer: Option<ElicitationAction>, can_elicit: bool) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger::at(dir.path().join("ledger.jsonl"));
    let log: Log = Arc::default();
    let closed = Arc::new(Mutex::new(0));
    let provider = FakeProvider { log: log.clone(), closed: closed.clone() };
    let server = Server::new(Catalog::embedded().unwrap(), provider, ledger.clone())
        .unwrap()
        .with_options(Options { poll_attempts: 3, poll_delay: Duration::from_millis(1) });

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });

    let capabilities = if can_elicit {
        ClientCapabilities::builder().enable_elicitation().build()
    } else {
        ClientCapabilities::default()
    };
    let prompts: Arc<Mutex<Vec<String>>> = Arc::default();
    let client = Client { capabilities, answer, prompts: prompts.clone() }
        .serve(client_io)
        .await
        .unwrap();
    Harness { client, log, closed, prompts, ledger, _dir: dir }
}

async fn call_tool(h: &Harness, name: &'static str, args: Value) -> CallToolResult {
    let args = args.as_object().unwrap().clone();
    h.client
        .call_tool(CallToolRequestParams::new(name).with_arguments(args))
        .await
        .unwrap()
}

fn text(r: &CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn lists_exactly_two_tools_with_schemas() {
    let h = start(None, false).await;
    let tools = h.client.list_all_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["find", "call"]);
    for t in &tools {
        assert_eq!(t.input_schema["type"], "object");
        assert!(t.description.as_ref().is_some_and(|d| d.len() > 40));
    }
    assert_eq!(tools[0].annotations.as_ref().unwrap().read_only_hint, Some(true));
}

#[tokio::test]
async fn find_returns_ranked_matches_with_schemas() {
    let h = start(None, false).await;
    let r = call_tool(&h, "find", json!({ "query": "search flights from SFO to JFK" })).await;
    assert_eq!(r.is_error, Some(false));
    let s = r.structured_content.unwrap();
    let first = &s["matches"][0];
    assert_eq!(first["id"], "googlechromelabs.github.io/searchFlights");
    assert_eq!(first["kind"], "read");
    assert!(first["input_schema"]["required"].as_array().unwrap().contains(&json!("origin")));
}

#[tokio::test]
async fn find_with_no_match_says_so_without_failing() {
    let h = start(None, false).await;
    let r = call_tool(&h, "find", json!({ "query": "quantum chromodynamics" })).await;
    assert_eq!(r.is_error, Some(false));
    let s = r.structured_content.unwrap();
    assert!(s["matches"].as_array().unwrap().is_empty());
    assert!(s["note"].as_str().unwrap().contains("No tool matches"));
}

#[tokio::test]
async fn find_without_a_query_explains_itself() {
    let h = start(None, false).await;
    let r = call_tool(&h, "find", json!({})).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("query"));
}

#[tokio::test]
async fn call_runs_a_read_tool_and_closes_the_browser() {
    let h = start(None, false).await;
    let r = call_tool(&h, "call", json!({ "id": "example.com/say_hello", "args": { "name": "Ada" } })).await;
    assert_eq!(r.is_error, Some(false), "{}", text(&r));
    let s = r.structured_content.unwrap();
    assert_eq!(s["id"], "example.com/say_hello");
    assert_eq!(s["origin"], "https://example.com");
    assert_eq!(s["output"]["echo"]["name"], "Ada");
    assert_eq!(*h.closed.lock().unwrap(), 1);
    assert!(h.prompts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_write_tool_prompts_the_user_and_runs_on_approval() {
    let h = start(Some(ElicitationAction::Accept), true).await;
    let r = call_tool(&h, "call", json!({ "id": "example.com/leave_note", "args": { "text": "hi" } })).await;
    assert_eq!(r.is_error, Some(false), "{}", text(&r));
    let prompts = h.prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("example.com/leave_note"), "{}", prompts[0]);
    assert!(prompts[0].contains("\"hi\""), "the user sees the arguments: {}", prompts[0]);
    assert!(h.log.lock().unwrap().iter().any(|l| l == "invoke leave_note"));
    let records = h.ledger.read_all().unwrap();
    assert_eq!(records[0].outcome, Outcome::Ok);
    assert!(records[0].approved);
}

#[tokio::test]
async fn declining_the_prompt_stops_the_call_before_the_browser_opens() {
    let h = start(Some(ElicitationAction::Decline), true).await;
    let r = call_tool(&h, "call", json!({ "id": "example.com/leave_note", "args": { "text": "hi" } })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("declined"), "{}", text(&r));
    assert!(h.log.lock().unwrap().is_empty(), "{:?}", h.log.lock().unwrap());
    assert_eq!(h.ledger.read_all().unwrap()[0].outcome, Outcome::Declined);
}

#[tokio::test]
async fn cancelling_the_prompt_also_stops_the_call() {
    let h = start(Some(ElicitationAction::Cancel), true).await;
    let r = call_tool(&h, "call", json!({ "id": "example.com/leave_note", "args": { "text": "hi" } })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(h.log.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_client_that_cannot_prompt_gets_a_clear_refusal() {
    let h = start(None, false).await;
    let r = call_tool(&h, "call", json!({ "id": "example.com/leave_note", "args": { "text": "hi" } })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("Nothing was run"), "{}", text(&r));
    assert!(h.log.lock().unwrap().is_empty());
    assert_eq!(h.ledger.read_all().unwrap()[0].outcome, Outcome::ApprovalRequired);
}

#[tokio::test]
async fn unknown_tool_ids_are_errors_the_agent_can_read() {
    let h = start(None, false).await;
    let r = call_tool(&h, "call", json!({ "id": "nope.com/x" })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("use `find`"), "{}", text(&r));
}
