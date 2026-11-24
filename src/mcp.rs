//! MCP server exposing `find`, `inspect`, and `call`.

use std::sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ElicitRequest,
    ElicitRequestParams, ElicitationAction, Implementation, InitializeResult, InputRequest,
    InputRequests, InputRequiredResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::index::Search;

const LIMIT_DEFAULT: usize = 5;
const LIMIT_MAX: usize = 20;

pub const INSTRUCTIONS: &str = "Check whether a website offers WebMCP tools before opening it, \
or search tools by task. `find` searches the public directory and returns the page to open \
and the tool to call there. Use `inspect` to check any live page (unindexed sites, localhost, \
or logged-in dashboards) and retrieve live input schemas without calling anything. Then `call` \
runs the tool through your configured run driver (`agent-browser`, `browse`, or `lightpanda`), \
or returns generic steps when `driver` is `none`.";

/// Which skills root the MCP server reads.
/// `Default` honors `AGENT_WEBMCP_SKILLS_DIR` exactly like the CLI;
/// `Explicit` pins one directory and never reads the process environment.
#[derive(Debug, Clone, Default)]
pub enum SkillsRoot {
    /// Production behavior: `AGENT_WEBMCP_SKILLS_DIR` applies, like `skills list`.
    #[default]
    Default,
    /// Test isolation: exactly this dir, no process-env reads.
    Explicit(std::path::PathBuf),
}

pub struct Server {
    search: Arc<Mutex<Search>>,
    fixed_config: Option<crate::config::Config>,
    /// Skills root for resources/read (production honors env, tests pin).
    skills_root: SkillsRoot,
    /// Explicit custom-tools dir for tests (no `AGENT_WEBMCP_TOOLS_DIR` read).
    tools_dir: Option<std::path::PathBuf>,
    /// Outstanding approval prompts: single-use token -> (deterministic
    /// binding hash of the call it approves, creation time). Each `Ask`
    /// prompt mints a fresh token; `retry_after_approval` removes it
    /// atomically on first use, so a replayed or forged `request_state`
    /// fails closed instead of running the tool a second time. Entries are
    /// only created for prompts that return an `InputRequiredResult` token
    /// to the client; the old elicitation path resolves in-place and never
    /// mints one. Stale entries expire after [`APPROVAL_TTL`] and the map is
    /// capped at [`MAX_PENDING_APPROVALS`] (oldest evicted first).
    pending_approvals: Mutex<std::collections::HashMap<String, (String, std::time::Instant)>>,
}

/// Maximum outstanding approval prompts retained per server.
const MAX_PENDING_APPROVALS: usize = 256;

/// Outstanding approvals older than this are expired on insert/consume.
const APPROVAL_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Process-wide approval-token nonce source: each prompt gets a distinct
/// single-use `request_state`, even for identical calls.
static APPROVAL_NONCE: AtomicU64 = AtomicU64::new(1);

/// Mint a fresh single-use approval token, unique per prompt.
fn fresh_approval_token() -> String {
    let n = APPROVAL_NONCE.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("appr-{nanos:032x}-{n:016x}")
}

/// Drop expired approval entries; called with the map already locked.
fn prune_expired_approvals(
    map: &mut std::collections::HashMap<String, (String, std::time::Instant)>,
) {
    let now = std::time::Instant::now();
    map.retain(|_, (_, at)| now.duration_since(*at) < APPROVAL_TTL);
}

