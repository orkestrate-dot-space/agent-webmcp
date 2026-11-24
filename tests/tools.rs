//! Custom Tools V1: `tools` CLI, `find` merging, `call`/`inspect` wiring, MCP pinning.

mod common;

use agent_webmcp::call::{self, CallOptions};
use agent_webmcp::config;
use agent_webmcp::index::Search;
use agent_webmcp::model::{Annotations, Index, Site, Status, Tool};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Empty index: custom tools resolve without any native entry.
fn empty_search() -> Search {
    let empty = Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![],
    };
    Search::build(empty).unwrap()
}

fn manifest(host: &str, page: &str, tools: Value) -> String {
    json!({
        "schemaVersion": 1,
        "host": host,
        "page": page,
        "tools": tools,
    })
    .to_string()
}

/// Canned `eval location.origin` answer for host-side origin rechecks.
#[cfg(unix)]
const ORIGIN_OK: &str =
    r#"{"success":true,"data":{"result":"https://example.com","origin":"https://example.com/"}}"#;

fn tool_def(name: &str, extra: Value) -> Value {
    let mut obj = json!({
        "name": name,
        "description": format!("Custom {name}."),
        "inputSchema": {"type": "object"},
    });
    if let Value::Object(map) = extra {
        for (k, v) in map {
            obj[k] = v;
        }
    }
    obj
}

fn write_host(root: &Path, dir_name: &str, manifest_text: &str, js: &str) {
    let dir = root.join(dir_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), manifest_text).unwrap();
    std::fs::write(dir.join("tools.js"), js).unwrap();
}

fn valid_host(root: &Path, host: &str, tool_name: &str, extra: Value, js: &str) {
    let dir_name = agent_webmcp::tools::host_dir_name(host).unwrap();
    let tools = json!([tool_def(tool_name, extra)]);
    let m = manifest(host, &format!("https://{host}/app"), tools);
    write_host(root, &dir_name, &m, js);
}

fn sample_index() -> Index {
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
                    input_schema: json!({"type": "object", "required": ["question"], "properties": {"question": {"type": "string"}}}),
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

fn fixture_index(dir: &tempfile::TempDir, index: &Index) -> PathBuf {
    let path = dir.path().join("index.json");
    std::fs::write(&path, serde_json::to_string(index).unwrap()).unwrap();
    path
}

fn cli_base_tools(
    dir: &tempfile::TempDir,
    index: &Path,
    tools_dir: &Path,
) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_agent-webmcp"));
    cmd.env_clear()
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", "")
        .env("AGENT_WEBMCP_INDEX", index)
        .env("AGENT_WEBMCP_TOOLS_DIR", tools_dir);
    cmd
}

// --- tools CLI ---

#[test]
fn tools_list_text_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir, &sample_index());
    let tools_root = dir.path().join("tools");
    std::fs::create_dir_all(&tools_root).unwrap();
    valid_host(
        &tools_root,
        "example.com",
        "get_item",
        json!({"readOnly": true}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({ok: 1}) } });",
    );

    // Text list.
    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "list"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("example.com"), "{stdout}");
    assert!(stdout.contains("get_item"), "{stdout}");

    // Explicit JSON.
    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hosts"][0]["host"], json!("example.com"));
    assert_eq!(v["hosts"][0]["valid"], json!(true));
    assert_eq!(v["hosts"][0]["tools"], json!(["get_item"]));

    // Parent --json honored.
    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "--json", "list"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hosts"][0]["host"], json!("example.com"));
}

#[test]
fn tools_bare_defaults_to_list() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir, &sample_index());
    let tools_root = dir.path().join("tools");
    std::fs::create_dir_all(&tools_root).unwrap();
    valid_host(
        &tools_root,
        "example.com",
        "get_item",
        json!({}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("example.com"));
}

#[test]
fn tools_validate_valid_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir, &sample_index());
    let tools_root = dir.path().join("tools");
    std::fs::create_dir_all(&tools_root).unwrap();
    valid_host(
        &tools_root,
        "example.com",
        "get_item",
        json!({}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );

    for args in [
        vec!["tools", "validate"],
        vec!["tools", "validate", "example.com"],
        vec!["tools", "validate", "--json"],
    ] {
        let out = cli_base_tools(&dir, &index, &tools_root)
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?} {out:?}");
    }
    // Parent --json.
    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "--json", "validate"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], json!(true));
}

#[test]
fn tools_validate_broken_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir, &sample_index());
    let tools_root = dir.path().join("tools");
    std::fs::create_dir_all(&tools_root).unwrap();
    let bad_dir = tools_root.join("broken.com");
    std::fs::create_dir_all(&bad_dir).unwrap();
    std::fs::write(bad_dir.join("manifest.json"), "not json").unwrap();
    std::fs::write(bad_dir.join("tools.js"), "x").unwrap();

    for args in [
        vec!["tools", "validate"],
        vec!["tools", "validate", "broken.com"],
    ] {
        let out = cli_base_tools(&dir, &index, &tools_root)
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{args:?} {out:?}");
    }
    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "validate", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], json!(false));
}

#[test]
fn tools_validate_on_regular_file_exits_one() {
    // `AGENT_WEBMCP_TOOLS_DIR` pointing at a regular file (not a dir) is
    // an error, while a missing dir lists nothing successfully.
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir, &sample_index());
    let file = dir.path().join("not-a-dir");
    std::fs::write(&file, b"{}").unwrap();
    let out = cli_base_tools(&dir, &index, &file)
        .args(["tools", "validate"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let out = cli_base_tools(&dir, &index, &file)
        .args(["tools", "validate", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], json!(false));

    let missing = dir.path().join("does-not-exist");
    let out = cli_base_tools(&dir, &index, &missing)
        .args(["tools", "validate"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn session_auth_requires_https_page() {
    // Session-authenticated tools carry the driver's cookies: manifest
    // validation rejects cleartext pages before any driver can open them.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tools");
    std::fs::create_dir_all(&root).unwrap();
    let js = "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });";

    // Manifest-level page over http with a session tool: rejected.
    let tools = json!([tool_def("get_item", json!({"auth": "session"}))]);
    write_host(
        &root,
        "example.com",
        &manifest("example.com", "http://example.com/app", tools),
        js,
    );
    let err = agent_webmcp::tools::load_host_in(&root, "example.com").unwrap_err();
    assert!(
        err.to_string().contains("https"),
        "session tool on http must fail: {err}"
    );

    // Tool-level override over http with a session tool: rejected.
    let tools = json!([tool_def(
        "get_item",
        json!({"auth": "session", "page": "http://example.com/other"})
    )]);
    write_host(
        &root,
        "example.com",
        &manifest("example.com", "https://example.com/app", tools),
        js,
    );
    let err = agent_webmcp::tools::load_host_in(&root, "example.com").unwrap_err();
    assert!(
        err.to_string().contains("https"),
        "session tool override on http must fail: {err}"
    );

    // Session tool on https: accepted.
    let tools = json!([tool_def("get_item", json!({"auth": "session"}))]);
    write_host(
        &root,
        "example.com",
        &manifest("example.com", "https://example.com/app", tools),
        js,
    );
    assert!(
        agent_webmcp::tools::load_host_in(&root, "example.com")
            .unwrap()
            .is_some()
    );

    // Non-session tools keep HTTP support.
    let tools = json!([tool_def("get_item", json!({}))]);
    write_host(
        &root,
        "example.com",
        &manifest("example.com", "http://example.com/app", tools),
        js,
    );
    assert!(
        agent_webmcp::tools::load_host_in(&root, "example.com")
            .unwrap()
            .is_some()
    );
}

#[test]
fn direct_http_url_for_session_tool_is_rejected() {
    // Even with a validated https manifest, a direct `http://` call target
    // for a session tool must not reach a driver.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tools");
    std::fs::create_dir_all(&root).unwrap();
    valid_host(
        &root,
        "example.com",
        "get_item",
        json!({"auth": "session"}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let search = empty_search();
    let err =
        call::resolve_in("http://example.com/app#get_item", &search, Some(&root)).unwrap_err();
    assert!(
        err.message.contains("https"),
        "direct http session target must fail: {err:?}"
    );
    // The https form still resolves.
    assert!(call::resolve_in("https://example.com/app#get_item", &search, Some(&root)).is_ok());
}

#[test]
fn tools_path_prints_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(&dir, &sample_index());
    let tools_root = dir.path().join("tools");
    std::fs::create_dir_all(&tools_root).unwrap();
    valid_host(
        &tools_root,
        "example.com",
        "get_item",
        json!({}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );

    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "path"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .ends_with("tools")
    );

    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "path", "example.com"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .ends_with("example.com")
    );

    let out = cli_base_tools(&dir, &index, &tools_root)
        .args(["tools", "path", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["path"].as_str().unwrap().ends_with("tools"));
}

// --- find merging ---

#[test]
fn find_merges_custom_below_native() {
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({"readOnly": true, "description": "Get an item by id."}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    // Native index has a different tool on the same host.
    let mut idx = sample_index();
    idx.sites.push(Site {
        host: "example.com".into(),
        status: Status::Verified,
        checked_at: "2026-11-19T00:00:00Z".into(),
        pages_checked: vec!["https://example.com/".into()],
        platform: None,
        tools: vec![Tool {
            name: "search_docs".into(),
            description: "Search example docs.".into(),
            input_schema: json!({"type": "object"}),
            page: "https://example.com/".into(),
            origin: "https://example.com".into(),
            annotations: Annotations {
                read_only: true,
                ..Default::default()
            },
            flags: vec![],
        }],
        note: None,
        missed: 0,
    });
    let search = Search::build(idx).unwrap();
    let hits = search
        .find_in("example.com", 5, Some(tools.path()))
        .unwrap();
    assert!(hits.len() >= 2, "{hits:?}");
    // Native always ahead of custom.
    assert_eq!(hits[0].source, None);
    let custom_pos = hits
        .iter()
        .position(|h| h.source.as_deref() == Some("custom"));
    assert!(custom_pos.is_some(), "{hits:?}");
    let custom_pos = custom_pos.unwrap();
    assert!(custom_pos > 0, "custom must rank below native: {hits:?}");
    let custom_hit = &hits[custom_pos];
    assert_eq!(custom_hit.site, "example.com");
    assert_eq!(custom_hit.tool, "get_item");
    let v = serde_json::to_value(&hits[custom_pos]).unwrap();
    assert_eq!(v["source"], json!("custom"));
    assert_eq!(v["toolHash"].as_str().unwrap().len(), 64);
    assert_eq!(v["headed"], json!(false));
    assert_eq!(v["allowBrowse"], json!(false));
    assert!(v["auth"].is_string());
}

#[test]
fn find_shadows_native_match() {
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "cal.com",
        "ask_cal_com",
        json!({"description": "Custom shadow, should never surface."}),
        "__webmcpCustom({ ask_cal_com: { execute: async (i) => ({}) } });",
    );
    let search = Search::build(sample_index()).unwrap();
    let hits = search
        .find_in("cal.com ask", 5, Some(tools.path()))
        .unwrap();
    // The native ask_cal_com wins; the custom shadow never appears.
    assert!(
        !hits.iter().any(|h| h.source.as_deref() == Some("custom")),
        "{hits:?}"
    );
    assert!(hits.iter().any(|h| h.id == "cal.com/ask_cal_com"));
}

// --- call permissions ---

fn custom_search_with_tools(tool_name: &str, extra: Value) -> (tempfile::TempDir, Search) {
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        tool_name,
        extra,
        &format!("__webmcpCustom({{ {tool_name}: {{ execute: async (i) => ({{ok: 1}}) }} }});"),
    );
    (tools, empty_search())
}

#[test]
fn call_custom_defaults_to_ask_even_when_readonly_get() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"readOnly": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    assert_eq!(resolved.kind, call::ToolKind::Read);
    // Never inferred_read auto-allow: empty perms still Ask.
    let decision = call::decide_for_resolved(&resolved, &config::Permissions::default());
    assert_eq!(decision, call::Decision::Ask);
    // prepare() agrees.
    let cfg = config::Config::default();
    let prepared = call::prepare_in(
        "example.com/get_item",
        json!({}),
        &cfg,
        &search,
        Some(tools.path()),
    )
    .unwrap();
    assert_eq!(prepared.decision, call::Decision::Ask);
}

#[test]
fn call_custom_allow_rules() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"readOnly": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    for pattern in ["custom/*", "example.com/get_item", "*"] {
        let mut perms = config::Permissions::default();
        perms.allow.push(pattern.into());
        assert_eq!(
            call::decide_for_resolved(&resolved, &perms),
            call::Decision::Allow,
            "{pattern}"
        );
    }
    // Deny beats allow across both ids.
    let mut perms = config::Permissions::default();
    perms.allow.push("custom/*".into());
    perms.deny.push("example.com/get_item".into());
    assert_eq!(
        call::decide_for_resolved(&resolved, &perms),
        call::Decision::Deny
    );
}

#[test]
fn port_specific_catalog_wins_over_bare_host_for_direct_url() {
    // When both `localhost` and `localhost:3000` define `get_item`, a
    // direct URL for port 3000 must resolve to the port-specific
    // definition (code, schema, and permission identity) — not the bare
    // host's.
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "localhost",
        "get_item",
        json!({"description": "Bare host tool."}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({bare: 1}) } });",
    );
    valid_host(
        tools.path(),
        "localhost:3000",
        "get_item",
        json!({"description": "Port-specific tool."}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({ported: 1}) } });",
    );
    let search = empty_search();
    let resolved = call::resolve_in(
        "http://localhost:3000/app#get_item",
        &search,
        Some(tools.path()),
    )
    .unwrap();
    assert!(resolved.is_custom(), "{resolved:?}");
    assert_eq!(resolved.site, "localhost:3000", "{resolved:?}");
    assert_eq!(
        resolved.custom.as_ref().unwrap().host,
        "localhost:3000",
        "{resolved:?}"
    );
    assert_eq!(
        resolved.custom_id().as_deref(),
        Some("custom/localhost:3000/get_item"),
        "{resolved:?}"
    );
    // The short form agrees.
    let short = call::resolve_in("localhost:3000/get_item", &search, Some(tools.path())).unwrap();
    assert_eq!(
        short.custom.as_ref().unwrap().tool_hash,
        resolved.custom.as_ref().unwrap().tool_hash,
        "short and URL forms must resolve to the same definition"
    );
}

