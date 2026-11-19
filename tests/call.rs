//! `call`: resolve, input, permissions, fake-driver runs, CLI, MCP, find.
//! Real network/browser is never used: agent-browser is a fake script.

mod common;

use agent_webmcp::call::{
    self, CallError, CallOptions, Decision, Outcome, ToolKind, decide, example_input, execute,
    glob_matches, parse_input, resolve, run_mcp, validate_input,
};
use agent_webmcp::config;
use agent_webmcp::index::Search;
use agent_webmcp::model::{Annotations, Index, Site, Tool};
use common::{
    HomeGuard, allow_all, ask_schema, ask_tool, plan_opts, read_tool, run_config,
    run_config_settle, run_opts, sample_search, sample_site,
};
use serde_json::{Value, json};

fn consequential() -> Annotations {
    common::consequential()
}

fn site(host: &str, tools: Vec<Tool>) -> Site {
    sample_site(host, tools)
}

fn index() -> Index {
    Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![site("cal.com", vec![ask_tool(), read_tool()])],
    }
}

fn search() -> Search {
    sample_search()
}

fn opts() -> CallOptions {
    plan_opts()
}

// --- resolve ---

#[test]
fn resolve_site_tool_found() {
    let r = resolve("cal.com/ask_cal_com", &search()).unwrap();
    assert_eq!(r.page, "https://cal.com/");
    assert_eq!(r.tool, "ask_cal_com");
    assert_eq!(r.site, "cal.com");
    assert_eq!(r.schema, Some(ask_schema()));
    assert_eq!(r.kind, ToolKind::Consequential);
    assert_eq!(r.id(), "cal.com/ask_cal_com");
}

#[test]
fn resolve_unknown_tool_suggests_find() {
    let e = resolve("cal.com/nope", &search()).unwrap_err();
    assert_eq!(e.code, "tool_not_found");
    assert!(
        e.message.contains("agent-webmcp find cal.com"),
        "{}",
        e.message
    );
}

#[test]
fn resolve_url_with_index_entry_uses_schema() {
    let r = resolve("https://cal.com/book#ask_cal_com", &search()).unwrap();
    assert_eq!(r.page, "https://cal.com/book");
    assert_eq!(r.site, "cal.com");
    assert_eq!(r.schema, Some(ask_schema()));
    assert_eq!(r.kind, ToolKind::Consequential);
}

#[test]
fn resolve_url_without_index_entry_is_unspecified() {
    let r = resolve("https://news.example/article#read_later", &search()).unwrap();
    assert_eq!(r.page, "https://news.example/article");
    assert_eq!(r.site, "news.example");
    assert_eq!(r.schema, None);
    assert_eq!(r.kind, ToolKind::Unspecified);
}

#[test]
fn resolve_garbage_is_usage() {
    for target in [
        "nonsense",
        "",
        "cal.com/",
        "/ask_cal_com",
        "https://cal.com/nontool",
    ] {
        let e = resolve(target, &search()).unwrap_err();
        assert_eq!(e.code, "usage", "{target}");
        assert!(e.message.contains("site/tool"), "{target}");
    }
}

// --- input ---

#[test]
fn positional_and_params_together_is_usage() {
    let e = parse_input(Some("{}"), Some("{}")).unwrap_err();
    assert_eq!(e.code, "usage");
}

#[test]
fn params_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.json");
    std::fs::write(&path, r#"{"question": "hi"}"#).unwrap();
    let value = parse_input(None, Some(&format!("@{}", path.display()))).unwrap();
    assert_eq!(value, json!({"question": "hi"}));
}

#[test]
fn missing_params_file_is_invalid_input() {
    let e = parse_input(None, Some("@/no/such/file.json")).unwrap_err();
    assert_eq!(e.code, "invalid_input");
    assert!(e.message.contains("/no/such/file.json"), "{}", e.message);
}

#[test]
fn non_object_input_is_invalid() {
    for text in ["[1, 2]", "42", "\"x\""] {
        let e = parse_input(Some(text), None).unwrap_err();
        assert_eq!(e.code, "invalid_input", "{text}");
    }
    assert_eq!(
        parse_input(None, None).unwrap(),
        json!({}),
        "neither means empty object"
    );
}