/// Insert a new approval binding, expiring stale entries and evicting the
/// oldest entry when full. Returns the fresh single-use token.
fn issue_approval(
    map: &mut std::collections::HashMap<String, (String, std::time::Instant)>,
    binding: String,
) -> String {
    prune_expired_approvals(map);
    if map.len() >= MAX_PENDING_APPROVALS
        && let Some(oldest) = map
            .iter()
            .min_by_key(|(_, (_, at))| *at)
            .map(|(k, _)| k.clone())
    {
        map.remove(&oldest);
    }
    let token = fresh_approval_token();
    map.insert(token.clone(), (binding, std::time::Instant::now()));
    token
}
impl Server {
    pub fn new(search: Search) -> Self {
        Self {
            search: Arc::new(Mutex::new(search)),
            fixed_config: None,
            skills_root: SkillsRoot::Default,
            tools_dir: None,
            pending_approvals: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Pinned config for tests; the handler uses it instead of loading per call.
    pub fn with_config(search: Search, config: crate::config::Config) -> Self {
        Self {
            search: Arc::new(Mutex::new(search)),
            fixed_config: Some(config),
            skills_root: SkillsRoot::Default,
            tools_dir: None,
            pending_approvals: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Per-fixture skills root; avoids mutating `AGENT_WEBMCP_SKILLS_DIR`.
    pub fn with_skills_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.skills_root = SkillsRoot::Explicit(dir);
        self
    }

    /// Per-fixture custom-tools root; avoids mutating `AGENT_WEBMCP_TOOLS_DIR`.
    pub fn with_tools_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.tools_dir = Some(dir);
        self
    }

    /// The shared search, so a background task can swap in a newer index.
    pub fn shared(&self) -> Arc<Mutex<Search>> {
        self.search.clone()
    }

    /// Check for a newer index every hour, with a little jitter so many
    /// clients do not ask at the same moment. A failure is reported once and
    /// the current index is kept. `AGENT_WEBMCP_INDEX` pins a file, so
    /// nothing is fetched then.
    pub fn spawn_refresh(&self) {
        if std::env::var_os("AGENT_WEBMCP_INDEX").is_some() {
            return;
        }
        let shared = self.shared();
        let jitter = Duration::from_secs(u64::from(std::process::id()) % 300);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60 * 60) + jitter).await;
            loop {
                let fresh = tokio::task::spawn_blocking(crate::store::refresh).await;
                match fresh {
                    Ok(Ok(index)) => match Search::build(index) {
                        Ok(search) => {
                            *shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = search;
                        }
                        Err(e) => eprintln!("agent-webmcp: could not build the new index: {e:#}"),
                    },
                    Ok(Err(e)) => eprintln!("agent-webmcp: keeping the current index ({e:#})"),
                    Err(e) => eprintln!("agent-webmcp: index refresh task failed: {e:#}"),
                }
                tokio::time::sleep(Duration::from_secs(60 * 60)).await;
            }
        });
    }

    async fn call(
        &self,
        request: &CallToolRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> CallToolResponse {
        let args = request.arguments.clone().unwrap_or_default();
        let Some(target) = args.get("target").and_then(Value::as_str) else {
            return complete_error(
                "`target` is required: site/tool from find, or https://page#tool",
            );
        };
        let target = target.to_string();
        let input = args.get("input").cloned().unwrap_or(json!(null));
        let input = if input.is_null() { json!({}) } else { input };
        let confirm = args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let shared = self.shared();
        // Config is cheap; the pipeline below may block on child processes.
        // Tests pin a config so they never depend on the machine's drivers.
        let cfg = match self.fixed_config.clone() {
            Some(cfg) => cfg,
            None => match crate::config::load() {
                Ok(loaded) => loaded.config,
                Err(e) => return complete_error(format!("could not load config: {e:#}")),
            },
        };
        let tools_dir = self.tools_dir.clone();
        let prepared = {
            let search = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match crate::call::prepare_in(
                &target,
                input.clone(),
                &cfg,
                &search,
                tools_dir.as_deref(),
            ) {
                Ok(prepared) => prepared,
                Err(e) => return complete_envelope(e.envelope()),
            }
        };
        if let Some(responses) = request.input_responses.as_ref() {
            return self
                .retry_after_approval(
                    prepared,
                    &target,
                    &input,
                    request.request_state.as_deref(),
                    responses,
                    cfg,
                )
                .await;
        }
        if !matches!(prepared.decision, crate::call::Decision::Ask) {
            return self.run_prepared(prepared, mcp_opts(confirm), cfg, None).await;
        }
        // The model asked for an `ask` tool: with form elicitation the human
        // decides, and the model's `confirm` is ignored entirely.
        if !client_supports_form(context) {
            let (mut envelope, is_error) = self
                .run_prepared_outcome(prepared, mcp_opts(confirm), cfg, None)
                .await;
            if !confirm {
                note_no_elicitation(&mut envelope);
            }
            return complete_envelope_with(envelope, is_error);
        }
        let id = prepared.resolved.id();
        // Resolve the effective driver BEFORE the prompt so the approval
        // is bound to it: a driver change invalidates the retry below.
        let driver = effective_driver_name(&cfg);
        let message = approval_message(&prepared, driver);
        let tool_hash = prepared
            .resolved
            .custom
            .as_ref()
            .map(|c| c.tool_hash.as_str());
        let page = prepared.resolved.page.as_str();
        // Each prompt gets a distinct single-use state, but the token is
        // only minted when the client can return it (`InputRequiredResult`):
        // the old elicitation path resolves in place and never uses a token,
        // so minting one there would leak a map entry on every prompt.
        if negotiated_version(context).is_some_and(|v| v >= ProtocolVersion::V_2026_07_28) {
            let session = crate::call::effective_session(&cfg, &prepared.resolved);
            let binding = crate::call::approval_state(
                &target,
                &input,
                tool_hash,
                driver,
                page,
                prepared.browser_profile.as_deref(),
                prepared.browser_cdp.as_deref(),
                Some(session.as_str()),
            );
            let state = {
                let mut pending = self
                    .pending_approvals
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                issue_approval(&mut pending, binding)
            };
            let mut requests = InputRequests::new();
            requests.insert("approve".to_string(), elicitation_request(&message));
            return InputRequiredResult::new(Some(requests), Some(state)).into();
        }
        let approved =
            crate::call::ApprovedScope::capture_for_resolved(driver, &cfg, &prepared.resolved);
        self.elicit_old(prepared, &target, &input, &message, &id, context, &approved)
            .await
    }

    /// Run a prepared call off the executor; child processes block.
    /// `approved` pins the prompt-time scope (driver + browser context):
    /// execution stops with `needs_confirmation` when the selection differs.
    async fn run_prepared(
        &self,
        prepared: crate::call::Prepared,
        opts: crate::call::CallOptions,
        cfg: crate::config::Config,
        approved: Option<crate::call::ApprovedScope>,
    ) -> CallToolResponse {
        let (envelope, is_error) = self
            .run_prepared_outcome(prepared, opts, cfg, approved)
            .await;
        complete_envelope_with(envelope, is_error)
    }

    /// The blocking half of `run_prepared`, shared with the approval retry.
    /// rmcp carries text and structured content side by side, so results
    /// carry the envelope as text AND as structured content.
    async fn run_prepared_outcome(
        &self,
        prepared: crate::call::Prepared,
        opts: crate::call::CallOptions,
        cfg: crate::config::Config,
        approved: Option<crate::call::ApprovedScope>,
    ) -> (Value, bool) {
        let joined = tokio::task::spawn_blocking(move || {
            crate::call::finish_with_driver(prepared, &opts, &cfg, approved.as_ref())
        })
        .await;
        match joined {
            // Neither tool ran for Blocked / NeedsConfirmation, but their
            // envelopes still say `success: false`: mark them as tool
            // errors like any failure. Ran and Planned stay non-errors.
            Ok(Ok(done)) => {
                let is_error = matches!(
                    done,
                    crate::call::Outcome::Blocked { .. }
                        | crate::call::Outcome::NeedsConfirmation { .. }
                );
                (done.envelope(), is_error)
            }
            Ok(Err(e)) => (e.envelope(), true),
            Err(e) => {
                let envelope = crate::call::CallError {
                    code: "driver_error",
                    message: format!("call task failed: {e}"),
                    data: None,
                }
                .envelope();
                (envelope, true)
            }
        }
    }

    /// A retried `call` carrying the user's answer: verify it, then run.
    /// The approval state is recomputed with the CURRENT effective driver,
    /// tool hash, and resolved page, so a driver change, a one-byte tool
    /// edit, or a re-resolution to a different page between prompt and
    /// retry invalidates the echoed approval and forces a fresh prompt
    /// instead of running under stale terms. A current deny always wins,
    /// even with a matching approval.
    #[allow(clippy::too_many_arguments)]
    async fn retry_after_approval(
        &self,
        _prepared: crate::call::Prepared,
        target: &str,
        input: &Value,
        echoed: Option<&str>,
        responses: &rmcp::model::InputResponses,
        cfg: crate::config::Config,
    ) -> CallToolResponse {
        let _ = cfg;
        // Reload the config and re-resolve against the current index: the
        // user may have taken minutes to answer, during which permissions,
        // the tool code, or the indexed page may have changed. Stale
        // prompt-time values must never authorize the run.
        let fresh_cfg = match self.fixed_config.clone() {
            Some(cfg) => cfg,
            None => match crate::config::load() {
                Ok(loaded) => loaded.config,
                Err(e) => return complete_error(format!("could not load config: {e:#}")),
            },
        };
        let tools_dir = self.tools_dir.clone();
        let fresh = {
            let shared = self.shared();
            let search = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match crate::call::prepare_in(
                target,
                input.clone(),
                &fresh_cfg,
                &search,
                tools_dir.as_deref(),
            ) {
                Ok(prepared) => prepared,
                Err(e) => return complete_envelope(e.envelope()),
            }
        };
        let id = fresh.resolved.id();
        // Current denies stay in force: an approval never overrides them.
        if matches!(fresh.decision, crate::call::Decision::Deny) {
            return complete_envelope(
                crate::call::CallError {
                    code: "denied",
                    message: format!("{id} is denied by config; a prior approval cannot override it"),
                    data: None,
                }
                .envelope(),
            );
        }
        let driver = effective_driver_name(&fresh_cfg);
        let tool_hash = fresh
            .resolved
            .custom
            .as_ref()
            .map(|c| c.tool_hash.as_str());
        let page = fresh.resolved.page.as_str();
        let session = crate::call::effective_session(&fresh_cfg, &fresh.resolved);
        let current = crate::call::approval_state(
            target,
            input,
            tool_hash,
            driver,
            page,
            fresh_cfg.drivers.agent_browser.profile.as_deref(),
            fresh_cfg.drivers.agent_browser.cdp.as_deref(),
            Some(session.as_str()),
        );
        // Single-use approvals: atomically consume the echoed token. A
        // replayed, concurrent duplicate, or forged `request_state` has no
        // entry and fails closed before anything runs; a token bound to
        // different call details fails the binding check below. Expired
        // entries are pruned here too so unanswered prompts cannot
        // accumulate.
        let bound = {
            let mut pending = self
                .pending_approvals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            prune_expired_approvals(&mut pending);
            match echoed.and_then(|s| pending.remove(s)) {
                Some((bound, _)) => bound,
                None => {
                    return complete_envelope(
                        crate::call::CallError {
                            code: "invalid_input",
                            message: "unknown or already-used approval; approve again for a new one"
                                .to_string(),
                            data: None,
                        }
                        .envelope(),
                    );
                }
            }
        };
        if bound != current {
            return complete_envelope(
                crate::call::CallError {
                    code: "invalid_input",
                    message: "the approval does not match this call".to_string(),
                    data: None,
                }
                .envelope(),
            );
        }
        if !is_approved(responses) {
            return complete_envelope(
                crate::call::CallError {
                    code: "denied",
                    message: format!("the user declined {id}"),
                    data: None,
                }
                .envelope(),
            );
        }
        // The user approved; their answer stands in for `confirm: true`.
        // Pin the validated scope (driver + browser context) through
        // execution: a detection or config change between this validation
        // and the execution `choose` stops the run with
        // `needs_confirmation` instead of running an unapproved scope.
        let approved =
            crate::call::ApprovedScope::capture_for_resolved(driver, &fresh_cfg, &fresh.resolved);
        self.run_prepared(
            fresh,
            crate::call::CallOptions {
                confirm: true,
                plan_only: false,
                fresh: false,
                retry_hint: crate::call::RETRY_MCP,
            },
            fresh_cfg,
            Some(approved),
        )
        .await
    }

    /// Old protocols: ask over a server-to-client request, 120 s to answer.
    /// The prompt-time scope (driver + browser context) is pinned through
    /// execution: the user may take the full window to answer, during which
    /// auto-detection can resolve differently or the operator can change the
    /// profile/CDP, and the run must stop instead of running the new scope
    /// unapproved. On accept the call is re-prepared against the current
    /// config and index: a permission, page, or browser-context change in
    /// the meantime invalidates the old approval instead of running under it.
    #[allow(clippy::too_many_arguments)]
    async fn elicit_old(
        &self,
        prepared: crate::call::Prepared,
        target: &str,
        input: &Value,
        message: &str,
        id: &str,
        context: &RequestContext<RoleServer>,
        approved: &crate::call::ApprovedScope,
    ) -> CallToolResponse {
        let params = approval_params(message);
        match context
            .peer
            .create_elicitation_with_timeout(params, Some(std::time::Duration::from_secs(120)))
            .await
        {
            Ok(result)
                if result.action == ElicitationAction::Accept
                    && is_approved_content(&result.content) =>
            {
                // The user may have taken up to 120 s to answer: reload the
                // config and re-resolve against the current index before
                // running anything.
                let fresh_cfg = match self.fixed_config.clone() {
                    Some(cfg) => cfg,
                    None => match crate::config::load() {
                        Ok(loaded) => loaded.config,
                        Err(e) => {
                            return complete_error(format!("could not load config: {e:#}"));
                        }
                    },
                };
                let tools_dir = self.tools_dir.clone();
                let fresh = {
                    let shared = self.shared();
                    let search = shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    match crate::call::prepare_in(
                        target,
                        input.clone(),
                        &fresh_cfg,
                        &search,
                        tools_dir.as_deref(),
                    ) {
                        Ok(prepared) => prepared,
                        Err(e) => return complete_envelope(e.envelope()),
                    }
                };
                // A current deny wins over the old approval.
                if matches!(fresh.decision, crate::call::Decision::Deny) {
                    return complete_envelope(
                        crate::call::CallError {
                            code: "denied",
                            message: format!("{id} is denied by config; the prior approval cannot override it"),
                            data: None,
                        }
                        .envelope(),
                    );
                }
                // The approval scope must be unchanged: same target, input,
                // tool hash, driver, browser context, and resolved page.
                // Anything else (a re-resolved page, a driver flip, a
                // profile/CDP change, a one-byte tool edit) invalidates the
                // accept and forces a fresh prompt.
                let fresh_driver = effective_driver_name(&fresh_cfg);
                let fresh_tool_hash = fresh
                    .resolved
                    .custom
                    .as_ref()
                    .map(|c| c.tool_hash.as_str());
                let current_session =
                    crate::call::effective_session(&fresh_cfg, &fresh.resolved);
                let prompt_state = crate::call::approval_state(
                    target,
                    input,
                    prepared
                        .resolved
                        .custom
                        .as_ref()
                        .map(|c| c.tool_hash.as_str()),
                    &approved.driver,
                    prepared.resolved.page.as_str(),
                    prepared.browser_profile.as_deref(),
                    prepared.browser_cdp.as_deref(),
                    approved.session.as_deref(),
                );
                let current_state = crate::call::approval_state(
                    target,
                    input,
                    fresh_tool_hash,
                    fresh_driver,
                    fresh.resolved.page.as_str(),
                    fresh.browser_profile.as_deref(),
                    fresh.browser_cdp.as_deref(),
                    Some(current_session.as_str()),
                );
                if prompt_state != current_state {
                    return complete_envelope(
                        crate::call::CallError {
                            code: "needs_confirmation",
                            message: "the call changed since the approval prompt; approve again"
                                .to_string(),
                            data: None,
                        }
                        .envelope(),
                    );
                }
                let approved_scope = Some(
                    crate::call::ApprovedScope::capture_for_resolved(
                        fresh_driver,
                        &fresh_cfg,
                        &fresh.resolved,
                    ),
                );
                self.run_prepared(
                    fresh,
                    crate::call::CallOptions {
                        confirm: true,
                        plan_only: false,
                        fresh: false,
                        retry_hint: crate::call::RETRY_MCP,
                    },
                    fresh_cfg,
                    approved_scope,
                )
                .await
            }
            Ok(_) => complete_envelope(
                crate::call::CallError {
                    code: "denied",
                    message: format!("the user declined {id}"),
                    data: None,
                }
                .envelope(),
            ),
            Err(rmcp::service::ServiceError::Timeout { .. }) => complete_envelope(
                crate::call::CallError {
                    code: "needs_confirmation",
                    message: "no answer from the user".to_string(),
                    data: None,
                }
                .envelope(),
            ),
            Err(e) => complete_envelope(
                crate::call::CallError {
                    code: "driver_error",
                    message: format!("could not ask the user: {e}"),
                    data: None,
                }
                .envelope(),
            ),
        }
    }

    /// Search the index, then the local custom catalog. The native phase
    /// runs under the search lock; the custom catalog scan (filesystem reads
    /// plus hashing) runs on the blocking pool after releasing the lock, and
    /// the merge re-takes it only for shadowing checks. A slow tools dir
    /// never stalls call preparation or index refresh.
    async fn find(&self, args: &serde_json::Map<String, Value>) -> CallToolResult {
        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return CallToolResult::error(vec![ContentBlock::text(
                "`query` is required: say what you want to do",
            )]);
        };
        let query = query.to_string();
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| (n as usize).clamp(1, LIMIT_MAX))
            .unwrap_or(LIMIT_DEFAULT);
        let tools_dir = self.tools_dir.clone();
        // Native phase under the lock; capture the merge state and release
        // before touching the filesystem.
        let (native, generation) = {
            let shared = self.shared();
            let search = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let generation = search.generation();
            match search.native_in(&query, limit) {
                Ok(native) => (native, generation),
                Err(e) => {
                    return CallToolResult::error(vec![ContentBlock::text(format!(
                        "search failed: {e:#}"
                    ))]);
                }
            }
        };
        let (native_hits, seen_ids, site, words) = native;
        if words.is_empty() {
            return CallToolResult::structured(json!({
                "matches": [],
                "note": "No indexed site has a tool for this. Use your browser driver as usual."
            }));
        }
        if native_hits.len() >= limit {
            return CallToolResult::structured(json!({ "matches": native_hits }));
        }
        // Filesystem work off the executor thread and outside the lock.
        let loaded = tokio::task::spawn_blocking(move || match tools_dir {
            Some(root) => crate::tools::load_all_valid_in(&root),
            None => crate::tools::load_all_valid(),
        })
        .await
        .unwrap_or_default();
        // Merge under a short second lock (shadowing checks only). If a
        // refresh swapped the index while the catalog loaded, the native
        // hits belong to the old generation while shadowing runs on the
        // new one — so rerun the native phase under this lock and merge
        // against the current generation instead.
        let hits = {
            let shared = self.shared();
            let search = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (native_hits, seen_ids, site, words) = if search.generation() == generation {
                (native_hits, seen_ids, site, words)
            } else {
                match search.native_in(&query, limit) {
                    Ok(native) => native,
                    Err(e) => {
                        return CallToolResult::error(vec![ContentBlock::text(format!(
                            "search failed: {e:#}"
                        ))]);
                    }
                }
            };
            if native_hits.len() >= limit {
                return CallToolResult::structured(json!({ "matches": native_hits }));
            }
            search.merge_custom_hits(native_hits, &words, site.as_deref(), &seen_ids, loaded, limit)
        };
        if hits.is_empty() {
            return CallToolResult::structured(json!({
                "matches": [],
                "note": "No indexed site has a tool for this. Use your browser driver as usual."
            }));
        }
        CallToolResult::structured(json!({ "matches": hits }))
    }
}

