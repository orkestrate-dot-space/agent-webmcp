use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::backend::{Backend, PageTool};
use crate::catalog::{Catalog, Kind, Source};

#[derive(Debug, Clone)]
pub struct Options {
    /// The user approved tools that change things on the site.
    pub approved: bool,
    /// How many times to look for the tool after the page opens.
    pub poll_attempts: u32,
    pub poll_delay: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            approved: false,
            poll_attempts: 20,
            poll_delay: Duration::from_millis(250),
        }
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

/// Run a catalog tool: open its page, add the community script if there is one, find the
/// page's tool, and call it.
pub async fn call<B: Backend>(
    backend: &B,
    catalog: &Catalog,
    id: &str,
    args: Value,
    opts: &Options,
) -> Result<Outcome> {
    let entry = catalog
        .get(id)
        .ok_or_else(|| anyhow!("unknown tool {id:?}; run `find` to see tool ids"))?;
    if !args.is_object() {
        bail!("arguments must be a JSON object");
    }
    if entry.kind == Kind::Write && !opts.approved {
        bail!("{id} changes things on the site and needs approval");
    }

    let tab = backend.open(&entry.url).await?;
    if entry.source == Source::Community {
        let script = catalog
            .script(entry)
            .ok_or_else(|| anyhow!("script for {id} is missing from the catalog"))?;
        backend.inject(&tab, script).await?;
    }

    let mut seen: Vec<String> = Vec::new();
    let mut elsewhere = false;
    let mut found = None;
    for attempt in 0..opts.poll_attempts.max(1) {
        if attempt > 0 {
            tokio::time::sleep(opts.poll_delay).await;
        }
        let tools = backend.list_tools(&tab).await?;
        seen = tools.iter().map(|t| t.name.clone()).collect();
        elsewhere = tools
            .iter()
            .any(|t| t.name == entry.name && !same_site(t.origin.as_deref(), &entry.url));
        found = tools
            .into_iter()
            .find(|t| t.name == entry.name && same_site(t.origin.as_deref(), &entry.url));
        if found.is_some() {
            break;
        }
    }
    let tool: PageTool = match found {
        Some(t) => t,
        None if elsewhere => bail!(
            "{} is registered, but by a different origin than {}; not calling it",
            entry.name,
            entry.url
        ),
        None => bail!(
            "{} is not registered on {} (the page has: {})",
            entry.name,
            entry.url,
            if seen.is_empty() { "no tools".to_string() } else { seen.join(", ") }
        ),
    };

    let inv = backend.invoke(&tab, &tool, args).await?;
    Ok(Outcome {
        id: entry.id.clone(),
        origin: inv.origin.or(tool.origin).unwrap_or_else(|| entry.url.clone()),
        output: inv.output,
        duration_ms: inv.duration_ms,
    })
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
