//! Shared fixtures for the integration tests: sample index, configs,
//! one fake agent-browser, argv logs, and data-dir isolation.
//!
//! Every test that runs the call pipeline holds a [`HomeGuard`], so no
//! lock file ever lands in the real data dir.

// One module, many test binaries: each binary only uses part of this, so
// per-binary dead-code warnings would be noise.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_webmcp::call::CallOptions;
use agent_webmcp::config;
use agent_webmcp::index::Search;
use agent_webmcp::model::{Annotations, Index, Site, Status, Tool};

// ---------------------------------------------------------------------------
// Envelope shapes: every constant mirrors the real agent-browser daemon.
// Sources are the vendored agent-browser design in
// /tmp/opencode/driver-design/ab-*.rs.
// ---------------------------------------------------------------------------

/// agent-browser --version output (parsed by detect_with).
pub const VERSION_43: &str = "agent-browser 0.43.1";

/// Open success with no page tools: the daemon wraps the page result and
/// inject_lifecycle adds `lifecycle` (ab-actions.rs).
pub const OPEN_PLAIN: &str =
    r#"{"success":true,"data":{"url":"https://cal.com/","lifecycle":{"reused":true}}}"#;

/// Open success announcing page tools under `data.webmcp.tools`
/// (open handler collects them like webmcp_list; ab-actions.rs).
pub const OPEN_ANNOUNCED: &str = r#"{"success":true,"data":{"url":"https://cal.com/","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"ask_cal_com","description":"Ask about Cal.","origin":"https://cal.com","frameId":"f"}]}}}"#;

/// Open success announcing only other tools.
pub const OPEN_OTHER: &str = r#"{"success":true,"data":{"url":"https://cal.com/","lifecycle":{"reused":true},"webmcp":{"tools":[{"name":"other_tool","description":"Other.","origin":"https://cal.com","frameId":"f"}]}}}"#;

/// Open success on a bot-check page (is_blocked_page mirrors the scanner).
pub const OPEN_BLOCKED: &str = r#"{"success":true,"data":{"url":"https://cal.com/","title":"Just a moment...","webmcp":{"tools":[]}}}"#;

/// Open failure: navigation errors map to page_unreachable
/// (error_response carries the code; ab-actions.rs).
pub const OPEN_UNREACHABLE: &str =
    r#"{"success":false,"code":"navigation_timeout","error":"timed out"}"#;

/// Completed invoke record (InvocationRecord::to_json; ab-webmcp.rs).
pub const INVOKE_OK: &str = r#"{"success":true,"data":{"invocationId":"inv-1","toolName":"ask_cal_com","frameId":"f","origin":"https://cal.com","status":"completed","durationMs":5,"output":{"answer":1}}}"#;

/// Completed record whose output carries isError (to_json `output`).
pub const INVOKE_TOOL_ERROR: &str = r#"{"success":true,"data":{"invocationId":"inv-1","toolName":"ask_cal_com","frameId":"f","origin":"https://cal.com","status":"completed","durationMs":5,"output":{"isError":true,"content":[{"type":"text","text":"boom"}]}}}"#;

/// Invoke error: resolve_tool finds no tool (ERR_TOOL_NOT_FOUND code via
/// error_response's webmcp_ prefix; ab-webmcp.rs + ab-actions.rs).
pub const INVOKE_NOT_FOUND: &str = r#"{"success":false,"code":"webmcp_tool_not_found","error":"webmcp_tool_not_found: No WebMCP tool named 'ask_cal_com'"}"#;

/// Invoke error: resolve_tool finds the tool in several frames
/// (ERR_AMBIGUOUS_TOOL; ab-webmcp.rs).
pub const INVOKE_AMBIGUOUS: &str = r#"{"success":false,"code":"webmcp_ambiguous_tool","error":"webmcp_ambiguous_tool: WebMCP tool 'ask_cal_com' exists in multiple frames. Retry with --frame <frame-id>. Matching frames: a, b","data":{"frames":[{"frameId":"a"},{"frameId":"b"}]}}"#;

