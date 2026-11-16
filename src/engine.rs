use std::fmt;
use std::time::Duration;

use anyhow::anyhow;
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::approval::{Approver, Decision, Request};
use crate::backend::{Backend, PageTool};
use crate::catalog::{Catalog, Entry, Source};
use crate::ledger::{Ledger, Outcome as Verdict, Record};
use crate::policy::{self, Reason};
use crate::validate::validate_args;

#[derive(Debug, Clone)]
pub struct Options {
    /// How many times to look for the tool after the page opens.
    pub poll_attempts: u32,
    pub poll_delay: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self { poll_attempts: 20, poll_delay: Duration::from_millis(250) }
    }
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub id: String,
    pub origin: String,
    pub output: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug)]
pub enum CallError {
    UnknownTool(String),
    BadArguments { id: String, reason: String },
    /// The tool needs approval and there is nobody to ask.
    ApprovalUnavailable { id: String },
    /// Approval was asked for, but there was no usable answer.
    ApprovalUnanswered { id: String },
    /// The user was asked and said no.
    Declined { id: String },
    Failed(anyhow::Error),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTool(id) => write!(f, "unknown tool {id:?}; use `find` to see tool ids"),
            Self::BadArguments { id, reason } => write!(f, "arguments for {id} are invalid: {reason}"),
            Self::ApprovalUnavailable { id } => write!(
                f,
                "{id} needs the user's approval, and there is no way to ask them here. Nothing was run."
            ),
            Self::ApprovalUnanswered { id } => write!(
                f,
                "asked for approval to run {id}, but there was no answer. Nothing was run."
            ),
            Self::Declined { id } => write!(f, "the user declined {id}. Nothing was run."),
            Self::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for CallError {}

impl From<anyhow::Error> for CallError {
    fn from(e: anyhow::Error) -> Self {
        Self::Failed(e)
    }
}

/// Run a catalog tool: check policy, open its page, add the community script if there is
/// one, find the page's tool, and call it. Every call, including refused ones, is
/// recorded in the ledger.
pub async fn call<B: Backend, A: Approver>(
    backend: &B,
    approver: &A,
    ledger: &Ledger,
    catalog: &Catalog,
    id: &str,
    args: Value,
    opts: &Options,
) -> Result<Outcome, CallError> {
    let mut approved = false;
    let result = run(backend, approver, catalog, id, &args, opts, &mut approved).await;

    let (outcome, origin, duration_ms, error) = match &result {
        Ok(o) => (Verdict::Ok, Some(o.origin.clone()), o.duration_ms, None),
        Err(CallError::ApprovalUnavailable { .. }) => (Verdict::ApprovalRequired, None, None, None),
        Err(CallError::ApprovalUnanswered { .. }) => (Verdict::Unanswered, None, None, None),
        Err(CallError::Declined { .. }) => (Verdict::Declined, None, None, None),
        Err(e) => (Verdict::Error, None, None, Some(e.to_string())),
    };
    let mut record = Record::new(id, outcome, approved, &args);
    record.origin = origin;
    record.duration_ms = duration_ms;
    record.error = error;
    if let Err(e) = ledger.append(&record) {
        eprintln!("agent-webmcp: could not write the ledger at {}: {e:#}", ledger.path().display());
    }
    result
}

async fn run<B: Backend, A: Approver>(
    backend: &B,
    approver: &A,
    catalog: &Catalog,
    id: &str,
    args: &Value,
    opts: &Options,
    approved: &mut bool,
) -> Result<Outcome, CallError> {
    let entry = catalog.get(id).ok_or_else(|| CallError::UnknownTool(id.to_string()))?;
    if let Err(reason) = validate_args(&entry.input_schema, args) {
        return Err(CallError::BadArguments { id: id.to_string(), reason });
    }

    // The catalog's own verdict comes first, before any browser is touched.
    if let Some(reason) = policy::approval_needed(entry.kind, false) {
        ask(approver, entry, args, reason).await?;
        *approved = true;
    }

    let tab = backend.open(&entry.url).await?;
    if entry.source == Source::Community {
        let script = catalog
            .script(entry)
            .ok_or_else(|| anyhow!("script for {id} is missing from the catalog"))?;
        backend.inject(&tab, script).await?;
    }

    let tool = find_page_tool(backend, &tab, entry, opts).await?;

    // The page's verdict comes second. It can only add a question, never remove one.
    if !*approved
        && let Some(reason) = policy::approval_needed(entry.kind, tool.consequential)
    {
        ask(approver, entry, args, reason).await?;
        *approved = true;
    }

    let inv = backend.invoke(&tab, &tool, args.clone()).await?;
    Ok(Outcome {
        id: entry.id.clone(),
        origin: inv.origin.or(tool.origin).unwrap_or_else(|| entry.url.clone()),
        output: inv.output,
        duration_ms: inv.duration_ms,
    })
}

async fn ask<A: Approver>(approver: &A, entry: &Entry, args: &Value, reason: Reason) -> Result<(), CallError> {
    let request = Request {
        id: &entry.id,
        site: &entry.site,
        description: &entry.description,
        args,
        reason,
    };
    match approver.ask(&request).await {
        Decision::Granted => Ok(()),
        Decision::Declined => Err(CallError::Declined { id: entry.id.clone() }),
        Decision::Unavailable => Err(CallError::ApprovalUnavailable { id: entry.id.clone() }),
        Decision::Unanswered => Err(CallError::ApprovalUnanswered { id: entry.id.clone() }),
    }
}

async fn find_page_tool<B: Backend>(
    backend: &B,
    tab: &crate::backend::Tab,
    entry: &Entry,
    opts: &Options,
) -> Result<PageTool, CallError> {
    let mut seen: Vec<String> = Vec::new();
    let mut elsewhere = false;
    for attempt in 0..opts.poll_attempts.max(1) {
        if attempt > 0 {
            tokio::time::sleep(opts.poll_delay).await;
        }
        let tools = backend.list_tools(tab).await?;
        seen = tools.iter().map(|t| t.name.clone()).collect();
        elsewhere = tools
            .iter()
            .any(|t| t.name == entry.name && !same_site(t.origin.as_deref(), &entry.url));
        if let Some(t) = tools
            .into_iter()
            .find(|t| t.name == entry.name && same_site(t.origin.as_deref(), &entry.url))
        {
            return Ok(t);
        }
    }
    Err(CallError::Failed(if elsewhere {
        anyhow!(
            "{} is registered, but by a different origin than {}; not calling it",
            entry.name,
            entry.url
        )
    } else {
        anyhow!(
            "{} is not registered on {} (the page has: {})",
            entry.name,
            entry.url,
            if seen.is_empty() { "no tools".to_string() } else { seen.join(", ") }
        )
    }))
}

/// Same scheme, host and port. A leading `www.` is ignored, since sites redirect to it.
/// An unknown origin is accepted.
fn same_site(origin: Option<&str>, entry_url: &str) -> bool {
    let Some(origin) = origin else { return true };
    let (Some(a), Some(b)) = (key(origin), key(entry_url)) else {
        return false;
    };
    a == b
}

fn key(url: &str) -> Option<(String, String, Option<u16>)> {
    let u = Url::parse(url).ok()?;
    let host = u.host_str()?.trim_start_matches("www.").to_ascii_lowercase();
    Some((u.scheme().to_string(), host, u.port_or_known_default()))
}