#[test]
fn indexed_tool_on_another_port_does_not_hide_custom_tool() {
    // Indexed `example.com/get_item` lives on port 443 while a local
    // `example.com:8443/get_item` exists: a direct URL on port 8443 must
    // resolve to the custom definition — not the unmatched native entry —
    // agreeing with the short target form.
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com:8443",
        "get_item",
        json!({"description": "Port-specific tool."}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({ported: 1}) } });",
    );
    let index = Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![Site {
            host: "example.com".into(),
            status: Status::Verified,
            checked_at: "2026-11-19T00:00:00Z".into(),
            pages_checked: vec!["https://example.com/".into()],
            platform: None,
            tools: vec![Tool {
                name: "get_item".into(),
                description: "Native get item.".into(),
                input_schema: json!({"type": "object"}),
                page: "https://example.com/".into(),
                origin: "https://example.com".into(),
                annotations: Annotations::default(),
                flags: vec![],
            }],
            note: None,
            missed: 0,
        }],
    };
    let search = Search::build(index).unwrap();
    let resolved = call::resolve_in(
        "https://example.com:8443/app#get_item",
        &search,
        Some(tools.path()),
    )
    .unwrap();
    assert!(resolved.is_custom(), "{resolved:?}");
    assert_eq!(resolved.site, "example.com:8443", "{resolved:?}");
    assert_eq!(
        resolved.custom_id().as_deref(),
        Some("custom/example.com:8443/get_item"),
        "{resolved:?}"
    );
    // The indexed origin itself still resolves natively.
    let native =
        call::resolve_in("https://example.com/#get_item", &search, Some(tools.path())).unwrap();
    assert!(!native.is_custom(), "{native:?}");
}

#[test]
fn bare_host_catalog_does_not_cover_other_ports() {
    // Grants are per-origin: a bare-host catalog declares
    // `https://example.com` (default port 443), so a direct URL for
    // `https://example.com:8443/` must NOT resolve to it.
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let search = empty_search();
    // The bare origin itself still resolves.
    let ok = call::resolve_in(
        "https://example.com/app#get_item",
        &search,
        Some(tools.path()),
    )
    .unwrap();
    assert!(ok.is_custom(), "{ok:?}");
    // An explicit default port normalizes to the same origin.
    let ok_default_port = call::resolve_in(
        "https://example.com:443/app#get_item",
        &search,
        Some(tools.path()),
    )
    .unwrap();
    assert!(ok_default_port.is_custom(), "{ok_default_port:?}");
    // A non-default port is rejected before any tool runs.
    let err = call::resolve_in(
        "https://example.com:8443/app#get_item",
        &search,
        Some(tools.path()),
    )
    .unwrap_err();
    assert!(
        err.message
            .contains("not defined for https://example.com:8443"),
        "{err:?}"
    );
}

#[test]
fn port_specific_custom_identity_matches_both_target_forms() {
    // A port-specific custom host keeps its port on the resolved identity,
    // so `custom/localhost:3000/tool` rules apply to the URL form and the
    // short form identically.
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "localhost:3000",
        "get_item",
        json!({"readOnly": true}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    assert_eq!(
        agent_webmcp::tools::host_dir_name("localhost:3000").unwrap(),
        "localhost__3000"
    );
    let search = empty_search();
    let short = call::resolve_in("localhost:3000/get_item", &search, Some(tools.path())).unwrap();
    let url = call::resolve_in(
        "http://localhost:3000/app#get_item",
        &search,
        Some(tools.path()),
    )
    .unwrap();
    for resolved in [&short, &url] {
        assert!(resolved.is_custom(), "{resolved:?}");
        assert_eq!(resolved.site, "localhost:3000", "{resolved:?}");
        assert_eq!(
            resolved.custom_id().as_deref(),
            Some("custom/localhost:3000/get_item"),
            "{resolved:?}"
        );
    }
    for pattern in ["custom/localhost:3000/*", "custom/*"] {
        let mut perms = config::Permissions::default();
        perms.allow.push(pattern.into());
        assert_eq!(
            call::decide_for_resolved(&short, &perms),
            call::Decision::Allow,
            "{pattern} (short)"
        );
        assert_eq!(
            call::decide_for_resolved(&url, &perms),
            call::Decision::Allow,
            "{pattern} (url)"
        );
    }
    let mut perms = config::Permissions::default();
    perms.allow.push("custom/*".into());
    perms.deny.push("custom/localhost:3000/get_item".into());
    assert_eq!(
        call::decide_for_resolved(&short, &perms),
        call::Decision::Deny,
        "short"
    );
    assert_eq!(
        call::decide_for_resolved(&url, &perms),
        call::Decision::Deny,
        "url"
    );
}

/// Fake `browse` CLI answering detection, navigate, and webmcp list/call.
/// `native` controls whether the live page registers `get_item`.
/// `custom_eval_suffix` is printed after the per-call marker when the test
/// runs custom JS through `execute --file -` (already JSON-encoded, ready
/// to append); `None` makes any `execute` call fail loudly.
#[cfg(unix)]
fn browse_fake(
    dir: &tempfile::TempDir,
    log: &Path,
    native: bool,
    custom_eval_suffix: Option<&str>,
) -> PathBuf {
    browse_fake_with_origin(dir, log, native, "https://example.com", custom_eval_suffix)
}

/// [`browse_fake`] with an explicit page origin for the `execute` probe.
#[cfg(unix)]
fn browse_fake_with_origin(
    dir: &tempfile::TempDir,
    log: &Path,
    native: bool,
    origin: &str,
    custom_eval_suffix: Option<&str>,
) -> PathBuf {
    let tools_json = if native {
        r#"[{"name":"get_item","frame":"https://example.com/app"}]"#.to_string()
    } else {
        "[]".to_string()
    };
    browse_fake_with_origin_and_list(dir, log, &tools_json, origin, custom_eval_suffix)
}

/// [`browse_fake_with_origin`] with an explicit `webmcp list` tool payload,
/// for frame-policy tests (foreign or ambiguous registrations).
#[cfg(unix)]
fn browse_fake_with_origin_and_list(
    dir: &tempfile::TempDir,
    log: &Path,
    tools_json: &str,
    origin: &str,
    custom_eval_suffix: Option<&str>,
) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    // Custom-eval answers are baked in as a shell single-quoted literal
    // (test JSON never contains apostrophes); origin probes answer inline.
    let canned = custom_eval_suffix.unwrap_or("{\"unexpected\":\"execute should not run\"}");
    assert!(
        !canned.contains('\''),
        "canned execute response must avoid apostrophes"
    );
    // `execute --file -` reads the wrapper script from stdin; the fake
    // extracts the per-call marker, saves stdin for assertions, and answers
    // according to the script kind: the exact origin-probe expression, the
    // custom eval wrapper, a native call (stdin transport keeps secret
    // input out of argv), or a frame-filtered native call.
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\ncase \"$1\" in\n  status) printf '{{\"connected\":true,\"relay\":{{\"version\":\"0.1.5\"}}}}\\n' ;;\n  navigate) printf '{{\"tab\":7,\"url\":\"https://example.com/app\",\"title\":\"App\"}}\\n' ;;\n  webmcp) case \"$3\" in *list*) printf '{{\"available\":true,\"tools\":{tools_json}}}\\n' ;; *) printf '{{\"result\":{{\"native\":1}}}}\\n' ;; esac ;;\n  tabs) printf '{{\"ok\":true}}\\n' ;;\n  execute) cat > '{log}.execute.stdin'\n    cat '{log}.execute.stdin' >> '{log}.execute.all.stdin'\n    marker=$(grep -o '__AWM_[^\"]*' '{log}.execute.stdin' | head -1)\n    if grep -q '\"expression\":\"location.origin\"' '{log}.execute.stdin'; then printf '%s\"{origin}\"\\n' \"$marker\"; elif grep -q 'browse.webmcp' '{log}.execute.stdin'; then printf '%s{{\"result\":{{\"native\":1}}}}\\n' \"$marker\"; elif grep -q 'webmcp.call' '{log}.execute.stdin'; then printf '%s{{\"result\":{{\"native\":1}}}}\\n' \"$marker\"; elif grep -q 'browse.eval' '{log}.execute.stdin'; then printf '%s%s\\n' \"$marker\" '{custom_eval_suffix}'; elif grep -q 'location.origin' '{log}.execute.stdin'; then printf '%s\"{origin}\"\\n' \"$marker\"; else printf '%s{{\"unexpected\":\"execute should not run\"}}\\n' \"$marker\"; fi ;;\nesac\n",
        origin = origin,
        log = log.display(),
        custom_eval_suffix = canned,
    );
    let path = dir.path().join("browse");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Exec-visibility barrier (mirrors `common::Fake::install`): a
    // just-written script can answer ETXTBSY under concurrency. The warmup
    // run appends a blank line; start the argv log clean.
    let _ = std::fs::read(&path);
    for _ in 0..100 {
        match std::process::Command::new(&path).output() {
            Ok(_) => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
        }
    }
    let _ = std::fs::write(log, "");
    path
}

