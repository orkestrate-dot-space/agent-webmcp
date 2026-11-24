//! Driver adapters: exact plan steps, agent-browser detection, choose().

mod common;

use agent_webmcp::config;
use agent_webmcp::driver::{self, CallRequest, Choice, Detection};
#[cfg(unix)]
use std::path::PathBuf;

fn sample() -> CallRequest {
    CallRequest {
        page: "https://cal.com/".to_string(),
        tool: "ask_cal_com".to_string(),
        input: serde_json::json!({"question": "hi"}),
        custom: None,
    }
}

fn config_with(driver: config::DriverChoice, command: &str) -> config::Config {
    let mut cfg = config::Config::default();
    cfg.driver = driver;
    cfg.drivers.agent_browser.command = command.to_string();
    cfg
}

#[test]
fn browse_plan_passes_the_tab() {
    let cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::Browse, &cfg).unwrap();
    assert_eq!(driver.id(), "browse");
    let plan = driver.plan(&sample());
    assert_eq!(
        plan.note.as_deref(),
        Some("uses your connected Chrome via the browse extension")
    );
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].tool, "navigate");
    assert_eq!(
        plan.steps[0].args,
        serde_json::json!({"url": "https://cal.com/"})
    );
    assert!(
        plan.steps[0].why.contains("tab id"),
        "{}",
        plan.steps[0].why
    );
    assert_eq!(plan.steps[1].tool, "webmcp");
    assert_eq!(
        plan.steps[1].args,
        serde_json::json!({"action": "call", "name": "ask_cal_com", "input": {"question": "hi"}})
    );
    assert_eq!(plan.steps[1].why, "pass the tab from step 1");
}

#[test]
fn generic_plan_is_plain_language() {
    let cfg = config_with(config::DriverChoice::None, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::None, &cfg).unwrap();
    assert_eq!(driver.id(), "generic");
    let plan = driver.plan(&sample());
    assert!(
        plan.note
            .as_deref()
            .unwrap()
            .contains("no browser driver configured"),
        "{:?}",
        plan.note
    );
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].tool, "open");
    assert_eq!(
        plan.steps[0].args,
        serde_json::json!({"url": "https://cal.com/"})
    );
    assert_eq!(plan.steps[1].tool, "call");
    assert_eq!(
        plan.steps[1].args,
        serde_json::json!({"tool": "ask_cal_com", "input": {"question": "hi"}})
    );
}

#[test]
fn agent_browser_plan_argv_comes_from_config() {
    let mut cfg = config_with(config::DriverChoice::AgentBrowser, "agent-browser");
    cfg.drivers.agent_browser.session = "work".to_string();
    let driver = driver::by_choice(config::DriverChoice::AgentBrowser, &cfg).unwrap();
    let plan = driver.plan(&sample());
    assert_eq!(plan.steps.len(), 2);
    for step in &plan.steps {
        assert_eq!(step.tool, "shell");
    }
    assert_eq!(
        plan.steps[0].args,
        serde_json::json!({"command": ["agent-browser", "--session", "work", "--json", "--args", "--enable-features=WebMCP", "open", "https://cal.com/"]})
    );
    assert_eq!(
        plan.steps[1].args,
        serde_json::json!({"command": ["agent-browser", "--session", "work", "--json", "webmcp", "invoke", "ask_cal_com", "--params", "{\"question\":\"hi\"}"]})
    );
}

fn browse_custom_req(page: &str, allow_browse: bool) -> CallRequest {
    use std::sync::Arc;
    CallRequest {
        page: page.to_string(),
        tool: "get_item".to_string(),
        input: serde_json::json!({}),
        custom: Some(driver::CustomExec {
            host: "example.com".to_string(),
            common: driver::CustomCommon {
                expected_origin: "https://example.com".to_string(),
                js_source: Arc::from("__webmcpCustom({});"),
                tool_hash: "abc123def456".to_string(),
                input_schema: serde_json::json!({}),
                headed: false,
                allow_browse,
            },
            dir: std::path::PathBuf::from("/tmp/x"),
            auth: agent_webmcp::tools::AuthMode::None,
            native_policy: driver::NativeCollisionPolicy::Ask,
        }),
    }
}

#[test]
fn browse_custom_plan_allowists_expected_origin() {
    let cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::Browse, &cfg).unwrap();
    let plan = driver.plan(&browse_custom_req("https://example.com/app", true));
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].tool, "navigate");
    assert_eq!(
        plan.steps[0].args,
        serde_json::json!({
            "url": "https://example.com/app",
            "allowedOrigins": ["https://example.com"],
        })
    );
    assert_eq!(plan.steps[1].tool, "shell");
    let cmd = plan.steps[1].args["command"].as_array().unwrap();
    assert_eq!(cmd[0], serde_json::json!("agent-webmcp"));
    assert_eq!(cmd[1], serde_json::json!("call"));
    assert!(
        cmd.windows(2)
            .any(|w| w == [serde_json::json!("--driver"), serde_json::json!("browse")]),
        "emitted call must pin the browse driver: {plan:?}"
    );
    assert!(
        cmd.windows(2)
            .any(|w| w == [serde_json::json!("--params"), serde_json::json!("@-")]),
        "input travels over stdin, never argv (CWE-214): {plan:?}"
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|s| { s.args.to_string().contains("<render_call_iife>") }),
        "no literal placeholder: {plan:?}"
    );
    assert!(
        !cmd.iter()
            .any(|v| v.as_str().is_some_and(|s| s.contains("hi"))),
        "no raw input in the printed command: {plan:?}"
    );
}

