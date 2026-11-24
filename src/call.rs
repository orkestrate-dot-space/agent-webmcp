//! `call`: resolve the target, check permissions, run or plan.
//!
//! The CLI and the MCP tool share this pipeline so they cannot diverge:
//! resolve → parse and validate input → permissions → choose → run or plan.

use serde_json::Value;

use crate::config::{Config, Permissions};
use crate::driver::{self, CallRequest, Choice, Detection, Driver, NativeCollisionPolicy, Plan};
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
    /// Local custom tool code, when the target resolved to one.
    pub custom: Option<CustomMeta>,
}

/// Local custom tool backing a resolved call.
#[derive(Debug, Clone)]
pub struct CustomMeta {
    /// Normalized host (with port, e.g. `localhost:3000`).
    pub host: String,
    /// Full expected origin (`scheme://host[:port]`) of the tool's page.
    pub origin: String,
    /// Host directory.
    pub dir: std::path::PathBuf,
    /// Raw `tools.js` source.
    pub js_source: String,
    /// Hex SHA-256 over `manifest.json` + `tools.js`.
    pub tool_hash: String,
    /// Needs a headed browser.
    pub headed: bool,
    /// May run in personal Chrome.
    pub allow_browse: bool,
    /// `"session"` or `"none"`.
    pub auth: String,
}

impl Resolved {
    /// `<site>/<tool>`, the id permissions match against.
    pub fn id(&self) -> String {
        format!("{}/{}", self.site, self.tool)
    }

    /// `custom/<host>/<tool>`, for `custom/*` rules.
    pub fn custom_id(&self) -> Option<String> {
        self.custom
            .as_ref()
            .map(|_| format!("custom/{}/{}", self.site, self.tool))
    }

