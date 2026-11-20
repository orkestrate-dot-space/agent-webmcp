//! Config precedence, merging, JSONC, substitution, and project limits.
//! Every test builds its own env closure; process env is never touched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use agent_webmcp::config;

fn base_env(home: &Path) -> HashMap<String, String> {
    HashMap::from([("HOME".to_string(), home.display().to_string())])
}

fn load_with(map: HashMap<String, String>, cwd: &Path) -> anyhow::Result<config::Loaded> {
    config::load_from(&|key: &str| map.get(key).cloned(), cwd)
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn workdir(tmp: &tempfile::TempDir, name: &str) -> (PathBuf, PathBuf) {
    let home = tmp.path().join("home");
    let cwd = tmp.path().join(name);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    (home, cwd)
}

#[test]
fn no_files_returns_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    let loaded = load_with(base_env(&home), &cwd).unwrap();
    assert_eq!(loaded.config.driver, config::DriverChoice::Auto);
    assert_eq!(loaded.config.call.timeout, 60_000);
    assert_eq!(loaded.config.drivers.agent_browser.command, "agent-browser");
    assert_eq!(loaded.config.drivers.agent_browser.session, "agent-webmcp");
    assert!(!loaded.config.drivers.agent_browser.headed);
    assert_eq!(loaded.config.drivers.lightpanda.command, "lightpanda");
    assert!(loaded.config.permissions.allow.is_empty());
    assert!(loaded.sources.is_empty());
    assert!(loaded.warnings.is_empty());
}

#[test]
fn user_file_only_is_used() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"driver": "agent-browser", "call": {"timeout": 5000}}"#,
    );
    let loaded = load_with(base_env(&home), &cwd).unwrap();
    assert_eq!(loaded.config.driver, config::DriverChoice::AgentBrowser);
    assert_eq!(loaded.config.call.timeout, 5000);
    assert_eq!(loaded.sources.len(), 1);
    assert_eq!(loaded.sources[0].kind, config::SourceKind::User);
}

#[test]
fn project_file_overrides_a_user_scalar() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"timeout": 5000}, "drivers": {"agent-browser": {"session": "mine"}}}"#,
    );
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"call": {"timeout": 7000}}"#,
    );
    let loaded = load_with(base_env(&home), &repo).unwrap();
    assert_eq!(loaded.config.call.timeout, 7000);
    assert_eq!(loaded.config.drivers.agent_browser.session, "mine");
    assert_eq!(loaded.sources.len(), 2);
    assert_eq!(loaded.sources[0].kind, config::SourceKind::User);
    assert_eq!(loaded.sources[1].kind, config::SourceKind::Project);
}

#[test]
fn permissions_lists_append_and_dedupe() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"permissions": {"allow": ["a"], "ask": ["x"], "deny": ["ud"]}}"#,
    );
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"permissions": {"ask": ["x", "y"], "deny": ["pd"]}}"#,
    );
    let loaded = load_with(base_env(&home), &repo).unwrap();
    assert_eq!(loaded.config.permissions.allow, vec!["a"]);
    assert_eq!(loaded.config.permissions.ask, vec!["x", "y"]);
    assert_eq!(loaded.config.permissions.deny, vec!["ud", "pd"]);
}

#[test]
fn jsonc_comments_trailing_commas_and_string_slashes() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.jsonc"),
        "{\n\
         // line comment\n\
         \"driver\": \"browse\", /* block comment */\n\
         \"drivers\": {\n\
         \"agent-browser\": {\n\
         \"session\": \"a//b\", // trailing comment\n\
         },\n\
         },\n\
         }\n",
    );
    let loaded = load_with(base_env(&home), &cwd).unwrap();
    assert_eq!(loaded.config.driver, config::DriverChoice::Browse);
    assert_eq!(loaded.config.drivers.agent_browser.session, "a//b");
}

#[test]
fn parse_error_names_path_and_line() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    let path = home.join(".config/agent-webmcp/agent-webmcp.json");
    write(&path, "{\"driver\": \n");
    let err = load_with(base_env(&home), &cwd).unwrap_err().to_string();
    assert!(err.contains(&path.display().to_string()), "{err}");
    assert!(err.contains("line"), "{err}");
}

