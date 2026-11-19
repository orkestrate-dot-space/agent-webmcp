//! `call`: resolve the target, check permissions, run or plan.
//!
//! The CLI and the MCP tool share this pipeline so they cannot diverge:
//! resolve → parse and validate input → permissions → choose → run or plan.

use serde_json::Value;

use crate::config::{Config, Permissions};
use crate::driver::{self, CallRequest, Choice, Detection, Driver, Plan};
use crate::index::Search;
use crate::model::Annotations;

/// Tool kind from annotations; consequential wins over read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Consequential,
    Unspecified,
}

/// What `call` will invoke, with the schema and kind when indexed.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Page to open.
    pub page: String,
    /// WebMCP tool name.
    pub tool: String,
    /// Host the tool belongs to.
    pub site: String,
    /// Input schema from the index, if the tool is indexed.
    pub schema: Option<Value>,
    pub kind: ToolKind,
    /// True when the kind was inferred from the name (no index hint).
    pub kind_inferred: bool,
}

impl Resolved {
    /// `<site>/<tool>`, the id permissions match against.
    pub fn id(&self) -> String {
        format!("{}/{}", self.site, self.tool)
    }
}

/// What to do about a call: allow, ask first, or never.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

/// Error codes, exactly the CLI/MCP contract.
#[derive(Debug, Clone, PartialEq)]
pub struct CallError {
    pub code: &'static str,
    pub message: String,
    pub data: Option<Value>,
}

impl CallError {
    fn new(code: &'static str, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Bad command use; the CLI exits 2.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new("usage", message.into())
    }

    /// The CLI/MCP failure envelope.
    pub fn envelope(&self) -> Value {
        serde_json::json!({
            "success": false,
            "code": self.code,
            "error": self.message,
            "data": self.data.clone().unwrap_or(Value::Null),
        })
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error ({}): {}", self.code, self.message)
    }
}

impl std::error::Error for CallError {}

/// Exit code: 0 success (run or plan), 1 failure, 2 usage.
pub fn exit_code(err: &CallError) -> i32 {
    if err.code == "usage" { 2 } else { 1 }
}

/// Retry hint when confirmation stops the call.
pub const RETRY_CLI: &str = "add --confirm";
/// Retry hint for the MCP tool, where flags do not exist.
pub const RETRY_MCP: &str = "call again with confirm: true";

/// Options that differ between the CLI and the MCP tool.
#[derive(Debug, Clone)]
pub struct CallOptions {
    pub confirm: bool,
    pub plan_only: bool,
    pub fresh: bool,
    pub retry_hint: &'static str,
}

/// A finished call: either a run outcome or steps for the agent.
#[derive(Debug, Clone)]
pub enum Outcome {
    Ran {
        id: String,
        tool: String,
        page: String,
        driver: &'static str,
        output: Value,
        duration_ms: u64,
        reused: bool,
        budget_ms: u64,
    },
    Planned {
        id: String,
        tool: String,
        page: String,
        plan: Plan,
    },
}

impl Outcome {
    /// The `--json` success envelope, shared with the MCP tool.
    pub fn envelope(&self) -> Value {
        match self {
            Outcome::Ran {
                id,
                tool,
                page,
                driver,
                output,
                duration_ms,
                reused,
                budget_ms,
            } => {
                let data = serde_json::json!({
                    "id": id,
                    "tool": tool,
                    "page": page,
                    "driver": driver,
                    "output": output,
                    "durationMs": duration_ms,
                    "reused": reused,
                    "budgetMs": budget_ms,
                });
                serde_json::json!({ "success": true, "data": data })
            }
            Outcome::Planned {
                id,
                tool,
                page,
                plan,
            } => serde_json::json!({
                "success": true,
                "data": { "id": id, "tool": tool, "page": page, "plan": plan },
            }),
        }
    }
}

/// Print a success for humans: value to stdout, status line to stderr.
pub fn print_human(outcome: &Outcome) {
    match outcome {
        Outcome::Ran {
            output,
            driver,
            id,
            duration_ms,
            ..
        } => {
            match output {
                Value::String(s) => println!("{}", crate::text::clean(s)),
                _ => println!(
                    "{}",
                    serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string())
                ),
            }
            eprintln!("{driver} · {id} · {}", format_duration(*duration_ms));
        }
        Outcome::Planned { plan, .. } => {
            println!("run these steps with {}:", plan.driver);
            for (i, step) in plan.steps.iter().enumerate() {
                let args = serde_json::to_string(&step.args).unwrap_or_default();
                println!("{}. {} {}  # {}", i + 1, step.tool, args, step.why);
            }
            if let Some(note) = &plan.note {
                println!("note: {note}");
            }
        }
    }
}