    /// True when the target resolved to a local custom tool.
    pub fn is_custom(&self) -> bool {
        self.custom.is_some()
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
        /// `"custom"` when a custom tool ran, else `"native"`.
        source: String,
        /// Code hash when a custom tool ran.
        tool_hash: Option<String>,
    },
    Planned {
        id: String,
        tool: String,
        page: String,
        plan: Plan,
    },
    /// A live native tool shadowed the resolved custom tool and native
    /// permission denied it: neither tool ran (`permission_denied`).
    Blocked {
        id: String,
        tool: String,
        page: String,
        native_id: String,
        message: String,
    },
    /// A live native tool shadowed the resolved custom tool and needs an
    /// explicit native grant: neither tool ran (`needs_confirmation` for the
    /// native identity).
    NeedsConfirmation {
        id: String,
        tool: String,
        page: String,
        native_id: String,
        retry_hint: String,
        message: String,
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
                source,
                tool_hash,
            } => {
                let mut data = serde_json::json!({
                    "id": id,
                    "tool": tool,
                    "page": page,
                    "driver": driver,
                    "output": output,
                    "durationMs": duration_ms,
                    "reused": reused,
                    "budgetMs": budget_ms,
                    "source": source,
                });
                if source == "custom" {
                    if let Some(h) = tool_hash {
                        data["toolHash"] = Value::String(h.clone());
                    }
                    data["untrusted"] = Value::Bool(true);
                }
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
            Outcome::Blocked {
                id,
                tool,
                page,
                native_id,
                message,
            } => serde_json::json!({
                "success": false,
                "code": "permission_denied",
                "error": message,
                "data": { "id": id, "tool": tool, "page": page, "nativeId": native_id },
            }),
            Outcome::NeedsConfirmation {
                id,
                tool,
                page,
                native_id,
                retry_hint,
                message,
            } => serde_json::json!({
                "success": false,
                "code": "needs_confirmation",
                "error": message,
                "data": {
                    "kind": "unspecified",
                    "id": native_id,
                    "customId": id,
                    "tool": tool,
                    "page": page,
                    "retry": retry_hint,
                },
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
        Outcome::Blocked { message, .. } => {
            eprintln!("error (permission_denied): {message}");
        }
        Outcome::NeedsConfirmation {
            message,
            retry_hint,
            ..
        } => {
            eprintln!("error (needs_confirmation): {message}");
            eprintln!("retry: {retry_hint}");
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
/// Indexed native tools win; otherwise a matching local custom tool resolves.
/// `tools_dir` overrides `AGENT_WEBMCP_TOOLS_DIR` (tests); `None` reads the
/// process environment like the CLI does.
pub fn resolve(target: &str, search: &Search) -> Result<Resolved, CallError> {
    resolve_in(target, search, None)
}

/// [`resolve`] with an explicit custom-tools root.
pub fn resolve_in(
    target: &str,
    search: &Search,
    tools_dir: Option<&std::path::Path>,
) -> Result<Resolved, CallError> {
    if target.starts_with("http://") || target.starts_with("https://") {
        // The tool name follows the last `#`; the page keeps everything
        // before it, including its own route fragment (`https://host/dash#/
        // route#name` addresses `name` on `https://host/dash#/route`). A
        // `#`-less page cannot name a tool, and the tool part must look
        // like a name (no `/`, `?`, `&`, or whitespace) so a bare route
        // fragment (`...#/route`) is a usage error, not a tool called
        // `/route`.
        let (url, tool) = target.rsplit_once('#').ok_or_else(|| {
            CallError::usage("target must be site/tool (from find) or https://page#tool")
        })?;
        if url.is_empty() || tool.is_empty() {
            return Err(CallError::usage(
                "target must be site/tool (from find) or https://page#tool",
            ));
        }
        if tool.contains('#')
            || tool
                .chars()
                .any(|c| c == '/' || c == '?' || c == '&' || c.is_whitespace())
        {
            return Err(CallError::usage(
                "target must be site/tool (from find) or https://page#tool",
            ));
        }
        let site = url_host(url).ok_or_else(|| {
            CallError::usage("target must be site/tool (from find) or https://page#tool")
        })?;
        if let Some(found) = search.tool(&site, tool) {
            // The indexed schema and kind belong to the indexed page: on a
            // different page neither applies. Reusing a read-only kind here
            // would auto-allow input the old schema rejected, letting the
            // ordinary native dispatch invoke a same-named consequential
            // tool without confirmation — so only the indexed origin itself
            // returns here. Any other page (another port, another path)
            // falls through to the local custom catalog below, and only to
            // the native fallback at the end when no custom tool matches.
            // This keeps `https://host:8443/#tool` and its equivalent short
            // target resolving to the same custom definition instead of the
            // port-443 native entry hiding it.
            if same_indexed_page(url, &found.page) {
                let (kind, kind_inferred) = kind_of(&found.name, &found.annotations);
                return Ok(Resolved {
                    page: url.to_string(),
                    tool: tool.to_string(),
                    site,
                    schema: Some(found.input_schema.clone()),
                    kind,
                    kind_inferred,
                    custom: None,
                });
            }
        }
        // For custom tools, try the port-aware authority first
        // (`localhost:3000` keeps its explicit port): when both a bare host
        // and a port-specific catalog define the tool, a direct URL and its
        // equivalent short target resolve to the same port-specific
        // definition, code, schema, and permission identity. The bare host
        // stays as a fallback — but ONLY when the bare-host catalog's
        // declared origin matches the requested origin. Grants are
        // per-origin: a bare-host catalog declares `https://<host>`
        // (default port 443), so `https://example.com:8443/...` must NOT
        // silently steal the `example.com` catalog, code, or permission
        // grant. An explicit `:443` still matches (`expected_origin`
        // normalizes default ports away on both sides).
        if let Some(custom) = url_custom_host(url)
            .and_then(|h| load_custom_tool(&h, tool, tools_dir))
            .or_else(|| load_custom_tool(&site, tool, tools_dir))
        {
            // A direct `http://` URL must not override the validated
            // `https://` manifest page of a session-authenticated tool: the
            // driver would send session cookies over cleartext.
            if custom.meta.auth == "session"
                && url
                    .split_once("://")
                    .map(|(scheme, _)| scheme.eq_ignore_ascii_case("http"))
                    .unwrap_or(false)
            {
                return Err(CallError::usage(format!(
                    "tool {tool:?} uses session auth and requires an https:// page, got {url:?}"
                )));
            }
            let origin =
                crate::tools::expected_origin(url).map_err(|e| CallError::usage(e.to_string()))?;
            // The catalog's declared host:port must be the host:port the
            // driver will visit: a bare-host fallback whose manifest
            // declares `example.com` (default port 443) never serves
            // `example.com:8443`, whose code, schema, and permission
            // identity belong to another origin. Compared without the
            // scheme: manifests declare an `https://` page while local
            // development URLs are commonly `http://`, and the scheme is
            // enforced separately (session-auth refusal, persistent-session
            // HTTP refusal). Both sides are `expected_origin`-normalized,
            // so an explicit default port (`:443`) still matches the bare
            // host.
            let declared_hostport = custom
                .meta
                .origin
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or_default();
            let requested_hostport = origin
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or_default();
            if declared_hostport != requested_hostport {
                return Err(CallError::usage(format!(
                    "tool {tool:?} is not defined for {origin} (the {site} catalog declares {})",
                    custom.meta.origin
                )));
            }
            let mut meta = custom.meta;
            meta.origin = origin;
            return Ok(Resolved {
                page: url.to_string(),
                tool: tool.to_string(),
                site: custom.site,
                schema: Some(custom.schema),
                kind: custom.kind,
                kind_inferred: false,
                custom: Some(meta),
            });
        }
        let (kind, kind_inferred) = (ToolKind::Unspecified, false);
        return Ok(Resolved {
            page: url.to_string(),
            tool: tool.to_string(),
            site,
            schema: None,
            kind,
            kind_inferred,
            custom: None,
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
                custom: None,
            })
        }
        None => match load_custom_tool(site, tool, tools_dir) {
            Some(custom) => {
                let origin = crate::tools::expected_origin(&custom.page)
                    .map_err(|e| CallError::usage(e.to_string()))?;
                let mut meta = custom.meta;
                meta.origin = origin;
                Ok(Resolved {
                    page: custom.page,
                    tool: tool.to_string(),
                    site: custom.site,
                    schema: Some(custom.schema),
                    kind: custom.kind,
                    kind_inferred: false,
                    custom: Some(meta),
                })
            }
            None => Err(CallError {
                code: "tool_not_found",
                message: format!(
                    "no tool {tool:?} on {site:?} in the index; run `agent-webmcp find {site}` to see what is available"
                ),
                data: None,
            }),
        },
    }
}

struct CustomResolved {
    page: String,
    site: String,
    schema: Value,
    kind: ToolKind,
    meta: CustomMeta,
}

/// Load a matching local custom tool, if any. Best-effort: broken hosts or
/// missing dirs mean no custom tool. `tools_dir` overrides
/// `AGENT_WEBMCP_TOOLS_DIR`; `None` reads the process environment.
fn load_custom_tool(
    site: &str,
    tool: &str,
    tools_dir: Option<&std::path::Path>,
) -> Option<CustomResolved> {
    let normalized = crate::tools::normalize_host(site).ok()?;
    // `load_host` normalizes again; use the normalized host for lookup.
    let loaded = match tools_dir {
        Some(root) => crate::tools::load_host_in(root, &normalized).ok()?,
        None => crate::tools::load_host(&normalized).ok()?,
    }?;
    let def = loaded.manifest.tools.iter().find(|t| t.name == tool)?;
    let page = def.effective_page(&loaded.manifest).to_string();
    let origin = crate::tools::expected_origin(&page).ok()?;
    let kind = if def.read_only {
        ToolKind::Read
    } else {
        ToolKind::Unspecified
    };
    Some(CustomResolved {
        page,
        site: loaded.host.clone(),
        schema: def.input_schema.clone(),
        kind,
        meta: CustomMeta {
            host: loaded.host.clone(),
            origin,
            dir: loaded.dir.clone(),
            js_source: loaded.js_source.clone(),
            tool_hash: loaded.sha256.clone(),
            headed: def.headed,
            allow_browse: def.allow_browse,
            auth: def.auth.as_str().to_string(),
        },
    })
}

/// Normalized host with port for custom lookup from an `http(s)` URL.
/// Unlike [`url_host`], this keeps every explicit port (`localhost:3000`,
/// `example.com:80`) for the catalog key.
fn url_custom_host(url: &str) -> Option<String> {
    let after = url.split_once("://")?.1;
    let authority = after.split(['/', '?', '#']).next().unwrap_or_default();
    let hostport = authority.rsplit('@').next().unwrap_or_default();
    if hostport.is_empty() || hostport.contains('[') {
        return None;
    }
    crate::tools::normalize_host(hostport).ok()
}

/// Host of an http(s) URL, lowercase, without port or userinfo.
fn url_host(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or_default();
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &rest[..end];
        if host.is_empty() {
            return None;
        }
        return Some(host.to_lowercase());
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => h,
        _ => authority,
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

/// Whether a direct-URL call target names the same page the index recorded
/// for the tool: same origin and same route, fragment included.
/// `/app#/reports` and `/app#/settings` can be different tools sharing a
/// name, so the fragment participates in the comparison; the query string
/// participates too (a site can register a read tool at `?view=read` and a
/// same-named write tool at `?view=edit`, so ignoring it would reuse an
/// indexed `Read` grant for a write page). Only a bare root slash is
/// normalized away (`/` and `` are the same page when neither carries a
/// query nor a fragment). A different route means the indexed
/// schema and kind belong to a different page and must not gate this call.
fn same_indexed_page(requested_url: &str, indexed_page: &str) -> bool {
    fn origin_and_route(url: &str) -> Option<(String, String)> {
        let origin = crate::tools::expected_origin(url).ok()?;
        let after = url.split_once("://")?.1;
        // Split off the fragment FIRST so a query never eats it:
        // `/app?view=compact#/reports` keeps route `/app?view=compact#/reports`.
        let (before_frag, frag) = match after.split_once('#') {
            Some((before, frag)) => (before, Some(frag)),
            None => (after, None),
        };
        let (path_part, query_part) = match before_frag.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (before_frag, None),
        };
        let path = path_part
            .split_once('/')
            .map(|(_, p)| format!("/{p}"))
            .unwrap_or_default();
        let path = if path == "/" { String::new() } else { path };
        let mut route = path;
        if let Some(query) = query_part {
            route.push('?');
            route.push_str(query);
        }
        if let Some(frag) = frag {
            route.push('#');
            route.push_str(frag);
        }
        Some((origin, route))
    }
    match (
        origin_and_route(requested_url),
        origin_and_route(indexed_page),
    ) {
        (Some((request_origin, request_route)), Some((indexed_origin, indexed_route))) => {
            request_origin == indexed_origin && request_route == indexed_route
        }
        _ => false,
    }
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
                    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                        return Err(CallError::usage(
                            "--params @- reads JSON from a pipe; stdin is a terminal",
                        ));
                    }
                    read_capped(std::io::stdin(), "stdin")?
                } else {
                    let file = std::fs::File::open(path).map_err(|e| CallError {
                        code: "invalid_input",
                        message: format!("could not read {path}: {e}"),
                        data: None,
                    })?;
                    read_capped(file, path)?
                };
                parse_object(&text, if path == "-" { "stdin" } else { path })
            } else {
                parse_object(raw, "--params")
            }
        }
    }
}

