//! MCP elicitation: the user approves `ask` calls through the client.
//! In-process client/server pairs over a duplex stream; no network.

use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use agent_webmcp::index::Search;
use agent_webmcp::mcp::Server;
use agent_webmcp::model::{Annotations, Index, Site, Status, Tool};
use rmcp::model::{
    CallToolRequestParams, ElicitResult, ElicitationAction, ErrorData as McpError, Implementation,
    InitializeRequestParams, InitializeResult, JsonObject, ProtocolVersion, ServerCapabilities,
};
use rmcp::service::{RequestContext, RoleClient, RoleServer, Service};
use serde_json::{Value, json};

fn ask_index() -> Index {
    Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![Site {
            host: "cal.com".into(),
            status: Status::Verified,
            checked_at: "2026-11-19T00:00:00Z".into(),
            pages_checked: vec!["https://cal.com/".into()],
            platform: None,
            tools: vec![
                Tool {
                    name: "ask_cal_com".into(),
                    description: "Ask about Cal.".into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["question"],
                        "properties": {"question": {"type": "string"}}
                    }),
                    page: "https://cal.com/".into(),
                    origin: "https://cal.com".into(),
                    annotations: Annotations {
                        consequential: true,
                        ..Default::default()
                    },
                    flags: vec![],
                },
                Tool {
                    name: "list_slots".into(),
                    description: "List open slots.".into(),
                    input_schema: json!({"type": "object"}),
                    page: "https://cal.com/slots".into(),
                    origin: "https://cal.com".into(),
                    annotations: Annotations {
                        read_only: true,
                        ..Default::default()
                    },
                    flags: vec![],
                },
            ],
            note: None,
            missed: 0,
        }],
    }
}

fn server() -> Server {
    // Pinned to driver:none: plans never touch the machine's browsers.
    let mut cfg = agent_webmcp::config::Config::default();
    cfg.driver = agent_webmcp::config::DriverChoice::None;
    Server::with_config(Search::build(ask_index()).unwrap(), cfg)
}

fn args(target: &str, input: Value, confirm: bool) -> JsonObject {
    let mut map = JsonObject::new();
    map.insert("target".into(), Value::String(target.into()));
    map.insert("input".into(), input);
    map.insert("confirm".into(), Value::Bool(confirm));
    map
}

/// The client answers elicitations with a fixed action.
#[derive(Clone)]
struct AnswerClient {
    action: ElicitationAction,
    approve: bool,
    elicitations: Arc<AtomicUsize>,
}