/// Print a failure for humans: one line plus the retry/install hints. A
/// missing driver lists every driver with how to get it.
pub fn print_failure(err: &CallError) {
    eprintln!("{err}");
    let data = err.data.as_ref();
    if err.code == "needs_confirmation"
        && let Some(retry) = data.and_then(|d| d.get("retry").and_then(Value::as_str))
    {
        eprintln!("retry: {retry}");
    } else if err.code == "no_driver"
        && let Some(drivers) = data
            .and_then(|d| d.get("drivers"))
            .and_then(Value::as_array)
    {
        for driver in drivers {
            let id = driver.get("id").and_then(Value::as_str).unwrap_or("?");
            let kind = driver.get("kind").and_then(Value::as_str).unwrap_or("?");
            let how = driver
                .get("install")
                .or_else(|| driver.get("setup"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if how.is_empty() {
                eprintln!("  {id} ({kind})");
            } else {
                eprintln!("  {id} ({kind}): {how}");
            }
        }
    }
}

fn format_duration(ms: u64) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

/// The tool kind from annotations, like `find` reports it. When neither
/// hint is set, a safe-looking name counts as read, and says so.
pub fn kind_of(name: &str, annotations: &Annotations) -> (ToolKind, bool) {
    if annotations.consequential {
        (ToolKind::Consequential, false)
    } else if annotations.read_only {
        (ToolKind::Read, false)
    } else if inferred_read(name) {
        (ToolKind::Read, true)
    } else {
        (ToolKind::Unspecified, false)
    }
}

/// Whether a hintless tool name looks read-only: a read verb first and no
/// write verb anywhere. Explicit annotations always win over this.
pub fn inferred_read(name: &str) -> bool {
    const READ: &[&str] = &[
        "ask", "search", "get", "list", "read", "find", "lookup", "query", "fetch", "view", "show",
        "describe", "check",
    ];
    const WRITE: &[&str] = &[
        "create",
        "add",
        "delete",
        "remove",
        "update",
        "edit",
        "set",
        "submit",
        "send",
        "post",
        "buy",
        "purchase",
        "pay",
        "checkout",
        "book",
        "reserve",
        "cancel",
        "order",
        "place",
        "write",
        "upload",
        "transfer",
        "subscribe",
        "unsubscribe",
        "invite",
        "share",
        "publish",
        "deploy",
        "start",
        "stop",
        "run",
        "execute",
        "manage",
    ];
    let words = split_name(name);
    match words.first() {
        Some(first) if READ.contains(&first.as_str()) => {
            !words.iter().any(|w| WRITE.contains(&w.as_str()))
        }
        _ => false,
    }
}

/// Split snake_case, kebab, dots and camelCase into lowercase words.
fn split_name(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    for part in name.split(['_', '-', '.']) {
        let mut current = String::new();
        let mut prev_lower = false;
        for c in part.chars() {
            if c.is_uppercase() && prev_lower && !current.is_empty() {
                words.push(std::mem::take(&mut current).to_lowercase());
            }
            current.push(c);
            prev_lower = c.is_lowercase() || c.is_ascii_digit();
        }
        if !current.is_empty() {
            words.push(current.to_lowercase());
        }
    }
    words.into_iter().filter(|w| !w.is_empty()).collect()
}

/// The kind as the index and envelopes spell it.
pub fn kind_str(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "read",
        ToolKind::Consequential => "consequential",
        ToolKind::Unspecified => "unspecified",
    }
}

/// Resolve `site/tool` through the index or `https://page#tool` directly.
pub fn resolve(target: &str, search: &Search) -> Result<Resolved, CallError> {
    if target.starts_with("http://") || target.starts_with("https://") {
        let (url, tool) = target.rsplit_once('#').ok_or_else(|| {
            CallError::usage("target must be site/tool (from find) or https://page#tool")
        })?;
        if url.is_empty() || tool.is_empty() {
            return Err(CallError::usage(
                "target must be site/tool (from find) or https://page#tool",
            ));
        }
        let site = url_host(url).ok_or_else(|| {
            CallError::usage("target must be site/tool (from find) or https://page#tool")
        })?;
        let found = search.tool(&site, tool);
        let (kind, kind_inferred) = found
            .as_ref()
            .map(|t| kind_of(&t.name, &t.annotations))
            .unwrap_or((ToolKind::Unspecified, false));
        return Ok(Resolved {
            page: url.to_string(),
            tool: tool.to_string(),
            site,
            schema: found.as_ref().map(|t| t.input_schema.clone()),
            kind,
            kind_inferred,
        });
    }
    let (site, tool) = target.split_once('/').ok_or_else(|| {
        CallError::usage("target must be site/tool (from find) or https://page#tool")
    })?;
    if site.is_empty() || tool.is_empty() {
        return Err(CallError::usage(
            "target must be site/tool (from find) or https://page#tool",
        ));
    }
    match search.tool(site, tool) {
        Some(found) => {
            let (kind, kind_inferred) = kind_of(&found.name, &found.annotations);
            Ok(Resolved {
                page: found.page.clone(),
                tool: tool.to_string(),
                site: site.to_string(),
                schema: Some(found.input_schema.clone()),
                kind,
                kind_inferred,
            })
        }
        None => Err(CallError {
            code: "tool_not_found",
            message: format!(
                "no tool {tool:?} on {site:?} in the index; run `agent-webmcp find {site}` to see what is available"
            ),
            data: None,
        }),
    }
}

/// Host of an http(s) URL, without port or userinfo.
fn url_host(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or_default();
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => h,
        _ => authority,
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_string())
}

