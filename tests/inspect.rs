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
        normalize_url("https://example.com?view=tools").unwrap(),
        "https://example.com/?view=tools"
    );
    assert_eq!(
        normalize_url("localhost/app").unwrap(),
        "http://localhost/app"
    );
    assert_eq!(
        normalize_url("localhost?view=tools").unwrap(),
        "http://localhost/?view=tools"
    );
    assert_eq!(
        normalize_url("localhost?next=https://example.com").unwrap(),
        "http://localhost/?next=https://example.com"
    );
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
fn inspect_reload_timeout_stops_inspect() {
    // A reload that times out closes the session: the timeout propagates
    // instead of listing and probing custom availability against a fresh
    // browser under the same session name.
    let dir = tempfile::tempdir().unwrap();
    let open_empty = r#"{"success":true,"data":{"url":"https://example.com/","title":"Example","webmcp":{"tools":[]}}}"#;
    let list_empty = r#"{"success":true,"data":{"tools":[]}}"#;
    let script = common::Fake::new()
        .open(open_empty)
        .lists(&[list_empty])
        .evals(&[r#"{"success":true,"data":{"result":"object"}}"#])
        .stall("reload", 30)
        .install(&dir, "agent-browser");
    let mut cfg = common::run_config(&script);
    cfg.driver = DriverChoice::AgentBrowser;
    cfg.call.timeout = 200;
    cfg.call.settle_ms = 1000;
    let err = run_inspect("https://example.com/", &InspectOptions::default(), &cfg).unwrap_err();
    assert_eq!(err.code, "timeout", "{err:?}");
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
    // Exec-visibility barrier: a just-written script can answer ETXTBSY
    // under concurrency. Warm it up once so detection never sees a busy
    // exec as a missing driver.
    let _ = std::fs::read(&path);
    for _ in 0..100 {
        match std::process::Command::new(&path).output() {
            Ok(_) => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
        }
    }

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
            &[],
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
#[test]
fn inspect_fragment_page_emits_resolvable_target() {
    // The advertised `call.target` keeps the page route and resolves back
    // to the same tool on the same page.
    let dir = tempfile::tempdir().unwrap();
    let open = r#"{"success":true,"data":{"url":"https://cal.com/dash#/route","lifecycle":{"reused":true},"webmcp":{"tools":[]}}}"#;
    let list = r#"{"success":true,"data":{"tools":[{"name":"ask_cal_com","description":"Ask","inputSchema":{"type":"object"}}]}}"#;
    let script = common::Fake::new()
        .open(open)
        .lists(&[list])
        .install(&dir, "agent-browser");
    let mut cfg = common::run_config(&script);
    cfg.driver = DriverChoice::AgentBrowser;
    let res = run_inspect(
        "https://cal.com/dash#/route",
        &InspectOptions::default(),
        &cfg,
    )
    .unwrap();
    assert_eq!(res.page, "https://cal.com/dash#/route");
    let env = res.envelope();
    let target = env["data"]["tools"][0]["call"]["target"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(target, "https://cal.com/dash#/route#ask_cal_com");
    let resolved = agent_webmcp::call::resolve(&target, &common::sample_search()).unwrap();
    assert_eq!(resolved.tool, "ask_cal_com");
    assert_eq!(resolved.page, "https://cal.com/dash#/route");
}

#[cfg(unix)]
#[test]
fn inspect_http_with_persistent_session_is_denied() {
    // agent-browser reuses its named session: an http:// inspect would send
    // its cookies in cleartext, so it is refused before any navigation.
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().install(&dir, "agent-browser");
    let mut cfg = common::run_config(&script);
    cfg.driver = DriverChoice::AgentBrowser;
    let err = run_inspect("http://example.com/", &InspectOptions::default(), &cfg).unwrap_err();
    assert_eq!(err.code, "denied", "{err:?}");
    assert!(err.message.contains("https"), "{err:?}");
    assert!(
        !std::fs::read_to_string(dir.path().join("agent-browser.argv"))
            .unwrap_or_default()
            .lines()
            .any(|l| l.contains(" open ")),
        "no driver navigation may run"
    );
    // A throwaway session carries no prior cookies, so --fresh stays allowed.
    let res = run_inspect(
        "http://example.com/",
        &InspectOptions { fresh: true },
        &cfg,
    )
    .unwrap();
    assert_eq!(res.driver, "agent-browser");
}

#[cfg(unix)]
#[test]
fn inspect_http_with_browse_is_denied() {
    // Browse always runs in the user's personal (persistent) profile, so
    // http:// inspection is refused even with `fresh` (which only closes
    // the tab afterwards).
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let script = format!(
        "#!/bin/sh\n\
printf '%s\\n' \"$*\" >> '{}'\n\
case \"$1\" in\n\
  status) printf '{{\"connected\":true,\"relay\":{{\"version\":\"0.1.5\"}}}}\\n' ;;\n\
  navigate) printf '{{\"tab\":7,\"url\":\"https://cal.com/\",\"title\":\"Cal.com\"}}\\n' ;;\n\
esac\n",
        log.display()
    );
    let path = dir.path().join("browse");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Exec-visibility barrier: a just-written script can answer ETXTBSY
    // under concurrency. Warm it up once so detection never sees a busy
    // exec as a missing driver.
    let _ = std::fs::read(&path);
    for _ in 0..100 {
        match std::process::Command::new(&path).output() {
            Ok(_) => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
        }
    }

    let mut cfg = common::allow_all();
    cfg.driver = DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    let err = run_inspect("http://cal.com/", &InspectOptions { fresh: true }, &cfg).unwrap_err();
    assert_eq!(err.code, "denied", "{err:?}");
    let logged = std::fs::read_to_string(log).unwrap_or_default();
    assert!(
        !logged.contains("navigate"),
        "no personal-Chrome navigation over http: {logged}"
    );
}

#[cfg(unix)]
#[test]
fn inspect_http_with_profile_or_cdp_is_denied_even_when_fresh() {
    // A configured profile or CDP endpoint carries persistent state even
    // into a throwaway session, so plain HTTP stays refused with `fresh`.
    for configure in [
        (|cfg: &mut config::Config| {
            cfg.drivers.agent_browser.profile = Some("persistent-profile".into());
        }) as fn(&mut config::Config),
        |cfg: &mut config::Config| {
            cfg.drivers.agent_browser.cdp = Some("http://127.0.0.1:9222".into());
        },
    ] {
        let dir = tempfile::tempdir().unwrap();
        let script = common::Fake::new().install(&dir, "agent-browser");
        let mut cfg = common::run_config(&script);
        cfg.driver = DriverChoice::AgentBrowser;
        configure(&mut cfg);
        let err = run_inspect(
            "http://example.com/",
            &InspectOptions { fresh: true },
            &cfg,
        )
        .unwrap_err();
        assert_eq!(err.code, "denied", "{err:?}");
        assert!(err.message.contains("https"), "{err:?}");
    }
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
            assert_ne!(res.is_error, Some(true));
            let env = res.structured_content.expect("structured envelope");
            assert_eq!(env["success"], json!(true));
            assert_eq!(env["data"]["driver"], json!("agent-browser"));
            assert_eq!(env["data"]["tools"][0]["name"], json!("searchFlights"));

            server_task.abort();
        })
        .await;
}