#[test]
fn both_json_and_jsonc_uses_jsonc_with_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    let dir = home.join(".config/agent-webmcp");
    write(&dir.join("agent-webmcp.json"), r#"{"driver": "browse"}"#);
    write(&dir.join("agent-webmcp.jsonc"), r#"{"driver": "none"}"#);
    let loaded = load_with(base_env(&home), &cwd).unwrap();
    assert_eq!(loaded.config.driver, config::DriverChoice::None);
    assert_eq!(loaded.warnings.len(), 1);
    assert!(
        loaded.warnings[0].contains(".jsonc"),
        "{}",
        loaded.warnings[0]
    );
}

#[test]
fn explicit_config_skips_the_project_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let custom = tmp.path().join("custom.json");
    write(&custom, r#"{"call": {"timeout": 111}}"#);
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"call": {"timeout": 222}}"#,
    );
    let mut env = base_env(&home);
    env.insert(
        "AGENT_WEBMCP_CONFIG".to_string(),
        custom.display().to_string(),
    );
    let loaded = load_with(env, &repo).unwrap();
    assert_eq!(loaded.config.call.timeout, 111);
    assert_eq!(loaded.sources.len(), 1);
    assert_eq!(loaded.sources[0].kind, config::SourceKind::User);
    assert_eq!(loaded.sources[0].path.as_ref().unwrap(), &custom);
}

#[test]
fn env_overrides_beat_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"timeout": 5000}, "drivers": {"agent-browser": {"session": "file"}}}"#,
    );
    let mut env = base_env(&home);
    env.insert("AGENT_WEBMCP_TIMEOUT".to_string(), "1234".to_string());
    env.insert("AGENT_WEBMCP_SESSION".to_string(), "env-s".to_string());
    env.insert("AGENT_WEBMCP_DRIVER".to_string(), "browse".to_string());
    env.insert("AGENT_WEBMCP_HEADED".to_string(), "true".to_string());
    env.insert(
        "AGENT_WEBMCP_AGENT_BROWSER".to_string(),
        "/usr/bin/ab".to_string(),
    );
    let loaded = load_with(env, &cwd).unwrap();
    assert_eq!(loaded.config.call.timeout, 1234);
    assert_eq!(loaded.config.drivers.agent_browser.session, "env-s");
    assert_eq!(loaded.config.driver, config::DriverChoice::Browse);
    assert!(loaded.config.drivers.agent_browser.headed);
    assert_eq!(loaded.config.drivers.agent_browser.command, "/usr/bin/ab");
    assert_eq!(loaded.sources.last().unwrap().kind, config::SourceKind::Env);
}

#[test]
fn bad_timeout_errors_naming_the_var() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    let mut env = base_env(&home);
    env.insert("AGENT_WEBMCP_TIMEOUT".to_string(), "soon".to_string());
    let err = load_with(env, &cwd).unwrap_err().to_string();
    assert!(err.contains("AGENT_WEBMCP_TIMEOUT"), "{err}");
}

#[test]
fn empty_env_value_counts_as_unset() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"timeout": 5000}}"#,
    );
    let mut env = base_env(&home);
    env.insert("AGENT_WEBMCP_TIMEOUT".to_string(), String::new());
    let loaded = load_with(env, &cwd).unwrap();
    assert_eq!(loaded.config.call.timeout, 5000);
    assert!(
        loaded
            .sources
            .iter()
            .all(|s| s.kind != config::SourceKind::Env)
    );
}

#[test]
fn project_file_setting_lightpanda_command_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"drivers": {"lightpanda": {"command": "/bin/evil"}}}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("drivers.lightpanda.command"), "{err}");
    assert!(err.contains("project config can only set"), "{err}");
}

#[test]
fn lightpanda_command_from_file_and_env() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"driver": "lightpanda", "drivers": {"lightpanda": {"command": "/bin/lp"}}}"#,
    );
    let mut env = base_env(&home);
    env.insert(
        "AGENT_WEBMCP_LIGHTPANDA".to_string(),
        "/usr/bin/lp".to_string(),
    );
    let loaded = load_with(env, &cwd).unwrap();
    assert_eq!(loaded.config.driver, config::DriverChoice::Lightpanda);
    assert_eq!(loaded.config.drivers.lightpanda.command, "/usr/bin/lp");
}