/// Failed record (to_json with `error`; ab-webmcp.rs).
pub const RECORD_FAILED: &str = r#"{"success":true,"data":{"invocationId":"inv-1","toolName":"ask_cal_com","frameId":"f","origin":"https://cal.com","status":"failed","durationMs":5,"error":"the tool blew up"}}"#;

/// Failed record for a mid-call navigation (mark_context_changed).
pub const RECORD_CONTEXT_CHANGED: &str = r#"{"success":true,"data":{"invocationId":"inv-1","toolName":"ask_cal_com","frameId":"f","origin":"https://cal.com","status":"failed","durationMs":5,"error":"webmcp_context_changed: The page context changed before the invocation completed"}}"#;

/// Timed-out record (mark_timed_out; ab-webmcp.rs).
pub const RECORD_TIMED_OUT: &str = r#"{"success":true,"data":{"invocationId":"inv-1","toolName":"ask_cal_com","frameId":"f","origin":"https://cal.com","status":"timed_out","durationMs":5,"error":"WebMCP invocation timed out"}}"#;

/// Canceled record (raw "Canceled" completion maps to `canceled`).
pub const RECORD_CANCELED: &str = r#"{"success":true,"data":{"invocationId":"inv-1","toolName":"ask_cal_com","frameId":"f","origin":"https://cal.com","status":"canceled","durationMs":5}}"#;

/// Empty tool list (handle_webmcp_list answers
/// `{experimental, untrusted, tools}`; ab-actions.rs).
pub const LIST_EMPTY: &str =
    r#"{"success":true,"data":{"experimental":true,"untrusted":true,"tools":[]}}"#;

/// Tool list containing the tool (ToolRecord is camelCase; ab-webmcp.rs).
pub const LIST_WITH_TOOL: &str = r#"{"success":true,"data":{"experimental":true,"untrusted":true,"tools":[{"name":"ask_cal_com","description":"Ask about Cal.","origin":"https://cal.com","frameId":"f"}]}}"#;

/// Probe answer when the page exposes the API
/// (handle_evaluate returns `{result, origin}`; ab-actions.rs).
pub const EVAL_OBJECT: &str =
    r#"{"success":true,"data":{"result":"object","origin":"https://cal.com/"}}"#;

/// Probe answer when the page has no WebMCP API.
pub const EVAL_UNDEFINED: &str =
    r#"{"success":true,"data":{"result":"undefined","origin":"https://cal.com/"}}"#;

/// Failed eval: the probe treats any failure as inconclusive.
pub const EVAL_ERROR: &str =
    r#"{"success":false,"error":"ReferenceError: document is not defined"}"#;

/// Generic daemon ok for close/reload/wait.
pub const GENERIC_OK: &str = r#"{"success":true,"data":{}}"#;

// ---------------------------------------------------------------------------
// Sample fixtures.
// ---------------------------------------------------------------------------

pub fn consequential() -> Annotations {
    Annotations {
        consequential: true,
        ..Default::default()
    }
}

pub fn ask_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["question"],
        "properties": {"question": {"type": "string"}}
    })
}

pub fn ask_tool() -> Tool {
    Tool {
        name: "ask_cal_com".into(),
        description: "Ask about Cal.".into(),
        input_schema: ask_schema(),
        page: "https://cal.com/".into(),
        origin: "https://cal.com".into(),
        annotations: consequential(),
        flags: vec![],
    }
}

pub fn read_tool() -> Tool {
    Tool {
        name: "list_slots".into(),
        description: "List open slots.".into(),
        input_schema: serde_json::json!({"type": "object"}),
        page: "https://cal.com/slots".into(),
        origin: "https://cal.com".into(),
        annotations: Annotations {
            read_only: true,
            ..Default::default()
        },
        flags: vec![],
    }
}

pub fn sample_site(host: &str, tools: Vec<Tool>) -> Site {
    Site {
        host: host.into(),
        status: Status::Verified,
        checked_at: "2026-11-19T00:00:00Z".into(),
        pages_checked: vec![format!("https://{host}/")],
        platform: None,
        tools,
        note: None,
        missed: 0,
    }
}

pub fn sample_index() -> Index {
    Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![sample_site("cal.com", vec![ask_tool(), read_tool()])],
    }
}

pub fn sample_search() -> Search {
    Search::build(sample_index()).unwrap()
}