/// Parse call input from positional JSON or `--params <json|@file>`; both or
/// neither beyond `{}` is an error. Always returns an object.
pub fn parse_input(positional: Option<&str>, params: Option<&str>) -> Result<Value, CallError> {
    match (positional, params) {
        (Some(_), Some(_)) => Err(CallError::usage(
            "pass input once: either as positional JSON or --params, not both",
        )),
        (None, None) => Ok(Value::Object(Default::default())),
        (Some(text), None) => parse_object(text, "input"),
        (None, Some(raw)) => {
            if let Some(path) = raw.strip_prefix('@') {
                let text = if path == "-" {
                    std::io::read_to_string(std::io::stdin()).map_err(|e| CallError {
                        code: "invalid_input",
                        message: format!("could not read stdin: {e}"),
                        data: None,
                    })?
                } else {
                    std::fs::read_to_string(path).map_err(|e| CallError {
                        code: "invalid_input",
                        message: format!("could not read {path}: {e}"),
                        data: None,
                    })?
                };
                parse_object(&text, if path == "-" { "stdin" } else { path })
            } else {
                parse_object(raw, "--params")
            }
        }
    }
}

fn parse_object(text: &str, what: &str) -> Result<Value, CallError> {
    let value: Value = serde_json::from_str(text).map_err(|e| CallError {
        code: "invalid_input",
        message: format!("{what} is not valid JSON: {e}"),
        data: None,
    })?;
    if !value.is_object() {
        return Err(CallError {
            code: "invalid_input",
            message: format!("{what} must be a JSON object"),
            data: None,
        });
    }
    Ok(value)
}

/// Check `input` against a JSON Schema subset: top-level object shape,
/// required keys, each present property's type and enum, and
/// additionalProperties. One level only; every problem is listed.
/// A guard, not a full JSON Schema implementation.
pub fn validate_input(schema: &Value, input: &Value) -> Result<(), CallError> {
    let mut problems = Vec::new();
    let Some(obj) = input.as_object() else {
        return Err(CallError {
            code: "invalid_input",
            message: "input must be a JSON object".to_string(),
            data: None,
        });
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !obj.contains_key(name) {
                problems.push(format!("missing required property {name:?}"));
            }
        }
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    match properties {
        Some(props) => {
            for (key, value) in obj {
                match props.get(key) {
                    None if closed(schema) => {
                        problems.push(format!("unknown property {key:?}"));
                    }
                    None => {}
                    Some(sub) => {
                        if let Some(expected) = sub.get("type")
                            && !type_matches(expected, value)
                        {
                            problems.push(format!(
                                "expected {} for {key:?}, got {}",
                                describe_type(expected),
                                json_type(value)
                            ));
                        }
                        if let Some(allowed) = sub.get("enum").and_then(Value::as_array)
                            && !allowed.contains(value)
                        {
                            problems.push(format!("{key:?} must be one of {allowed:?}"));
                        }
                    }
                }
            }
        }
        None if closed(schema) => {
            for key in obj.keys() {
                problems.push(format!("unknown property {key:?}"));
            }
        }
        None => {}
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(CallError {
            code: "invalid_input",
            message: problems.join("\n"),
            data: None,
        })
    }
}

fn closed(schema: &Value) -> bool {
    schema.get("additionalProperties") == Some(&Value::Bool(false))
}

