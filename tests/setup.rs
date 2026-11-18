use agent_webmcp::setup::{
    Configured, browser_report, configure_opencode, merge_opencode_mcp, newer_available, skill_dirs,
};
use std::path::PathBuf;
use std::sync::Mutex;

/// Tests below mutate XDG_CONFIG_HOME; serialise them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

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
fn versions_follow_semver_prerelease_order() {
    assert!(newer_available("0.3.0-rc.1", "0.3.0"));
    assert!(!newer_available("0.3.0", "0.3.0-rc.1"));
    assert!(newer_available("0.2.9", "0.3.0-rc.1"));
    assert!(newer_available("0.3.0-rc.1", "0.3.0-rc.2"));
    assert!(newer_available("0.3.0-rc.9", "0.3.0-rc.10"));
    assert!(newer_available("0.3.0-alpha", "0.3.0-beta"));
    assert!(!newer_available("0.2.0", "0.2"));
    assert!(!newer_available("0.2.0", "0.2.1.4"));
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

/// A config with comments (JSONC) is not valid strict JSON: setup must leave
/// it untouched and report Skipped instead of aborting.
#[test]
fn jsonc_config_is_left_untouched() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cfg_dir = dir.path().join("opencode");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let path = cfg_dir.join("opencode.json");
    let original = "{\n  // comments are not strict JSON\n  \"mcp\": {}\n}\n";
    std::fs::write(&path, original).unwrap();

    let old = std::env::var_os("XDG_CONFIG_HOME");
    unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
    let result = configure_opencode();
    match old {
        Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }

    assert!(matches!(result.unwrap(), Configured::Skipped(_)));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

/// Valid JSON with the wrong shape is a real error, not a silent skip.
#[test]
fn valid_json_with_wrong_shape_still_errors() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cfg_dir = dir.path().join("opencode");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("opencode.json"), r#"{"mcp": 42}"#).unwrap();

    let old = std::env::var_os("XDG_CONFIG_HOME");
    unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
    let result = configure_opencode();
    match old {
        Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }

    assert!(result.is_err());
}