#[test]
fn validation_lists_every_problem() {
    let schema = json!({
        "type": "object",
        "required": ["question", "n"],
        "properties": {
            "question": {"type": "string"},
            "n": {"type": "integer", "enum": [1, 2]},
            "extra": {"type": "boolean"}
        },
        "additionalProperties": false
    });
    let e = validate_input(&schema, &json!({"n": "x", "surprise": 1})).unwrap_err();
    assert_eq!(e.code, "invalid_input");
    for problem in [
        "missing required property \"question\"",
        "expected integer for \"n\", got string",
        "\"n\" must be one of",
        "unknown property \"surprise\"",
    ] {
        assert!(e.message.contains(problem), "{}\n{}", problem, e.message);
    }
    assert_eq!(e.message.lines().count(), 4, "{}", e.message);
}

#[test]
fn validation_type_arrays_and_kinds() {
    let schema = json!({
        "type": "object",
        "properties": {
            "a": {"type": ["string", "null"]},
            "b": {"type": "number"},
            "c": {"type": "boolean"},
            "d": {"type": "array"},
            "e": {"type": "object"}
        }
    });
    validate_input(
        &schema,
        &json!({"a": null, "b": 1.5, "c": true, "d": [], "e": {}}),
    )
    .unwrap();
    validate_input(&schema, &json!({"b": 2})).unwrap();
    let e = validate_input(&schema, &json!({"a": 1})).unwrap_err();
    assert!(
        e.message.contains("expected string or null"),
        "{}",
        e.message
    );
}

// --- permissions ---

#[test]
fn glob_matches_across_slash_and_dot() {
    assert!(glob_matches("*", "cal.com/ask_cal_com"));
    assert!(glob_matches("cal.com/*", "cal.com/ask_cal_com"));
    assert!(!glob_matches("cal.com/*", "other.com/x"));
    assert!(glob_matches("*/checkout*", "shop.example/a/checkout2"));
    assert!(glob_matches("bank.example/*", "bank.example/x"));
    assert!(!glob_matches("bank.example/*", "bank.example.evil/x"));
    assert!(glob_matches("exact", "exact"));
    assert!(!glob_matches("exact", "exact2"));
}

#[test]
fn deny_beats_ask_beats_allow() {
    let mut perms = config::Permissions::default();
    perms.allow.push("*".into());
    perms.ask.push("cal.com/*".into());
    perms.deny.push("*/secret*".into());
    assert_eq!(
        decide("other.example/x", ToolKind::Read, &perms),
        Decision::Allow
    );
    assert_eq!(
        decide("cal.com/ask_cal_com", ToolKind::Read, &perms),
        Decision::Ask
    );
    assert_eq!(
        decide("cal.com/secret_tool", ToolKind::Read, &perms),
        Decision::Deny
    );
}

#[test]
fn default_decision_comes_from_kind() {
    let perms = config::Permissions::default();
    assert_eq!(decide("x/y", ToolKind::Read, &perms), Decision::Allow);
    assert_eq!(
        decide("x/y", ToolKind::Consequential, &perms),
        Decision::Ask
    );
    assert_eq!(decide("x/y", ToolKind::Unspecified, &perms), Decision::Ask);
}

#[test]
fn ask_without_confirm_stops_before_any_plan() {
    let cfg = config::Config::default();
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(
        &resolved,
        json!({"question": "hi"}),
        &CallOptions {
            confirm: false,
            plan_only: true,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        },
        &cfg,
    )
    .unwrap_err();
    assert_eq!(e.code, "needs_confirmation");
    assert_eq!(e.data.as_ref().unwrap()["retry"], json!("add --confirm"));
}

#[test]
fn deny_never_produces_a_plan() {
    let mut cfg = config::Config::default();
    cfg.permissions.deny = vec!["cal.com/*".into()];
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "denied");
    assert!(e.message.contains("cal.com/*"), "{}", e.message);
    assert!(e.message.contains("from config"), "{}", e.message);
}

#[test]
fn ask_with_confirm_proceeds() {
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &opts(), &allow_all()) {
        Ok(Outcome::Planned { .. }) => {}
        other => panic!("expected a plan, got {other:?}"),
    }
}

// --- choose integration (no scripts) ---

#[test]
fn driver_none_plans_generic() {
    let mut cfg = allow_all();
    cfg.driver = config::DriverChoice::None;
    let resolved = resolve("cal.com/list_slots", &search()).unwrap();
    match execute(&resolved, json!({}), &opts(), &cfg) {
        Ok(Outcome::Planned { plan, .. }) => assert_eq!(plan.driver, "generic"),
        other => panic!("expected a generic plan, got {other:?}"),
    }
}

#[test]
fn missing_named_driver_is_no_driver() {
    let mut cfg = allow_all();
    cfg.driver = config::DriverChoice::AgentBrowser;
    cfg.drivers.agent_browser.command = "agent-webmcp-definitely-missing".into();
    let resolved = resolve("cal.com/list_slots", &search()).unwrap();
    let opts = run_opts();
    let e = execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "no_driver");
    assert!(e.message.contains("npm i -g"), "{}", e.message);
}