/// Whether `value` is one of the named JSON Schema types.
fn type_matches(expected: &Value, value: &Value) -> bool {
    let names: Vec<&str> = match expected {
        Value::String(s) => vec![s.as_str()],
        Value::Array(arr) => arr.iter().filter_map(Value::as_str).collect(),
        _ => return true,
    };
    names
        .iter()
        .any(|n| *n == json_type(value) || (*n == "number" && json_type(value) == "integer"))
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn describe_type(expected: &Value) -> String {
    match expected {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => "anything".to_string(),
    }
}

/// Example input with a placeholder per required property, for `find`.
pub fn example_input(schema: &Value) -> Value {
    let mut out = serde_json::Map::new();
    let required = schema.get("required").and_then(Value::as_array);
    let properties = schema.get("properties").and_then(Value::as_object);
    for name in required.into_iter().flatten().filter_map(Value::as_str) {
        let placeholder = properties
            .and_then(|p| p.get(name))
            .and_then(|s| s.get("type"))
            .map(placeholder_for_type)
            .unwrap_or(Value::String(String::new()));
        out.insert(name.to_string(), placeholder);
    }
    Value::Object(out)
}

fn placeholder_for_type(ty: &Value) -> Value {
    let first = match ty {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(Value::as_str)
            .next()
            .unwrap_or("string")
            .to_string(),
        _ => "string".to_string(),
    };
    match first.as_str() {
        "number" | "integer" => serde_json::json!(0),
        "boolean" => serde_json::json!(false),
        "array" => serde_json::json!([]),
        "object" => serde_json::json!({}),
        _ => Value::String(String::new()),
    }
}

/// `*` matches any run, including `/` and `.`; nothing else is special.
pub fn glob_matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    if !parts[0].is_empty() {
        if let Some(next) = rest.strip_prefix(parts[0]) {
            rest = next;
        } else {
            return false;
        }
    }
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    match parts.last() {
        Some(&"") => true,
        Some(last) => rest.ends_with(last),
        None => true,
    }
}

/// Decide a call from its `<site>/<tool>` id: deny beats ask beats allow,
/// else the tool kind decides.
pub fn decide(id: &str, kind: ToolKind, perms: &Permissions) -> Decision {
    if perms.deny.iter().any(|p| glob_matches(p, id)) {
        return Decision::Deny;
    }
    if perms.ask.iter().any(|p| glob_matches(p, id)) {
        return Decision::Ask;
    }
    if perms.allow.iter().any(|p| glob_matches(p, id)) {
        return Decision::Allow;
    }
    match kind {
        ToolKind::Read => Decision::Allow,
        ToolKind::Consequential | ToolKind::Unspecified => Decision::Ask,
    }
}

/// Resolve and validate up to the permission decision, without running.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub resolved: Resolved,
    pub input: Value,
    pub decision: Decision,
}

/// Resolve, check the input shape, and decide. No driver runs here.
pub fn prepare(
    target: &str,
    input: Value,
    cfg: &Config,
    search: &Search,
) -> Result<Prepared, CallError> {
    let resolved = resolve(target, search)?;
    if !input.is_object() {
        return Err(CallError {
            code: "invalid_input",
            message: "input must be a JSON object".to_string(),
            data: None,
        });
    }
    if let Some(schema) = &resolved.schema {
        validate_input(schema, &input)?;
    }
    let id = resolved.id();
    let decision = decide(&id, resolved.kind, &cfg.permissions);
    Ok(Prepared {
        resolved,
        input,
        decision,
    })
}