#[test]
fn invalid_driver_value_names_lightpanda() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    let mut env = base_env(&home);
    env.insert("AGENT_WEBMCP_DRIVER".to_string(), "nope".to_string());
    let err = load_with(env, &cwd).unwrap_err().to_string();
    assert!(err.contains("lightpanda"), "{err}");
}

#[test]
fn project_file_setting_command_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"command": "/bin/evil"}}}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("drivers.agent-browser.command"), "{err}");
    assert!(err.contains("project config can only set"), "{err}");
}

#[test]
fn project_file_setting_profile_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"profile": "/tmp/p"}}}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("drivers.agent-browser.profile"), "{err}");
    assert!(err.contains("project config can only set"), "{err}");
}

#[test]
fn project_file_setting_cdp_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"cdp": "http://x"}}}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("drivers.agent-browser.cdp"), "{err}");
    assert!(err.contains("project config can only set"), "{err}");
}

#[test]
fn unknown_key_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"bogus": 1}"#,
    );
    let err = load_with(base_env(&home), &cwd).unwrap_err().to_string();
    assert!(err.contains("bogus"), "{err}");
    assert!(
        err.contains(
            &home
                .join(".config/agent-webmcp/agent-webmcp.json")
                .display()
                .to_string()
        ),
        "{err}"
    );
}

#[test]
fn env_substitution_replaces_and_empties_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"session": "{env:WB_SESSION}-x"}}, "permissions": {"allow": ["{env:WB_MISSING}"]}}"#,
    );
    let mut env = base_env(&home);
    env.insert("WB_SESSION".to_string(), "live".to_string());
    let loaded = load_with(env, &cwd).unwrap();
    assert_eq!(loaded.config.drivers.agent_browser.session, "live-x");
    assert_eq!(loaded.config.permissions.allow, vec!["".to_string()]);
}

#[test]
fn file_substitution_reads_from_home() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(&home.join("token.txt"), "s3cret\n");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"session": "{file:~/token.txt}"}}}"#,
    );
    let loaded = load_with(base_env(&home), &cwd).unwrap();
    assert_eq!(loaded.config.drivers.agent_browser.session, "s3cret");
}

#[test]
fn file_substitution_reads_relative_to_the_config_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    let dir = home.join(".config/agent-webmcp");
    write(&dir.join("secret.txt"), "rel\n");
    write(
        &dir.join("agent-webmcp.json"),
        r#"{"call": {"timeout": 999}, "drivers": {"agent-browser": {"session": "{file:secret.txt}"}}}"#,
    );
    let loaded = load_with(base_env(&home), &cwd).unwrap();
    assert_eq!(loaded.config.drivers.agent_browser.session, "rel");
}

#[test]
fn project_file_substitution_outside_the_root_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    write(&tmp.path().join("outer.txt"), "nope\n");
    let sub = tmp.path().join("proj/sub");
    std::fs::create_dir_all(tmp.path().join("proj/.git")).unwrap();
    std::fs::create_dir_all(&sub).unwrap();
    write(
        &sub.join("agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"session": "{file:../../outer.txt}"}}}"#,
    );
    let err = load_with(base_env(&home), &sub).unwrap_err().to_string();
    assert!(err.contains("outside"), "{err}");
}

#[test]
fn project_walk_stops_at_the_git_root() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    write(
        &tmp.path().join("outer/agent-webmcp.json"),
        r#"{"call": {"timeout": 1111}}"#,
    );
    let sub = tmp.path().join("outer/repo/sub");
    std::fs::create_dir_all(tmp.path().join("outer/repo/.git")).unwrap();
    std::fs::create_dir_all(&sub).unwrap();
    let loaded = load_with(base_env(&home), &sub).unwrap();
    assert_eq!(loaded.config.call.timeout, 60_000);
    assert!(loaded.sources.is_empty());
}

#[test]
fn xdg_config_home_is_respected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    let xdg = tmp.path().join("xdg");
    write(
        &xdg.join("agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"timeout": 4321}}"#,
    );
    let mut env = base_env(&home);
    env.insert("XDG_CONFIG_HOME".to_string(), xdg.display().to_string());
    let loaded = load_with(env, &cwd).unwrap();
    assert_eq!(loaded.config.call.timeout, 4321);
    assert_eq!(loaded.sources.len(), 1);
}