// --- example input + find ---

#[test]
fn example_input_placeholders_by_type() {
    let schema = json!({
        "type": "object",
        "required": ["q", "n", "f", "l", "o", "skip"],
        "properties": {
            "q": {"type": "string"},
            "n": {"type": "integer"},
            "f": {"type": "number"},
            "l": {"type": "array"},
            "o": {"type": "object"},
            "skip": {"type": "string"}
        }
    });
    let mut schema = schema;
    schema["required"] = json!(["q", "n", "f", "l", "o"]);
    assert_eq!(
        example_input(&schema),
        json!({"q": "", "n": 0, "f": 0, "l": [], "o": {}})
    );
}

#[test]
fn find_hit_carries_call() {
    let hits = search().find("cal.com", 5).unwrap();
    let hit = hits
        .iter()
        .find(|h| h.id == "cal.com/ask_cal_com")
        .expect("fixture hit missing");
    assert_eq!(hit.call.target, "cal.com/ask_cal_com");
    assert_eq!(hit.call.input, json!({"question": ""}));
    let json = serde_json::to_value(&hits).unwrap();
    assert_eq!(
        json[0]["call"]["target"],
        json!("cal.com/ask_cal_com"),
        "{json:?}"
    );
}

// --- inferred read ---

#[test]
fn name_inference_table() {
    let none = Annotations::default();
    for name in ["ask_cal_com", "searchFlights", "search_docs", "read_page"] {
        assert_eq!(call::kind_of(name, &none), (ToolKind::Read, true), "{name}");
        assert!(call::inferred_read(name), "{name}");
    }
    for name in [
        "create_order",
        "get_and_delete",
        "manage_pizza",
        "checkout",
        "do_thing",
    ] {
        assert_eq!(
            call::kind_of(name, &none),
            (ToolKind::Unspecified, false),
            "{name}"
        );
    }
    // Explicit hints always win over the name.
    assert_eq!(
        call::kind_of("search_x", &consequential()),
        (ToolKind::Consequential, false)
    );
    assert_eq!(
        call::kind_of(
            "delete_y",
            &Annotations {
                read_only: true,
                ..Default::default()
            }
        ),
        (ToolKind::Read, false)
    );
    assert_eq!(call::kind_str(ToolKind::Read), "read");
}

fn inferred_index() -> Index {
    Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![site(
            "help.example",
            vec![Tool {
                name: "lookup_faq".into(),
                description: "Look up answers.".into(),
                input_schema: json!({"type": "object"}),
                page: "https://help.example/".into(),
                origin: "https://help.example".into(),
                annotations: Annotations::default(),
                flags: vec![],
            }],
        )],
    }
}

#[test]
fn inferred_read_is_allowed_without_confirm() {
    let search = Search::build(inferred_index()).unwrap();
    let resolved = resolve("help.example/lookup_faq", &search).unwrap();
    assert_eq!(resolved.kind, ToolKind::Read);
    assert!(resolved.kind_inferred);
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::None;
    match execute(&resolved, json!({}), &opts(), &cfg) {
        Ok(Outcome::Planned { plan, .. }) => assert_eq!(plan.driver, "generic"),
        other => panic!("inferred read should be allowed, got {other:?}"),
    }
}

#[test]
fn ask_pattern_still_stops_inferred_read() {
    let search = Search::build(inferred_index()).unwrap();
    let resolved = resolve("help.example/lookup_faq", &search).unwrap();
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::None;
    cfg.permissions.ask.push("help.example/*".into());
    let e = execute(
        &resolved,
        json!({}),
        &CallOptions {
            confirm: false,
            plan_only: true,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        },
        &cfg,
    )
    .unwrap_err();
    assert_eq!(e.code, "needs_confirmation");
}

#[test]
fn find_marks_inferred_kind() {
    let search = Search::build(inferred_index()).unwrap();
    let hits = search.find("help.example faq", 5).unwrap();
    let hit = hits
        .iter()
        .find(|h| h.id == "help.example/lookup_faq")
        .expect("fixture hit missing");
    assert_eq!(hit.kind, "read");
    assert!(hit.kind_inferred);
    let json = serde_json::to_value(hit).unwrap();
    assert_eq!(json["kind"], json!("read"));
    assert_eq!(json["kind_inferred"], json!(true));
}

// --- envelope shapes + exit codes ---