/// Derive the MCP `mime_type` from a skill-relative path:
/// `.md` -> `text/markdown`, `.json` -> `application/json`,
/// `.sh` -> `text/x-shellscript`, etc. Returns `None` when unknown so the
/// field is omitted instead of guessing.
fn mime_for_path(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "sh" => "text/x-shellscript",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "yaml" | "yml" => "text/yaml",
        "toml" => "text/toml",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "css" => "text/css",
        _ => return None,
    })
}

/// Every skill and reference as an MCP resource, using the same
/// `crate::skills` API as the CLI. Discovery only: attachment paths are
/// listed without reading bodies.
fn skill_resources_for(root: &SkillsRoot) -> Vec<Resource> {
    match root {
        // Production path: honors `AGENT_WEBMCP_SKILLS_DIR` exactly like
        // `skills list` on the CLI.
        SkillsRoot::Default => {
            let summaries = crate::skills::list_skills();
            let dir = crate::skills::explicit_skills_dir();
            map_skill_summaries(&summaries, dir.as_deref())
        }
        SkillsRoot::Explicit(dir) => skill_resources_with(Some(dir)),
    }
}

/// Same as [`skill_resources_for`] with an injected explicit override dir
/// (no `AGENT_WEBMCP_SKILLS_DIR` read). Pass `None` for a clean bundled view.
fn skill_resources_with(explicit_dir: Option<&std::path::Path>) -> Vec<Resource> {
    let summaries = crate::skills::list_skills_with(explicit_dir);
    map_skill_summaries(&summaries, explicit_dir)
}