#[test]
fn empty_xdg_config_home_falls_back_to_home() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"timeout": 999}}"#,
    );
    let mut env = base_env(&home);
    env.insert("XDG_CONFIG_HOME".to_string(), String::new());
    let loaded = load_with(env, &cwd).unwrap();
    assert_eq!(loaded.config.call.timeout, 999);
}

#[test]
fn apply_overrides_beats_env() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, cwd) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"timeout": 1000}}"#,
    );
    let mut env = base_env(&home);
    env.insert("AGENT_WEBMCP_TIMEOUT".to_string(), "2000".to_string());
    env.insert("AGENT_WEBMCP_DRIVER".to_string(), "browse".to_string());
    let mut loaded = load_with(env, &cwd).unwrap();
    assert_eq!(loaded.config.call.timeout, 2000);
    loaded.config.apply_overrides(&config::Overrides {
        driver: Some(config::DriverChoice::None),
        session: Some("flag-s".to_string()),
        headed: Some(true),
        timeout: Some(3000),
        settle: Some(1500),
        command: Some("/bin/from-flag".to_string()),
    });
    assert_eq!(loaded.config.call.timeout, 3000);
    assert_eq!(loaded.config.call.settle_ms, 1500);
    assert_eq!(loaded.config.driver, config::DriverChoice::None);
    assert_eq!(loaded.config.drivers.agent_browser.session, "flag-s");
    assert!(loaded.config.drivers.agent_browser.headed);
    assert_eq!(
        loaded.config.drivers.agent_browser.command,
        "/bin/from-flag"
    );
}

#[test]
fn project_file_with_allow_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"permissions": {"allow": ["cal.com/*"]}}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("permissions.allow"), "{err}");
    assert!(err.contains("project config can only set"), "{err}");
}

#[test]
fn project_file_with_driver_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(&repo.join("agent-webmcp.json"), r#"{"driver": "none"}"#);
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("sets driver;"), "{err}");
}

#[test]
fn project_file_with_session_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"session": "shared"}}}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("drivers.agent-browser.session"), "{err}");
}

#[test]
fn project_file_with_only_allowed_keys_loads() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    write(
        &home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"timeout": 5000}, "drivers": {"agent-browser": {"headed": false}}, "permissions": {"ask": ["u"], "deny": ["ud"]}}"#,
    );
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"$schema": "https://web.orkestrate.space/webmcp/config.json", "call": {"timeout": 7000}, "drivers": {"agent-browser": {"headed": true}}, "permissions": {"ask": ["p"], "deny": ["pd"]}}"#,
    );
    let loaded = load_with(base_env(&home), &repo).unwrap();
    assert_eq!(loaded.config.call.timeout, 7000);
    assert!(loaded.config.drivers.agent_browser.headed);
    assert_eq!(loaded.config.permissions.ask, vec!["u", "p"]);
    assert_eq!(loaded.config.permissions.deny, vec!["ud", "pd"]);
}

#[test]
fn project_file_with_wrong_type_names_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let path = repo.join("agent-webmcp.json");
    write(&path, r#"{"call": {"timeout": "fast"}}"#);
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains(&path.display().to_string()), "{err}");
}

#[test]
fn explicit_config_may_set_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let custom = tmp.path().join("custom.json");
    write(
        &custom,
        r#"{"driver": "agent-browser", "drivers": {"agent-browser": {"command": "/bin/ab", "session": "s", "profile": "/tmp/p", "cdp": "http://x"}}, "permissions": {"allow": ["*"]}}"#,
    );
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"call": {"timeout": 1}}"#,
    );
    let mut env = base_env(&home);
    env.insert(
        "AGENT_WEBMCP_CONFIG".to_string(),
        custom.display().to_string(),
    );
    let loaded = load_with(env, &repo).unwrap();
    assert_eq!(loaded.config.driver, config::DriverChoice::AgentBrowser);
    assert_eq!(loaded.config.drivers.agent_browser.command, "/bin/ab");
    assert_eq!(loaded.config.drivers.agent_browser.session, "s");
    assert_eq!(loaded.config.permissions.allow, vec!["*"]);
}