#[test]
fn failure_envelope_shape() {
    let e = CallError::usage("nope");
    assert_eq!(
        e.envelope(),
        json!({"success": false, "code": "usage", "error": "nope", "data": null})
    );
    assert_eq!(call::exit_code(&e), 2);
    let e = CallError {
        code: "denied",
        message: "no".into(),
        data: None,
    };
    assert_eq!(call::exit_code(&e), 1);
}

// --- fake agent-browser runs (unix): the one shared builder lives in
// `common` (`Fake`), with real envelope shapes as named constants. ---

#[cfg(unix)]
#[test]
fn happy_path_returns_output_reused_and_removes_params() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_OK])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = run_opts();
    match execute(&resolved, json!({"question": "hi"}), &opts, &cfg) {
        Ok(Outcome::Ran {
            output,
            reused,
            driver,
            ..
        }) => {
            assert_eq!(output, json!({"answer": 1}));
            assert!(reused);
            assert_eq!(driver, "agent-browser");
        }
        other => panic!("expected a run, got {other:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    let open = lines.iter().find(|l| l.contains(" open ")).unwrap();
    assert!(open.contains("--session test-s-"), "{open}");
    assert!(open.contains("open https://cal.com/"), "{open}");
    assert!(
        open.contains("--args --enable-features=WebMCP"),
        "open launches with the WebMCP feature: {open}"
    );
    assert!(!open.contains("--init-script"), "{open}");
    assert!(!open.contains("--profile"), "{open}");
    let invoke = lines.iter().find(|l| l.contains(" invoke ")).unwrap();
    assert!(invoke.contains("--session test-s-"), "{invoke}");
    assert!(invoke.contains("--timeout 1234"), "{invoke}");
    assert!(!invoke.contains("--detach"), "{invoke}");
    assert!(!invoke.contains("--profile"), "{invoke}");
    let at = invoke
        .split_whitespace()
        .find(|w| w.starts_with('@'))
        .unwrap();
    assert!(
        !std::path::Path::new(&at[1..]).exists(),
        "params file must be removed"
    );
}

#[cfg(unix)]
#[test]
fn tool_is_error_is_tool_error() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_TOOL_ERROR])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = run_opts();
    let e = execute(&resolved, json!({"question": "hi"}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "tool_error");
    assert_eq!(e.message, "boom");
}

#[cfg(unix)]
#[test]
fn tool_not_found_invokes_once_without_retry() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    // The second queued answer would succeed: a retry would use it.
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_NOT_FOUND, common::INVOKE_OK])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = run_opts();
    let e = execute(&resolved, json!({"question": "hi"}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "tool_not_found");
    assert!(e.message.contains("may need a login"), "{}", e.message);
    let lines = common::Fake::argv(&dir, "ab");
    assert_eq!(
        lines.iter().filter(|l| l.contains(" open ")).count(),
        1,
        "no re-open: {lines:?}"
    );
    assert_eq!(
        lines.iter().filter(|l| l.contains(" invoke ")).count(),
        1,
        "no retry: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn non_json_stdout_is_driver_error() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&["not json at all"])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = run_opts();
    let e = execute(&resolved, json!({"question": "hi"}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "driver_error");
    assert!(e.message.contains("not JSON"), "{}", e.message);
}

#[cfg(unix)]
#[test]
fn sleeping_driver_times_out() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .stall("invoke", 30)
        .install(&dir, "ab");
    let mut cfg = run_config(&script);
    cfg.call.timeout = 200;
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = run_opts();
    let e = execute(&resolved, json!({"question": "hi"}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "timeout");
}

#[cfg(unix)]
#[test]
fn fresh_uses_a_unique_session_and_closes() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_OK])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: true,
        retry_hint: call::RETRY_CLI,
    };
    match execute(&resolved, json!({"question": "hi"}), &opts, &cfg) {
        Ok(Outcome::Ran { .. }) => {}
        other => panic!("expected a run, got {other:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    let open = lines.iter().find(|l| l.contains(" open ")).unwrap();
    let session = open
        .split_whitespace()
        .skip_while(|w| *w != "--session")
        .nth(1)
        .unwrap();
    assert_ne!(session, "test-s");
    assert!(
        session.starts_with(&format!("agent-webmcp-{}-", std::process::id())),
        "{session}"
    );
    let close = lines
        .iter()
        .find(|l| l.split_whitespace().last() == Some("close"))
        .unwrap();
    assert!(close.contains(session), "{close}");
}

#[cfg(unix)]
#[test]
fn profile_goes_on_open_only() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_OK])
        .install(&dir, "ab");
    let mut cfg = run_config(&script);
    cfg.drivers.agent_browser.profile = Some("/tmp/p".into());
    cfg.drivers.agent_browser.cdp = Some("http://127.0.0.1:1".into());
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = run_opts();
    match execute(&resolved, json!({"question": "hi"}), &opts, &cfg) {
        Ok(Outcome::Ran { .. }) => {}
        other => panic!("expected a run, got {other:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    let open = lines.iter().find(|l| l.contains(" open ")).unwrap();
    assert!(open.contains("--profile /tmp/p"), "{open}");
    assert!(open.contains("--cdp http://127.0.0.1:1"), "{open}");
    let invoke = lines.iter().find(|l| l.contains(" invoke ")).unwrap();
    assert!(!invoke.contains("--profile"), "{invoke}");
    assert!(!invoke.contains("--cdp"), "{invoke}");
}

#[cfg(unix)]
#[test]
fn unreachable_page_is_page_unreachable() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_UNREACHABLE)
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let opts = run_opts();
    let e = execute(&resolved, json!({"question": "hi"}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "page_unreachable");
}