#[test]
fn browse_opted_out_plan_routes_native_through_guarded_cli() {
    let cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::Browse, &cfg).unwrap();
    let plan = driver.plan(&browse_custom_req("https://example.com/app", false));
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].tool, "navigate");
    // The native tool must go through the guarded CLI call path, which
    // enforces the native Ask/Deny policy, the origin match, and the
    // same-origin frame binding — never a direct agent-side webmcp
    // invocation that would skip all three (CWE-863).
    assert!(
        plan.steps.iter().all(|s| s.tool != "webmcp"),
        "no direct webmcp step: {plan:?}"
    );
    assert_eq!(plan.steps[1].tool, "shell");
    let cmd = plan.steps[1].args["command"].as_array().unwrap();
    assert!(
        cmd.windows(2)
            .any(|w| w == [serde_json::json!("--driver"), serde_json::json!("browse")]),
        "emitted call must pin the browse driver: {plan:?}"
    );
}

#[test]
fn browse_custom_plan_refuses_http_navigation() {
    let cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::Browse, &cfg).unwrap();
    // Opted-in and opted-out custom tools alike: no navigation step for
    // cleartext pages, mirroring the runtime refusal.
    for allow_browse in [true, false] {
        let plan = driver.plan(&browse_custom_req("http://example.com/app", allow_browse));
        assert!(
            plan.steps.iter().all(|s| s.tool != "navigate"),
            "no navigate step for http: {plan:?}"
        );
        assert!(
            plan.note.as_deref().unwrap_or("").contains("http"),
            "refusal must name the reason: {plan:?}"
        );
    }
}

#[test]
fn agent_browser_custom_plan_uses_guarded_cli() {
    let cfg = config_with(config::DriverChoice::AgentBrowser, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::AgentBrowser, &cfg).unwrap();
    let plan = driver.plan(&browse_custom_req("https://example.com/app", true));
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].tool, "shell");
    let cmd = plan.steps[0].args["command"].as_array().unwrap();
    assert_eq!(cmd[0], serde_json::json!("agent-webmcp"));
    assert_eq!(cmd[1], serde_json::json!("call"));
    assert!(
        cmd.windows(2).any(|w| w
            == [
                serde_json::json!("--driver"),
                serde_json::json!("agent-browser")
            ]),
        "emitted call must pin the agent-browser driver: {plan:?}"
    );
    assert!(
        cmd.windows(2)
            .any(|w| w == [serde_json::json!("--params"), serde_json::json!("@-")]),
        "input travels over stdin, never argv (CWE-214): {plan:?}"
    );
    assert!(
        !cmd.iter()
            .any(|v| v.as_str().is_some_and(|s| s.contains("hi"))),
        "no raw input in the printed command: {plan:?}"
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|s| s.args.to_string().contains("<render_call_iife>")),
        "no literal placeholder: {plan:?}"
    );
}

#[test]
fn agent_browser_custom_plan_uses_fresh_for_http_navigation() {
    let cfg = config_with(config::DriverChoice::AgentBrowser, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::AgentBrowser, &cfg).unwrap();
    let plan = driver.plan(&browse_custom_req("http://example.com/app", true));
    // An http:// page needs a clean throwaway session: the plan recommends
    // the guarded CLI with --fresh (a command the runtime accepts) instead
    // of refusing with no steps.
    assert_eq!(plan.steps.len(), 1, "one guarded call: {plan:?}");
    let cmd = plan.steps[0].args["command"].as_array().unwrap();
    assert!(
        cmd.iter().any(|v| v == "--fresh"),
        "http plan must pass --fresh: {plan:?}"
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|s| s.args.to_string().contains("<render_call_iife>")),
        "no literal placeholder: {plan:?}"
    );
}