/// Read piped `execute --file -` stdin captured by [`browse_fake`].
#[cfg(unix)]
fn browse_execute_stdin(log: &Path) -> String {
    let path = format!("{}.execute.stdin", log.display());
    std::fs::read_to_string(path).unwrap_or_default()
}

/// All piped `execute --file -` scripts in one native-substitution run
/// (origin probe plus the stdin-transport native call), in order.
#[cfg(unix)]
fn browse_execute_all_stdin(log: &Path) -> String {
    let path = format!("{}.execute.all.stdin", log.display());
    std::fs::read_to_string(path).unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn browse_opt_out_fails_without_eval_when_no_native() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": false}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    // No live native tool: browse opens the page to check, then refuses
    // before evaluating any custom JS.
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake(&dir, &log, false, None);
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    // Allow it so permissions don't stop the driver check.
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "browse_not_allowed", "{e:?}");
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        logged.contains("navigate"),
        "native check opens first: {logged}"
    );
    assert!(
        !logged.contains("execute"),
        "must not run custom JS: {logged}"
    );
    assert!(
        !std::path::Path::new(&format!("{}.execute.stdin", log.display())).exists(),
        "no custom script may be piped"
    );
}

#[cfg(unix)]
#[test]
fn browse_native_wins_over_opt_out() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": false}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    // A live native tool runs even though the custom twin opts out.
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake(&dir, &log, true, None);
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(outcome) => {
            let env = outcome.envelope();
            assert_eq!(env["data"]["source"], json!("native"), "{env:?}");
            assert_eq!(env["data"]["output"], json!({"native": 1}), "{env:?}");
        }
        Err(e) => panic!("expected native run, got {e:?}"),
    }
    // The native call itself also travels over stdin (never argv), so the
    // cumulative capture holds both scripts.
    let stdin = browse_execute_all_stdin(&log);
    assert!(stdin.contains("location.origin"), "{stdin:?}");
    assert!(stdin.contains("browse.webmcp"), "{stdin:?}");
    // The invoke is filtered to the verified same-origin frame.
    assert!(
        stdin.contains("https://example.com/app"),
        "invoke must carry the verified frame: {stdin:?}"
    );
    assert!(
        !stdin.contains("__webmcpCustom"),
        "custom JS must not run on the native path: {stdin:?}"
    );
    // No `webmcp --json` call action in argv: list probes may poll, but the
    // secret-bearing invoke travels over stdin.
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !logged.contains("\"action\":\"call\""),
        "native input must not travel in argv: {logged}"
    );
    // The custom navigation is allowlisted to the expected origin, so a
    // redirect hop outside it is refused before cookies travel.
    let nav_line = logged
        .lines()
        .find(|l| l.contains("navigate"))
        .unwrap()
        .to_string();
    assert!(nav_line.contains("allowedOrigins"), "{nav_line}");
    assert!(
        nav_line.contains("https://example.com"),
        "navigate carries the expected origin: {nav_line}"
    );
}

#[cfg(unix)]
#[test]
fn browse_custom_http_page_is_denied() {
    // Personal Chrome always carries the user's profile cookies, so a
    // custom tool page over cleartext HTTP never opens in browse —
    // including `auth: none` tools.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tools");
    std::fs::create_dir_all(&root).unwrap();
    let tools_json = json!([tool_def("get_item", json!({}))]);
    write_host(
        &root,
        "example.com",
        &manifest("example.com", "http://example.com/app", tools_json),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let search = empty_search();
    let resolved = call::resolve_in("example.com/get_item", &search, Some(&root)).unwrap();
    assert!(resolved.is_custom());
    let logdir = tempfile::tempdir().unwrap();
    let log = logdir.path().join("browse.log");
    let path = browse_fake(&logdir, &log, false, None);
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "denied", "{e:?}");
    assert!(e.message.contains("http://"), "{e:?}");
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !logged.contains("navigate"),
        "no personal-Chrome navigation over http: {logged}"
    );
}

#[cfg(unix)]
#[test]
fn browse_native_collision_wrong_origin_refuses() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    // The live page reports the same native tool name from a different
    // origin: no invoke, origin_mismatch — even with a blanket allow.
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake_with_origin(&dir, &log, true, "https://evil.com", None);
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "origin_mismatch", "{e:?}");
    let logged = std::fs::read_to_string(log).unwrap_or_default();
    assert!(
        !logged.contains("\"action\":\"call\""),
        "redirected native tool must not run: {logged}"
    );
}

#[cfg(unix)]
#[test]
fn browse_native_collision_foreign_frame_refuses() {
    // Top-level origin matches, but the sole same-named registration lives
    // in a foreign-origin iframe: the input must never reach it.
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake_with_origin_and_list(
        &dir,
        &log,
        r#"[{"name":"get_item","frame":"https://evil.com/frame"}]"#,
        "https://example.com",
        None,
    );
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "origin_mismatch", "{e:?}");
    let stdin = browse_execute_all_stdin(&log);
    assert!(
        !stdin.contains("browse.webmcp"),
        "no invoke may run for a foreign frame: {stdin:?}"
    );
}

#[cfg(unix)]
#[test]
fn browse_native_collision_lying_origin_field_refuses() {
    // A foreign iframe can claim the top-level origin in its page-supplied
    // `origin` field: authorization comes only from the browser-reported
    // frame URL, so the input still never reaches it.
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake_with_origin_and_list(
        &dir,
        &log,
        r#"[{"name":"get_item","origin":"https://example.com","frame":"https://evil.com/frame"}]"#,
        "https://example.com",
        None,
    );
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "origin_mismatch", "{e:?}");
    let stdin = browse_execute_all_stdin(&log);
    assert!(
        !stdin.contains("browse.webmcp"),
        "a claimed origin must not authorize the invoke: {stdin:?}"
    );
}

#[cfg(unix)]
#[test]
fn browse_native_collision_ambiguous_frame_refuses() {
    // Two same-origin registrations of one name cannot be bound to a single
    // frame: refuse instead of invoking either.
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake_with_origin_and_list(
        &dir,
        &log,
        r#"[{"name":"get_item","frame":"https://example.com/a"},{"name":"get_item","frame":"https://example.com/b"}]"#,
        "https://example.com",
        None,
    );
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "ambiguous_tool", "{e:?}");
    let stdin = browse_execute_all_stdin(&log);
    assert!(
        !stdin.contains("browse.webmcp"),
        "no invoke may run for ambiguous frames: {stdin:?}"
    );
}