/// Read a whole input source, capped at 1 MiB (agent-browser rejects more).
fn read_capped(source: impl std::io::Read, what: &str) -> Result<String, CallError> {
    use std::io::Read as _;
    const MAX_INPUT_BYTES: u64 = 1024 * 1024;
    let mut text = String::new();
    source
        .take(MAX_INPUT_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| CallError {
            code: "invalid_input",
            message: format!("could not read {what}: {e}"),
            data: None,
        })?;
    if text.len() as u64 > MAX_INPUT_BYTES {
        return Err(CallError {
            code: "invalid_input",
            message: "input is larger than 1 MiB".to_string(),
            data: None,
        });
    }
    Ok(text)
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

/// Validate `input` against a custom tool's schema right before its JS
/// runs. Drivers call this only when no live native tool runs instead, so
/// a native tool with a different schema is never blocked by the local
/// custom schema (and invalid input never reaches custom JS).
pub fn validate_custom_input(schema: &Value, input: &Value) -> Result<(), CallError> {
    validate_input(schema, input)
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

/// Decide for a resolved target, including custom tools. Checks `deny` >
/// `ask` > `allow` against both `host/tool` and `custom/<host>/<tool>` (so
/// `custom/*` works). Custom tools never auto-allow via `inferred_read` or
/// `readOnly`: without an explicit allow they always Ask.
pub fn decide_for_resolved(resolved: &Resolved, perms: &Permissions) -> Decision {
    if !resolved.is_custom() {
        return decide(&resolved.id(), resolved.kind, perms);
    }
    let id = resolved.id();
    let custom_id = resolved.custom_id().unwrap_or_default();
    if perms
        .deny
        .iter()
        .any(|p| glob_matches(p, &id) || glob_matches(p, &custom_id))
    {
        return Decision::Deny;
    }
    if perms
        .ask
        .iter()
        .any(|p| glob_matches(p, &id) || glob_matches(p, &custom_id))
    {
        return Decision::Ask;
    }
    if perms
        .allow
        .iter()
        .any(|p| glob_matches(p, &id) || glob_matches(p, &custom_id))
    {
        return Decision::Allow;
    }
    Decision::Ask
}

/// Resolve and validate up to the permission decision, without running.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub resolved: Resolved,
    pub input: Value,
    pub decision: Decision,
    /// Effective `agent-browser` browser context snapshotted at prepare
    /// time. Bound into [`approval_state`] and pinned through execution by
    /// [`finish_with_driver`]: an operator change to `profile`, `cdp`, or
    /// the named `session` while a prompt is open invalidates the approval.
    pub browser_profile: Option<String>,
    pub browser_cdp: Option<String>,
    pub browser_session: Option<String>,
}

/// Resolve, check the input shape, and decide. No driver runs here.
pub fn prepare(
    target: &str,
    input: Value,
    cfg: &Config,
    search: &Search,
) -> Result<Prepared, CallError> {
    prepare_in(target, input, cfg, search, None)
}

/// [`prepare`] with an explicit custom-tools root (tests pass `Some`; the
/// CLI passes `None` to read the process environment).
pub fn prepare_in(
    target: &str,
    input: Value,
    cfg: &Config,
    search: &Search,
    tools_dir: Option<&std::path::Path>,
) -> Result<Prepared, CallError> {
    let resolved = resolve_in(target, search, tools_dir)?;
    if !input.is_object() {
        return Err(CallError {
            code: "invalid_input",
            message: "input must be a JSON object".to_string(),
            data: None,
        });
    }
    // Custom targets validate later: only when the driver confirms no live
    // native tool runs instead (which may accept different input), right
    // before custom JS would run. Validating here would wrongly block a
    // shadowing native tool with a different schema.
    if !resolved.is_custom()
        && let Some(schema) = &resolved.schema
    {
        validate_input(schema, &input)?;
    }
    let decision = decide_for_resolved(&resolved, &cfg.permissions);
    Ok(Prepared {
        resolved,
        input,
        decision,
        browser_profile: cfg.drivers.agent_browser.profile.clone(),
        browser_cdp: cfg.drivers.agent_browser.cdp.clone(),
        browser_session: Some(cfg.drivers.agent_browser.session.clone()),
    })
}

/// Native-collision permission for a resolved custom tool.
///
/// Policy design (enforced by all three run drivers before a live native
/// tool executes in place of a resolved custom tool):
/// - The native permission is evaluated strictly against the native
///   identity `<site>/<tool>` with [`ToolKind::Unspecified`]: the live
///   tool is unindexed, so its kind is unknown. `custom/*` rules never
///   grant the native tool, and the custom tool's `readOnly` flag never
///   applies to it. An explicit native `allow` rule for `<site>/<tool>`
///   is the only grant.
/// - `Allow`: the driver may invoke the live native tool (`used_native`).
/// - `Deny`: the driver must neither invoke the live tool nor fall back
///   to custom code; the signal maps to [`Outcome::Blocked`].
/// - `Ask` (no explicit native grant — even with `custom/*`, `--confirm`,
///   or MCP approval for the custom hash): the driver must neither invoke
///   nor fall back; the signal maps to [`Outcome::NeedsConfirmation`].
pub fn native_collision_policy(resolved: &Resolved, perms: &Permissions) -> NativeCollisionPolicy {
    match decide(&resolved.id(), ToolKind::Unspecified, perms) {
        Decision::Allow => NativeCollisionPolicy::Allow,
        Decision::Ask => NativeCollisionPolicy::Ask,
        Decision::Deny => NativeCollisionPolicy::Deny,
    }
}

/// The approval-bound execution scope: the effective driver plus the
/// `agent-browser` browser context (`profile`, `cdp`, named `session`)
/// active at approval time. The `agent-browser` driver reads all three from
/// the current config for every run, so an operator change while a prompt
/// is open would otherwise run approved code under a new browser context
/// (different cookies, storage, session, or remote endpoint) without a new
/// prompt.
#[derive(Debug, Clone)]
pub struct ApprovedScope {
    pub driver: String,
    pub profile: Option<String>,
    pub cdp: Option<String>,
    pub session: Option<String>,
}

impl ApprovedScope {
    pub fn capture(driver: &str, cfg: &Config) -> Self {
        Self {
            driver: driver.to_string(),
            profile: cfg.drivers.agent_browser.profile.clone(),
            cdp: cfg.drivers.agent_browser.cdp.clone(),
            session: Some(cfg.drivers.agent_browser.session.clone()),
        }
    }

    /// Capture bound to a resolved custom tool: headed tools run in the
    /// `{session}-headed` session, so the approval must pin that effective
    /// name instead of the configured one.
    pub fn capture_for_resolved(driver: &str, cfg: &Config, resolved: &Resolved) -> Self {
        Self {
            driver: driver.to_string(),
            profile: cfg.drivers.agent_browser.profile.clone(),
            cdp: cfg.drivers.agent_browser.cdp.clone(),
            session: Some(effective_session(cfg, resolved)),
        }
    }
}

/// Effective agent-browser session for a resolved target: headed custom
/// tools run in `{session}-headed`, everything else in the configured
/// session (mirrors `AgentBrowser::prepare_session`).
pub fn effective_session(cfg: &Config, resolved: &Resolved) -> String {
    let force_headed = resolved.custom.as_ref().is_some_and(|c| c.headed);
    if force_headed && !cfg.drivers.agent_browser.headed {
        format!(
            "{}-headed",
            sanitize_session_name(&cfg.drivers.agent_browser.session)
        )
    } else {
        cfg.drivers.agent_browser.session.clone()
    }
}

/// Restrict a session name to agent-browser's session alphabet
/// (`[A-Za-z0-9_-]`), replacing anything else with `-`. Mirrors the
/// driver's `sanitize_session_name` so approval binding agrees with it.
fn sanitize_session_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "session".to_string()
    } else {
        cleaned
    }
}
/// Enforce the decision, then run or plan. Shared by the CLI and MCP.
///
/// The approved scope must match the execution scope: pass the effective
/// driver plus browser context captured at prompt time via
/// [`finish_with_driver`] from both the elicitation and retry approval
/// paths, and execution stops with `needs_confirmation` when the driver
/// or browser context selected for execution differs. Without a pinned
/// scope (`None`, the CLI path) the current selection runs directly.
pub fn finish(prepared: Prepared, opts: &CallOptions, cfg: &Config) -> Result<Outcome, CallError> {
    finish_with_driver(prepared, opts, cfg, None)
}