impl AnswerClient {
    fn accept() -> Self {
        Self {
            action: ElicitationAction::Accept,
            approve: true,
            elicitations: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn decline() -> Self {
        Self {
            action: ElicitationAction::Decline,
            approve: false,
            elicitations: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl rmcp::ClientHandler for AnswerClient {
    async fn create_elicitation(
        &self,
        _request: rmcp::model::ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, McpError> {
        self.elicitations.fetch_add(1, Ordering::SeqCst);
        Ok(ElicitResult::new(self.action.clone()).with_content(json!({"approve": self.approve})))
    }
}

fn client_info(version: ProtocolVersion, elicitation: bool) -> InitializeRequestParams {
    let capabilities = if elicitation {
        rmcp::model::ClientCapabilities::builder()
            .enable_elicitation()
            .build()
    } else {
        rmcp::model::ClientCapabilities::builder().build()
    };
    InitializeRequestParams::new(capabilities, Implementation::new("elicit-test", "0.0.0"))
        .with_protocol_version(version)
}

fn server_info(version: ProtocolVersion) -> InitializeResult {
    let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
    info.protocol_version = version;
    info
}

async fn with_pair<S, C, F, Fut>(
    server: S,
    client_handler: C,
    version: ProtocolVersion,
    elicitation: bool,
    body: F,
) -> anyhow::Result<()>
where
    S: Service<RoleServer>,
    C: rmcp::ClientHandler,
    F: FnOnce(rmcp::service::RunningService<RoleClient, C>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    tokio::task::LocalSet::new()
        .run_until(async move {
            let (server_transport, client_transport) = tokio::io::duplex(8192);
            let server_version = version.clone();
            let server_task = tokio::task::spawn_local(async move {
                let running = rmcp::service::serve_directly::<RoleServer, _, _, _, _>(
                    server,
                    server_transport,
                    Some(client_info(server_version, elicitation)),
                );
                running.waiting().await?;
                anyhow::Ok(())
            });
            let client = rmcp::service::serve_directly::<RoleClient, _, _, _, _>(
                client_handler,
                client_transport,
                Some(server_info(version).into()),
            );
            let result = body(client).await;
            server_task.abort();
            result
        })
        .await
}

fn call_params(target: &str, input: Value, confirm: bool) -> CallToolRequestParams {
    CallToolRequestParams::new("call").with_arguments(args(target, input, confirm))
}

fn envelope_of(result: &rmcp::model::CallToolResult) -> Value {
    result
        .structured_content
        .clone()
        .expect("structured envelope")
}

#[tokio::test(flavor = "current_thread")]
async fn accept_runs_and_ignores_model_confirm() -> anyhow::Result<()> {
    let client_handler = AnswerClient::accept();
    let elicitations = client_handler.elicitations.clone();
    with_pair(
        server(),
        client_handler,
        ProtocolVersion::V_2026_07_28,
        true,
        |client| async move {
            // The model says confirm:false; the human's accept still runs it
            // (driver none, so "runs" returns the plan envelope).
            let result = client
                .call_tool(call_params(
                    "cal.com/ask_cal_com",
                    json!({"question": "hi"}),
                    false,
                ))
                .await?;
            let envelope = envelope_of(&result);
            assert_eq!(envelope["success"], json!(true));
            assert_eq!(envelope["data"]["plan"]["driver"], json!("generic"));
            assert_eq!(elicitations.load(Ordering::SeqCst), 1);
            Ok(())
        },
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn decline_is_denied() -> anyhow::Result<()> {
    with_pair(
        server(),
        AnswerClient::decline(),
        ProtocolVersion::V_2026_07_28,
        true,
        |client| async move {
            let result = client
                .call_tool(call_params(
                    "cal.com/ask_cal_com",
                    json!({"question": "hi"}),
                    true,
                ))
                .await?;
            let envelope = envelope_of(&result);
            assert_eq!(envelope["code"], json!("denied"));
            assert!(
                envelope["error"]
                    .as_str()
                    .unwrap()
                    .contains("declined cal.com/ask_cal_com"),
                "{envelope}"
            );
            Ok(())
        },
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn mismatched_retry_is_invalid_input() -> anyhow::Result<()> {
    with_pair(
        server(),
        AnswerClient::accept(),
        ProtocolVersion::V_2026_07_28,
        true,
        |client| async move {
            // First leg: no retry fields, so the server asks for approval.
            let pending = client
                .call_tool_once(call_params(
                    "cal.com/ask_cal_com",
                    json!({"question": "hi"}),
                    false,
                ))
                .await?;
            let state = match pending {
                rmcp::model::CallToolResponse::InputRequired(input) => {
                    input.request_state.expect("approval state")
                }
                other => panic!("expected an approval round, got {other:?}"),
            };
            // Retry with different arguments but the echoed state: rejected.
            let mut responses = BTreeMap::new();
            responses.insert(
                "approve".into(),
                json!({"action": "accept", "content": {"approve": true}}),
            );
            let params = CallToolRequestParams::new("call")
                .with_arguments(args(
                    "cal.com/ask_cal_com",
                    json!({"question": "bye"}),
                    false,
                ))
                .with_input_responses(responses)
                .with_request_state(state);
            let result = client.call_tool(params).await?;
            let envelope = envelope_of(&result);
            assert_eq!(envelope["code"], json!("invalid_input"));
            assert!(
                envelope["error"]
                    .as_str()
                    .unwrap()
                    .contains("does not match"),
                "{envelope}"
            );
            Ok(())
        },
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn no_elicitation_capability_keeps_confirm_flow() -> anyhow::Result<()> {
    with_pair(
        server(),
        AnswerClient::decline(),
        ProtocolVersion::V_2026_07_28,
        false,
        |client| async move {
            let result = client
                .call_tool(call_params(
                    "cal.com/ask_cal_com",
                    json!({"question": "hi"}),
                    false,
                ))
                .await?;
            let envelope = envelope_of(&result);
            assert_eq!(envelope["code"], json!("needs_confirmation"));
            assert!(
                envelope["data"]["note"]
                    .as_str()
                    .unwrap()
                    .contains("permission settings"),
                "{envelope}"
            );
            // ... but confirm:true still proceeds, without any elicitation.
            let result = client
                .call_tool(call_params(
                    "cal.com/ask_cal_com",
                    json!({"question": "hi"}),
                    true,
                ))
                .await?;
            let envelope = envelope_of(&result);
            assert_eq!(envelope["success"], json!(true));
            assert_eq!(envelope["data"]["plan"]["driver"], json!("generic"));
            Ok(())
        },
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn read_tool_never_elicits() -> anyhow::Result<()> {
    let client_handler = AnswerClient::accept();
    let elicitations = client_handler.elicitations.clone();
    with_pair(
        server(),
        client_handler,
        ProtocolVersion::V_2026_07_28,
        true,
        |client| async move {
            let result = client
                .call_tool(call_params("cal.com/list_slots", json!({}), false))
                .await?;
            let envelope = envelope_of(&result);
            assert_eq!(envelope["success"], json!(true));
            assert_eq!(elicitations.load(Ordering::SeqCst), 0);
            Ok(())
        },
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn old_protocol_elicitation_accepts() -> anyhow::Result<()> {
    let client_handler = AnswerClient::accept();
    let elicitations = client_handler.elicitations.clone();
    with_pair(
        server(),
        client_handler,
        ProtocolVersion::V_2025_11_25,
        true,
        |client| async move {
            let result = client
                .call_tool(call_params(
                    "cal.com/ask_cal_com",
                    json!({"question": "hi"}),
                    false,
                ))
                .await?;
            let envelope = envelope_of(&result);
            assert_eq!(envelope["success"], json!(true));
            assert_eq!(envelope["data"]["plan"]["driver"], json!("generic"));
            assert_eq!(elicitations.load(Ordering::SeqCst), 1);
            Ok(())
        },
    )
    .await
}
