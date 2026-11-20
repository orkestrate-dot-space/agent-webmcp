//! Integration tests for `agent-webmcp inspect` across all three run drivers.

mod common;

use agent_webmcp::config::{self, DriverChoice};
use agent_webmcp::driver::{self, RunOptions};
use agent_webmcp::inspect::{InspectOptions, normalize_url, run_inspect};
use serde_json::json;

#[test]
fn normalize_url_handles_hosts_localhost_and_schemes() {
    assert_eq!(normalize_url("cal.com").unwrap(), "https://cal.com/");
    assert_eq!(
        normalize_url("localhost:3000/app").unwrap(),
        "http://localhost:3000/app"
    );
    assert_eq!(
        normalize_url("https://example.com/docs").unwrap(),
        "https://example.com/docs"
    );
    assert_eq!(
        normalize_url("javascript:alert(1)").unwrap_err().code,
        "usage"
    );
    assert_eq!(
        normalize_url("file:///etc/passwd").unwrap_err().code,
        "usage"
    );
    assert_eq!(normalize_url("  ").unwrap_err().code, "usage");
}

#[test]
fn inspect_with_none_driver_is_no_driver() {
    let mut cfg = common::allow_all();
    cfg.driver = DriverChoice::None;
    let err = run_inspect("https://cal.com/", &InspectOptions::default(), &cfg).unwrap_err();
    assert_eq!(err.code, "no_driver");
}

#[cfg(unix)]
#[test]
fn inspect_agent_browser_returns_tools_and_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let list_json = json!({
        "success": true,
        "data": {
            "tools": [
                {
                    "name": "searchFlights",
                    "description": "Search flights",
                    "inputSchema": {
                        "type": "object",
                        "required": ["origin"],
                        "properties": {"origin": {"type": "string"}}
                    },
                    "annotations": {"readOnlyHint": true}
                },
                {
                    "name": "bookFlight",
                    "description": "Book flight",
                    "inputSchema": {"type": "object"},
                    "annotations": {"readOnlyHint": false}
                }
            ]
        }
    })
    .to_string();
    let script = common::Fake::new()
        .lists(&[&list_json])
        .install(&dir, "agent-browser");
    let mut cfg = common::run_config(&script);
    cfg.driver = DriverChoice::AgentBrowser;

    let res = run_inspect("cal.com", &InspectOptions { fresh: true }, &cfg).unwrap();
    assert_eq!(res.driver, "agent-browser");
    assert_eq!(res.tools.len(), 2);
    let env = res.envelope();
    assert_eq!(env["success"], json!(true));
    assert_eq!(env["data"]["tools"][0]["name"], json!("searchFlights"));
    assert_eq!(env["data"]["tools"][0]["kind"], json!("read"));
    assert_eq!(env["data"]["tools"][0]["kind_inferred"], json!(false));
    assert_eq!(
        env["data"]["tools"][0]["call"]["input"],
        json!({"origin": ""})
    );
    assert_eq!(env["data"]["tools"][1]["kind"], json!("consequential"));
}

#[cfg(unix)]
#[test]
fn inspect_agent_browser_empty_list_returns_empty_not_error() {
    let dir = tempfile::tempdir().unwrap();
    let open_empty = r#"{"success":true,"data":{"url":"https://example.com/","title":"Example","webmcp":{"tools":[]}}}"#;
    let list_empty = r#"{"success":true,"data":{"tools":[]}}"#;
    let script = common::Fake::new()
        .open(open_empty)
        .lists(&[list_empty])
        .install(&dir, "agent-browser");
    let mut cfg = common::run_config(&script);
    cfg.driver = DriverChoice::AgentBrowser;
    cfg.call.settle_ms = 0;

    let res = run_inspect("https://example.com/", &InspectOptions::default(), &cfg).unwrap();
    assert!(res.tools.is_empty());
}

#[cfg(unix)]
#[test]
fn inspect_agent_browser_missing_api_is_webmcp_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let open_empty = r#"{"success":true,"data":{"url":"https://example.com/","title":"Example","webmcp":{"tools":[]}}}"#;
    let list_empty = r#"{"success":true,"data":{"tools":[]}}"#;
    let script = common::Fake::new()
        .open(open_empty)
        .lists(&[list_empty])
        .evals(&[r#"{"success":true,"data":{"result":"undefined"}}"#])
        .install(&dir, "agent-browser");
    let mut cfg = common::run_config(&script);
    cfg.driver = DriverChoice::AgentBrowser;

    let err = run_inspect("https://example.com/", &InspectOptions::default(), &cfg).unwrap_err();
    assert_eq!(err.code, "webmcp_unsupported");
}

#[cfg(unix)]
#[test]
fn inspect_browse_returns_tools_and_fresh_closes_tab() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let script = format!(
        "#!/bin/sh\n\
printf '%s\\n' \"$*\" >> '{}'\n\
case \"$1\" in\n\
  status) printf '{{\"connected\":true,\"relay\":{{\"version\":\"0.1.5\"}}}}\\n' ;;\n\
  navigate) printf '{{\"tab\":7,\"url\":\"https://cal.com/\",\"title\":\"Cal.com\"}}\\n' ;;\n\
  webmcp) printf '{{\"available\":true,\"tools\":[{{\"name\":\"ask_cal_com\",\"description\":\"Ask\",\"inputSchema\":{{\"type\":\"object\"}}}}]}}\\n' ;;\n\
  tabs) printf '{{\"ok\":true}}\\n' ;;\n\
esac\n",
        log.display()
    );
    let path = dir.path().join("browse");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut cfg = common::allow_all();
    cfg.driver = DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();

    let res = run_inspect("https://cal.com/", &InspectOptions { fresh: true }, &cfg).unwrap();
    assert_eq!(res.driver, "browse");
    assert_eq!(res.tools.len(), 1);
    assert_eq!(res.tools[0].name, "ask_cal_com");
    let logged = std::fs::read_to_string(log).unwrap();
    assert!(logged.contains("tabs --json"), "{logged}");
}