#[cfg(unix)]
#[test]
fn plan_with_ready_fake_is_agent_browser_plan() {
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().install(&dir, "ab");
    let mut cfg = run_config(&script);
    cfg.driver = config::DriverChoice::AgentBrowser;
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &opts(), &cfg) {
        Ok(Outcome::Planned { plan, .. }) => {
            assert_eq!(plan.driver, "agent-browser");
            assert_eq!(plan.steps.len(), 2);
            assert!(plan.steps.iter().all(|s| s.tool == "shell"));
        }
        other => panic!("expected a plan, got {other:?}"),
    }
}

// --- CLI (binary, env-isolated) ---

fn fixture_index(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("index.json");
    std::fs::write(&path, serde_json::to_string(&index()).unwrap()).unwrap();
    path
}

fn cli_base(dir: &tempfile::TempDir, index: &std::path::Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_agent-webmcp"));
    cmd.env_clear()
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", "")
        .env("AGENT_WEBMCP_INDEX", index);
    cmd
}

#[test]
fn cli_plan_envelope_and_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let mut cmd = cli_base(&dir, &index);
    cmd.args([
        "call",
        "cal.com/ask_cal_com",
        r#"{"question":"x"}"#,
        "--plan",
        "--confirm",
        "--json",
    ]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["success"], json!(true));
    assert_eq!(value["data"]["id"], json!("cal.com/ask_cal_com"));
    assert_eq!(value["data"]["plan"]["driver"], json!("generic"));
    assert_eq!(value["data"]["plan"]["steps"][1]["tool"], json!("call"));
}