#[cfg(unix)]
#[test]
fn browse_custom_eval_runs_over_stdin_without_leaking_argv() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    // Canned custom envelope, double-encoded exactly like the real
    // `browse execute` wrapper prints it (marker + JSON-stringified res).
    let inner = json!({"ok": true, "output": {"answer": 42}});
    let suffix = serde_json::to_string(&serde_json::to_string(&inner).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake(&dir, &log, false, Some(&suffix));
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let secret = json!({"secret": "s3cr3t-custom-input"});
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, secret.clone(), &opts, &cfg) {
        Ok(outcome) => {
            let env = outcome.envelope();
            assert_eq!(env["data"]["source"], json!("custom"), "{env:?}");
            assert_eq!(env["data"]["output"], json!({"answer": 42}), "{env:?}");
            assert_eq!(
                env["data"]["toolHash"],
                json!(resolved.custom.as_ref().unwrap().tool_hash),
                "{env:?}"
            );
            assert_eq!(env["data"]["untrusted"], json!(true), "{env:?}");
        }
        Err(e) => panic!("expected custom run, got {e:?}"),
    }
    // The secret input and the inlined script traveled over stdin only.
    let stdin = browse_execute_stdin(&log);
    assert!(stdin.contains("__webmcpCustom"), "{stdin:?}");
    assert!(stdin.contains("s3cr3t-custom-input"), "{stdin:?}");
    let logged = std::fs::read_to_string(log).unwrap_or_default();
    assert!(
        !logged.contains("s3cr3t-custom-input"),
        "secret input must not appear in argv: {logged}"
    );
    assert!(
        !logged.contains("__webmcpCustom"),
        "custom script must not appear in argv: {logged}"
    );
    // Exact-tab, version-compatible flags (Browse 0.1.5 has no --timeoutMs).
    assert!(
        logged.contains("--timeout-ms="),
        "execute must bound the run: {logged}"
    );
    assert!(
        !logged.contains("--timeoutMs"),
        "unknown flags break execute: {logged}"
    );
}

/// Fake `browse` whose `execute` aborts without the marked result line:
/// `stderr_msg` goes to stderr, stdout keeps only the relay chatter.
#[cfg(unix)]
fn browse_fake_execute_aborts(dir: &tempfile::TempDir, log: &Path, stderr_msg: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    assert!(!stderr_msg.contains('\''), "stderr must avoid apostrophes");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\ncase \"$1\" in\n  status) printf '{{\"connected\":true,\"relay\":{{\"version\":\"0.1.5\"}}}}\\n' ;;\n  navigate) printf '{{\"tab\":7,\"url\":\"https://example.com/app\",\"title\":\"App\"}}\\n' ;;\n  webmcp) printf '{{\"available\":true,\"tools\":[]}}\\n' ;;\n  tabs) printf '{{\"ok\":true}}\\n' ;;\n  execute) body=$(cat); printf 'tab: 7\\n' ; printf '%s\\n' '{stderr_msg}' >&2 ;;\nesac\n",
        log = log.display(),
        stderr_msg = stderr_msg,
    );
    let path = dir.path().join("browse");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::read(&path);
    for _ in 0..100 {
        match std::process::Command::new(&path).output() {
            Ok(_) => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
        }
    }
    let _ = std::fs::write(log, "");
    path
}

#[cfg(unix)]
#[test]
fn browse_execute_timeout_without_marker_is_timeout() {
    // `TimeoutError` with no marked line maps to `timeout` (with the real
    // detail), not a generic driver_error — so tab cleanup and retry treat
    // it correctly.
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake_execute_aborts(
        &dir,
        &log,
        "TimeoutError: eval timed out after 60000 ms (page may be busy)",
    );
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "timeout", "{e:?}");
    assert!(e.message.contains("browse execute timed out"), "{e:?}");
    assert!(e.message.contains("60000"), "{e:?}");
}

#[cfg(unix)]
#[test]
fn browse_execute_error_without_marker_is_driver_error_with_detail() {
    // A non-timeout abort without the marker still surfaces its detail
    // instead of the generic "no marked result".
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake_execute_aborts(&dir, &log, "Error: tab 7 not found");
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "driver_error", "{e:?}");
    assert!(e.message.contains("browse execute failed"), "{e:?}");
    assert!(e.message.contains("tab 7 not found"), "{e:?}");
}

#[cfg(unix)]
#[test]
fn browse_inspect_surfaces_eval_failure_remediation() {
    // `inspect --driver browse` reports the eval failure detail as
    // remediation so authors can diagnose it.
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({"allowBrowse": true}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake_execute_aborts(&dir, &log, "Error: frame detached");
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    let res = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: false },
        &cfg,
        Some(tools.path()),
    )
    .unwrap();
    let tool = res.tools.iter().find(|t| t.name == "get_item").unwrap();
    assert_eq!(tool.available, Some(false));
    assert_eq!(
        tool.unavailable_reason.as_deref(),
        Some("eval failed"),
        "{tool:?}"
    );
    assert!(
        tool.remediation
            .as_deref()
            .unwrap_or("")
            .contains("frame detached"),
        "remediation carries the eval detail: {tool:?}"
    );
}

#[cfg(unix)]
#[test]
fn browse_inspect_timeout_propagates() {
    // A timed-out probe aborts inspection instead of reporting a false
    // empty catalog with per-tool marks.
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({"allowBrowse": true}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path =
        browse_fake_execute_aborts(&dir, &log, "TimeoutError: eval timed out after 60000 ms");
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    let e = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: false },
        &cfg,
        Some(tools.path()),
    )
    .unwrap_err();
    assert_eq!(e.code, "timeout", "{e:?}");
}

#[cfg(unix)]
#[test]
fn browse_run_and_inspect_close_tab_by_default() {
    // Default (`fresh: false`) browse `run` and `inspect` both close the
    // brand-new background tab when finished — tabs must not leak.
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let inner = json!({"ok": true, "output": {"answer": 1}});
    let suffix = serde_json::to_string(&serde_json::to_string(&inner).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let path = browse_fake(&dir, &log, false, Some(&suffix));
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    call::execute(&resolved, json!({}), &opts, &cfg).unwrap();
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        logged
            .lines()
            .any(|l| l.contains("tabs") && l.contains("close")),
        "run closes its tab by default: {logged}"
    );
    agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: false },
        &cfg,
        Some(tools.path()),
    )
    .unwrap();
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    let closes = logged
        .lines()
        .filter(|l| l.contains("tabs") && l.contains("close"))
        .count();
    assert!(
        closes >= 2,
        "run and inspect each close their tab: {logged}"
    );
}

/// Fake `lightpanda` CLI answering `version` and sequential `run`
/// responses (`first` for the native probe, `rest` for later runs), saving
/// each run script for assertions.
#[cfg(unix)]
fn lightpanda_fake(dir: &tempfile::TempDir, log: &Path, first: &str, rest: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\nif [ \"$1\" = \"version\" ]; then echo \"lightpanda 1.1.0\"; exit 0; fi\nn=$(cat '{log}.count' 2>/dev/null || echo 0); echo $((n + 1)) > '{log}.count'\ncp \"$2\" '{log}.script.'$n\nif [ \"$n\" = \"0\" ]; then printf '%s\\n' '{first}'; else printf '%s\\n' '{rest}'; fi\n",
        log = log.display(),
    );
    let path = dir.path().join("lightpanda");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::read(&path);
    for _ in 0..100 {
        match std::process::Command::new(&path).arg("version").output() {
            Ok(_) => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
        }
    }
    path
}

#[cfg(unix)]
fn lightpanda_run_count(log: &Path) -> usize {
    std::fs::read_to_string(format!("{}.count", log.display()))
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or(0)
}

#[cfg(unix)]
fn lightpanda_run_script(log: &Path, n: usize) -> String {
    std::fs::read_to_string(format!("{}.script.{}", log.display(), n)).unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn lightpanda_headed_without_native_match_needs_headed() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"headed": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.custom.as_ref().unwrap().headed);
    // The single document reports needs_headed without running custom JS:
    // the headed gate precedes the evaluation.
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("lp.log");
    let path = lightpanda_fake(&dir, &log, r#"{"ok":false,"code":"needs_headed"}"#, "{}");
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Lightpanda;
    cfg.drivers.lightpanda.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "needs_headed", "{e:?}");
    assert!(e.message.contains("headless"), "{e:?}");
    assert_eq!(lightpanda_run_count(&log), 1, "one document");
    let script = lightpanda_run_script(&log, 0);
    assert!(script.contains("needs_headed"), "{script}");
    assert!(
        script.find("needs_headed").unwrap() < script.find("__webmcpCustom").unwrap(),
        "headed gate precedes evaluation: {script}"
    );
}