#[cfg(unix)]
fn ok_output(stdout: &str) -> std::process::Output {
    use std::os::unix::process::ExitStatusExt;
    std::process::Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

#[cfg(unix)]
#[test]
fn detect_parses_version_strings() {
    use agent_webmcp::driver::agent_browser::detect_with;
    for (stdout, version) in [
        ("agent-browser 0.43.1\n", "0.43.1"),
        ("0.43.0", "0.43.0"),
        ("v0.44.0-beta.1", "0.44.0-beta.1"),
    ] {
        match detect_with("agent-browser", &|_| Ok(ok_output(stdout))) {
            Detection::Ready { version: found } => assert_eq!(found, version),
            other => panic!("{stdout:?} gave {other:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn detect_garbage_output_is_missing() {
    use agent_webmcp::driver::agent_browser::detect_with;
    for stdout in ["garbage", "", "version unknown"] {
        match detect_with("agent-browser", &|_| Ok(ok_output(stdout))) {
            Detection::Missing { install, detail } => {
                assert!(install.contains("npm i -g"), "{install}");
                assert!(detail.is_some(), "{stdout:?} had no detail");
            }
            other => panic!("{stdout:?} gave {other:?}"),
        }
    }
}

#[test]
fn detect_not_found_is_missing_without_detail() {
    use agent_webmcp::driver::agent_browser::detect_with;
    let run = |_: &str| -> std::io::Result<std::process::Output> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file",
        ))
    };
    match detect_with("agent-browser", &run) {
        Detection::Missing { install, detail } => {
            assert!(install.contains("npm i -g"), "{install}");
            assert_eq!(detail, None);
        }
        other => panic!("gave {other:?}"),
    }
}

#[test]
fn detect_other_errors_are_missing_with_detail() {
    use agent_webmcp::driver::agent_browser::detect_with;
    let run = |_: &str| -> std::io::Result<std::process::Output> {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "took too long",
        ))
    };
    match detect_with("agent-browser", &run) {
        Detection::Missing { detail, .. } => assert!(detail.is_some()),
        other => panic!("gave {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn detect_old_version_is_too_old() {
    use agent_webmcp::driver::agent_browser::detect_with;
    match detect_with("agent-browser", &|_| Ok(ok_output("agent-browser 0.40.0"))) {
        Detection::TooOld {
            version,
            min,
            install,
        } => {
            assert_eq!(version, "0.40.0");
            assert_eq!(min, "0.43.0");
            assert!(install.contains("npm i -g"), "{install}");
        }
        other => panic!("gave {other:?}"),
    }
}

#[test]
fn choose_named_missing_is_unusable() {
    let cfg = config_with(
        config::DriverChoice::AgentBrowser,
        "agent-webmcp-definitely-missing-binary",
    );
    match driver::choose(&cfg) {
        Choice::Unusable { driver, .. } => assert_eq!(driver, "agent-browser"),
        _ => panic!("a missing named driver must not fall back"),
    }
}

#[test]
fn choose_auto_missing_both_is_unusable() {
    let mut cfg = config_with(
        config::DriverChoice::Auto,
        "agent-webmcp-definitely-missing-binary",
    );
    cfg.drivers.browse.command = "browse-definitely-missing-binary".to_string();
    cfg.drivers.lightpanda.command = "lightpanda-definitely-missing-binary".to_string();
    match driver::choose(&cfg) {
        Choice::Unusable { driver, .. } => assert_eq!(driver, "agent-browser"),
        _ => panic!("auto with nothing installed must not run or plan"),
    }
}

#[test]
fn choose_none_plans_generic() {
    let cfg = config_with(config::DriverChoice::None, "agent-browser");
    match driver::choose(&cfg) {
        Choice::Plan(driver) => assert_eq!(driver.id(), "generic"),
        _ => panic!("none should plan generic"),
    }
}

#[cfg(unix)]
#[test]
fn fake_script_ready_detects_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().install(&dir, "agent-browser");
    let cfg = config_with(
        config::DriverChoice::AgentBrowser,
        &script.display().to_string(),
    );
    match driver::choose(&cfg) {
        Choice::Run(driver, version) => {
            assert_eq!(driver.id(), "agent-browser");
            assert_eq!(version, "0.43.1");
        }
        Choice::Plan(_) => panic!("the fake script should be ready, got a plan"),
        Choice::Unusable { driver, detection } => {
            panic!("the fake script should be ready, got unusable {driver}: {detection:?}")
        }
    }
}

#[cfg(unix)]
#[test]
fn fake_script_old_detects_too_old() {
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .version("0.40.0")
        .install(&dir, "agent-browser");
    let cfg = config_with(
        config::DriverChoice::AgentBrowser,
        &script.display().to_string(),
    );
    match driver::choose(&cfg) {
        Choice::Unusable {
            driver,
            detection: Detection::TooOld { version, .. },
        } => {
            assert_eq!(driver, "agent-browser");
            assert_eq!(version, "0.40.0");
        }
        _ => panic!("the old script should be too old"),
    }
}

#[cfg(unix)]
#[test]
fn choose_auto_ready_runs() {
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().install(&dir, "agent-browser");
    let cfg = config_with(config::DriverChoice::Auto, &script.display().to_string());
    match driver::choose(&cfg) {
        Choice::Run(driver, _) => assert_eq!(driver.id(), "agent-browser"),
        Choice::Plan(_) => panic!("auto with a ready driver should run, got a plan"),
        Choice::Unusable { driver, detection } => {
            panic!("auto with a ready driver should run, got unusable {driver}: {detection:?}")
        }
    }
}

