//! `agent-webmcp inspect`: open a live URL and list the WebMCP tools it registers.

use serde_json::{Value, json};

use crate::call::{CallError, ToolKind, example_input, inferred_read, no_driver};
use crate::config::Config;
use crate::driver::{
    self, Choice, InspectOutcome as DriverInspectOutcome, InspectedTool, RunOptions,
};

/// Options for `run_inspect`.
#[derive(Debug, Clone, Default)]
pub struct InspectOptions {
    pub fresh: bool,
}

/// Successful `inspect` output ready for human or JSON formatting.
#[derive(Debug, Clone)]
pub struct InspectResult {
    pub driver: &'static str,
    pub page: String,
    pub tools: Vec<InspectedTool>,
    pub duration_ms: u64,
    pub reused: bool,
    pub budget_ms: u64,
}

impl InspectResult {
    fn from_driver(driver: &'static str, out: DriverInspectOutcome) -> Self {
        Self {
            driver,
            page: out.page,
            tools: out.tools,
            duration_ms: out.duration_ms,
            reused: out.reused,
            budget_ms: out.budget_ms,
        }
    }

    pub fn envelope(&self) -> Value {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|t| {
                let (kind, kind_inferred) = tool_kind(t);
                let kind_str = match kind {
                    ToolKind::Read => "read",
                    ToolKind::Consequential => "consequential",
                    ToolKind::Unspecified => "unspecified",
                };
                let mut obj = json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                    "kind": kind_str,
                    "kind_inferred": kind_inferred,
                    "call": {
                        "target": format!("{}#{}", self.page, t.name),
                        "input": example_input(&t.input_schema),
                    }
                });
                if let Some(s) = &t.source {
                    obj["source"] = Value::String(s.clone());
                }
                if let Some(a) = t.available {
                    obj["available"] = Value::Bool(a);
                }
                if let Some(r) = &t.unavailable_reason {
                    obj["unavailableReason"] = Value::String(r.clone());
                }
                if let Some(r) = &t.remediation {
                    obj["remediation"] = Value::String(r.clone());
                }
                if let Some(h) = t.headed {
                    obj["headed"] = Value::Bool(h);
                }
                if let Some(a) = &t.auth {
                    obj["auth"] = Value::String(a.clone());
                }
                if let Some(b) = t.allow_browse {
                    obj["allowBrowse"] = Value::Bool(b);
                }
                if let Some(h) = &t.tool_hash {
                    obj["toolHash"] = Value::String(h.clone());
                }
                obj
            })
            .collect();
        json!({
            "success": true,
            "data": {
                "driver": self.driver,
                "page": self.page,
                "durationMs": self.duration_ms,
                "reused": self.reused,
                "budgetMs": self.budget_ms,
                "tools": tools,
            }
        })
    }
}

/// Normalize a user-supplied URL or bare host (`cal.com` -> `https://cal.com/`).
pub fn normalize_url(raw: &str) -> Result<String, CallError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CallError::usage("expected a URL or host to inspect"));
    }
    if trimmed.contains(char::is_whitespace) {
        return Err(CallError::usage(format!("invalid URL {raw:?}")));
    }
    let host_prefix = trimmed.split(['/', '?', '#']).next().unwrap_or("");
    let has_scheme = host_prefix.ends_with(':')
        && trimmed
            .get(host_prefix.len()..)
            .is_some_and(|rest| rest.starts_with("//"));
    let with_scheme = if has_scheme {
        trimmed.to_string()
    } else {
        if let Some((_, after_colon)) = host_prefix.split_once(':')
            && !after_colon.chars().all(|c| c.is_ascii_digit())
        {
            return Err(CallError::usage(format!(
                "unsupported URL scheme in {raw:?}; expected http or https"
            )));
        }
        let host_only = host_prefix.split(':').next().unwrap_or("");
        if host_only.eq_ignore_ascii_case("localhost") {
            format!("http://{trimmed}")
        } else {
            format!("https://{trimmed}")
        }
    };
    let Some((scheme, rest)) = with_scheme.split_once("://") else {
        return Err(CallError::usage(format!("invalid URL {raw:?}")));
    };
    if scheme != "http" && scheme != "https" {
        return Err(CallError::usage(format!(
            "unsupported URL scheme {scheme:?}; expected http or https"
        )));
    }
    let before_query = rest.split(['?', '#']).next().unwrap_or("");
    let host_part = before_query.split('/').next().unwrap_or("");
    if host_part.is_empty() {
        return Err(CallError::usage(format!("missing host in URL {raw:?}")));
    }
    if !before_query.contains('/') {
        let split_at = scheme.len() + 3 + before_query.len();
        let (head, tail) = with_scheme.split_at(split_at);
        Ok(format!("{head}/{tail}"))
    } else {
        Ok(with_scheme)
    }
}

/// Open `raw_url` with the configured run driver and list registered WebMCP tools.
pub fn run_inspect(
    raw_url: &str,
    opts: &InspectOptions,
    cfg: &Config,
) -> Result<InspectResult, CallError> {
    run_inspect_in(raw_url, opts, cfg, None)
}