pub fn allow_all() -> config::Config {
    let mut cfg = config::Config::default();
    cfg.permissions.allow.push("*".into());
    cfg
}

/// Plan-only options: confirm, no run.
pub fn plan_opts() -> CallOptions {
    CallOptions {
        confirm: true,
        plan_only: true,
        fresh: false,
        retry_hint: agent_webmcp::call::RETRY_CLI,
    }
}

/// Run options: confirm, execute the pipeline.
pub fn run_opts() -> CallOptions {
    CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: agent_webmcp::call::RETRY_CLI,
    }
}

// ---------------------------------------------------------------------------
// Fake agent-browser: one builder for every test.
// ---------------------------------------------------------------------------

/// Subcommands the fake can answer, in case-branch order.
const SUBCOMMANDS: [&str; 7] = ["invoke", "open", "list", "reload", "wait", "eval", "close"];

/// Fake agent-browser: takes a map of subcommand to response line(s),
/// served in order with the last one repeating. Argv is logged, `--version`
/// prints the version, and responses live in files in the test temp dir so
/// no JSON is ever escaped inside shell or format strings.
pub struct Fake {
    responses: HashMap<&'static str, Vec<String>>,
    stalls: HashMap<&'static str, u64>,
    version: String,
}

impl Fake {
    pub fn new() -> Self {
        let mut responses: HashMap<&'static str, Vec<String>> = HashMap::new();
        responses.insert("open", vec![OPEN_ANNOUNCED.to_string()]);
        responses.insert("invoke", vec![INVOKE_OK.to_string()]);
        responses.insert("list", vec![LIST_WITH_TOOL.to_string()]);
        responses.insert("eval", vec![EVAL_OBJECT.to_string()]);
        responses.insert("reload", vec![GENERIC_OK.to_string()]);
        responses.insert("wait", vec![GENERIC_OK.to_string()]);
        responses.insert("close", vec![GENERIC_OK.to_string()]);
        Self {
            responses,
            stalls: HashMap::new(),
            version: VERSION_43.to_string(),
        }
    }

    fn set_lines(&mut self, sub: &'static str, lines: &[&str]) {
        self.responses
            .insert(sub, lines.iter().map(|l| l.to_string()).collect());
    }

    pub fn open(mut self, line: &str) -> Self {
        self.set_lines("open", &[line]);
        self
    }

    pub fn invoke(mut self, lines: &[&str]) -> Self {
        self.set_lines("invoke", lines);
        self
    }

    pub fn lists(mut self, lines: &[&str]) -> Self {
        self.set_lines("list", lines);
        self
    }

    pub fn evals(mut self, lines: &[&str]) -> Self {
        self.set_lines("eval", lines);
        self
    }

    /// Sleep before answering one subcommand (timeout tests).
    /// `--version` and `close` always answer at once.
    pub fn stall(mut self, sub: &'static str, secs: u64) -> Self {
        self.stalls.insert(sub, secs);
        self
    }

    pub fn version(mut self, version: &str) -> Self {
        self.version = version.to_string();
        self
    }