/// Map skill summaries (plus their attachment paths, bodies unread) to MCP
/// resources. Shared by the env-aware production path and the isolated
/// test path so both list exactly the same shape.
fn map_skill_summaries(
    summaries: &[crate::skills::SkillSummary],
    explicit_dir: Option<&std::path::Path>,
) -> Vec<Resource> {
    let mut out = Vec::new();
    for summary in summaries {
        // Normalize the reserved `core` alias (any case) to lowercase `core`
        // so `skill://agent-webmcp/Core` lists as `skill://agent-webmcp/core`.
        let canon_name = if summary.name.eq_ignore_ascii_case("core") {
            "core".to_string()
        } else {
            summary.name.clone()
        };
        let uri = format!("skill://agent-webmcp/{canon_name}");
        let mut resource = Resource::new(uri, canon_name.clone()).with_mime_type("text/markdown");
        if let Some(description) = &summary.description {
            resource = resource.with_description(description.clone());
        }
        resource = resource.with_title(format!("{canon_name} skill"));
        out.push(resource);
        // Attachment paths without bodies; a failure lists just the skill itself.
        // Exclude attachments exceeding MAX_SKILL_FILE_BYTES (via resolved
        // file metadata) so `resources/list` never advertises unreadable
        // bodies; `resources/read` keeps the bounded read check and still
        // reports them as I/O.
        let paths =
            crate::skills::attachment_paths_with(&summary.name, explicit_dir).unwrap_or_default();
        for ref_path in paths {
            if crate::skills::attachment_exceeds_limit(&summary.name, &ref_path, explicit_dir) {
                continue;
            }
            let ref_uri = format!("skill://agent-webmcp/{canon_name}/{ref_path}");
            let mut r = Resource::new(ref_uri, format!("{canon_name}/{ref_path}"));
            if let Some(mime) = mime_for_path(&ref_path) {
                r = r.with_mime_type(mime);
            }
            r = r.with_title(format!("{canon_name} {ref_path}"));
            out.push(r);
        }
    }
    out
}