#[cfg(unix)]
#[test]
fn lightpanda_headed_native_collision_runs_natively() {
    let (tools, search) = custom_search_with_tools("get_item", json!({"headed": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.custom.as_ref().unwrap().headed);
    // A live native tool takes precedence even for a headed custom tool,
    // with an explicit native grant — discovered and invoked in one document.
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("lp.log");
    let path = lightpanda_fake(
        &dir,
        &log,
        r#"{"ok":true,"output":{"native":1},"used_native":true}"#,
        "{}",
    );
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Lightpanda;
    cfg.drivers.lightpanda.command = path.display().to_string();
    cfg.permissions.allow.push("example.com/get_item".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(outcome) => {
            let env = outcome.envelope();
            assert_eq!(env["data"]["source"], json!("native"), "{env:?}");
            assert_eq!(env["data"]["output"], json!({"native": 1}), "{env:?}");
        }
        Err(e) => panic!("expected native run, got {e:?}"),
    }
    assert_eq!(lightpanda_run_count(&log), 1, "one document");
    let script = lightpanda_run_script(&log, 0);
    assert_eq!(script.matches("page.goto").count(), 1, "{script}");
    assert!(
        script.contains("callPageTool"),
        "the document invokes the native tool"
    );
    assert!(
        script.contains("origin_mismatch"),
        "native invoke stays bound to the expected origin"
    );
}

#[cfg(unix)]
#[test]
fn lightpanda_native_collision_wrong_origin_refuses() {
    // A redirect to a different origin surfaces as origin_mismatch instead
    // of running native code — checked in the same document that invokes.
    let (tools, search) = custom_search_with_tools("get_item", json!({}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("lp.log");
    let path = lightpanda_fake(
        &dir,
        &log,
        r#"{"ok":false,"code":"origin_mismatch","error":"page origin https://evil.com does not match"}"#,
        "{}",
    );
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Lightpanda;
    cfg.drivers.lightpanda.command = path.display().to_string();
    cfg.permissions.allow.push("example.com/get_item".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "origin_mismatch", "{e:?}");
    assert_eq!(lightpanda_run_count(&log), 1, "one document");
}

#[cfg(unix)]
#[test]
fn lightpanda_blocked_interstitial_is_page_unreachable() {
    // Genuine challenge interstitials still stop the call: the document
    // reports `blocked` and maps to `page_unreachable`.
    let (tools, search) = custom_search_with_tools("get_item", json!({}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("lp.log");
    let path = lightpanda_fake(&dir, &log, r#"{"ok":false,"code":"blocked"}"#, "{}");
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Lightpanda;
    cfg.drivers.lightpanda.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "page_unreachable", "{e:?}");
    assert_eq!(lightpanda_run_count(&log), 1, "one document");
}

#[cfg(unix)]
#[test]
fn lightpanda_headless_custom_runs_when_no_native() {
    // Ordinary pages proceed: no native match, valid input, custom JS runs.
    let (tools, search) = custom_search_with_tools("get_item", json!({}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("lp.log");
    let path = lightpanda_fake(&dir, &log, r#"{"ok":true,"output":{"answer":3}}"#, "{}");
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Lightpanda;
    cfg.drivers.lightpanda.command = path.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(outcome) => {
            let env = outcome.envelope();
            assert_eq!(env["data"]["source"], json!("custom"), "{env:?}");
            assert_eq!(env["data"]["output"], json!({"answer": 3}), "{env:?}");
            assert_eq!(
                env["data"]["toolHash"],
                json!(resolved.custom.as_ref().unwrap().tool_hash),
                "{env:?}"
            );
        }
        Err(e) => panic!("expected custom run, got {e:?}"),
    }
    assert_eq!(lightpanda_run_count(&log), 1, "one document");
    assert!(
        lightpanda_run_script(&log, 0).contains("__webmcpCustom"),
        "the run evaluates the custom tool"
    );
}

#[cfg(unix)]
fn agent_browser_custom_cfg(script: &Path, headed_cfg: bool) -> config::Config {
    let _ = headed_cfg;
    let mut cfg = common::allow_all();
    cfg.drivers.agent_browser.command = script.display().to_string();
    cfg.drivers.agent_browser.session = format!(
        "test-tools-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    cfg.call.timeout = 1234;
    cfg.call.settle_ms = 0;
    cfg
}

#[cfg(unix)]
fn eval_outer(inner: &Value) -> String {
    let inner_str = serde_json::to_string(inner).unwrap();
    json!({"success": true, "data": {"result": inner_str, "origin": "https://example.com/"}})
        .to_string()
}

#[cfg(unix)]
#[test]
fn agent_browser_headed_custom_runs_eval_and_reports_source() {
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let tools = tempfile::tempdir().unwrap();
    let js = "__webmcpCustom({ get_item: { execute: async (i) => ({id: i.id}) } });";
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({"headed": true}),
        js,
    );
    let empty = Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![],
    };
    let search = Search::build(empty).unwrap();
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    let tool_hash = resolved.custom.as_ref().unwrap().tool_hash.clone();

    let dir = tempfile::tempdir().unwrap();
    let inner_ok = json!({"ok": true, "output": {"id": "x"}});
    let script = common::Fake::new()
        .open(r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":false},"webmcp":{"tools":[]}}}"#)
        .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
        .evals(&[ORIGIN_OK, &eval_outer(&inner_ok)])
        .install(&dir, "ab");
    let cfg = agent_browser_custom_cfg(&script, true);
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(outcome) => {
            let env = outcome.envelope();
            assert_eq!(env["data"]["source"], json!("custom"), "{env:?}");
            assert_eq!(env["data"]["toolHash"], json!(tool_hash), "{env:?}");
            assert_eq!(env["data"]["untrusted"], json!(true), "{env:?}");
        }
        Err(e) => panic!("expected custom run, got {e:?}"),
    }
    // --headed passed on open for headed:true.
    let lines = common::Fake::argv(&dir, "ab");
    let open = lines.iter().find(|l| l.contains(" open ")).unwrap();
    assert!(open.contains("--headed"), "{open}");
    // Custom JS travels over stdin, never in argv (no secrets on cmdlines).
    assert!(
        lines
            .iter()
            .any(|l| l.contains(" eval ") && l.contains("--stdin")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains(" -b ")),
        "no base64 argv: {lines:?}"
    );
    let stdin = common::Fake::eval_stdin(&dir, "ab");
    assert!(stdin.contains("get_item"), "{stdin:?}");
    assert!(stdin.contains("location.origin"), "{stdin:?}");
}

#[cfg(unix)]
#[test]
fn agent_browser_headed_custom_uses_separate_session() {
    use common::HomeGuard;
    // A per-tool headed override must not flip the shared reusable session
    // headed: it runs in `<session>-headed`, leaving `<session>` alone.
    for (headed, session) in [(true, "fixed-s-headed"), (false, "fixed-s")] {
        let _home = HomeGuard::isolate();
        let tools = tempfile::tempdir().unwrap();
        valid_host(
            tools.path(),
            "example.com",
            "get_item",
            json!({"headed": headed}),
            "__webmcpCustom({ get_item: { execute: async (i) => ({id: 1}) } });",
        );
        let search = empty_search();
        let resolved =
            call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let inner_ok = json!({"ok": true, "output": {"id": 1}});
        let script = common::Fake::new()
            .open(r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":false},"webmcp":{"tools":[]}}}"#)
            .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
            .evals(&[ORIGIN_OK, &eval_outer(&inner_ok)])
            .install(&dir, "ab");
        let mut cfg = common::allow_all();
        cfg.drivers.agent_browser.command = script.display().to_string();
        cfg.drivers.agent_browser.session = "fixed-s".to_string();
        cfg.drivers.agent_browser.headed = false;
        cfg.call.timeout = 1234;
        cfg.call.settle_ms = 0;
        let opts = CallOptions {
            confirm: true,
            plan_only: false,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        };
        match call::execute(&resolved, json!({}), &opts, &cfg) {
            Ok(_) => {}
            Err(e) => panic!("expected custom run, got {e:?}"),
        }
        let lines = common::Fake::argv(&dir, "ab");
        let open = lines.iter().find(|l| l.contains(" open ")).unwrap();
        assert!(
            open.contains(&format!("--session {session} ")),
            "headed={headed}: {open}"
        );
        if headed {
            assert!(open.contains("--headed"), "{open}");
        } else {
            assert!(!open.contains("--headed"), "{open}");
        }
    }
}

#[cfg(unix)]
#[test]
fn agent_browser_custom_https_installs_http_abort_route() {
    use common::HomeGuard;
    // A custom https navigation pre-installs a session abort route for
    // cleartext HTTP, so a server redirect hop cannot carry session
    // cookies; the route lands before the open.
    let _home = HomeGuard::isolate();
    let (tools, search) = custom_search_with_tools("get_item", json!({}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let inner_ok = json!({"ok": true, "output": {"answer": 1}});
    let script = common::Fake::new()
        .open(r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":false},"webmcp":{"tools":[]}}}"#)
        .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
        .evals(&[ORIGIN_OK, &eval_outer(&inner_ok)])
        .install(&dir, "ab");
    let mut cfg = common::allow_all();
    cfg.drivers.agent_browser.command = script.display().to_string();
    cfg.drivers.agent_browser.session = "fixed-route-s".to_string();
    cfg.call.timeout = 1234;
    cfg.call.settle_ms = 0;
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(_) => {}
        Err(e) => panic!("expected custom run, got {e:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    let route = lines.iter().find(|l| l.contains(" network ")).unwrap();
    assert!(route.contains(" route "), "{route}");
    assert!(route.contains("http://*"), "{route}");
    assert!(route.contains("--abort"), "{route}");
    let open_at = lines.iter().position(|l| l.contains(" open ")).unwrap();
    let route_at = lines.iter().position(|l| l.contains(" network ")).unwrap();
    assert!(
        route_at < open_at,
        "route installs before navigation: {lines:?}"
    );
    // The restriction is scoped to the operation: dropping the guard
    // removes it so later calls reusing the session are unaffected.
    let route_line = lines.iter().find(|l| l.contains(" network ")).unwrap();
    assert!(
        route_line.contains("http://***/***"),
        "owned route pattern (independent of operator http://* routes): {route_line}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("unroute") && l.contains("http://***/***")),
        "route is removed after the call: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn agent_browser_custom_http_with_persistent_session_is_denied() {
    use common::HomeGuard;
    // An http custom page must never open in a persistent session.
    let _home = HomeGuard::isolate();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tools");
    std::fs::create_dir_all(&root).unwrap();
    let tools_json = json!([tool_def("get_item", json!({}))]);
    write_host(
        &root,
        "example.com",
        &manifest("example.com", "http://example.com/app", tools_json),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let search = empty_search();
    let resolved = call::resolve_in("example.com/get_item", &search, Some(&root)).unwrap();
    assert!(resolved.is_custom());
    let logdir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().install(&logdir, "ab");
    let mut cfg = common::allow_all();
    cfg.drivers.agent_browser.command = script.display().to_string();
    cfg.drivers.agent_browser.session = "fixed-http-s".to_string();
    cfg.call.timeout = 1234;
    cfg.call.settle_ms = 0;
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "denied", "{e:?}");
    assert!(e.message.contains("http://"), "{e:?}");
    let lines = common::Fake::argv(&logdir, "ab");
    assert!(
        !lines.iter().any(|l| l.contains(" open ")),
        "no navigation may run: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn agent_browser_native_shadows_custom_at_runtime() {
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({custom: 1}) } });",
    );
    // Empty index so resolve() yields the custom tool.
    let empty = Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![],
    };
    let search = Search::build(empty).unwrap();
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());

    // Live page registers a native tool with the same name.
    let dir = tempfile::tempdir().unwrap();
    let open = r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"get_item"}]}}}"#;
    let invoke = r#"{"success":true,"data":{"output":{"native":1}}}"#;
    let origin_ok = r#"{"success":true,"data":{"result":"https://example.com","origin":"https://example.com/"}}"#;
    let script = common::Fake::new()
        .open(open)
        .invoke(&[invoke])
        .lists(&[r#"{"success":true,"data":{"tools":[{"name":"get_item","origin":"https://example.com","frameId":"ABC123"}]}}"#])
        .evals(&[origin_ok])
        .install(&dir, "ab");
    let cfg = agent_browser_custom_cfg(&script, false);
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(outcome) => {
            let env = outcome.envelope();
            assert_eq!(env["data"]["source"], json!("native"), "{env:?}");
            assert!(env["data"].get("toolHash").is_none(), "{env:?}");
            assert!(env["data"].get("untrusted").is_none(), "{env:?}");
            assert_eq!(env["data"]["output"], json!({"native": 1}), "{env:?}");
        }
        Err(e) => panic!("expected native shadow run, got {e:?}"),
    }
    // The origin binding ran over stdin before invoking, and the invoke
    // was bound to the verified frame.
    let lines = common::Fake::argv(&dir, "ab");
    assert!(
        lines
            .iter()
            .any(|l| l.contains(" eval ") && l.contains("--stdin")),
        "{lines:?}"
    );
    let invoke_line = lines.iter().find(|l| l.contains(" invoke ")).unwrap();
    assert!(invoke_line.contains("--frame ABC123"), "{invoke_line}");
}

/// A redirect to a different origin that registers the same native tool
/// name must NOT invoke it — even with a blanket allow — and reports
/// `origin_mismatch` instead.
#[cfg(unix)]
#[test]
fn native_collision_wrong_origin_refuses() {
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let (tools, search) = custom_search_with_tools("get_item", json!({}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());

    let dir = tempfile::tempdir().unwrap();
    let open = r#"{"success":true,"data":{"url":"https://evil.com/app","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"get_item"}]}}}"#;
    let origin_evil =
        r#"{"success":true,"data":{"result":"https://evil.com","origin":"https://evil.com/"}}"#;
    let script = common::Fake::new()
        .open(open)
        .invoke(&[common::INVOKE_OK])
        .evals(&[origin_evil])
        .install(&dir, "ab");
    let cfg = agent_browser_custom_cfg(&script, false);
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "origin_mismatch", "{e:?}");
    let lines = common::Fake::argv(&dir, "ab");
    assert!(
        !lines.iter().any(|l| l.contains(" invoke ")),
        "redirected native tool must not run: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn native_collision_ambiguous_same_origin_refuses() {
    use common::HomeGuard;
    // Two same-origin registrations of one name cannot be bound to a
    // single frame: refuse instead of invoking either.
    let _home = HomeGuard::isolate();
    let (tools, search) = custom_search_with_tools("get_item", json!({}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let open = r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"get_item"}]}}}"#;
    let origin_ok = r#"{"success":true,"data":{"result":"https://example.com","origin":"https://example.com/"}}"#;
    let script = common::Fake::new()
        .open(open)
        .invoke(&[common::INVOKE_OK])
        .lists(&[r#"{"success":true,"data":{"tools":[{"name":"get_item","origin":"https://example.com","frameId":"AAA"},{"name":"get_item","origin":"https://example.com","frameId":"BBB"}]}}"#])
        .evals(&[origin_ok])
        .install(&dir, "ab");
    let cfg = agent_browser_custom_cfg(&script, false);
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
    assert_eq!(e.code, "ambiguous_tool", "{e:?}");
    let lines = common::Fake::argv(&dir, "ab");
    assert!(
        !lines.iter().any(|l| l.contains(" invoke ")),
        "ambiguous native tools must not run: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn native_collision_missing_frame_id_refuses_unframed_invoke() {
    // A same-origin registration without a frameId must not fall back to an
    // unframed invoke: refuse instead of running under the daemon's default
    // resolution. Covers both a missing and an explicitly empty frameId.
    use common::HomeGuard;
    for tools_json in [
        r#"{"success":true,"data":{"tools":[{"name":"get_item","origin":"https://example.com"}]}}"#,
        r#"{"success":true,"data":{"tools":[{"name":"get_item","origin":"https://example.com","frameId":""}]}}"#,
    ] {
        let _home = HomeGuard::isolate();
        let (tools, search) = custom_search_with_tools("get_item", json!({}));
        let resolved =
            call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let open = r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"get_item"}]}}}"#;
        let origin_ok = r#"{"success":true,"data":{"result":"https://example.com","origin":"https://example.com/"}}"#;
        let script = common::Fake::new()
            .open(open)
            .invoke(&[common::INVOKE_OK])
            .lists(&[tools_json])
            .evals(&[origin_ok])
            .install(&dir, "ab");
        let cfg = agent_browser_custom_cfg(&script, false);
        let opts = CallOptions {
            confirm: true,
            plan_only: false,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        };
        let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
        assert_eq!(e.code, "origin_mismatch", "{e:?} for {tools_json}");
        let lines = common::Fake::argv(&dir, "ab");
        assert!(
            !lines.iter().any(|l| l.contains(" invoke ")),
            "unverifiable frame must not run: {lines:?}"
        );
    }
}

