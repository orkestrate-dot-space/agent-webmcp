//! Driver adapters: exact plan steps, agent-browser detection, choose().

mod common;

use agent_webmcp::config;
use agent_webmcp::driver::{self, CallRequest, Choice, Detection};

fn sample() -> CallRequest {
    CallRequest {
        page: "https://cal.com/".to_string(),
        tool: "ask_cal_com".to_string(),
        input: serde_json::json!({"question": "hi"}),
    }
}

fn config_with(driver: config::DriverChoice, command: &str) -> config::Config {
    let mut cfg = config::Config::default();
    cfg.driver = driver;
    cfg.drivers.agent_browser.command = command.to_string();
    cfg
}

#[test]
fn devtools_plan_uses_its_own_tool_names() {
    let cfg = config_with(config::DriverChoice::ChromeDevtoolsMcp, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::ChromeDevtoolsMcp, &cfg).unwrap();
    assert_eq!(driver.id(), "chrome-devtools-mcp");
    let plan = driver.plan(&sample());
    assert_eq!(plan.driver, "chrome-devtools-mcp");
    assert!(
        plan.note
            .as_deref()
            .unwrap()
            .contains("--categoryExperimentalWebmcp"),
        "{:?}",
        plan.note
    );
    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.steps[0].tool, "navigate_page");
    assert_eq!(
        plan.steps[0].args,
        serde_json::json!({"url": "https://cal.com/"})
    );
    assert_eq!(plan.steps[1].tool, "list_webmcp_tools");
    assert_eq!(plan.steps[1].args, serde_json::json!({}));
    assert!(
        plan.steps[1].why.contains("pageId"),
        "{}",
        plan.steps[1].why
    );
    assert_eq!(plan.steps[2].tool, "execute_webmcp_tool");
    assert_eq!(
        plan.steps[2].args,
        serde_json::json!({"toolName": "ask_cal_com", "input": "{\"question\":\"hi\"}"})
    );
}

#[test]
fn playwright_plan_prefixes_the_tool_name() {
    let cfg = config_with(config::DriverChoice::PlaywrightMcp, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::PlaywrightMcp, &cfg).unwrap();
    assert_eq!(driver.id(), "playwright-mcp");
    let plan = driver.plan(&sample());
    assert!(
        plan.note
            .as_deref()
            .unwrap()
            .contains("--enable-features=WebMCP"),
        "{:?}",
        plan.note
    );
    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.steps[0].tool, "browser_navigate");
    assert_eq!(
        plan.steps[0].args,
        serde_json::json!({"url": "https://cal.com/"})
    );
    assert_eq!(plan.steps[1].tool, "browser_snapshot");
    assert!(
        plan.steps[1].why.contains("snapshot"),
        "{}",
        plan.steps[1].why
    );
    assert_eq!(plan.steps[2].tool, "webmcp_ask_cal_com");
    assert_eq!(plan.steps[2].args, serde_json::json!({"question": "hi"}));
}

#[test]
fn browse_plan_passes_the_tab() {
    let cfg = config_with(config::DriverChoice::Browse, "agent-browser");
    let driver = driver::by_choice(config::DriverChoice::Browse, &cfg).unwrap();
    assert_eq!(driver.id(), "browse");
    let plan = driver.plan(&sample());
    assert_eq!(plan.note, None);
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
fn choose_auto_missing_plans_generic() {
    let cfg = config_with(
        config::DriverChoice::Auto,
        "agent-webmcp-definitely-missing-binary",
    );
    match driver::choose(&cfg) {
        Choice::Plan(driver) => assert_eq!(driver.id(), "generic"),
        _ => panic!("auto with nothing installed should plan generic"),
    }
}

#[test]
fn choose_named_plan_driver_plans() {
    let cfg = config_with(config::DriverChoice::PlaywrightMcp, "agent-browser");
    match driver::choose(&cfg) {
        Choice::Plan(driver) => assert_eq!(driver.id(), "playwright-mcp"),
        _ => panic!("playwright-mcp should plan"),
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