/// Read `skill://agent-webmcp/<name>` and `skill://agent-webmcp/<name>/<ref>`
/// with the exact CLI code path. Returns the canonical URI, text, and MIME
/// (omitted when unknown). `SkillError::Io` is preserved so unreadable
/// skills/overrides surface as internal errors, not "unknown skill".
fn read_skill_for(
    root: &SkillsRoot,
    uri: &str,
) -> Result<(String, String, Option<String>), crate::skills::SkillError> {
    // Isolated path shares the exact shape via `read_skill_uri_with`.
    if let SkillsRoot::Explicit(dir) = root {
        return read_skill_uri_with(uri, Some(dir));
    }
    // Production path: the env-aware CLI functions, honoring
    // `AGENT_WEBMCP_SKILLS_DIR` exactly like `skills get`.
    let (name, ref_part) = crate::skills::parse_resource_uri(uri).ok_or_else(|| {
        crate::skills::SkillError::Invalid(format!("unknown skill resource {uri:?}"))
    })?;
    let canon_name = if name.eq_ignore_ascii_case("core") {
        "core".to_string()
    } else {
        name.clone()
    };
    match ref_part {
        None => {
            let skill = crate::skills::get_skill_shallow(&name)?;
            Ok((
                format!("skill://agent-webmcp/{canon_name}"),
                skill.content,
                Some("text/markdown".to_string()),
            ))
        }
        Some(query) => {
            let hit = crate::skills::get_ref_content(&name, &query)?;
            let mime = mime_for_path(&hit.ref_path).map(str::to_string);
            Ok((
                format!("skill://agent-webmcp/{canon_name}/{}", hit.ref_path),
                hit.content,
                mime,
            ))
        }
    }
}