    /// Write response files and the script into `dir`; return the script.
    /// Ends with an exec-visibility barrier (see below), so the argv log
    /// starts clean.
    pub fn install(&self, dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let base = dir.path().join(name);
        let base_str = base.display().to_string();
        let mut written = vec![base_str.clone()];
        for sub in SUBCOMMANDS {
            if let Some(lines) = self.responses.get(sub) {
                for (i, line) in lines.iter().enumerate() {
                    let path = format!("{base_str}.{sub}.{}", i + 1);
                    std::fs::write(&path, format!("{line}\n")).unwrap();
                    written.push(path);
                }
            }
            if let Some(secs) = self.stalls.get(sub) {
                let path = format!("{base_str}.{sub}.stall");
                std::fs::write(&path, secs.to_string()).unwrap();
                written.push(path);
            }
        }
        let log = dir.path().join(format!("{name}.argv"));
        let mut script = String::from("#!/bin/sh\n");
        script.push_str("LOG=\"");
        script.push_str(&log.display().to_string());
        script.push_str("\"\nserve() {\nn=$(cat \"$1.count\" 2>/dev/null || echo 0)\n");
        script.push_str("echo $((n + 1)) > \"$1.count\"\ni=$((n + 1))\n");
        script.push_str("if [ \"$i\" -gt \"$2\" ]; then i=\"$2\"; fi\ncat \"$1.$i\"\n}\n");
        script.push_str(
            "maybe_stall() {\nif [ -f \"$1.stall\" ]; then sleep \"$(cat \"$1.stall\")\"; fi\n}\n",
        );
        script.push_str("echo \"$@\" >> \"$LOG\"\ncase \" $* \" in\n");
        script.push_str(" *\" --version \"*) echo \"");
        script.push_str(&self.version);
        script.push_str("\";;\n");
        for sub in SUBCOMMANDS {
            let total = self.responses.get(sub).map(Vec::len).unwrap_or(0);
            if total == 0 {
                continue;
            }
            script.push_str(" *\" ");
            script.push_str(sub);
            script.push_str(" \"*) maybe_stall \"");
            script.push_str(&base_str);
            script.push('.');
            script.push_str(sub);
            script.push_str("\"; serve \"");
            script.push_str(&base_str);
            script.push('.');
            script.push_str(sub);
            script.push_str("\" ");
            script.push_str(&total.to_string());
            script.push_str(";;\n");
        }
        script.push_str(" *) ;;\nesac\n");
        std::fs::write(&base, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Exec-visibility barrier: this box can answer ETXTBSY ("Text file
        // busy) when a just-written script is executed under concurrency, so
        // read everything back and run the script once (no args: it matches
        // nothing and exits 0), retrying a busy exec. Afterwards the child
        // is guaranteed to see complete files.
        for path in &written {
            let _ = std::fs::read(path);
        }
        #[cfg(unix)]
        for _ in 0..100 {
            match std::process::Command::new(&base).output() {
                Ok(_) => break,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        // The warmup run appended a blank line; start the argv log clean.
        let _ = std::fs::write(&log, "");
        base
    }

    /// Argv lines the fake logged.
    pub fn argv(dir: &tempfile::TempDir, name: &str) -> Vec<String> {
        std::fs::read_to_string(dir.path().join(format!("{name}.argv")))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Run config + data-dir isolation.
// ---------------------------------------------------------------------------

static SESSION_N: AtomicU64 = AtomicU64::new(0);

/// Pipeline config: unique session per test, short timeout. The settle
/// value exercises the default settle path (list/probe/poll) instead of
/// the legacy `settle_ms = 0` fast path; Config::default() settles 8000 ms
/// but 300 ms keeps tests fast.
pub fn run_config(script: &Path) -> config::Config {
    run_config_settle(script, 300)
}

pub fn run_config_settle(script: &Path, settle_ms: u64) -> config::Config {
    let mut cfg = allow_all();
    cfg.drivers.agent_browser.command = script.display().to_string();
    // Unique per test: parallel runs hold their own session lock.
    let n = SESSION_N.fetch_add(1, Ordering::Relaxed);
    cfg.drivers.agent_browser.session = format!("test-s-{n}");
    cfg.call.timeout = 1234;
    cfg.call.settle_ms = settle_ms;
    cfg
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Points AGENT_WEBMCP_HOME at a fresh temp dir for the test's lifetime.
/// Process env is global, so guards serialize on a static mutex and the
/// previous value is restored on drop.
pub struct HomeGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
    temp: tempfile::TempDir,
    saved: Option<std::ffi::OsString>,
}

impl HomeGuard {
    pub fn isolate() -> Self {
        // A panicking test poisons the mutex; carry on so one failure
        // cannot cascade into every other pipeline test.
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let saved = std::env::var_os("AGENT_WEBMCP_HOME");
        unsafe { std::env::set_var("AGENT_WEBMCP_HOME", temp.path()) };
        Self {
            _guard: guard,
            temp,
            saved,
        }
    }

    pub fn path(&self) -> &Path {
        self.temp.path()
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match &self.saved {
            Some(v) => unsafe { std::env::set_var("AGENT_WEBMCP_HOME", v) },
            None => unsafe { std::env::remove_var("AGENT_WEBMCP_HOME") },
        }
    }
}