/// A live native collision with only a custom-scoped grant asks for the
/// native identity instead of running anything: no invoke, NeedsConfirmation
/// (both without and with `confirm`, since custom approval never covers the
/// native tool).
#[cfg(unix)]
#[test]
fn native_collision_without_native_grant_needs_confirmation() {
    use common::HomeGuard;
    for confirm in [false, true] {
        let _home = HomeGuard::isolate();
        let (tools, search) = custom_search_with_tools("get_item", json!({"readOnly": true}));
        let resolved =
            call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
        assert!(resolved.is_custom());
        let dir = tempfile::tempdir().unwrap();
        let open = r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"get_item"}]}}}"#;
        let script = common::Fake::new()
            .open(open)
            .invoke(&[common::INVOKE_OK])
            .install(&dir, "ab");
        let mut cfg = agent_browser_custom_cfg(&script, false);
        cfg.permissions.allow.clear();
        cfg.permissions.allow.push("custom/*".into());
        let opts = CallOptions {
            confirm,
            plan_only: false,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        };
        match call::execute(&resolved, json!({}), &opts, &cfg) {
            Ok(call::Outcome::NeedsConfirmation {
                native_id, message, ..
            }) => {
                assert_eq!(native_id, "example.com/get_item", "{message}");
                assert!(message.contains("example.com/get_item"), "{message}");
            }
            other => panic!("expected NeedsConfirmation, got {other:?} (confirm={confirm})"),
        }
        let lines = common::Fake::argv(&dir, "ab");
        assert!(
            !lines.iter().any(|l| l.contains(" invoke ")),
            "native tool must not run without a native grant: {lines:?}"
        );
    }
}

/// A live native collision denied by config blocks: no invoke, Blocked.
#[cfg(unix)]
#[test]
fn native_collision_denied_blocks() {
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let (tools, search) = custom_search_with_tools("get_item", json!({"readOnly": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    let dir = tempfile::tempdir().unwrap();
    let open = r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"get_item"}]}}}"#;
    let script = common::Fake::new()
        .open(open)
        .invoke(&[common::INVOKE_OK])
        .install(&dir, "ab");
    let mut cfg = agent_browser_custom_cfg(&script, false);
    cfg.permissions.allow.clear();
    cfg.permissions.allow.push("custom/*".into());
    cfg.permissions.deny.push("example.com/get_item".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(call::Outcome::Blocked {
            native_id, message, ..
        }) => {
            assert_eq!(native_id, "example.com/get_item", "{message}");
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
    let lines = common::Fake::argv(&dir, "ab");
    assert!(
        !lines.iter().any(|l| l.contains(" invoke ")),
        "denied native tool must not run: {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn custom_unavailable_and_origin_mismatch_map() {
    use common::HomeGuard;
    for (inner, code) in [
        (
            json!({"ok": false, "code": "unavailable", "reason": "login needed", "remediation": "log in"}),
            "unavailable",
        ),
        (
            json!({"ok": false, "code": "origin_mismatch", "error": "page redirected to evil.com"}),
            "origin_mismatch",
        ),
        (
            json!({"ok": false, "code": "tool_error", "error": "boom"}),
            "tool_error",
        ),
    ] {
        let _home = HomeGuard::isolate();
        let tools = tempfile::tempdir().unwrap();
        valid_host(
            tools.path(),
            "example.com",
            "get_item",
            json!({}),
            "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
        );
        let empty = Index {
            schema_version: 1,
            generated_at: "2026-11-19T00:00:00Z".into(),
            sites: vec![],
        };
        let search = Search::build(empty).unwrap();
        let resolved =
            call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = common::Fake::new()
            .open(r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":false},"webmcp":{"tools":[]}}}"#)
            .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
            .evals(&[ORIGIN_OK, &eval_outer(&inner)])
            .install(&dir, "ab");
        let cfg = agent_browser_custom_cfg(&script, false);
        let opts = CallOptions {
            confirm: true,
            plan_only: false,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        };
        let e = call::execute(&resolved, json!({}), &opts, &cfg).unwrap_err();
        assert_eq!(e.code, code, "{inner:?} got {e:?}");
        if code == "unavailable" {
            assert_eq!(e.data.as_ref().unwrap()["reason"], json!("login needed"));
            assert_eq!(e.data.as_ref().unwrap()["remediation"], json!("log in"));
        }
    }
}

/// A strict custom schema must not block a shadowing live native tool
/// with a different input shape: the native tool runs.
#[cfg(unix)]
#[test]
fn native_collision_ignores_custom_schema() {
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({
            "inputSchema": {
                "type": "object",
                "required": ["path"],
                "properties": {"path": {"type": "string"}},
                "additionalProperties": false,
            },
        }),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let search = empty_search();
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    assert!(resolved.is_custom());
    // Input valid for nothing custom-side, but the live native tool runs.
    let dir = tempfile::tempdir().unwrap();
    let open = r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"get_item"}]}}}"#;
    let invoke = r#"{"success":true,"data":{"output":{"native":1}}}"#;
    let origin_ok = r#"{"success":true,"data":{"result":"https://example.com","origin":"https://example.com/"}}"#;
    let script = common::Fake::new()
        .open(open)
        .invoke(&[invoke])
        .lists(&[r#"{"success":true,"data":{"tools":[{"name":"get_item","origin":"https://example.com","frameId":"ABC123"}]}}"#])
        .evals(&[origin_ok])
        .install(&dir, "ab");
    let mut cfg = agent_browser_custom_cfg(&script, false);
    cfg.permissions.allow.clear();
    cfg.permissions.allow.push("example.com/get_item".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({"query": "x"}), &opts, &cfg) {
        Ok(outcome) => {
            assert_eq!(outcome.envelope()["data"]["source"], json!("native"));
            assert_eq!(outcome.envelope()["data"]["output"], json!({"native": 1}));
        }
        Err(e) => panic!("expected native run, got {e:?}"),
    }
}

/// Invalid custom input never reaches custom JS when no native tool runs.
#[cfg(unix)]
#[test]
fn invalid_custom_input_fails_before_custom_js() {
    use common::HomeGuard;
    let schema = json!({
        "inputSchema": {
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}},
            "additionalProperties": false,
        },
    });
    // agent-browser: no eval runs at all.
    {
        let _home = HomeGuard::isolate();
        let tools = tempfile::tempdir().unwrap();
        valid_host(
            tools.path(),
            "example.com",
            "get_item",
            schema.clone(),
            "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
        );
        let search = empty_search();
        let resolved =
            call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = common::Fake::new()
            .open(r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":false},"webmcp":{"tools":[]}}}"#)
            .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
            .install(&dir, "ab");
        let cfg = agent_browser_custom_cfg(&script, false);
        let opts = CallOptions {
            confirm: true,
            plan_only: false,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        };
        let e = call::execute(&resolved, json!({"query": "x"}), &opts, &cfg).unwrap_err();
        assert_eq!(e.code, "invalid_input", "{e:?}");
        assert!(e.message.contains("path"), "{e:?}");
        let lines = common::Fake::argv(&dir, "ab");
        assert!(
            !lines.iter().any(|l| l.contains(" eval ")),
            "invalid input must not reach custom JS: {lines:?}"
        );
    }
    // browse: no execute runs at all.
    {
        let tools = tempfile::tempdir().unwrap();
        let mut browse_schema = schema.clone();
        browse_schema["allowBrowse"] = json!(true);
        valid_host(
            tools.path(),
            "example.com",
            "get_item",
            browse_schema,
            "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
        );
        let search = empty_search();
        let resolved =
            call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("browse.log");
        let path = browse_fake(&dir, &log, false, None);
        let mut cfg = config::Config::default();
        cfg.driver = config::DriverChoice::Browse;
        cfg.drivers.browse.command = path.display().to_string();
        cfg.permissions.allow.push("custom/*".into());
        let opts = CallOptions {
            confirm: true,
            plan_only: false,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        };
        let e = call::execute(&resolved, json!({"query": "x"}), &opts, &cfg).unwrap_err();
        assert_eq!(e.code, "invalid_input", "{e:?}");
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            !logged.contains("execute"),
            "invalid input must not reach custom JS: {logged}"
        );
    }
    // lightpanda: the single document gates evaluation on the embedded
    // validity verdict instead, and reports input_invalid without running
    // custom JS; Rust maps it back to the stored validation error.
    {
        let tools = tempfile::tempdir().unwrap();
        valid_host(
            tools.path(),
            "example.com",
            "get_item",
            schema.clone(),
            "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
        );
        let search = empty_search();
        let resolved =
            call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("lp.log");
        let path = lightpanda_fake(&dir, &log, r#"{"ok":false,"code":"input_invalid"}"#, "{}");
        let mut cfg = config::Config::default();
        cfg.driver = config::DriverChoice::Lightpanda;
        cfg.drivers.lightpanda.command = path.display().to_string();
        cfg.permissions.allow.push("custom/*".into());
        let opts = CallOptions {
            confirm: true,
            plan_only: false,
            fresh: false,
            retry_hint: call::RETRY_CLI,
        };
        let e = call::execute(&resolved, json!({"query": "x"}), &opts, &cfg).unwrap_err();
        assert_eq!(e.code, "invalid_input", "{e:?}");
        assert!(e.message.contains("path"), "{e:?}");
        assert_eq!(lightpanda_run_count(&log), 1, "one document");
        let script = lightpanda_run_script(&log, 0);
        assert!(script.contains("input_invalid"), "{script}");
        assert!(
            script.find("input_invalid").unwrap() < script.find("__webmcpCustom").unwrap(),
            "input gate precedes evaluation: {script}"
        );
    }
}

