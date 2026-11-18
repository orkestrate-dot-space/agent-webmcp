use agent_webmcp::setup::{browser_report, merge_opencode_mcp, newer_available, skill_dirs};
use std::path::PathBuf;

#[test]
fn merge_adds_server() {
    let out = merge_opencode_mcp(r#"{"mcp": {}}"#, "/bin/agent-webmcp")
        .unwrap()
        .unwrap();
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["mcp"]["agent-webmcp"]["type"], "local");
    assert_eq!(doc["mcp"]["agent-webmcp"]["command"][1], "mcp");
}

#[test]
fn merge_keeps_existing_when_present() {
    let existing = r#"{"mcp": {"agent-webmcp": {"type": "local", "command": ["x", "mcp"]}}}"#;
    assert!(
        merge_opencode_mcp(existing, "/bin/agent-webmcp")
            .unwrap()
            .is_none()
    );
}

#[test]
fn merge_rejects_bad_json() {
    assert!(merge_opencode_mcp("not json", "/bin/agent-webmcp").is_err());
}

#[test]
fn versions_compare_numerically() {
    assert!(newer_available("0.2.0", "0.2.1"));
    assert!(newer_available("0.2.0", "0.10.0"));
    assert!(!newer_available("0.2.1", "0.2.0"));
    assert!(!newer_available("0.2.0", "0.2.0"));
    assert!(!newer_available("0.2.0", "next"));
}

#[test]
fn skill_dirs_cover_the_three_clients() {
    let dirs = skill_dirs(&PathBuf::from("/home/test"));
    assert_eq!(dirs.len(), 3);
    assert!(dirs.iter().all(|d| d.starts_with("/home/test")));
}

#[test]
fn browser_report_covers_drivers() {
    let report = browser_report().join("\n");
    assert!(report.contains("agent-browser"));
    assert!(report.contains("Playwright"));
    assert!(report.contains("DevTools"));
    assert!(report.contains("Browse"));
}