#[test]
fn cli_needs_confirmation_exit_one() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let mut cmd = cli_base(&dir, &index);
    cmd.args(["call", "cal.com/ask_cal_com", r#"{"question":"x"}"#]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("needs_confirmation"), "{stderr}");
}

#[test]
fn cli_bad_driver_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let mut cmd = cli_base(&dir, &index);
    cmd.args(["call", "cal.com/ask_cal_com", "--driver", "nope"]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown driver"), "{stderr}");
}

#[test]
fn cli_both_inputs_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let mut cmd = cli_base(&dir, &index);
    cmd.args([
        "call",
        "cal.com/ask_cal_com",
        r#"{"question":"x"}"#,
        "--params",
        r#"{"question":"y"}"#,
    ]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

#[test]
fn cli_params_file_and_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let input = dir.path().join("input.json");
    std::fs::write(&input, r#"{"question":"file"}"#).unwrap();
    let mut cmd = cli_base(&dir, &index);
    cmd.args([
        "call",
        "cal.com/ask_cal_com",
        "--params",
        &format!("@{}", input.display()),
        "--plan",
        "--confirm",
        "--json",
    ]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["data"]["plan"]["steps"][0]["args"],
        json!({"url": "https://cal.com/"}),
        "{value:?}"
    );

    let mut child = cli_base(&dir, &index)
        .args([
            "call",
            "cal.com/ask_cal_com",
            "--params",
            "@-",
            "--plan",
            "--confirm",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"question":"stdin"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("stdin"), "{stdout}");
}

// --- MCP ---

#[test]
fn mcp_call_tool_is_listed_with_schema() {
    let tool = agent_webmcp::mcp::call_tool();
    assert_eq!(tool.name, "call");
    assert!(
        tool.description
            .as_deref()
            .unwrap()
            .contains("agent-browser")
    );
    let schema = tool.input_schema.as_ref();
    assert_eq!(schema["required"], json!(["target"]), "{schema:?}");
    assert!(schema["properties"]["confirm"].is_object());
    let annotations = tool.annotations.unwrap();
    assert_eq!(annotations.read_only_hint, Some(false));
    assert_eq!(annotations.destructive_hint, Some(true));
    assert_eq!(annotations.open_world_hint, Some(true));
}

#[test]
fn mcp_pipeline_with_driver_none_returns_plan() {
    let mut cfg = allow_all();
    cfg.driver = config::DriverChoice::None;
    let (envelope, is_error) = run_mcp(
        "cal.com/ask_cal_com",
        json!({"question": "hi"}),
        false,
        &cfg,
        &search(),
    );
    assert!(!is_error);
    assert_eq!(envelope["success"], json!(true));
    assert_eq!(envelope["data"]["plan"]["driver"], json!("generic"));
}

#[test]
fn mcp_pipeline_needs_confirmation_without_confirm() {
    let cfg = config::Config::default();
    let (envelope, is_error) = run_mcp(
        "cal.com/ask_cal_com",
        json!({"question": "hi"}),
        false,
        &cfg,
        &search(),
    );
    assert!(is_error);
    assert_eq!(envelope["code"], json!("needs_confirmation"));
    assert_eq!(
        envelope["data"]["retry"],
        json!("call again with confirm: true")
    );
}

#[cfg(unix)]
#[test]
fn announced_tool_skips_settle() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_OK])
        .install(&dir, "ab");
    let cfg = run_config_settle(&script, 3000);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg) {
        Ok(Outcome::Ran { output, .. }) => assert_eq!(output, json!({"answer": 1})),
        other => panic!("expected a run, got {other:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    assert!(
        !lines.iter().any(|l| l.contains(" list ")),
        "no settle polls for an announced tool: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains(" invoke ")), "{lines:?}");
}

#[cfg(unix)]
#[test]
fn late_tool_found_on_third_list() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_PLAIN)
        .lists(&[
            common::LIST_EMPTY,
            common::LIST_EMPTY,
            common::LIST_WITH_TOOL,
        ])
        .invoke(&[common::INVOKE_OK])
        .evals(&[common::EVAL_OBJECT])
        .install(&dir, "ab");
    let cfg = run_config_settle(&script, 5000);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg) {
        Ok(Outcome::Ran {
            output,
            duration_ms,
            ..
        }) => {
            assert_eq!(output, json!({"answer": 1}));
            // One 500 ms settle sleep happened between the empty polls.
            assert!(duration_ms >= 450, "settle waits count: {duration_ms}");
        }
        other => panic!("expected a run, got {other:?}"),
    }
    let lists = common::Fake::argv(&dir, "ab")
        .iter()
        .filter(|l| l.contains(" list "))
        .count();
    assert_eq!(lists, 3, "tool appears on the third poll");
}