/// Same as [`read_skill_for`] with an injected explicit override dir (no
/// process-env read). The reserved `core` alias (any case) is normalized to
/// lowercase `"core"` in both the main-body and reference responses.
fn read_skill_uri_with(
    uri: &str,
    explicit_dir: Option<&std::path::Path>,
) -> Result<(String, String, Option<String>), crate::skills::SkillError> {
    let (name, ref_part) = crate::skills::parse_resource_uri(uri).ok_or_else(|| {
        crate::skills::SkillError::Invalid(format!("unknown skill resource {uri:?}"))
    })?;
    let canon_name = if name.eq_ignore_ascii_case("core") {
        "core".to_string()
    } else {
        name.clone()
    };
    match ref_part {
        None => {
            let skill = crate::skills::get_skill_shallow_with(&name, explicit_dir)?;
            Ok((
                format!("skill://agent-webmcp/{canon_name}"),
                skill.content,
                Some("text/markdown".to_string()),
            ))
        }
        Some(query) => {
            let hit = crate::skills::get_ref_content_with(&name, &query, explicit_dir)?;
            let mime = mime_for_path(&hit.ref_path).map(str::to_string);
            Ok((
                format!("skill://agent-webmcp/{canon_name}/{}", hit.ref_path),
                hit.content,
                mime,
            ))
        }
    }
}

fn find_tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "A site (cal.com), a URL, or what you want to do, in plain words. Example: search flights from SFO to JFK." },
            "limit": { "type": "integer", "minimum": 1, "maximum": LIMIT_MAX, "description": "How many matches. Default 5." }
        },
        "required": ["query"],
        "additionalProperties": false
    });
    let Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "find",
        "Check whether a site offers WebMCP tools, or find a tool for a task. Pass a site, a URL, or what \
         you need in plain words. Returns, for each match, the page to open and the tool name and input \
         schema. Open the page with your browser driver, then call the tool through the driver's WebMCP support.",
        Arc::new(schema),
    )
    .annotate(ToolAnnotations::new().read_only(true).open_world(false))
}

impl ServerHandler for Server {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "agent-webmcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(vec![
            find_tool(),
            inspect_tool(),
            call_tool(),
        ]))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        // Synchronous FS I/O must not block the executor; match `inspect`.
        let root = self.skills_root.clone();
        let resources = tokio::task::spawn_blocking(move || skill_resources_for(&root))
            .await
            .map_err(|e| McpError::internal_error(format!("skill listing failed: {e}"), None))?;
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        // Synchronous FS I/O must not block the executor; match `inspect`.
        let uri = request.uri.clone();
        let root = self.skills_root.clone();
        let outcome = tokio::task::spawn_blocking(move || read_skill_for(&root, &uri))
            .await
            .map_err(|e| McpError::internal_error(format!("skill read failed: {e}"), None))?;
        match outcome {
            Ok((uri, text, mime)) => {
                let contents = match mime {
                    Some(m) => ResourceContents::text(text, uri).with_mime_type(m),
                    None => ResourceContents::TextResourceContents {
                        uri,
                        mime_type: None,
                        text,
                        meta: None,
                    },
                };
                Ok(ReadResourceResult::new(vec![contents]).into())
            }
            Err(crate::skills::SkillError::Io(msg)) => Err(McpError::internal_error(msg, None)),
            Err(e) => Err(McpError::invalid_params(
                format!("unknown skill resource {:?}: {e}", request.uri),
                None,
            )),
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        match request.name.as_ref() {
            "find" => Ok(self.find(&request.arguments.unwrap_or_default()).await.into()),
            "inspect" => Ok(self.inspect(&request.arguments.unwrap_or_default()).await),
            "call" => Ok(self.call(&request, &context).await),
            other => Err(McpError::invalid_params(
                format!("unknown tool {other:?}"),
                None,
            )),
        }
    }
}

impl Server {
    async fn inspect(&self, args: &serde_json::Map<String, Value>) -> CallToolResponse {
        let Some(url) = args.get("url").and_then(Value::as_str) else {
            return complete_error("`url` is required: https://page or host to inspect");
        };
        let url = url.to_string();
        let fresh = args.get("fresh").and_then(Value::as_bool).unwrap_or(false);
        let cfg = match self.fixed_config.clone() {
            Some(cfg) => cfg,
            None => match crate::config::load() {
                Ok(loaded) => loaded.config,
                Err(e) => return complete_error(format!("could not load config: {e:#}")),
            },
        };
        let tools_dir = self.tools_dir.clone();
        let opts = crate::inspect::InspectOptions { fresh };
        let res = tokio::task::spawn_blocking(move || {
            crate::inspect::run_inspect_in(&url, &opts, &cfg, tools_dir.as_deref())
        })
        .await;
        match res {
            Ok(Ok(done)) => complete_envelope_with(done.envelope(), false),
            Ok(Err(e)) => complete_envelope(e.envelope()),
            Err(e) => complete_error(format!("inspect task failed: {e}")),
        }
    }
}

