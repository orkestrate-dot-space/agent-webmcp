//! Driver adapters: detect what can run here, or plan steps for the agent.
//!
//! Three run drivers (`agent-browser`, `browse`, `lightpanda`) execute tools
//! through their CLIs; `none` (`generic`) emits a driver-agnostic plan.

pub mod agent_browser;
pub mod browse;
pub mod exec;
pub mod generic;
pub mod lightpanda;
pub mod lock;

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
    /// Install line for a run driver, shown in the no-driver setup help.
    fn install(&self) -> Option<&'static str> {
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
    /// Inspect a live page and return its registered WebMCP tools without calling them.
    fn inspect(&self, url: &str, opts: &RunOptions) -> Result<InspectOutcome, CallError> {
        let _ = (url, opts);
        Err(CallError {
            code: "driver_error",
            message: format!("{} cannot inspect live pages", self.id()),
            data: None,
        })
    }
}

/// One WebMCP tool discovered on a live page by `inspect`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectedTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

/// What `inspect` found on a live page.
#[derive(Debug, Clone)]
pub struct InspectOutcome {
    pub page: String,
    pub tools: Vec<InspectedTool>,
    pub duration_ms: u64,
    pub reused: bool,
    pub budget_ms: u64,
}

/// Parse a JSON array of WebMCP tool objects at `pointer` into `InspectedTool`s.
pub(crate) fn parse_inspected_tools(value: &Value, pointer: &str) -> Vec<InspectedTool> {
    let Some(arr) = value.pointer(pointer).and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let description = item
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let input_schema = item
                .get("inputSchema")
                .or_else(|| item.get("input_schema"))
                .filter(|v| v.is_object())
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let read_only = item
                .pointer("/annotations/readOnlyHint")
                .and_then(Value::as_bool)
                .or_else(|| item.get("readOnly").and_then(Value::as_bool));
            Some(InspectedTool {
                name,
                description,
                input_schema,
                read_only,
            })
        })
        .collect()
}

/// How to run: universal call budgets and session freshness.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub timeout_ms: u64,
    /// How long to wait for a late tool registration, in milliseconds.
    pub settle_ms: u64,
    pub fresh: bool,
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
    vec![
        Box::new(agent_browser::AgentBrowser::from_config(cfg)),
        Box::new(browse::Browse::from_config(cfg)),
        Box::new(lightpanda::Lightpanda::from_config(cfg)),
        Box::new(generic::Generic),
    ]
}

/// The named driver, if the choice names one; `auto` names none.
pub fn by_choice(choice: DriverChoice, cfg: &Config) -> Option<Box<dyn Driver>> {
    let driver: Box<dyn Driver> = match choice {
        DriverChoice::Auto => return None,
        DriverChoice::AgentBrowser => Box::new(agent_browser::AgentBrowser::from_config(cfg)),
        DriverChoice::Browse => Box::new(browse::Browse::from_config(cfg)),
        DriverChoice::Lightpanda => Box::new(lightpanda::Lightpanda::from_config(cfg)),
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
        DriverChoice::Browse => {
            let driver = browse::Browse::from_config(cfg);
            match driver.detect(cfg) {
                Detection::Ready { version } => Choice::Run(Box::new(driver), version),
                detection => Choice::Unusable {
                    driver: "browse",
                    detection,
                },
            }
        }
        DriverChoice::Lightpanda => {
            let driver = lightpanda::Lightpanda::from_config(cfg);
            match driver.detect(cfg) {
                Detection::Ready { version } => Choice::Run(Box::new(driver), version),
                detection => Choice::Unusable {
                    driver: "lightpanda",
                    detection,
                },
            }
        }
        DriverChoice::None => Choice::Plan(Box::new(generic::Generic)),
        DriverChoice::Auto => {
            let agent = agent_browser::AgentBrowser::from_config(cfg);
            match agent.detect(cfg) {
                Detection::Ready { version } => Choice::Run(Box::new(agent), version),
                agent_detection => {
                    let browse = browse::Browse::from_config(cfg);
                    if let Detection::Ready { version } = browse.detect(cfg) {
                        return Choice::Run(Box::new(browse), version);
                    }
                    let lightpanda = lightpanda::Lightpanda::from_config(cfg);
                    match lightpanda.detect(cfg) {
                        Detection::Ready { version } => Choice::Run(Box::new(lightpanda), version),
                        _ => Choice::Unusable {
                            driver: "agent-browser",
                            detection: agent_detection,
                        },
                    }
                }
            }
        }
    }
}

/// One `status` line per driver: detection for run drivers, note for plans.
pub fn status_line(driver: &dyn Driver, cfg: &Config) -> String {
    match driver.kind() {
        Kind::Run => match driver.detect(cfg) {
            Detection::Ready { version } => {
                if driver.id() == "lightpanda" {
                    format!(
                        "driver {}: ready {version} (run, experimental)",
                        driver.id()
                    )
                } else {
                    format!("driver {}: ready {version} (run)", driver.id())
                }
            }
            Detection::Missing {
                install, detail, ..
            } => match detail {
                Some(detail) => format!(
                    "driver {}: missing ({detail}); install: {install}",
                    driver.id()
                ),
                None => format!("driver {}: missing; install: {install}", driver.id()),
            },
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

/// The `no_driver` data: every run driver and how to get it.
/// Skips the generic fallback, which is not a driver.
pub fn setup_help(cfg: &Config, config_path: &str) -> serde_json::Value {
    let mut drivers = Vec::new();
    for driver in all(cfg) {
        if driver.id() == "generic" {
            continue;
        }
        let kind = match driver.kind() {
            Kind::Run => "run",
            Kind::Plan => "plan",
        };
        let mut entry = serde_json::json!({"id": driver.id(), "kind": kind});
        match driver.kind() {
            Kind::Run => {
                let (status, install) = match driver.detect(cfg) {
                    Detection::Ready { version } => ("ready", Some(version)),
                    Detection::TooOld {
                        version, install, ..
                    } => {
                        entry["version"] = serde_json::json!(version);
                        ("too_old", Some(install))
                    }
                    Detection::Missing { install, .. } => ("missing", Some(install)),
                    Detection::Unknown => ("unknown", driver.install().map(str::to_string)),
                };
                entry["status"] = serde_json::json!(status);
                if let Some(install) = install.filter(|_| status != "ready") {
                    entry["install"] = serde_json::json!(install);
                }
                if let Some(note) = driver.note() {
                    entry["note"] = serde_json::json!(note);
                }
            }
            Kind::Plan => {
                if let Some(note) = driver.note() {
                    entry["setup"] = serde_json::json!(note);
                }
            }
        }
        drivers.push(entry);
    }
    serde_json::json!({
        "drivers": drivers,
        "config": config_path,
        "configure": "set \"driver\" in agent-webmcp.json, e.g. {\"driver\": \"browse\"}",
    })
}