#[cfg(unix)]
#[test]
fn agent_browser_absent_native_api_falls_through_to_custom() {
    use common::HomeGuard;
    // With settle enabled but no native WebMCP API at all, a custom-only
    // call still runs instead of failing with webmcp_unsupported.
    let _home = HomeGuard::isolate();
    let (tools, search) = custom_search_with_tools("get_item", json!({}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let inner_ok = json!({"ok": true, "output": {"answer": 1}});
    let script = common::Fake::new()
        .open(r#"{"success":true,"data":{"url":"https://example.com/app","lifecycle":{"reused":false},"webmcp":{"tools":[]}}}"#)
        .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
        .evals(&[common::EVAL_UNDEFINED, ORIGIN_OK, &eval_outer(&inner_ok)])
        .install(&dir, "ab");
    let mut cfg = agent_browser_custom_cfg(&script, false);
    cfg.call.settle_ms = 300;
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(outcome) => {
            assert_eq!(outcome.envelope()["data"]["source"], json!("custom"));
            assert_eq!(outcome.envelope()["data"]["output"], json!({"answer": 1}));
        }
        Err(e) => panic!("expected custom run, got {e:?}"),
    }
}

#[cfg(unix)]
#[test]
fn browse_unsupported_list_falls_back_to_custom_eval() {
    // A `webmcp_unsupported` list failure means no native tool can exist:
    // the custom tool runs instead of erroring.
    let (tools, search) = custom_search_with_tools("get_item", json!({"allowBrowse": true}));
    let resolved = call::resolve_in("example.com/get_item", &search, Some(tools.path())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let inner = json!({"ok": true, "output": {"answer": 7}});
    let suffix = serde_json::to_string(&serde_json::to_string(&inner).unwrap()).unwrap();
    let script = {
        use std::os::unix::fs::PermissionsExt;
        let s = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\ncase \"$1\" in\n  status) printf '{{\"connected\":true,\"relay\":{{\"version\":\"0.1.5\"}}}}\\n' ;;\n  navigate) printf '{{\"tab\":7,\"url\":\"https://example.com/app\",\"title\":\"App\"}}\\n' ;;\n  webmcp) printf '{{\"error\":\"navigator.modelContext is not available\"}}\\n' ;;\n  tabs) printf '{{\"ok\":true}}\\n' ;;\n  execute) cat > '{log}.execute.stdin'\n    marker=$(grep -o '__AWM_[^\"]*' '{log}.execute.stdin' | head -1)\n    if grep -q '\"expression\":\"location.origin\"' '{log}.execute.stdin'; then printf '%s\"https://example.com\"\\n' \"$marker\"; else printf '%s%s\\n' \"$marker\" '{suffix}'; fi ;;\nesac\n",
            log = log.display(),
        );
        let path = dir.path().join("browse");
        std::fs::write(&path, s).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::read(&path);
        for _ in 0..100 {
            match std::process::Command::new(&path).arg("status").output() {
                Ok(_) => break,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        let _ = std::fs::write(&log, "");
        path
    };
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = script.display().to_string();
    cfg.permissions.allow.push("custom/*".into());
    let opts = CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    };
    match call::execute(&resolved, json!({}), &opts, &cfg) {
        Ok(outcome) => {
            assert_eq!(outcome.envelope()["data"]["source"], json!("custom"));
            assert_eq!(outcome.envelope()["data"]["output"], json!({"answer": 7}));
        }
        Err(e) => panic!("expected custom fallback run, got {e:?}"),
    }
}

#[test]
fn mcp_approval_pins_tool_hash() {
    let tools = tempfile::tempdir().unwrap();
    let js_v1 = "__webmcpCustom({ get_item: { execute: async (i) => 1 } });";
    valid_host(tools.path(), "example.com", "get_item", json!({}), js_v1);
    let loaded_v1 = agent_webmcp::tools::load_host_in(tools.path(), "example.com")
        .unwrap()
        .unwrap();
    let input = json!({});
    let page = "https://example.com/app";
    let s1 = call::approval_state(
        "example.com/get_item",
        &input,
        Some(&loaded_v1.sha256),
        "agent-browser",
        page,
        None,
        None,
        None,
    );
    // One-byte edit to tools.js changes the hash and invalidates approval.
    let js_path = tools.path().join("example.com/tools.js");
    std::fs::write(&js_path, format!("{js_v1}\n")).unwrap();
    let loaded_v2 = agent_webmcp::tools::load_host_in(tools.path(), "example.com")
        .unwrap()
        .unwrap();
    assert_ne!(loaded_v1.sha256, loaded_v2.sha256);
    let s2 = call::approval_state(
        "example.com/get_item",
        &input,
        Some(&loaded_v2.sha256),
        "agent-browser",
        page,
        None,
        None,
        None,
    );
    assert_ne!(s1, s2, "tool-hash pinning must invalidate retry approval");
    // A driver change between prompt and retry invalidates approval too.
    let s3 = call::approval_state(
        "example.com/get_item",
        &input,
        Some(&loaded_v1.sha256),
        "browse",
        page,
        None,
        None,
        None,
    );
    assert_ne!(s1, s3, "driver change must invalidate retry approval");
    // A re-resolution to a different page invalidates approval too.
    let s4 = call::approval_state(
        "example.com/get_item",
        &input,
        Some(&loaded_v1.sha256),
        "agent-browser",
        "https://example.com/other",
        None,
        None,
        None,
    );
    assert_ne!(s1, s4, "page change must invalidate retry approval");
    // A browser-context change (profile or CDP) invalidates approval too.
    let s5 = call::approval_state(
        "example.com/get_item",
        &input,
        Some(&loaded_v1.sha256),
        "agent-browser",
        page,
        Some("work"),
        None,
        None,
    );
    assert_ne!(s1, s5, "profile change must invalidate retry approval");
    let s6 = call::approval_state(
        "example.com/get_item",
        &input,
        Some(&loaded_v1.sha256),
        "agent-browser",
        page,
        None,
        Some("http://127.0.0.1:9222"),
        None,
    );
    assert_ne!(s1, s6, "cdp change must invalidate retry approval");
    // A session change invalidates approval too.
    let s7 = call::approval_state(
        "example.com/get_item",
        &input,
        Some(&loaded_v1.sha256),
        "agent-browser",
        page,
        None,
        None,
        Some("other-session"),
    );
    assert_ne!(s1, s7, "session change must invalidate retry approval");
}

// --- inspect merging ---

#[cfg(unix)]
#[test]
fn inspect_merges_custom_with_live_availability() {
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let avail_inner = json!({"ok": true, "tools": {"get_item": {"ok": true}}});
    let script = common::Fake::new()
        .open(r#"{"success":true,"data":{"url":"https://example.com/app","title":"App","webmcp":{"tools":[]}}}"#)
        .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
        .evals(&[
            r#"{"success":true,"data":{"result":"object","origin":"https://example.com/"}}"#,
            ORIGIN_OK,
            &eval_outer(&avail_inner),
        ])
        .install(&dir, "ab");
    let mut cfg = common::run_config(&script);
    cfg.driver = config::DriverChoice::AgentBrowser;
    cfg.call.settle_ms = 0;
    let res = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: false },
        &cfg,
        Some(tools.path()),
    )
    .unwrap();
    let custom_tool = res
        .tools
        .iter()
        .find(|t| t.name == "get_item")
        .expect("custom merged");
    assert_eq!(custom_tool.source.as_deref(), Some("custom"));
    assert_eq!(custom_tool.available, Some(true));
    let env = res.envelope();
    let entry = env["data"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "get_item")
        .unwrap();
    assert_eq!(entry["source"], json!("custom"));
    assert_eq!(entry["available"], json!(true));
}

#[cfg(unix)]
#[test]
fn inspect_denied_custom_tool_is_never_evaluated() {
    // A custom tool denied by `cfg.permissions` must not have its
    // `tools.js` evaluated during `inspect`: it is filtered before the
    // driver's probe and reported unavailable without any browser eval of
    // its script.
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(r#"{"success":true,"data":{"url":"https://example.com/app","title":"App","webmcp":{"tools":[]}}}"#)
        .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
        .evals(&[
            r#"{"success":true,"data":{"result":"object","origin":"https://example.com/"}}"#,
        ])
        .install(&dir, "ab");
    let mut cfg = common::run_config(&script);
    cfg.driver = config::DriverChoice::AgentBrowser;
    cfg.call.settle_ms = 0;
    cfg.permissions.deny = vec!["example.com/get_item".to_string()];
    let res = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: false },
        &cfg,
        Some(tools.path()),
    )
    .unwrap();
    let custom_tool = res
        .tools
        .iter()
        .find(|t| t.name == "get_item")
        .expect("denied custom still listed");
    assert_eq!(custom_tool.available, Some(false));
    assert_eq!(
        custom_tool.unavailable_reason.as_deref(),
        Some("denied by permissions")
    );
    let stdin = common::Fake::eval_stdin(&dir, "ab");
    assert!(
        !stdin.contains("__webmcpCustom"),
        "denied tools.js must never reach the page: {stdin:?}"
    );
}

#[cfg(unix)]
#[test]
fn inspect_sibling_of_denied_tool_is_not_probed() {
    // Both tools on a host share one `tools.js`: when one is denied, the
    // whole host is refused — the permitted sibling must not cause the
    // shared script (with the denied tool's top-level code) to be
    // evaluated, and both report unavailable without any probe.
    use common::HomeGuard;
    let _home = HomeGuard::isolate();
    let tools = tempfile::tempdir().unwrap();
    let dir_name = agent_webmcp::tools::host_dir_name("example.com").unwrap();
    let tools_json = json!([
        tool_def("allow_me", json!({})),
        tool_def("deny_me", json!({}))
    ]);
    let m = manifest("example.com", "https://example.com/app", tools_json);
    write_host(
        tools.path(),
        &dir_name,
        &m,
        "__webmcpCustom({ allow_me: { execute: async (i) => ({}) }, deny_me: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(r#"{"success":true,"data":{"url":"https://example.com/app","title":"App","webmcp":{"tools":[]}}}"#)
        .lists(&[r#"{"success":true,"data":{"tools":[]}}"#])
        .evals(&[
            r#"{"success":true,"data":{"result":"object","origin":"https://example.com/"}}"#,
        ])
        .install(&dir, "ab");
    let mut cfg = common::run_config(&script);
    cfg.driver = config::DriverChoice::AgentBrowser;
    cfg.call.settle_ms = 0;
    cfg.permissions.deny = vec!["example.com/deny_me".to_string()];
    let res = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: false },
        &cfg,
        Some(tools.path()),
    )
    .unwrap();
    assert_eq!(res.tools.len(), 2, "both host tools listed: {res:?}");
    for t in &res.tools {
        assert_eq!(t.available, Some(false), "{t:?}");
        assert!(
            t.unavailable_reason.as_deref().is_some(),
            "unavailable reason set: {t:?}"
        );
    }
    let denied = res.tools.iter().find(|t| t.name == "deny_me").unwrap();
    assert_eq!(
        denied.unavailable_reason.as_deref(),
        Some("denied by permissions")
    );
    let stdin = common::Fake::eval_stdin(&dir, "ab");
    assert!(
        !stdin.contains("__webmcpCustom"),
        "shared tools.js must never reach the page when a sibling is denied: {stdin:?}"
    );
}

#[cfg(unix)]
#[test]
fn inspect_browse_allowlist_excludes_http_custom_origins() {
    // A custom tool declared on http:// must never enter the navigate
    // allowlist when the navigation can carry personal-Chrome cookies.
    // Its availability probe still fails closed as origin_mismatch without
    // running any JS.
    use std::os::unix::fs::PermissionsExt;
    let tools = tempfile::tempdir().unwrap();
    let root = tools.path().join("tools");
    std::fs::create_dir_all(&root).unwrap();
    let tools_json = json!([tool_def("get_item", json!({"allowBrowse": true}))]);
    write_host(
        &root,
        "example.com",
        &manifest("example.com", "http://example.com/app", tools_json),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  status) printf '{{\"connected\":true,\"relay\":{{\"version\":\"0.1.5\"}}}}\\n' ;;\n  navigate) printf '{{\"tab\":7,\"url\":\"https://example.com/app\",\"title\":\"App\"}}\\n' ;;\n  webmcp) printf '{{\"available\":true,\"tools\":[]}}\\n' ;;\n  tabs) printf '{{\"ok\":true}}\\n' ;;\n  execute) body=$(cat); marker=$(printf '%s' \"$body\" | grep -o '__AWM_[^\"]*' | head -1); if printf '%s' \"$body\" | grep -q '\"expression\":\"location.origin\"'; then printf '%s\"https://example.com\"\\n' \"$marker\"; else printf 'unexpected: execute should not run custom JS\\n' >&2; exit 2; fi ;;\nesac\n",
        log.display()
    );
    let path = dir.path().join("browse");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cfg = common::allow_all();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.call.settle_ms = 0;
    let res = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: true },
        &cfg,
        Some(root.as_path()),
    )
    .unwrap();
    let logged = std::fs::read_to_string(&log).unwrap();
    let nav_line = logged
        .lines()
        .find(|l| l.contains("navigate"))
        .unwrap()
        .to_string();
    assert!(nav_line.contains("allowedOrigins"), "{nav_line}");
    assert!(
        !nav_line.contains("http://"),
        "no http origin may enter the allowlist: {nav_line}"
    );
    let custom_tool = res
        .tools
        .iter()
        .find(|t| t.name == "get_item")
        .expect("custom merged");
    assert_eq!(custom_tool.available, Some(false));
    assert_eq!(
        custom_tool.unavailable_reason.as_deref(),
        Some("origin_mismatch")
    );
}