/// [`finish`] with the approval-bound scope.
///
/// `approved` is the effective driver plus browser context captured when
/// the user approved (elicitation prompt or retry validation). When
/// `Some`, the driver and `agent-browser` `profile`/`cdp`/`session`
/// actually selected for execution must match it; a mismatch returns
/// `needs_confirmation` so the user approves again under the new scope
/// instead of running code the approval never covered. This covers both
/// the elicitation path (the user may take up to 120 s to answer, during
/// which auto-detection can resolve differently or the operator can change
/// the profile/CDP/session) and the retry path (detection or config can
/// change between the retry validation and the execution `choose` inside
/// the blocking task).
pub fn finish_with_driver(
    prepared: Prepared,
    opts: &CallOptions,
    cfg: &Config,
    approved: Option<&ApprovedScope>,
) -> Result<Outcome, CallError> {
    let id = prepared.resolved.id();
    match prepared.decision {
        Decision::Deny => {
            let custom_id = prepared.resolved.custom_id().unwrap_or_default();
            let pattern = cfg
                .permissions
                .deny
                .iter()
                .find(|p| glob_matches(p, &id) || glob_matches(p, &custom_id))
                .cloned()
                .unwrap_or_default();
            // A denied custom target never runs: report Blocked (neither
            // the custom tool nor any shadowing native tool executes).
            if prepared.resolved.is_custom() {
                return Ok(Outcome::Blocked {
                    id: id.clone(),
                    tool: prepared.resolved.tool.clone(),
                    page: prepared.resolved.page.clone(),
                    native_id: id.clone(),
                    message: format!(
                        "{id} matches denied pattern {pattern:?} from config; refusing the custom tool (permission_denied)"
                    ),
                });
            }
            return Err(CallError {
                code: "denied",
                message: format!("{id} matches denied pattern {pattern:?} from config"),
                data: None,
            });
        }
        Decision::Ask if !opts.confirm => {
            if let Some(custom) = &prepared.resolved.custom {
                let short: String = custom.tool_hash.chars().take(12).collect();
                let mut message = format!(
                    "[custom] {id} ({}) sha {short} needs confirmation to run",
                    kind_str(prepared.resolved.kind)
                );
                if cfg.driver == crate::config::DriverChoice::Browse {
                    message.push_str("; warning: running in personal Chrome (browse)");
                }
                return Err(CallError {
                    code: "needs_confirmation",
                    message,
                    data: Some(serde_json::json!({
                        "kind": kind_str(prepared.resolved.kind),
                        "id": id,
                        "retry": opts.retry_hint,
                        "toolHash": custom.tool_hash,
                    })),
                });
            }
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
    let native_policy = native_collision_policy(&prepared.resolved, &cfg.permissions);
    let custom_exec = prepared
        .resolved
        .custom
        .as_ref()
        .map(|c| driver::CustomExec {
            host: c.host.clone(),
            common: driver::CustomCommon {
                expected_origin: c.origin.clone(),
                js_source: c.js_source.as_str().into(),
                tool_hash: c.tool_hash.clone(),
                headed: c.headed,
                allow_browse: c.allow_browse,
                input_schema: prepared
                    .resolved
                    .schema
                    .clone()
                    .unwrap_or_else(|| Value::Object(Default::default())),
            },
            auth: match c.auth.as_str() {
                "session" => crate::tools::AuthMode::Session,
                _ => crate::tools::AuthMode::None,
            },
            dir: c.dir.clone(),
            native_policy,
        });
    let req = CallRequest {
        page: prepared.resolved.page.clone(),
        tool: prepared.resolved.tool.clone(),
        input: prepared.input.clone(),
        custom: custom_exec,
    };
    let choice = driver::choose(cfg);
    // The approval binds the executing driver and browser context: when the
    // caller pinned the approved scope, a different selection stops here so
    // the user approves again under the new scope. This compares the driver
    // that will actually run the tool below plus the live profile/CDP.
    if let Some(approved) = approved {
        let actual: &str = match &choice {
            Choice::Run(driver, _) => driver.id(),
            Choice::Plan(driver) => driver.id(),
            Choice::Unusable { driver, .. } => driver,
        };
        let actual_profile = cfg.drivers.agent_browser.profile.as_deref();
        let actual_cdp = cfg.drivers.agent_browser.cdp.as_deref();
        let actual_session = effective_session(cfg, &prepared.resolved);
        // A CDP endpoint can carry credentials (userinfo, `token=` query
        // params): never print one raw in a diagnostic the caller
        // receives. Keep enough shape to tell endpoints apart.
        fn redact_endpoint(value: &str) -> String {
            let (base, had_query) = match value.split_once('?') {
                Some((base, _)) => (base, true),
                None => (value, false),
            };
            let base = match base.split_once("://") {
                Some((scheme, rest)) => match rest.rsplit_once('@') {
                    Some((_, host)) => format!("{scheme}://…@{host}"),
                    None => base.to_string(),
                },
                None => base.to_string(),
            };
            if had_query {
                format!("{base}?…")
            } else {
                base
            }
        }
        if actual != approved.driver
            || actual_profile != approved.profile.as_deref()
            || actual_cdp != approved.cdp.as_deref()
            || actual_session.as_str() != approved.session.as_deref().unwrap_or_default()
        {
            return Err(CallError {
                code: "needs_confirmation",
                message: format!(
                    "approved scope (driver {}, profile {:?}, cdp {}, session {:?}) differs from current scope (driver {actual}, profile {actual_profile:?}, cdp {}, session {actual_session:?}); approve again to run under the current scope",
                    approved.driver,
                    approved.profile,
                    approved
                        .cdp
                        .as_deref()
                        .map(redact_endpoint)
                        .as_deref()
                        .unwrap_or("none"),
                    approved.session,
                    redact_endpoint(actual_cdp.unwrap_or("none")),
                ),
                data: Some(serde_json::json!({
                    "kind": kind_str(prepared.resolved.kind),
                    "id": id,
                    "retry": opts.retry_hint,
                    "approvedDriver": approved.driver,
                    "driver": actual,
                })),
            });
        }
    }
    match choice {
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
                let outcome = match driver.run(
                    &req,
                    &driver::RunOptions {
                        timeout_ms: cfg.call.timeout,
                        settle_ms: cfg.call.settle_ms.min(30_000),
                        fresh: opts.fresh,
                    },
                ) {
                    // A live native tool shadowed the custom target: map the
                    // driver signal to Blocked / NeedsConfirmation instead of
                    // running anything.
                    Err(e)
                        if prepared.resolved.is_custom()
                            && (e.code == "native_denied"
                                || e.code == "native_needs_confirmation") =>
                    {
                        let native_id = e
                            .data
                            .as_ref()
                            .and_then(|d| d.get("nativeId"))
                            .and_then(Value::as_str)
                            .unwrap_or(id.as_str())
                            .to_string();
                        let custom_id = id.clone();
                        if e.code == "native_denied" {
                            let message = format!(
                                "live native tool {native_id} shadows custom tool {custom_id} and is denied by config (permission_denied); refusing to run either"
                            );
                            return Ok(Outcome::Blocked {
                                id: custom_id,
                                tool: prepared.resolved.tool.clone(),
                                page: prepared.resolved.page.clone(),
                                native_id,
                                message,
                            });
                        }
                        let message = format!(
                            "live native tool {native_id} shadows custom tool {custom_id} and requires an explicit native allow rule for {native_id} (custom approval does not cover the native tool)"
                        );
                        return Ok(Outcome::NeedsConfirmation {
                            id: custom_id,
                            tool: prepared.resolved.tool.clone(),
                            page: prepared.resolved.page.clone(),
                            native_id,
                            retry_hint: opts.retry_hint.to_string(),
                            message,
                        });
                    }
                    Err(e) => return Err(e),
                    Ok(outcome) => outcome,
                };
                let driver::RunOutcome {
                    output,
                    duration_ms,
                    reused,
                    budget_ms,
                    used_native,
                } = outcome;
                let (source, tool_hash) = match &prepared.resolved.custom {
                    Some(c) if !used_native => ("custom".to_string(), Some(c.tool_hash.clone())),
                    _ => ("native".to_string(), None),
                };
                Ok(Outcome::Ran {
                    id,
                    tool: prepared.resolved.tool.clone(),
                    page: prepared.resolved.page.clone(),
                    driver: driver.id(),
                    output,
                    duration_ms,
                    reused,
                    budget_ms,
                    source,
                    tool_hash,
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
        decision: decide_for_resolved(resolved, &cfg.permissions),
        browser_profile: cfg.drivers.agent_browser.profile.clone(),
        browser_cdp: cfg.drivers.agent_browser.cdp.clone(),
        browser_session: Some(cfg.drivers.agent_browser.session.clone()),
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

pub fn no_driver(cfg: &Config) -> CallError {
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
        Ok(done @ (Outcome::Blocked { .. } | Outcome::NeedsConfirmation { .. })) => {
            (done.envelope(), true)
        }
        Ok(done) => (done.envelope(), false),
        Err(e) => (e.envelope(), true),
    }
}

/// Opaque approval handle: hex SHA-256 of target, canonical input, the
/// custom tool hash (when the target is a custom tool), the resolved page,
/// the effective browser driver, and the effective `agent-browser` browser
/// context (`profile`, `cdp`). Editing `manifest.json` or `tools.js`
/// by one byte — switching drivers between prompt and retry, changing the
/// browser profile/CDP/session while a prompt is open, or between
/// retry validation and execution — or resolving to a different page (e.g.
/// an index refresh changing the target's page between prompt and retry)
/// invalidates prior approval and triggers a fresh prompt.
/// The elicitation path pins the prompt-time scope through execution
/// (`finish_with_driver`); the retry path pins the validated scope the
/// same way.
///
/// Eight arguments is the binding surface, not accretion: every parameter
/// is hashed into the digest, so bundling would only hide dimensions.
#[allow(clippy::too_many_arguments)]
pub fn approval_state(
    target: &str,
    input: &Value,
    tool_hash: Option<&str>,
    effective_driver: &str,
    resolved_page: &str,
    browser_profile: Option<&str>,
    browser_cdp: Option<&str>,
    browser_session: Option<&str>,
) -> String {
    let canonical = serde_json::to_string(input).unwrap_or_default();
    let hash_part = tool_hash.unwrap_or("");
    crate::store::sha256_hex(
        format!(
            "{target}\n{canonical}\n{hash_part}\n{effective_driver}\n{resolved_page}\n{}\n{}\n{}",
            browser_profile.unwrap_or(""),
            browser_cdp.unwrap_or(""),
            browser_session.unwrap_or(""),
        )
        .as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::same_indexed_page;

    #[test]
    fn query_does_not_swallow_route_fragment() {
        // Same query, different fragment routes: different pages.
        assert!(!same_indexed_page(
            "https://example.com/app?view=compact#/reports",
            "https://example.com/app?view=compact#/settings"
        ));
        // A query mismatch is a different page: an indexed Read grant for
        // one view must not gate a same-named write tool in another view.
        assert!(!same_indexed_page(
            "https://example.com/app?view=compact#/reports",
            "https://example.com/app?view=full#/reports"
        ));
        assert!(!same_indexed_page(
            "https://example.com/app?view=compact",
            "https://example.com/app"
        ));
        // Exact route matches still hold, with and without queries.
        assert!(same_indexed_page(
            "https://example.com/app?view=compact#/reports",
            "https://example.com/app?view=compact#/reports"
        ));
        assert!(same_indexed_page(
            "https://example.com/app#/reports",
            "https://example.com/app#/reports"
        ));
        assert!(same_indexed_page(
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(same_indexed_page(
            "https://example.com",
            "https://example.com/"
        ));
        // Different paths or origins never match.
        assert!(!same_indexed_page(
            "https://example.com/app#/reports",
            "https://example.com/other#/reports"
        ));
        assert!(!same_indexed_page(
            "http://example.com/app#/reports",
            "https://example.com/app#/reports"
        ));
    }
}
