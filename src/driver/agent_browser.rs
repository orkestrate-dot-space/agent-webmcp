//! agent-browser: the only driver agent-webmcp runs itself, over a session.

use super::{CallRequest, Detection, Driver, Kind, Plan, Step};
use crate::config::Config;

/// Earliest agent-browser with WebMCP on by default in managed Chrome.
const MIN_VERSION: &str = "0.43.0";
/// One-line install for a missing or outdated agent-browser.
const INSTALL: &str = "npm i -g agent-browser && agent-browser install";

/// Run driver: opens the page and invokes the tool in a named session.
pub struct AgentBrowser {
    command: String,
    session: String,
}

impl AgentBrowser {
    /// Build from config; the registry calls this so plans stay in sync.
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            command: cfg.drivers.agent_browser.command.clone(),
            session: cfg.drivers.agent_browser.session.clone(),
        }
    }
}

impl Driver for AgentBrowser {
    fn id(&self) -> &'static str {
        "agent-browser"
    }

    fn kind(&self) -> Kind {
        Kind::Run
    }

    fn detect(&self, _cfg: &Config) -> Detection {
        detect_with(&self.command, &run_version)
    }

    fn plan(&self, req: &CallRequest) -> Plan {
        let input = serde_json::to_string(&req.input).unwrap_or_default();
        Plan {
            driver: self.id(),
            note: self.note().map(str::to_string),
            steps: vec![
                Step {
                    tool: "shell".to_string(),
                    args: serde_json::json!({"command": [&self.command, "--session", &self.session, "--json", "open", &req.page]}),
                    why: "open the page in the session".to_string(),
                },
                Step {
                    tool: "shell".to_string(),
                    args: serde_json::json!({"command": [&self.command, "--session", &self.session, "--json", "webmcp", "invoke", &req.tool, "--params", &input]}),
                    why: "invoke the tool with the JSON input".to_string(),
                },
            ],
        }
    }
}

/// Detect agent-browser by running `<command> --version`; the runner is
/// injectable so tests can fake it.
pub fn detect_with(
    command: &str,
    run: &dyn Fn(&str) -> std::io::Result<std::process::Output>,
) -> Detection {
    match run(command) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Detection::Missing {
            install: INSTALL.to_string(),
            detail: None,
        },
        Err(e) => Detection::Missing {
            install: INSTALL.to_string(),
            detail: Some(format!("could not run {command} --version: {e}")),
        },
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            match parse_version(&text) {
                Some(version) if crate::setup::is_exact_version(&version) => {
                    if crate::setup::newer_available(&version, MIN_VERSION) {
                        Detection::TooOld {
                            version,
                            min: MIN_VERSION.to_string(),
                            install: INSTALL.to_string(),
                        }
                    } else {
                        Detection::Ready { version }
                    }
                }
                _ => Detection::Missing {
                    install: INSTALL.to_string(),
                    detail: Some(format!(
                        "could not parse a version from {command} --version"
                    )),
                },
            }
        }
    }
}

/// Run `<command> --version`, giving up after five seconds.
fn run_version(command: &str) -> std::io::Result<std::process::Output> {
    use std::time::Duration;
    let mut child = std::process::Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out running --version",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// First `x.y.z` (plus an optional `-pre`) in `text`, if any.
fn parse_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut j = i;
        for _ in 0..2 {
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'.' {
                j += 1;
            } else {
                j = i;
                break;
            }
        }
        if j == i {
            i += 1;
            continue;
        }
        let end = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == end {
            i += 1;
            continue;
        }
        if j < bytes.len() && bytes[j] == b'-' {
            let mut k = j + 1;
            while k < bytes.len()
                && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'.' || bytes[k] == b'-')
            {
                k += 1;
            }
            if k > j + 1 {
                j = k;
            }
        }
        return Some(text[i..j].to_string());
    }
    None
}