#[cfg(unix)]
#[test]
fn inspect_browse_opt_out_reports_unavailable_without_js() {
    use std::os::unix::fs::PermissionsExt;
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({"allowBrowse": false}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("browse.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  status) printf '{{\"connected\":true,\"relay\":{{\"version\":\"0.1.5\"}}}}\\n' ;;\n  navigate) printf '{{\"tab\":7,\"url\":\"https://example.com/app\",\"title\":\"App\"}}\\n' ;;\n  webmcp) printf '{{\"available\":true,\"tools\":[]}}\\n' ;;\n  tabs) printf '{{\"ok\":true}}\\n' ;;\n  execute) printf 'unexpected: execute should not run\\n' >&2; exit 2 ;;\nesac\n",
        log.display()
    );
    let path = dir.path().join("browse");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cfg = common::allow_all();
    cfg.driver = config::DriverChoice::Browse;
    cfg.drivers.browse.command = path.display().to_string();
    cfg.call.settle_ms = 0;
    let res = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: true },
        &cfg,
        Some(tools.path()),
    )
    .unwrap();
    let custom_tool = res
        .tools
        .iter()
        .find(|t| t.name == "get_item")
        .expect("custom merged");
    assert_eq!(custom_tool.available, Some(false));
    assert_eq!(
        custom_tool.unavailable_reason.as_deref(),
        Some("browse_not_allowed")
    );
    let logged = std::fs::read_to_string(log).unwrap();
    assert!(
        !logged.contains("execute"),
        "opt-out must not run JS: {logged}"
    );
}

#[cfg(unix)]
#[test]
fn inspect_lightpanda_headed_reports_needs_headed() {
    use std::os::unix::fs::PermissionsExt;
    let tools = tempfile::tempdir().unwrap();
    valid_host(
        tools.path(),
        "example.com",
        "get_item",
        json!({"headed": true}),
        "__webmcpCustom({ get_item: { execute: async (i) => ({}) } });",
    );
    let dir = tempfile::tempdir().unwrap();
    let script = "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then echo \"lightpanda 1.1.0\"; exit 0; fi\nprintf '{\"ok\":true,\"tools\":[]}\\n'\n";
    let path = dir.path().join("lightpanda");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cfg = config::Config::default();
    cfg.driver = config::DriverChoice::Lightpanda;
    cfg.drivers.lightpanda.command = path.display().to_string();
    cfg.call.settle_ms = 0;
    let res = agent_webmcp::inspect::run_inspect_in(
        "https://example.com/app",
        &agent_webmcp::inspect::InspectOptions { fresh: false },
        &cfg,
        Some(tools.path()),
    )
    .unwrap();
    let custom_tool = res
        .tools
        .iter()
        .find(|t| t.name == "get_item")
        .expect("custom merged");
    assert_eq!(custom_tool.available, Some(false));
    assert_eq!(
        custom_tool.unavailable_reason.as_deref(),
        Some("needs_headed")
    );
}