#[cfg(unix)]
#[test]
fn missing_tool_reloads_once_then_not_found() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_PLAIN)
        .lists(&[common::LIST_EMPTY])
        .evals(&[common::EVAL_ERROR])
        .install(&dir, "ab");
    let cfg = run_config_settle(&script, 1000);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "tool_not_found");
    assert!(e.message.contains("did not register"), "{}", e.message);
    let lines = common::Fake::argv(&dir, "ab");
    let opens = lines.iter().filter(|l| l.contains(" open ")).count();
    assert_eq!(opens, 1, "no re-open; reload replaces it: {lines:?}");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.split_whitespace().last() == Some("reload"))
            .count(),
        1,
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains(" wait ")),
        "wait for load: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains(" invoke ")),
        "never invoke a missing tool: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn settle_zero_skips_list() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_PLAIN)
        .lists(&[common::LIST_WITH_TOOL])
        .invoke(&[common::INVOKE_OK])
        .install(&dir, "ab");
    let mut cfg = run_config(&script);
    cfg.call.settle_ms = 0;
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg) {
        Ok(Outcome::Ran { .. }) => {}
        other => panic!("expected a run, got {other:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    let invoke_at = lines.iter().position(|l| l.contains(" invoke "));
    let list_at = lines.iter().position(|l| l.contains(" list "));
    assert!(invoke_at.is_some(), "{lines:?}");
    assert!(
        list_at.is_none() || list_at > invoke_at,
        "no list before first invoke: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn blocked_title_stops_before_invoke() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_BLOCKED)
        .lists(&[common::LIST_EMPTY])
        .install(&dir, "ab");
    let cfg = run_config_settle(&script, 5000);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "page_unreachable");
    assert!(e.message.contains("bot check"), "{}", e.message);
    let lines = common::Fake::argv(&dir, "ab");
    assert!(
        !lines.iter().any(|l| l.contains(" invoke ")),
        "blocked pages are never invoked: {lines:?}"
    );
}

#[test]
fn blocked_check_matches_scanner_signals() {
    use agent_webmcp::driver::agent_browser::is_blocked_page;
    for title in [
        "Just a moment...",
        "Attention Required! | Cloudflare",
        "Access Denied",
        "Verify you are human",
        "Are you a robot?",
    ] {
        assert!(is_blocked_page(title, "https://x.example/", ""), "{title}");
    }
    assert!(is_blocked_page("", "https://x.example/cf-challenge", ""));
    assert!(is_blocked_page(
        "",
        "https://x.example/",
        "please solve the captcha"
    ));
    assert!(is_blocked_page(
        "",
        "https://x.example/",
        "blocked by a Cloudflare bot challenge"
    ));
    for (title, url, body) in [
        ("Cal.com", "https://cal.com/", ""),
        ("", "https://cal.com/book", "book a meeting"),
        ("A momentary thought", "https://blog.example/", "an essay"),
    ] {
        assert!(!is_blocked_page(title, url, body), "{title} {url}");
    }
}

#[cfg(unix)]
#[test]
fn extra_args_come_before_the_webmcp_flag() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_PLAIN)
        .lists(&[common::LIST_WITH_TOOL])
        .invoke(&[common::INVOKE_OK])
        .install(&dir, "ab");
    let mut cfg = run_config(&script);
    cfg.drivers.agent_browser.extra_args = vec!["first".into(), "--flag=x".into()];
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg) {
        Ok(Outcome::Ran { .. }) => {}
        other => panic!("expected a run, got {other:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    let open = lines.iter().find(|l| l.contains(" open ")).unwrap();
    let flat = open.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("--args first --args --flag=x --args --enable-features=WebMCP"),
        "{open}"
    );
}

#[cfg(unix)]
#[test]
fn announced_tools_skip_the_probe() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_OTHER)
        .lists(&[common::LIST_EMPTY])
        .install(&dir, "ab");
    let cfg = run_config_settle(&script, 3000);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "tool_not_found");
    assert!(e.message.contains("did not register"), "{}", e.message);
    let lines = common::Fake::argv(&dir, "ab");
    assert!(
        !lines
            .iter()
            .any(|l| l.contains("typeof document.modelContext")),
        "no API probe when open announced tools: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn missing_api_is_webmcp_unsupported() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_PLAIN)
        .lists(&[common::LIST_EMPTY])
        .evals(&[common::EVAL_UNDEFINED])
        .install(&dir, "ab");
    let cfg = run_config_settle(&script, 3000);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "webmcp_unsupported");
    assert!(
        e.message.contains("--enable-features=WebMCP"),
        "{}",
        e.message
    );
    let lines = common::Fake::argv(&dir, "ab");
    assert_eq!(
        lines.iter().filter(|l| l.contains(" list ")).count(),
        1,
        "probe runs after the first empty list: {lines:?}"
    );
    assert!(!lines.iter().any(|l| l.contains(" invoke ")), "{lines:?}");
}

#[cfg(unix)]
#[test]
fn big_stdout_does_not_deadlock() {
    let _home = HomeGuard::isolate();
    let big = "x".repeat(200 * 1024);
    let invoke = format!(
        "{{\"success\":true,\"data\":{{\"output\":{{\"answer\":\"{big}\"}},\"durationMs\":5}}}}"
    );
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[&invoke])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg) {
        Ok(Outcome::Ran { output, .. }) => {
            assert_eq!(output["answer"], json!(big));
        }
        other => panic!("expected a run, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn huge_stdout_is_truncated() {
    let _home = HomeGuard::isolate();
    let big = "y".repeat(9 * 1024 * 1024);
    let invoke = format!(
        "{{\"success\":true,\"data\":{{\"output\":{{\"answer\":\"{big}\"}},\"durationMs\":5}}}}"
    );
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[&invoke])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "driver_error");
    assert!(e.message.contains("not JSON"), "{}", e.message);
    assert_eq!(e.data.as_ref().unwrap()["truncated"], json!(true));
}

