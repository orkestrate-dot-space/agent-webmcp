mod common;

use agent_webmcp::setup::{
    Configured, InstallFlag, Offer, configure_opencode, driver_report, merge_opencode_mcp,
    newer_available, should_offer, skill_dirs,
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

#[cfg(unix)]
#[test]
fn driver_report_comes_from_detection() {
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().install(&dir, "ab");
    let mut cfg = common::run_config(&script);
    cfg.drivers.lightpanda.command = dir.path().join("no-such-lightpanda").display().to_string();
    let report = driver_report(&cfg).join("\n");
    assert!(report.contains("agent-browser (run): ready"), "{report}");
    assert!(
        report.contains("lightpanda (run, experimental): missing; install:"),
        "{report}"
    );
    assert!(report.contains("playwright-mcp (plan): needs"), "{report}");
    assert!(
        report.contains("chrome-devtools-mcp (plan): needs"),
        "{report}"
    );
    assert!(report.contains("browse (plan)"), "{report}");
    assert!(report.contains("call will use: agent-browser"), "{report}");
}

#[cfg(unix)]
#[test]
fn driver_report_without_run_driver() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = common::allow_all();
    cfg.drivers.agent_browser.command = dir
        .path()
        .join("no-such-agent-browser")
        .display()
        .to_string();
    cfg.drivers.lightpanda.command = dir.path().join("no-such-lightpanda").display().to_string();
    let report = driver_report(&cfg).join("\n");
    assert!(
        report.contains("agent-browser (run): missing; install:"),
        "{report}"
    );
    assert!(
        report.contains("lightpanda (run, experimental): missing; install:"),
        "{report}"
    );
    assert!(
        report.contains("call will use: nothing yet (no run driver installed)"),
        "{report}"
    );
}

#[test]
fn offer_logic() {
    use InstallFlag::{InstallLightpanda, NoInstall};
    use Offer::{Ask, Install, Never};
    // A ready run driver never offers, whatever the flags say.
    assert_eq!(should_offer(true, true, InstallFlag::None), Never);
    assert_eq!(should_offer(true, true, InstallLightpanda), Never);
    assert_eq!(should_offer(true, false, InstallLightpanda), Never);
    // No terminal: never ask, `--install-driver` still installs.
    assert_eq!(should_offer(false, false, InstallFlag::None), Never);
    assert_eq!(should_offer(false, false, NoInstall), Never);
    assert_eq!(should_offer(false, false, InstallLightpanda), Install);
    // `--no-install` never asks, even on a terminal.
    assert_eq!(should_offer(false, true, NoInstall), Never);
    // Interactive with no flag asks; the flag installs without asking.
    assert_eq!(should_offer(false, true, InstallFlag::None), Ask);
    assert_eq!(should_offer(false, true, InstallLightpanda), Install);
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