/// The `inspect` tool definition, as `list_tools` advertises it.
pub fn inspect_tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "description": "Page URL or host to inspect (e.g. https://cal.com/ or cal.com)" },
            "fresh": { "type": "boolean", "description": "Use a throwaway session or tab, closed afterwards" }
        },
        "required": ["url"],
        "additionalProperties": false
    });
    let Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "inspect",
        "Open a live URL in the browser and list the WebMCP tools it registers (names, descriptions, \
         and live input schemas) without calling any of them.",
        Arc::new(schema),
    )
    .annotate(ToolAnnotations::new().read_only(true).open_world(true))
}

/// The `call` tool definition, as `list_tools` advertises it.
pub fn call_tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "target": { "type": "string", "description": "site/tool from find, or https://page#tool" },
            "input": { "type": "object", "description": "Tool input, matching the input_schema from find" },
            "confirm": { "type": "boolean", "description": "Set true to approve a tool that needs confirmation" }
        },
        "required": ["target"],
        "additionalProperties": false
    });
    let Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "call",
        "Call a WebMCP tool: if agent-webmcp can drive a browser (agent-browser, browse, or lightpanda) it returns \
         the tool's output, otherwise it returns generic steps when `driver` is `none`. \
         When the client supports it, agent-webmcp asks the user itself before calls that \
         need approval; `confirm` only applies to clients that cannot ask.",
        Arc::new(schema),
    )
    .annotate(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .open_world(true),
    )
}

/// `confirm: true` only matters where the client cannot ask the user.
fn mcp_opts(confirm: bool) -> crate::call::CallOptions {
    crate::call::CallOptions {
        confirm,
        plan_only: false,
        fresh: false,
        retry_hint: crate::call::RETRY_MCP,
    }
}

/// A failure envelope as a complete tool result.
fn complete_error(message: impl Into<String>) -> CallToolResponse {
    complete_envelope(
        crate::call::CallError {
            code: "driver_error",
            message: message.into(),
            data: None,
        }
        .envelope(),
    )
}

/// An envelope as a complete tool result.
fn complete_envelope(envelope: Value) -> CallToolResponse {
    complete_envelope_with(envelope, true)
}

/// An envelope as a complete tool result, with an explicit error flag.
fn complete_envelope_with(envelope: Value, is_error: bool) -> CallToolResponse {
    let text = serde_json::to_string_pretty(&envelope).unwrap_or_default();
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(envelope);
    result.is_error = Some(is_error);
    result.into()
}

/// Whether the client renders a form the user can answer.
fn client_supports_form(context: &RequestContext<RoleServer>) -> bool {
    // New-protocol requests carry capabilities per call, but may arrive
    // empty; the handshake stays authoritative for what it names.
    let from_meta = context.client_capabilities().and_then(|c| c.elicitation);
    let from_peer = context
        .peer
        .peer_info()
        .and_then(|info| info.capabilities.elicitation.clone());
    [from_meta, from_peer].into_iter().flatten().any(|e| {
        // No mode listed means form, matching the SDK's own fallback.
        e.form.is_some() || e.url.is_none()
    })
}

/// Negotiated protocol version from request meta or the handshake.
fn negotiated_version(context: &RequestContext<RoleServer>) -> Option<ProtocolVersion> {
    context.protocol_version().or_else(|| {
        context
            .peer
            .peer_info()
            .map(|info| info.protocol_version.clone())
    })
}

/// The driver that will actually run the call: the configured driver, or
/// whatever `auto` resolves to. Approval text and approval state bind to
/// this (not the raw config value) so a driver change between prompt and
/// retry invalidates the approval and warns correctly.
fn effective_driver_name(cfg: &crate::config::Config) -> &'static str {
    match crate::driver::choose(cfg) {
        crate::driver::Choice::Run(driver, _) => driver.id(),
        crate::driver::Choice::Plan(driver) => driver.id(),
        crate::driver::Choice::Unusable { driver, .. } => driver,
    }
}

/// The question the user answers, with the call it approves.
fn approval_message(prepared: &crate::call::Prepared, effective_driver: &str) -> String {
    let id = prepared.resolved.id();
    let mut input = serde_json::to_string(&prepared.input).unwrap_or_default();
    if input.chars().count() > 300 {
        input = input.chars().take(300).collect();
    }
    if let Some(custom) = &prepared.resolved.custom {
        let short: String = custom.tool_hash.chars().take(12).collect();
        let mut msg = format!(
            "Allow agent-webmcp to call [custom] {id} on {}? It is {}; sha {short}; input: {input}",
            prepared.resolved.page,
            crate::call::kind_str(prepared.resolved.kind),
        );
        // Warn on the effective driver: `auto` may resolve to Browse too.
        if effective_driver == "browse" {
            msg.push_str(" (warning: running in personal Chrome (browse))");
        }
        return msg;
    }
    format!(
        "Allow agent-webmcp to call {id} on {}? It is {}; input: {input}",
        prepared.resolved.page,
        crate::call::kind_str(prepared.resolved.kind),
    )
}

/// Human approval form: one boolean, required.
fn approval_params(message: &str) -> ElicitRequestParams {
    let requested_schema = serde_json::from_value(json!({
        "type": "object",
        "properties": {"approve": {"type": "boolean", "title": "Approve this call"}},
        "required": ["approve"]
    }))
    .expect("approval schema is fixed");
    ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: message.to_string(),
        requested_schema,
    }
}