/// [`run_inspect`] with an explicit custom-tools root (tests pass `Some`;
/// the CLI passes `None` to read the process environment).
pub fn run_inspect_in(
    raw_url: &str,
    opts: &InspectOptions,
    cfg: &Config,
    tools_dir: Option<&std::path::Path>,
) -> Result<InspectResult, CallError> {
    let url = normalize_url(raw_url)?;
    let custom = load_custom_for_url(&url, tools_dir);
    // A tool denied for `call` must never have its `tools.js` evaluated
    // during `inspect`: the availability probe inlines the full script into
    // the page before calling `available()`, and top-level statements run
    // even though `execute()` is never called. All tools from one host
    // catalog share a single `tools.js` that cannot be split per-tool, so
    // when ANY tool from the host is denied, the whole host is withheld:
    // no tool from it reaches `driver.inspect`, and every tool from it is
    // reported unavailable without any browser evaluation. Ask-gated tools
    // are still probed: the explicit `inspect` invocation is the user's
    // approval for the availability probe (`execute()` never runs), and
    // withholding everything without an explicit allow would make `inspect`
    // useless for local custom tools.
    let host_refused = custom
        .iter()
        .any(|c| custom_denied_by_permissions(c, &cfg.permissions));
    let (custom_allowed, custom_denied): (Vec<_>, Vec<_>) = if host_refused {
        (Vec::new(), custom)
    } else {
        (custom, Vec::new())
    };
    match driver::choose(cfg) {
        Choice::Run(driver, _) => {
            // Never send a persistent browser session over cleartext HTTP:
            // the browser would transmit its cookies where a network
            // observer can read them. `browse` always runs in the user's
            // personal (persistent) profile. `agent-browser` reuses the
            // named session unless `fresh` allocates a throwaway one — but
            // a configured `profile` or `cdp` endpoint still carries
            // persistent state even into fresh sessions, so those refuse
            // too. `lightpanda` starts clean on every run and fresh
            // profile-free agent-browser sessions carry no prior cookies,
            // so plain HTTP stays allowed there.
            let persistent = match driver.id() {
                "browse" => true,
                "agent-browser" => {
                    !opts.fresh
                        || cfg.drivers.agent_browser.profile.is_some()
                        || cfg.drivers.agent_browser.cdp.is_some()
                }
                _ => false,
            };
            if url_scheme_is_http(&url) && persistent {
                return Err(CallError {
                    code: "denied",
                    message: format!(
                        "refusing to inspect {url} over http:// in the persistent {} session; use an https:// URL or a clean --fresh session without --profile/--cdp",
                        driver.id()
                    ),
                    data: None,
                });
            }
            let out = match driver.inspect(
                &url,
                &custom_allowed,
                &RunOptions {
                    timeout_ms: cfg.call.timeout,
                    settle_ms: cfg.call.settle_ms.min(30_000),
                    fresh: opts.fresh,
                },
            ) {
                Ok(out) => out,
                // No native API and nothing allowed to probe: still report
                // the withheld tools instead of failing outright, so a deny
                // rule is visible even on pages without WebMCP support.
                Err(e)
                    if e.code == "webmcp_unsupported" && !custom_denied.is_empty() =>
                {
                    DriverInspectOutcome {
                        page: url.clone(),
                        tools: Vec::new(),
                        duration_ms: 0,
                        reused: false,
                        budget_ms: cfg.call.timeout.saturating_add(30_000),
                    }
                }
                Err(e) => return Err(e),
            };
            let mut result = InspectResult::from_driver(driver.id(), out);
            // Withheld tools were never probed (their shared script never
            // reached the page): report them as unavailable with the reason,
            // unless a live native tool of the same name shadows them —
            // the native entry already advertises that call target.
            for c in &custom_denied {
                if result.tools.iter().any(|t| t.name == c.name) {
                    continue;
                }
                let reason = if custom_denied_by_permissions(c, &cfg.permissions) {
                    "denied by permissions"
                } else {
                    "shared tools.js contains a tool denied by permissions"
                };
                result.tools.push(driver::custom_inspected(
                    c,
                    false,
                    Some(reason.to_string()),
                    None,
                ));
            }
            Ok(result)
        }
        Choice::Plan(_) | Choice::Unusable { .. } => Err(no_driver(cfg)),
    }
}

/// Whether `url` uses plain HTTP.
fn url_scheme_is_http(url: &str) -> bool {
    url.split_once("://")
        .map(|(scheme, _)| scheme.eq_ignore_ascii_case("http"))
        .unwrap_or(false)
}