#[cfg(unix)]
#[test]
fn inspect_lightpanda_returns_tools() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = "#!/bin/sh\n\
if [ \"$1\" = \"version\" ]; then echo \"lightpanda 1.1.0\"; exit 0; fi\n\
printf '{\"ok\":true,\"tools\":[{\"name\":\"get_docs\",\"description\":\"Docs\",\"inputSchema\":{\"type\":\"object\"}}]}\\n'\n";
    let path = dir.path().join("lightpanda");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut cfg = config::Config::default();
    cfg.driver = DriverChoice::Lightpanda;
    cfg.drivers.lightpanda.command = path.display().to_string();

    let d = driver::by_choice(DriverChoice::Lightpanda, &cfg).unwrap();
    let out = d
        .inspect(
            "https://cal.com/",
            &RunOptions {
                timeout_ms: 5000,
                settle_ms: 300,
                fresh: false,
            },
        )
        .unwrap();
    assert_eq!(out.tools.len(), 1);
    assert_eq!(out.tools[0].name, "get_docs");
}

#[test]
fn mcp_inspect_tool_has_expected_schema() {
    let tool = agent_webmcp::mcp::inspect_tool();
    assert_eq!(tool.name.as_ref(), "inspect");
    assert_eq!(
        tool.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn mcp_server_lists_and_runs_inspect() {
    use agent_webmcp::index::Search;
    use agent_webmcp::mcp::Server;
    use agent_webmcp::model::Index;
    use rmcp::model::{
        CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
        InitializeResult, JsonObject, ProtocolVersion, ServerCapabilities,
    };
    use rmcp::service::{RoleClient, RoleServer};

    #[derive(Clone)]
    struct PlainClient;
    impl rmcp::ClientHandler for PlainClient {
        fn get_info(&self) -> rmcp::model::ClientConfig {
            InitializeRequestParams::new(
                ClientCapabilities::builder().build(),
                Implementation::new("inspect-test", "0.0.0"),
            )
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let list_json = json!({
        "success": true,
        "data": {
            "tools": [{
                "name": "searchFlights",
                "description": "Search flights",
                "inputSchema": {"type": "object"},
                "annotations": {"readOnlyHint": true}
            }]
        }
    })
    .to_string();
    let script = common::Fake::new()
        .lists(&[&list_json])
        .install(&dir, "agent-browser");
    let mut cfg = common::run_config(&script);
    cfg.driver = DriverChoice::AgentBrowser;

    let empty_index = Index {
        schema_version: 1,
        generated_at: "2026-11-20T00:00:00Z".into(),
        sites: vec![],
    };
    let server = Server::with_config(Search::build(empty_index).unwrap(), cfg);

    tokio::task::LocalSet::new()
        .run_until(async move {
            let (server_transport, client_transport) = tokio::io::duplex(8192);
            let client_init = InitializeRequestParams::new(
                ClientCapabilities::builder().build(),
                Implementation::new("inspect-test", "0.0.0"),
            )
            .with_protocol_version(ProtocolVersion::V_2026_07_28);
            let mut server_info =
                InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
            server_info.protocol_version = ProtocolVersion::V_2026_07_28;

            let server_task = tokio::task::spawn_local(async move {
                let running = rmcp::service::serve_directly::<RoleServer, _, _, _, _>(
                    server,
                    server_transport,
                    Some(client_init),
                );
                let _ = running.waiting().await;
            });
            let client = rmcp::service::serve_directly::<RoleClient, _, _, _, _>(
                PlainClient,
                client_transport,
                Some(server_info.into()),
            );

            let listed = client.list_tools(None).await.unwrap();
            let names: Vec<&str> = listed.tools.iter().map(|t| t.name.as_ref()).collect();
            assert_eq!(names, vec!["find", "inspect", "call"]);

            let mut map = JsonObject::new();
            map.insert("url".into(), json!("https://cal.com/"));
            map.insert("fresh".into(), json!(true));
            let res = client
                .call_tool(CallToolRequestParams::new("inspect").with_arguments(map))
                .await
                .unwrap();
            let env = res.structured_content.expect("structured envelope");
            assert_eq!(env["success"], json!(true));
            assert_eq!(env["data"]["driver"], json!("agent-browser"));
            assert_eq!(env["data"]["tools"][0]["name"], json!("searchFlights"));

            server_task.abort();
        })
        .await;
}
