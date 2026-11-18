//! Driver adapters: detect what can run here, or plan steps for the agent.
//!
//! Only agent-browser runs from a separate process; the MCP drivers live
//! inside the agent's session, so agent-webmcp hands back exact steps.

pub mod agent_browser;
pub mod browse;
pub mod devtools_mcp;
pub mod generic;
pub mod playwright_mcp;

use serde::Serialize;
use serde_json::Value;

use crate::call::CallError;
use crate::config::{Config, DriverChoice};

/// Whether agent-webmcp drives the browser itself or hands back steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Run,
    Plan,
}

/// What to call: resolved before any driver is involved.
#[derive(Debug, Clone, PartialEq)]
pub struct CallRequest {
    /// URL to open.
    pub page: String,
    /// WebMCP tool name.
    pub tool: String,
    /// Object, already validated (phase 3).
    pub input: serde_json::Value,
}

/// What detection found: ready to run, what to install, or unknowable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Ready {
        version: String,
    },
    Missing {
        install: String,
        detail: Option<String>,
    },
    TooOld {
        version: String,
        min: String,
        install: String,
    },
    Unknown,
}

/// Exact steps for the agent's own driver, in that driver's tool names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    pub driver: &'static str,
    /// Setup hint, e.g. a required flag.
    pub note: Option<String>,
    pub steps: Vec<Step>,
}

/// One step: the driver's own MCP tool name, or "open"/"call"/"shell".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub tool: String,
    pub args: serde_json::Value,
    /// One short line telling the agent why this step matters.
    pub why: String,
}

pub trait Driver {
    fn id(&self) -> &'static str;
    fn kind(&self) -> Kind;
    fn detect(&self, cfg: &Config) -> Detection;
    fn plan(&self, req: &CallRequest) -> Plan;
    /// Setup hint shown by `status`; plans carry it in `Plan.note`.
    fn note(&self) -> Option<&'static str> {
        None
    }
    /// Run the tool; plan-only drivers keep this default.
    fn run(&self, req: &CallRequest, opts: &RunOptions) -> Result<RunOutcome, CallError> {
        let _ = (req, opts);
        Err(CallError {
            code: "driver_error",
            message: format!("{} only plans; it cannot run tools", self.id()),
            data: None,
        })
    }
}

/// How to run: budgets and which browser surface.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub timeout_ms: u64,
    /// How long to wait for a late tool registration, in milliseconds.
    pub settle_ms: u64,
    /// Extra `--args` values, passed on `open` before the WebMCP one.
    pub extra_args: Vec<String>,
    pub headed: bool,
    pub fresh: bool,
    pub profile: Option<String>,
    pub cdp: Option<String>,
}

/// What a run returned.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub output: Value,
    pub duration_ms: u64,
    pub reused: bool,
    /// timeout + settle + 30 s grace, the budget the run lived under.
    pub budget_ms: u64,
}

/// Every driver, agent-browser first; sessions come from config.
pub fn all(cfg: &Config) -> Vec<Box<dyn Driver>> {
    let drivers: Vec<Box<dyn Driver>> = vec![
        Box::new(agent_browser::AgentBrowser::from_config(cfg)),
        Box::new(playwright_mcp::PlaywrightMcp),
        Box::new(devtools_mcp::DevtoolsMcp),
        Box::new(browse::Browse),
        Box::new(generic::Generic),
    ];
    drivers
}

/// The named driver, if the choice names one; `auto` names none.
pub fn by_choice(choice: DriverChoice, cfg: &Config) -> Option<Box<dyn Driver>> {
    let driver: Box<dyn Driver> = match choice {
        DriverChoice::Auto => return None,
        DriverChoice::AgentBrowser => Box::new(agent_browser::AgentBrowser::from_config(cfg)),
        DriverChoice::PlaywrightMcp => Box::new(playwright_mcp::PlaywrightMcp),
        DriverChoice::ChromeDevtoolsMcp => Box::new(devtools_mcp::DevtoolsMcp),
        DriverChoice::Browse => Box::new(browse::Browse),
        DriverChoice::None => Box::new(generic::Generic),
    };
    Some(driver)
}

/// What `call` will do: run here, hand back a plan, or report unusable.
/// A named driver never silently falls back to another.
pub enum Choice {
    Run(Box<dyn Driver>, String),
    Plan(Box<dyn Driver>),
    Unusable {
        driver: &'static str,
        detection: Detection,
    },
}

/// Pick the driver for this config, detecting the run driver when needed.
pub fn choose(cfg: &Config) -> Choice {
    match cfg.driver {
        DriverChoice::AgentBrowser => {
            let driver = agent_browser::AgentBrowser::from_config(cfg);
            match driver.detect(cfg) {
                Detection::Ready { version } => Choice::Run(Box::new(driver), version),
                detection => Choice::Unusable {
                    driver: "agent-browser",
                    detection,
                },
            }
        }
        DriverChoice::PlaywrightMcp => Choice::Plan(Box::new(playwright_mcp::PlaywrightMcp)),
        DriverChoice::ChromeDevtoolsMcp => Choice::Plan(Box::new(devtools_mcp::DevtoolsMcp)),
        DriverChoice::Browse => Choice::Plan(Box::new(browse::Browse)),
        DriverChoice::None => Choice::Plan(Box::new(generic::Generic)),
        DriverChoice::Auto => {
            let driver = agent_browser::AgentBrowser::from_config(cfg);
            match driver.detect(cfg) {
                Detection::Ready { version } => Choice::Run(Box::new(driver), version),
                _ => Choice::Plan(Box::new(generic::Generic)),
            }
        }
    }
}

/// One `status` line per driver: detection for run drivers, note for plans.
pub fn status_line(driver: &dyn Driver, cfg: &Config) -> String {
    match driver.kind() {
        Kind::Run => match driver.detect(cfg) {
            Detection::Ready { version } => {
                format!("driver {}: ready {version} (run)", driver.id())
            }
            Detection::Missing { install, .. } => {
                format!("driver {}: missing; install: {install}", driver.id())
            }
            Detection::TooOld {
                version,
                min,
                install,
            } => format!(
                "driver {}: too old {version} (need {min}); install: {install}",
                driver.id()
            ),
            Detection::Unknown => format!("driver {}: unknown", driver.id()),
        },
        Kind::Plan => match driver.note() {
            Some(note) => format!("driver {}: plan only ({note})", driver.id()),
            None => format!("driver {}: plan only", driver.id()),
        },
    }
}