// --- lightpanda ---

/// Fake lightpanda: answers `version`, and `run` prints one canned file.
/// The invoked script file is saved for injection assertions.
#[cfg(unix)]
struct LpFake {
    version: String,
    stdout: String,
    stderr: String,
    stall_secs: u64,
}

#[cfg(unix)]
impl LpFake {
    fn new(stdout: &str) -> Self {
        Self {
            version: "1.1.0-nightly.11745+1b60dcc7a".to_string(),
            stdout: stdout.to_string(),
            stderr: String::new(),
            stall_secs: 0,
        }
    }

    fn stderr(mut self, text: &str) -> Self {
        self.stderr = text.to_string();
        self
    }

    fn stall(mut self, secs: u64) -> Self {
        self.stall_secs = secs;
        self
    }

    fn install(&self, dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let base = dir.path().join(name);
        let response = dir.path().join(format!("{name}.response"));
        std::fs::write(&response, &self.stdout).unwrap();
        let err = dir.path().join(format!("{name}.stderr"));
        std::fs::write(&err, &self.stderr).unwrap();
        let log = dir.path().join(format!("{name}.argv"));
        let mut script = String::from("#!/bin/sh\n");
        script.push_str(&format!("echo \"$@\" >> \"{}\"\n", log.display()));
        script.push_str(&format!(
            "if [ \"$1\" = \"version\" ]; then echo \"{}\"; exit 0; fi\n",
            self.version
        ));
        if self.stall_secs > 0 {
            script.push_str(&format!("sleep {}\n", self.stall_secs));
        }
        script.push_str(&format!("cp \"$2\" \"{}.script\"\n", base.display()));
        script.push_str(&format!("cat \"{}\"\n", response.display()));
        script.push_str(&format!("cat \"{}\" >&2\n", err.display()));
        std::fs::write(&base, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..100 {
            match std::process::Command::new(&base).arg("version").output() {
                Ok(_) => break,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        base
    }
}

#[cfg(unix)]
fn lp_config(script: &std::path::Path) -> config::Config {
    let mut cfg = config_with(config::DriverChoice::Lightpanda, "agent-browser");
    cfg.drivers.lightpanda.command = script.display().to_string();
    cfg
}

#[cfg(unix)]
fn lp_run_opts() -> driver::RunOptions {
    driver::RunOptions {
        timeout_ms: 1234,
        settle_ms: 300,
        fresh: false,
    }
}

#[cfg(unix)]
fn lp_run(
    response: &str,
    tool: &str,
) -> Result<agent_webmcp::driver::RunOutcome, agent_webmcp::call::CallError> {
    let dir = tempfile::tempdir().unwrap();
    let script = LpFake::new(response).install(&dir, "lp");
    let cfg = lp_config(&script);
    let driver = driver::by_choice(config::DriverChoice::Lightpanda, &cfg).unwrap();
    let req = CallRequest {
        page: "https://cal.com/".to_string(),
        tool: tool.to_string(),
        input: serde_json::json!({"question": "hi"}),
        custom: None,
    };
    driver.run(&req, &lp_run_opts())
}

#[cfg(unix)]
#[test]
fn lightpanda_detect_parses_nightly_version() {
    use agent_webmcp::driver::lightpanda::detect_with;
    let seen = std::cell::RefCell::new(Vec::new());
    let run = |cmd: &str, arg: &str| -> std::io::Result<std::process::Output> {
        seen.borrow_mut().push(arg.to_string());
        assert_eq!(cmd, "lightpanda");
        Ok(ok_output("1.1.0-nightly.11745+1b60dcc7a"))
    };
    match detect_with("lightpanda", &run) {
        // `parse_version` keeps `-pre` but not `+build`: still parseable.
        Detection::Ready { version } => assert_eq!(version, "1.1.0-nightly.11745"),
        other => panic!("gave {other:?}"),
    }
    // Bare `version`, never `--version` (which is a fatal error there).
    assert_eq!(seen.borrow().clone(), vec!["version".to_string()]);
}

#[cfg(unix)]
#[test]
fn lightpanda_detect_missing_names_the_installer() {
    use agent_webmcp::driver::lightpanda::detect_with;
    let run = |_: &str, _: &str| -> std::io::Result<std::process::Output> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file",
        ))
    };
    match detect_with("lightpanda", &run) {
        Detection::Missing { install, detail } => {
            assert!(install.contains("pkg.lightpanda.io"), "{install}");
            assert_eq!(detail, None);
        }
        other => panic!("gave {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_detect_garbage_is_missing_with_detail() {
    use agent_webmcp::driver::lightpanda::detect_with;
    match detect_with("lightpanda", &|_, _| Ok(ok_output("garbage"))) {
        Detection::Missing { detail, .. } => assert!(detail.is_some()),
        other => panic!("gave {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_run_ok_content_passes_through() {
    match lp_run(
        r#"{"ok":true,"output":{"content":[{"type":"text","text":"hi"}]}}"#,
        "ask_cal_com",
    ) {
        Ok(outcome) => {
            assert_eq!(
                outcome.output,
                serde_json::json!({"content":[{"type":"text","text":"hi"}]})
            );
            assert!(!outcome.reused);
            assert_eq!(outcome.budget_ms, 1234 + 300 + 30_000);
        }
        Err(e) => panic!("expected a run, got {e:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_run_plain_string_is_wrapped() {
    match lp_run(r#"{"ok":true,"output":"hello"}"#, "ask_cal_com") {
        Ok(outcome) => assert_eq!(
            outcome.output,
            serde_json::json!({"content":[{"type":"text","text":"hello"}]})
        ),
        Err(e) => panic!("expected a run, got {e:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_run_not_found_is_tool_not_found() {
    for response in [
        r#"{"ok":false,"code":"not_found"}"#,
        r#"{"ok":false,"error":"Error: No WebMCP tool named \"x\" on this page; pageTools lists them."}"#,
    ] {
        match lp_run(response, "ask_cal_com") {
            Err(e) => {
                assert_eq!(e.code, "tool_not_found", "{response}");
                assert!(e.message.contains("ask_cal_com"), "{}", e.message);
            }
            Ok(outcome) => panic!("expected tool_not_found, got {outcome:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_run_unsupported_api_is_webmcp_unsupported() {
    match lp_run(r#"{"ok":false,"code":"unsupported"}"#, "ask_cal_com") {
        Err(e) => {
            assert_eq!(e.code, "webmcp_unsupported");
            assert!(
                e.message.contains("--enable-features=WebMCP"),
                "{}",
                e.message
            );
        }
        Ok(outcome) => panic!("expected webmcp_unsupported, got {outcome:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_run_thrown_tool_is_tool_error() {
    match lp_run(
        r#"{"ok":false,"error":"Error: the tool blew up"}"#,
        "ask_cal_com",
    ) {
        Err(e) => {
            assert_eq!(e.code, "tool_error");
            assert!(e.message.contains("blew up"), "{}", e.message);
        }
        Ok(outcome) => panic!("expected tool_error, got {outcome:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_run_navigation_failed_is_page_unreachable() {
    let dir = tempfile::tempdir().unwrap();
    let script = LpFake::new("")
        .stderr("Error: navigation failed")
        .install(&dir, "lp");
    let cfg = lp_config(&script);
    let driver = driver::by_choice(config::DriverChoice::Lightpanda, &cfg).unwrap();
    let req = CallRequest {
        page: "https://cal.com/".to_string(),
        tool: "ask_cal_com".to_string(),
        input: serde_json::json!({}),
        custom: None,
    };
    match driver.run(&req, &lp_run_opts()) {
        Err(e) => assert_eq!(e.code, "page_unreachable"),
        Ok(outcome) => panic!("expected page_unreachable, got {outcome:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_run_stalled_child_times_out() {
    let dir = tempfile::tempdir().unwrap();
    let script = LpFake::new(r#"{"ok":true,"output":"late"}"#)
        .stall(60)
        .install(&dir, "lp");
    let cfg = lp_config(&script);
    let driver = driver::by_choice(config::DriverChoice::Lightpanda, &cfg).unwrap();
    let req = CallRequest {
        page: "https://cal.com/".to_string(),
        tool: "ask_cal_com".to_string(),
        input: serde_json::json!({}),
        custom: None,
    };
    let mut opts = lp_run_opts();
    opts.timeout_ms = 200;
    opts.settle_ms = 0;
    match driver.run(&req, &opts) {
        Err(e) => assert_eq!(e.code, "timeout"),
        Ok(outcome) => panic!("expected timeout, got {outcome:?}"),
    }
}

#[cfg(unix)]
#[test]
fn lightpanda_script_embeds_the_tool_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let script = LpFake::new(r#"{"ok":true,"output":"x"}"#).install(&dir, "lp");
    let cfg = lp_config(&script);
    let driver = driver::by_choice(config::DriverChoice::Lightpanda, &cfg).unwrap();
    let tool = "evil\"tool\nname";
    let req = CallRequest {
        page: "https://cal.com/".to_string(),
        tool: tool.to_string(),
        input: serde_json::json!({"q": "hi\"there"}),
        custom: None,
    };
    let _ = driver.run(&req, &lp_run_opts());
    let saved = std::fs::read_to_string(dir.path().join("lp.script")).unwrap();
    assert!(
        saved.contains(&serde_json::to_string(tool).unwrap()),
        "tool name must be JSON-encoded: {saved}"
    );
    // Input crosses as data: the script double-encodes it for JSON.parse,
    // so `{"__proto__": ...}` survives as an own property instead of
    // becoming a prototype assignment in an object literal.
    let input_json = serde_json::to_string(&serde_json::json!({"q": "hi\"there"})).unwrap();
    let double = serde_json::to_string(&input_json).unwrap();
    assert!(
        saved.contains("JSON.parse(") && saved.contains(&double),
        "input must be double-encoded for JSON.parse: {saved}"
    );
}

#[cfg(unix)]
#[test]
fn lightpanda_plan_uses_its_own_tool_names() {
    let dir = tempfile::tempdir().unwrap();
    let script = LpFake::new("").install(&dir, "lp");
    let cfg = lp_config(&script);
    let driver = driver::by_choice(config::DriverChoice::Lightpanda, &cfg).unwrap();
    assert_eq!(driver.id(), "lightpanda");
    let plan = driver.plan(&sample());
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].tool, "goto");
    assert_eq!(
        plan.steps[0].args,
        serde_json::json!({"url": "https://cal.com/"})
    );
    assert_eq!(plan.steps[1].tool, "callPageTool");
    assert_eq!(
        plan.steps[1].args,
        serde_json::json!({"name": "ask_cal_com", "arguments": {"question": "hi"}})
    );
    assert!(
        plan.note.as_deref().unwrap().contains("pageTools"),
        "{:?}",
        plan.note
    );
}

#[test]
fn generic_custom_plan_routes_through_guarded_cli() {
    // With no run driver configured, nothing can register the local
    // script: the plan must not emit a manual `call` step that would fail
    // against the unregistered name. It points at a run driver and the
    // guarded CLI workflow instead.
    let cfg = config_with(config::DriverChoice::None, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::None, &cfg).unwrap();
    let plan = driver.plan(&browse_custom_req("https://example.com/app", true));
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].tool, "shell");
    let cmd = plan.steps[0].args["command"].as_array().unwrap();
    assert_eq!(cmd[0], serde_json::json!("agent-webmcp"));
    assert_eq!(cmd[1], serde_json::json!("call"));
    assert!(
        cmd.windows(2).any(|w| w
            == [
                serde_json::json!("--driver"),
                serde_json::json!("agent-browser")
            ]),
        "emitted call must name a run driver: {plan:?}"
    );
    assert!(
        cmd.windows(2)
            .any(|w| w == [serde_json::json!("--params"), serde_json::json!("@-")]),
        "input travels over stdin, never argv (CWE-214): {plan:?}"
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|s| { s.args.to_string().contains("untrusted local code") || s.tool == "call" }),
        "no manual open/call steps for unregistered custom tools: {plan:?}"
    );
    assert!(
        !cmd.iter()
            .any(|v| v.as_str().is_some_and(|s| s.contains("hi"))),
        "no raw input in the printed command: {plan:?}"
    );
}

#[test]
fn lightpanda_custom_plan_uses_guarded_cli() {
    let cfg = config_with(config::DriverChoice::Lightpanda, "lightpanda");
    let driver = driver::by_choice(config::DriverChoice::Lightpanda, &cfg).unwrap();
    let plan = driver.plan(&browse_custom_req("https://example.com/app", true));
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].tool, "shell");
    let cmd = plan.steps[0].args["command"].as_array().unwrap();
    assert_eq!(cmd[0], serde_json::json!("agent-webmcp"));
    assert_eq!(cmd[1], serde_json::json!("call"));
    assert!(
        cmd.windows(2).any(|w| w
            == [
                serde_json::json!("--driver"),
                serde_json::json!("lightpanda")
            ]),
        "emitted call must pin the lightpanda driver: {plan:?}"
    );
    assert!(
        cmd.windows(2)
            .any(|w| w == [serde_json::json!("--params"), serde_json::json!("@-")]),
        "input travels over stdin, never argv (CWE-214): {plan:?}"
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|s| { s.args.to_string().contains("<render_call_promise_expr>") }),
        "no literal placeholder: {plan:?}"
    );
    assert!(
        !cmd.iter()
            .any(|v| v.as_str().is_some_and(|s| s.contains("hi"))),
        "no raw input in the printed command: {plan:?}"
    );
}

#[cfg(unix)]
#[test]
fn lightpanda_status_marks_experimental() {
    let dir = tempfile::tempdir().unwrap();
    let script = LpFake::new("").install(&dir, "lp");
    let cfg = lp_config(&script);
    let driver = driver::by_choice(config::DriverChoice::Lightpanda, &cfg).unwrap();
    assert!(
        driver::status_line(driver.as_ref(), &cfg).contains("(run, experimental)"),
        "{}",
        driver::status_line(driver.as_ref(), &cfg)
    );
}

#[cfg(unix)]
#[test]
fn choose_auto_prefers_agent_browser_when_both_ready() {
    let dir = tempfile::tempdir().unwrap();
    let ab = common::Fake::new().install(&dir, "agent-browser");
    let lp = LpFake::new("").install(&dir, "lightpanda");
    let mut cfg = config_with(config::DriverChoice::Auto, &ab.display().to_string());
    cfg.drivers.lightpanda.command = lp.display().to_string();
    match driver::choose(&cfg) {
        Choice::Run(driver, _) => assert_eq!(driver.id(), "agent-browser"),
        _ => panic!("both ready should run agent-browser"),
    }
}

#[cfg(unix)]
#[test]
fn choose_auto_falls_back_to_lightpanda() {
    let dir = tempfile::tempdir().unwrap();
    let lp = LpFake::new("").install(&dir, "lightpanda");
    let mut cfg = config_with(
        config::DriverChoice::Auto,
        "agent-webmcp-definitely-missing-binary",
    );
    cfg.drivers.browse.command = "browse-definitely-missing-binary".to_string();
    cfg.drivers.lightpanda.command = lp.display().to_string();
    match driver::choose(&cfg) {
        Choice::Run(driver, _) => assert_eq!(driver.id(), "lightpanda"),
        _ => panic!("only lightpanda ready should run lightpanda"),
    }
}

#[test]
fn choose_named_lightpanda_missing_is_unusable() {
    let mut cfg = config_with(config::DriverChoice::Lightpanda, "agent-browser");
    cfg.drivers.lightpanda.command = "lightpanda-definitely-missing-binary".to_string();
    match driver::choose(&cfg) {
        Choice::Unusable { driver, .. } => assert_eq!(driver, "lightpanda"),
        _ => panic!("a missing named driver must not fall back"),
    }
}

#[cfg(unix)]
struct BrowseFake {
    connected: bool,
    nav_json: String,
    list_json: String,
    call_json: String,
}

#[cfg(unix)]
impl BrowseFake {
    fn ready() -> Self {
        Self {
            connected: true,
            nav_json: r#"{"tab":12,"url":"https://cal.com/","title":"Cal.com"}"#.to_string(),
            list_json: r#"{"available":true,"tools":[{"name":"ask_cal_com"}]}"#.to_string(),
            call_json: r#"{"result":{"content":[{"type":"text","text":"ok"}]}}"#.to_string(),
        }
    }

    fn install(&self, dir: &tempfile::TempDir, name: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.path().join(format!("{name}.log"));
        let status_json = format!(
            r#"{{"connected":{},"relay":{{"version":"0.1.5"}}}}"#,
            self.connected
        );
        let script = format!(
            "#!/bin/sh\n\
printf '%s\\n' \"$*\" >> '{}'\n\
case \"$1\" in\n\
  status) printf '%s\\n' '{}' ;;\n\
  navigate) printf '%s\\n' '{}' ;;\n\
  webmcp)\n\
    case \"$3\" in\n\
      *list*) printf '%s\\n' '{}' ;;\n\
      *) printf '%s\\n' '{}' ;;\n\
    esac ;;\n\
  execute)\n\
    body=$(cat);\n\
    marker=$(printf '%s' \"$body\" | grep -o '__AWM_[^_]*_[^_]*__' | head -n 1);\n\
    case \"$body\" in\n\
      *location.origin*) printf '%s\"https://cal.com\"\\n' \"$marker\" ;;\n\
      *) printf '%s%s\\n' \"$marker\" '{}' ;;\n\
    esac ;;\n\
  tabs) printf '{{\"ok\":true}}\\n' ;;\n\
  *) echo \"unknown: $*\" >&2; exit 2 ;;\n\
esac\n",
            log.display(),
            status_json,
            self.nav_json,
            self.list_json,
            self.call_json,
            self.call_json
        );
        let path = dir.path().join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Exec-visibility barrier (mirrors `common::Fake::install`): a
        // just-written script can answer ETXTBSY under concurrency. Warm it
        // up once so the argv log starts clean.
        let _ = std::fs::read(&path);
        for _ in 0..100 {
            match std::process::Command::new(&path).output() {
                Ok(_) => break,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        (path, log)
    }
}

#[cfg(unix)]
#[test]
fn choose_auto_prefers_browse_over_lightpanda() {
    let dir = tempfile::tempdir().unwrap();
    let (br, _) = BrowseFake::ready().install(&dir, "browse");
    let lp = LpFake::new("").install(&dir, "lightpanda");
    let mut cfg = config_with(
        config::DriverChoice::Auto,
        "agent-webmcp-definitely-missing-binary",
    );
    cfg.drivers.browse.command = br.display().to_string();
    cfg.drivers.lightpanda.command = lp.display().to_string();
    match driver::choose(&cfg) {
        Choice::Run(driver, version) => {
            assert_eq!(driver.id(), "browse");
            assert_eq!(version, "0.1.5");
        }
        _ => panic!("connected browse should run before lightpanda"),
    }
}

#[cfg(unix)]
#[test]
fn browse_disconnected_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = BrowseFake::ready();
    fake.connected = false;
    let (br, _) = fake.install(&dir, "browse");
    let mut cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    cfg.drivers.browse.command = br.display().to_string();
    match driver::choose(&cfg) {
        Choice::Unusable {
            driver,
            detection: Detection::Missing { detail, .. },
        } => {
            assert_eq!(driver, "browse");
            assert!(
                detail.as_deref().unwrap_or("").contains("not connected")
                    || detail
                        .as_deref()
                        .unwrap_or("")
                        .contains("no browser extension"),
                "{detail:?}"
            );
        }
        _ => panic!("disconnected browse must be unusable"),
    }
}

#[cfg(unix)]
#[test]
fn browse_run_happy_path_and_fresh_closes_tab() {
    let dir = tempfile::tempdir().unwrap();
    let (br, log) = BrowseFake::ready().install(&dir, "browse");
    let mut cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    cfg.drivers.browse.command = br.display().to_string();
    let driver = driver::by_choice(config::DriverChoice::Browse, &cfg).unwrap();
    let mut opts = lp_run_opts();
    opts.fresh = true;
    let out = driver.run(&sample(), &opts).unwrap();
    assert_eq!(out.output["content"][0]["text"], "ok");
    let logged = std::fs::read_to_string(log).unwrap();
    assert!(logged.contains("navigate --json"), "{logged}");
    assert!(logged.contains("webmcp --json"), "{logged}");
    // The native call travels over `execute --file -` (stdin), never argv.
    assert!(logged.contains("execute --file -"), "{logged}");
    assert!(logged.contains("tabs --json"), "{logged}");
    // Secret-bearing input must not appear in child argv.
    assert!(!logged.contains("question"), "{logged}");
}

#[cfg(unix)]
#[test]
fn browse_run_maps_challenge_unsupported_and_missing_tool() {
    let dir = tempfile::tempdir().unwrap();

    let mut blocked = BrowseFake::ready();
    blocked.nav_json =
        r#"{"tab":12,"url":"https://cal.com/","title":"Just a moment...","challenge":true}"#
            .to_string();
    let (br1, _) = blocked.install(&dir, "browse1");
    let mut cfg1 = config_with(config::DriverChoice::Browse, "agent-browser");
    cfg1.drivers.browse.command = br1.display().to_string();
    let err1 = driver::by_choice(config::DriverChoice::Browse, &cfg1)
        .unwrap()
        .run(&sample(), &lp_run_opts())
        .unwrap_err();
    assert_eq!(err1.code, "page_unreachable");

    let mut unsup = BrowseFake::ready();
    unsup.list_json = r#"{"available":false,"tools":[]}"#.to_string();
    let (br2, _) = unsup.install(&dir, "browse2");
    let mut cfg2 = config_with(config::DriverChoice::Browse, "agent-browser");
    cfg2.drivers.browse.command = br2.display().to_string();
    let err2 = driver::by_choice(config::DriverChoice::Browse, &cfg2)
        .unwrap()
        .run(&sample(), &lp_run_opts())
        .unwrap_err();
    assert_eq!(err2.code, "webmcp_unsupported");

    let mut missing = BrowseFake::ready();
    missing.list_json = r#"{"available":true,"tools":[]}"#.to_string();
    let (br3, _) = missing.install(&dir, "browse3");
    let mut cfg3 = config_with(config::DriverChoice::Browse, "agent-browser");
    cfg3.drivers.browse.command = br3.display().to_string();
    let mut opts3 = lp_run_opts();
    opts3.settle_ms = 50;
    let err3 = driver::by_choice(config::DriverChoice::Browse, &cfg3)
        .unwrap()
        .run(&sample(), &opts3)
        .unwrap_err();
    assert_eq!(err3.code, "tool_not_found");
}

#[cfg(unix)]
#[test]
fn browse_native_error_field_maps_to_tool_error() {
    // A native tool failure surfacing as a top-level string `error` field
    // must not masquerade as a successful result.
    let dir = tempfile::tempdir().unwrap();
    let mut fake = BrowseFake::ready();
    fake.call_json = r#"{"error":"boom"}"#.to_string();
    let (br, _) = fake.install(&dir, "browse");
    let mut cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    cfg.drivers.browse.command = br.display().to_string();
    let err = driver::by_choice(config::DriverChoice::Browse, &cfg)
        .unwrap()
        .run(&sample(), &lp_run_opts())
        .unwrap_err();
    assert_eq!(err.code, "tool_error");
    assert!(err.message.contains("boom"), "{err:?}");
}

#[cfg(unix)]
#[test]
fn lightpanda_run_is_error_maps_to_tool_error() {
    match lp_run(
        r#"{"ok":true,"output":{"isError":true,"content":[{"type":"text","text":"boom"}]}}"#,
        "ask_cal_com",
    ) {
        Err(e) => {
            assert_eq!(e.code, "tool_error");
            assert_eq!(e.message, "boom");
        }
        Ok(outcome) => panic!("expected tool_error, got {outcome:?}"),
    }
}