/// The `approve` elicitation request, keyed `"approve"` per the MRTR pattern.
fn elicitation_request(message: &str) -> InputRequest {
    InputRequest::Elicitation(ElicitRequest::new(approval_params(message)))
}

/// Approval only counts from the user's answer: accept plus approve true.
fn is_approved(responses: &rmcp::model::InputResponses) -> bool {
    responses
        .get("approve")
        .filter(|r| r.get("action").and_then(Value::as_str) == Some("accept"))
        .and_then(|r| {
            r.get("content")
                .and_then(|c| c.get("approve"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

/// Same check for a direct elicitation result on old protocols.
fn is_approved_content(content: &Option<Value>) -> bool {
    content
        .as_ref()
        .and_then(|c| c.get("approve"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Clients without elicitation get a hint to configure approval themselves.
fn note_no_elicitation(envelope: &mut Value) {
    envelope["data"]["note"] = json!(
        "this client cannot ask you directly; set the agent-webmcp call tool to ask in your agent's permission settings"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn poisoned_lock_still_answers_find() {
        let index = crate::model::Index {
            schema_version: 1,
            generated_at: String::new(),
            sites: vec![],
        };
        let search = Search::build(index).unwrap();
        let server = Server::new(search);
        let shared = server.shared();
        let _ = std::thread::spawn(move || {
            let _guard = shared.lock().unwrap();
            panic!("poison the search lock");
        })
        .join();
        let mut args = serde_json::Map::new();
        args.insert("query".to_string(), json!("cal.com"));
        let _ = server.find(&args).await;
    }

    #[test]
    fn mime_derived_from_extension() {
        assert_eq!(
            mime_for_path("references/drivers.md"),
            Some("text/markdown")
        );
        assert_eq!(
            mime_for_path("templates/scaffold.json"),
            Some("application/json")
        );
        assert_eq!(
            mime_for_path("templates/run.sh"),
            Some("text/x-shellscript")
        );
        assert_eq!(mime_for_path("notes.txt"), Some("text/plain"));
        assert_eq!(mime_for_path("page.html"), Some("text/html"));
        assert_eq!(mime_for_path("config.yaml"), Some("text/yaml"));
        assert_eq!(mime_for_path("SKILL.md"), Some("text/markdown"));
        assert_eq!(mime_for_path("blob.bin"), None);
        assert_eq!(mime_for_path("noext"), None);
    }

    #[test]
    fn read_skill_uri_preserves_io() {
        // Isolated from `AGENT_WEBMCP_SKILLS_DIR` overrides: inject no override
        // rather than reading process env, so a parallel test's override (or a
        // developer's local `core`) cannot flip bundled results to local I/O.
        let no_override: Option<&std::path::Path> = None;
        // Unknown skill -> Invalid (mapped to invalid_params downstream).
        let err = read_skill_uri_with("skill://agent-webmcp/does-not-exist-xyz", no_override)
            .unwrap_err();
        assert!(matches!(
            err,
            crate::skills::SkillError::UnknownSkill(_) | crate::skills::SkillError::Invalid(_)
        ));

        // Bundled core body works without I/O.
        let (uri, text, mime) =
            read_skill_uri_with("skill://agent-webmcp/core", no_override).unwrap();
        assert_eq!(uri, "skill://agent-webmcp/core");
        assert!(text.contains("agent-webmcp"));
        assert_eq!(mime.as_deref(), Some("text/markdown"));

        // Reserved alias normalizes to lowercase `core`.
        let (uri, _, _) = read_skill_uri_with("skill://agent-webmcp/Core", no_override).unwrap();
        assert_eq!(uri, "skill://agent-webmcp/core");

        // Bundled ref carries markdown MIME (alias also normalizes).
        let (uri, text, mime) =
            read_skill_uri_with("skill://agent-webmcp/core/drivers", no_override).unwrap();
        assert_eq!(uri, "skill://agent-webmcp/core/references/drivers.md");
        assert!(text.contains("Driver matrix") || text.contains("Driver"));
        assert_eq!(mime.as_deref(), Some("text/markdown"));

        let (uri, _, _) =
            read_skill_uri_with("skill://agent-webmcp/Core/drivers", no_override).unwrap();
        assert_eq!(uri, "skill://agent-webmcp/core/references/drivers.md");
    }

    #[test]
    fn resources_list_excludes_oversized_but_read_errors() {
        // `resources/list` uses injected dirs (no process-env read) so this
        // stays isolated from parallel tests mutating `AGENT_WEBMCP_SKILLS_DIR`.
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("bigres");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: bigres\ndescription: Big.\n---\n\n# big\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
        let big = vec![b'a'; (crate::skills::MAX_SKILL_FILE_BYTES + 10) as usize];
        std::fs::write(skill_root.join("references/big.md"), &big).unwrap();

        let resources = skill_resources_with(Some(dir.path()));
        let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(
            uris.contains(&"skill://agent-webmcp/bigres/references/ok.md"),
            "{uris:?}"
        );
        assert!(
            !uris.contains(&"skill://agent-webmcp/bigres/references/big.md"),
            "oversized must be excluded from resources/list, got {uris:?}"
        );

        // Explicit read keeps the bounded check.
        let err = read_skill_uri_with(
            "skill://agent-webmcp/bigres/references/big.md",
            Some(dir.path()),
        )
        .unwrap_err();
        assert!(
            matches!(err, crate::skills::SkillError::Io(_)),
            "oversized read must be Io, got {err:?}"
        );
    }
}