#[cfg(unix)]
#[test]
fn timeout_names_the_phase_and_stays_bounded() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().stall("open", 30).install(&dir, "ab");
    let mut cfg = run_config(&script);
    cfg.call.timeout = 200;
    cfg.call.settle_ms = 200;
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let started = std::time::Instant::now();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    let elapsed = started.elapsed();
    assert_eq!(e.code, "timeout");
    assert!(e.message.contains("open"), "{}", e.message);
    assert!(
        elapsed < std::time::Duration::from_secs(45),
        "total stays near timeout+settle+30s grace: {elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn fresh_close_runs_on_open_failure() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_UNREACHABLE)
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let mut opts = run_opts();
    opts.fresh = true;
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "page_unreachable");
    let lines = common::Fake::argv(&dir, "ab");
    let closes: Vec<_> = lines
        .iter()
        .filter(|l| l.split_whitespace().last() == Some("close"))
        .collect();
    assert_eq!(closes.len(), 1, "{lines:?}");
    assert!(
        closes[0].contains(&format!("agent-webmcp-{}-", std::process::id())),
        "{lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn fresh_close_runs_on_fast_path_invoke_failure() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let invoke = r#"{"success":true,"data":{"output":{"isError":true,"content":[{"type":"text","text":"bad"}]}}}"#;
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[invoke])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let mut opts = run_opts();
    opts.fresh = true;
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "tool_error");
    let lines = common::Fake::argv(&dir, "ab");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.split_whitespace().last() == Some("close"))
            .count(),
        1,
        "{lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn run_envelope_carries_budget() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_PLAIN)
        .lists(&[
            common::LIST_EMPTY,
            common::LIST_EMPTY,
            common::LIST_WITH_TOOL,
        ])
        .invoke(&[common::INVOKE_OK])
        .evals(&[common::EVAL_OBJECT])
        .install(&dir, "ab");
    let cfg = run_config_settle(&script, 5000);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    match execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg) {
        Ok(outcome @ Outcome::Ran { .. }) => {
            assert_eq!(
                outcome.envelope()["data"]["budgetMs"],
                json!(1234 + 5000 + 30_000)
            );
        }
        other => panic!("expected a run, got {other:?}"),
    }
}

#[test]
fn cli_timeout_zero_is_usage() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let mut cmd = cli_base(&dir, &index);
    cmd.args(["call", "cal.com/ask_cal_com", "--timeout", "0"]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--timeout"), "{stderr}");
    assert!(stderr.contains("timeout must be 1..3600000 ms"), "{stderr}");
}

#[test]
fn cli_huge_settle_is_usage() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let mut cmd = cli_base(&dir, &index);
    cmd.args(["call", "cal.com/ask_cal_com", "--settle", "99999999"]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--settle"), "{stderr}");
}

#[test]
fn cli_huge_timeout_is_usage() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir);
    let mut cmd = cli_base(&dir, &index);
    cmd.args(["call", "cal.com/ask_cal_com", "--timeout", "99999999"]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("timeout must be 1..3600000 ms"), "{stderr}");
}

// --- invoke record statuses (unix) ---

#[cfg(unix)]
#[test]
fn invoke_record_statuses_map_to_errors() {
    for (name, record, code, message) in [
        (
            "failed",
            common::RECORD_FAILED,
            "tool_error",
            "the tool blew up",
        ),
        (
            "context",
            common::RECORD_CONTEXT_CHANGED,
            "driver_error",
            "webmcp_context_changed",
        ),
        ("timed_out", common::RECORD_TIMED_OUT, "timeout", "invoke"),
        (
            "canceled",
            common::RECORD_CANCELED,
            "driver_error",
            "canceled",
        ),
    ] {
        let _home = HomeGuard::isolate();
        let dir = tempfile::tempdir().unwrap();
        let script = common::Fake::new()
            .open(common::OPEN_ANNOUNCED)
            .invoke(&[record])
            .install(&dir, "ab");
        let cfg = run_config(&script);
        let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
        let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
        assert_eq!(e.code, code, "{name}");
        assert!(e.message.contains(message), "{name}: {}", e.message);
    }
}

#[cfg(unix)]
#[test]
fn failed_record_carries_the_record_as_data() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let record = r#"{"success":true,"data":{"invocationId":"inv-1","toolName":"ask_cal_com","frameId":"f","origin":"https://cal.com","status":"failed","durationMs":5,"error":"bad input"}}"#;
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[record])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "tool_error");
    assert_eq!(e.data.as_ref().unwrap()["status"], json!("failed"));
    assert_eq!(e.data.as_ref().unwrap()["error"], json!("bad input"));
}

#[cfg(unix)]
#[test]
fn ambiguous_tool_names_tool_and_frames() {
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_AMBIGUOUS])
        .install(&dir, "ab");
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "ambiguous_tool");
    assert!(e.message.contains("ask_cal_com"), "{}", e.message);
    assert!(e.message.contains("2 frames"), "{}", e.message);
    assert!(e.message.contains("browser driver"), "{}", e.message);
}