#[test]
fn call_settle_ms_parses_and_env_overrides() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("work");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let mut base = HashMap::from([("HOME".to_string(), home.display().to_string())]);
    std::fs::create_dir_all(home.join(".config/agent-webmcp")).unwrap();
    std::fs::write(
        home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"settleMs": 9000}}"#,
    )
    .unwrap();
    let loaded = load_with(base.clone(), &cwd).unwrap();
    assert_eq!(loaded.config.call.settle_ms, 9000);
    base.insert("AGENT_WEBMCP_SETTLE".to_string(), "2500".to_string());
    let loaded = load_with(base, &cwd).unwrap();
    assert_eq!(loaded.config.call.settle_ms, 2500);
    assert!(
        loaded
            .sources
            .iter()
            .any(|s| s.kind == config::SourceKind::Env)
    );
}

#[test]
fn bad_settle_errors_naming_the_var() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut env = HashMap::from([("HOME".to_string(), home.display().to_string())]);
    env.insert("AGENT_WEBMCP_SETTLE".to_string(), "soon".to_string());
    let err = load_with(env, tmp.path()).unwrap_err().to_string();
    assert!(err.contains("AGENT_WEBMCP_SETTLE"), "{err}");
}

#[test]
fn timeout_zero_or_huge_in_file_is_rejected() {
    for timeout in ["0", "3600001"] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("work");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(home.join(".config/agent-webmcp")).unwrap();
        std::fs::write(
            home.join(".config/agent-webmcp/agent-webmcp.json"),
            format!(r#"{{"call": {{"timeout": {timeout}}}}}"#),
        )
        .unwrap();
        let err = load_with(base_env(&home), &cwd).unwrap_err().to_string();
        assert!(err.contains("timeout must be 1..3600000 ms"), "{err}");
        assert!(err.contains("agent-webmcp.json"), "names the file: {err}");
    }
}

#[test]
fn settle_ms_out_of_range_in_file_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("work");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(home.join(".config/agent-webmcp")).unwrap();
    std::fs::write(
        home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"call": {"settleMs": 99999}}"#,
    )
    .unwrap();
    let err = load_with(base_env(&home), &cwd).unwrap_err().to_string();
    assert!(err.contains("settle_ms must be 0..30000 ms"), "{err}");
}

#[test]
fn env_timeout_out_of_range_names_the_var() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut env = base_env(&home);
    env.insert("AGENT_WEBMCP_TIMEOUT".to_string(), "0".to_string());
    let err = load_with(env, tmp.path()).unwrap_err().to_string();
    assert!(err.contains("AGENT_WEBMCP_TIMEOUT"), "{err}");
    assert!(err.contains("timeout must be 1..3600000 ms"), "{err}");
}

#[test]
fn extra_args_merge_and_env_parse() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("work");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(home.join(".config/agent-webmcp")).unwrap();
    std::fs::write(
        home.join(".config/agent-webmcp/agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"extraArgs": ["--from-file"]}}}"#,
    )
    .unwrap();
    let mut env = base_env(&home);
    env.insert(
        "AGENT_WEBMCP_EXTRA_ARGS".to_string(),
        "a, b,,c ".to_string(),
    );
    let loaded = load_with(env, &cwd).unwrap();
    assert_eq!(
        loaded.config.drivers.agent_browser.extra_args,
        vec!["a", "b", "c"]
    );
}

#[test]
fn project_file_setting_extra_args_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"drivers": {"agent-browser": {"extraArgs": ["--x"]}}}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("drivers.agent-browser.extraArgs"), "{err}");
}

#[test]
fn project_file_positional_arrays_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, _) = workdir(&tmp, "work");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    // Root array would otherwise decode positionally into Config.
    write(
        &repo.join("agent-webmcp.json"),
        r#"["lightpanda", {"agent-browser": {"command": "/tmp/evil"}}]"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("<root>"), "{err}");

    // Container array inside an allowed key is also rejected.
    write(
        &repo.join("agent-webmcp.json"),
        r#"{"drivers": [{"command": "/tmp/evil"}]}"#,
    );
    let err = load_with(base_env(&home), &repo).unwrap_err().to_string();
    assert!(err.contains("drivers"), "{err}");
}