/// Local custom tools for the normalized host of `url`, if any. Best-effort:
/// missing dirs, broken hosts, or unreadable roots mean no custom tools.
/// The tools share one `Arc` of `tools.js` source instead of cloning the
/// (up to 512 KiB) script per tool definition.
///
/// Selection is by exact catalog host, explicit ports included: inspecting
/// `https://example.com:8443/` never loads tools catalogued under
/// `example.com` (those are skipped, not evaluated). Each loaded tool is
/// then bound to the origin of its *declared* page (the manifest default
/// or the tool's own override, including scheme and port) — never to the
/// caller-supplied inspect URL. Availability probes compare the live page
/// origin against that declared origin and report `origin_mismatch`
/// without evaluating the script when they differ (e.g. inspecting
/// `http://example.com/` for a tool declared on `https://example.com/app`),
/// so a custom script only ever runs on the origin its manifest declares.
fn load_custom_for_url(
    url: &str,
    tools_dir: Option<&std::path::Path>,
) -> Vec<crate::driver::CustomInspect> {
    let Some(after) = url.split_once("://").map(|(_, r)| r) else {
        return Vec::new();
    };
    let authority = after.split(['/', '?', '#']).next().unwrap_or_default();
    let hostport = authority.rsplit('@').next().unwrap_or_default();
    if hostport.is_empty() || hostport.contains('[') {
        return Vec::new();
    }
    let normalized = match crate::tools::normalize_host(hostport) {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    let loaded = match tools_dir {
        Some(root) => crate::tools::load_host_in(root, &normalized),
        None => crate::tools::load_host(&normalized),
    };
    let loaded = match loaded {
        Ok(Some(l)) => l,
        _ => return Vec::new(),
    };
    let js: std::sync::Arc<str> = loaded.js_source.as_str().into();
    loaded
        .manifest
        .tools
        .iter()
        .filter_map(|def| {
            // Declared page origin, not the inspect URL: a tool the manifest
            // places on another origin must not evaluate here.
            let page = def.effective_page(&loaded.manifest);
            let tool_origin = crate::tools::expected_origin(page).ok()?;
            Some(crate::driver::CustomInspect {
                name: def.name.clone(),
                host: loaded.host.clone(),
                common: crate::driver::CustomCommon {
                    expected_origin: tool_origin,
                    js_source: js.clone(),
                    tool_hash: loaded.sha256.clone(),
                    headed: def.headed,
                    allow_browse: def.allow_browse,
                    input_schema: def.input_schema.clone(),
                },
                auth: def.auth,
                description: def.description.clone(),
            })
        })
        .collect()
}

/// Whether a custom inspect candidate is denied by `cfg.permissions`,
/// mirroring the deny clause of `call::decide_for_resolved` (deny beats
/// everything, independent of tool kind): deny matches against the
/// port-aware catalog host (`<host>/<tool>` and `custom/<host>/<tool>`),
/// exactly the identities `call` matches, so an explicit-port deny rule
/// withholds the shared `tools.js` too.
fn custom_denied_by_permissions(
    c: &crate::driver::CustomInspect,
    perms: &crate::config::Permissions,
) -> bool {
    let id = format!("{}/{}", c.host, c.name);
    let custom_id = format!("custom/{}/{}", c.host, c.name);
    perms
        .deny
        .iter()
        .any(|p| crate::call::glob_matches(p, &id) || crate::call::glob_matches(p, &custom_id))
}

/// Print human-readable `inspect` output.
pub fn print_human(res: &InspectResult) {
    let page = crate::text::clean(&res.page);
    if res.tools.is_empty() {
        println!(
            "no WebMCP tools registered on {page} (via {}, {} ms)",
            res.driver, res.duration_ms
        );
        return;
    }
    println!(
        "{page} ({} {}, via {}, {} ms)",
        res.tools.len(),
        if res.tools.len() == 1 {
            "tool"
        } else {
            "tools"
        },
        res.driver,
        res.duration_ms
    );
    for tool in &res.tools {
        let (kind, inferred) = tool_kind(tool);
        let mut tag = match (kind, inferred) {
            (ToolKind::Read, true) => "[read, inferred]".to_string(),
            (ToolKind::Read, false) => "[read]".to_string(),
            (ToolKind::Consequential, _) => "[consequential]".to_string(),
            (ToolKind::Unspecified, _) => "[unspecified]".to_string(),
        };
        if tool.source.as_deref() == Some("custom") {
            tag.push_str("  [custom]");
        }
        if tool.available == Some(false) {
            let reason = tool.unavailable_reason.as_deref().unwrap_or("unavailable");
            tag.push_str(&format!("  [unavailable: {}]", crate::text::clean(reason)));
        }
        if tool.headed == Some(true) {
            tag.push_str("  [headed]");
        }
        if tool.auth.as_deref() == Some("session") {
            tag.push_str("  [auth:session]");
        }
        let name = crate::text::clean(&tool.name);
        println!("{name}  {tag}");
        if let Some(desc) = &tool.description {
            println!("    {}", crate::text::clean(desc));
        }
        if let Some(rem) = &tool.remediation {
            println!("    {}", crate::text::clean(rem));
        }
        let ex = serde_json::to_string(&example_input(&tool.input_schema))
            .unwrap_or_else(|_| "{}".to_string());
        println!(
            "    call agent-webmcp call {page}#{name} '{}'",
            crate::text::clean(&ex)
        );
    }
}

fn tool_kind(tool: &InspectedTool) -> (ToolKind, bool) {
    match tool.read_only {
        Some(true) => (ToolKind::Read, false),
        Some(false) => (ToolKind::Consequential, false),
        None if inferred_read(&tool.name) => (ToolKind::Read, true),
        None => (ToolKind::Unspecified, false),
    }
}