/// Enforce the decision, then run or plan. Shared by the CLI and MCP.
pub fn finish(prepared: Prepared, opts: &CallOptions, cfg: &Config) -> Result<Outcome, CallError> {
    let id = prepared.resolved.id();
    match prepared.decision {
        Decision::Deny => {
            let pattern = cfg
                .permissions
                .deny
                .iter()
                .find(|p| glob_matches(p, &id))
                .cloned()
                .unwrap_or_default();
            return Err(CallError {
                code: "denied",
                message: format!("{id} matches denied pattern {pattern:?} from config"),
                data: None,
            });
        }
        Decision::Ask if !opts.confirm => {
            return Err(CallError {
                code: "needs_confirmation",
                message: format!(
                    "{} ({}) needs confirmation to run",
                    id,
                    kind_str(prepared.resolved.kind)
                ),
                data: Some(serde_json::json!({
                    "kind": kind_str(prepared.resolved.kind),
                    "id": id,
                    "retry": opts.retry_hint,
                })),
            });
        }
        Decision::Ask | Decision::Allow => {}
    }
    let req = CallRequest {
        page: prepared.resolved.page.clone(),
        tool: prepared.resolved.tool.clone(),
        input: prepared.input,
    };
    match driver::choose(cfg) {
        Choice::Run(driver, _) => {
            if opts.plan_only {
                let plan = driver.plan(&req);
                Ok(Outcome::Planned {
                    id,
                    tool: prepared.resolved.tool.clone(),
                    page: prepared.resolved.page.clone(),
                    plan,
                })
            } else {
                let outcome = driver.run(
                    &req,
                    &driver::RunOptions {
                        timeout_ms: cfg.call.timeout,
                        settle_ms: cfg.call.settle_ms.min(30_000),
                        extra_args: cfg.drivers.agent_browser.extra_args.clone(),
                        headed: cfg.drivers.agent_browser.headed,
                        fresh: opts.fresh,
                        profile: cfg.drivers.agent_browser.profile.clone(),
                        cdp: cfg.drivers.agent_browser.cdp.clone(),
                    },
                )?;
                let driver::RunOutcome {
                    output,
                    duration_ms,
                    reused,
                    budget_ms,
                } = outcome;
                Ok(Outcome::Ran {
                    id,
                    tool: prepared.resolved.tool.clone(),
                    page: prepared.resolved.page.clone(),
                    driver: driver.id(),
                    output,
                    duration_ms,
                    reused,
                    budget_ms,
                })
            }
        }
        Choice::Plan(driver) => {
            let plan = driver.plan(&req);
            Ok(Outcome::Planned {
                id,
                tool: prepared.resolved.tool.clone(),
                page: prepared.resolved.page.clone(),
                plan,
            })
        }
        Choice::Unusable { detection, .. } => {
            if opts.plan_only {
                Ok(Outcome::Planned {
                    id,
                    tool: prepared.resolved.tool.clone(),
                    page: prepared.resolved.page.clone(),
                    plan: unusable_plan(&req, &detection),
                })
            } else {
                Err(no_driver(cfg))
            }
        }
    }
}

/// The CLI pipeline: parse input, then the shared prepare/finish path.
pub fn run_call(
    target: &str,
    positional: Option<&str>,
    params: Option<&str>,
    opts: &CallOptions,
    cfg: &Config,
    search: &Search,
) -> Result<Outcome, CallError> {
    let input = parse_input(positional, params)?;
    finish(prepare(target, input, cfg, search)?, opts, cfg)
}

/// The pre-`prepare` entry point both front ends used; kept for tests.
pub fn execute(
    resolved: &Resolved,
    input: Value,
    opts: &CallOptions,
    cfg: &Config,
) -> Result<Outcome, CallError> {
    let prepared = Prepared {
        resolved: resolved.clone(),
        input,
        decision: decide(&resolved.id(), resolved.kind, &cfg.permissions),
    };
    finish(prepared, opts, cfg)
}

fn unusable_plan(req: &CallRequest, detection: &Detection) -> Plan {
    let mut plan = driver::generic::Generic.plan(req);
    let install = match detection {
        Detection::Missing { install, .. } | Detection::TooOld { install, .. } => install.clone(),
        Detection::Ready { .. } | Detection::Unknown => return plan,
    };
    plan.note = Some(match plan.note {
        Some(note) => format!("{note}; install: {install}"),
        None => format!("install: {install}"),
    });
    plan
}

fn no_driver(cfg: &Config) -> CallError {
    CallError {
        code: "no_driver",
        message: "no browser driver is available".to_string(),
        data: Some(driver::setup_help(cfg, &user_config_path())),
    }
}

/// The user config path for the setup help, without failing the call.
fn user_config_path() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    crate::config::user_path(&|key: &str| std::env::var(key).ok(), &cwd)
        .display()
        .to_string()
}

/// The MCP entry point: same pipeline, MCP retry hint, envelope + error flag.
pub fn run_mcp(
    target: &str,
    input: Value,
    confirm: bool,
    cfg: &Config,
    search: &Search,
) -> (Value, bool) {
    let outcome = prepare(target, input, cfg, search).and_then(|prepared| {
        finish(
            prepared,
            &CallOptions {
                confirm,
                plan_only: false,
                fresh: false,
                retry_hint: RETRY_MCP,
            },
            cfg,
        )
    });
    match outcome {
        Ok(done) => (done.envelope(), false),
        Err(e) => (e.envelope(), true),
    }
}

/// Opaque approval handle: hex SHA-256 of target and canonical input.
pub fn approval_state(target: &str, input: &Value) -> String {
    let canonical = serde_json::to_string(input).unwrap_or_default();
    crate::store::sha256_hex(format!("{target}\n{canonical}").as_bytes())
}
