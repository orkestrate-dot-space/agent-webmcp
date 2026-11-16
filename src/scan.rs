//! Check which WebMCP tools a site registers, by loading its pages in Chrome.
//!
//! The scan only reads. It never invokes a tool, submits a form, or solves a
//! challenge. A page that shows a bot wall is recorded as blocked.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use crate::cdp::{Client, Event};
use crate::lint;
use crate::model::{Annotations, Site, Status, Tool};

#[derive(Debug, Clone)]
pub struct Options {
    /// Longest wait for a page to load.
    pub load_timeout: Duration,
    /// After load, keep listening this long for tools that register late.
    pub settle: Duration,
    /// Pages to try per site, including the homepage.
    pub max_pages: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { load_timeout: Duration::from_secs(25), settle: Duration::from_secs(5), max_pages: 4 }
    }
}

const CMD: Duration = Duration::from_secs(15);

/// Outcome of loading one page.
#[derive(Debug, Default)]
pub struct PageResult {
    pub url: String,
    pub final_url: String,
    pub tools: Vec<RawTool>,
    pub blocked: bool,
    pub error: Option<String>,
    pub links: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RawTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub annotations: Annotations,
    pub origin: String,
}

/// Load one page in a fresh tab and collect the tools it registers.
pub async fn scan_page(client: &Client, url: &str, opts: &Options) -> PageResult {
    let mut out = PageResult { url: url.to_string(), ..Default::default() };
    let target = match client.send("Target.createTarget", json!({ "url": "about:blank" }), None, CMD).await {
        Ok(v) => v["targetId"].as_str().unwrap_or_default().to_string(),
        Err(e) => {
            out.error = Some(format!("{e:#}"));
            return out;
        }
    };
    if let Err(e) = scan_in_target(client, &target, url, opts, &mut out).await {
        out.error = Some(format!("{e:#}"));
    }
    let _ = client.send("Target.closeTarget", json!({ "targetId": target }), None, CMD).await;
    out
}

async fn scan_in_target(client: &Client, target: &str, url: &str, opts: &Options, out: &mut PageResult) -> Result<()> {
    let session = client
        .send("Target.attachToTarget", json!({ "targetId": target, "flatten": true }), None, CMD)
        .await?["sessionId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let s = Some(session.as_str());
    let mut events = client.subscribe(&session);
    let result = listen(client, s, url, opts, out, &mut events).await;
    client.unsubscribe(&session);
    result
}

async fn listen(
    client: &Client,
    s: Option<&str>,
    url: &str,
    opts: &Options,
    out: &mut PageResult,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<Event>,
) -> Result<()> {

    client.send("Page.enable", json!({}), s, CMD).await?;
    client.send("Runtime.enable", json!({}), s, CMD).await?;
    if let Err(e) = client.send("WebMCP.enable", json!({}), s, CMD).await {
        anyhow::bail!("this browser has no WebMCP support ({e:#}); use Chrome 149+ with WebMCP enabled");
    }
    client.send("Page.navigate", json!({ "url": url }), s, CMD).await?;

    // Wait for load, collecting tool events, then keep listening for the settle window.
    let mut tools: BTreeMap<String, RawTool> = BTreeMap::new();
    let mut frame_origins: BTreeMap<String, String> = BTreeMap::new();
    let load_deadline = tokio::time::Instant::now() + opts.load_timeout;
    let mut loaded = false;
    let mut settle_deadline = load_deadline;
    loop {
        let deadline = if loaded { settle_deadline } else { load_deadline };
        let event = match tokio::time::timeout_at(deadline, events.recv()).await {
            Err(_) => break,
            Ok(None) => anyhow::bail!("the browser connection closed"),
            Ok(Some(ev)) => ev,
        };
        match event.method.as_str() {
            "Page.loadEventFired" if !loaded => {
                loaded = true;
                settle_deadline = tokio::time::Instant::now() + opts.settle;
            }
            "Runtime.executionContextCreated" => remember_origin(&event, &mut frame_origins),
            "WebMCP.toolsAdded" => {
                for t in event.params["tools"].as_array().into_iter().flatten() {
                    if let Some(tool) = raw_tool(t, &frame_origins) {
                        tools.insert(tool.name.clone(), tool);
                    }
                }
            }
            "WebMCP.toolsRemoved" => {
                for t in event.params["tools"].as_array().into_iter().flatten() {
                    if let Some(name) = t["name"].as_str() {
                        tools.remove(name);
                    }
                }
            }
            _ => {}
        }
    }

    // Page facts, read once at the end.
    let probe = r#"(() => {
      const t = (document.title || "").toLowerCase();
      const body = (document.body && document.body.innerText || "").slice(0, 2000).toLowerCase();
      const blocked = /just a moment|attention required|access denied|verify you are human|are you a robot/.test(t + " " + body)
        || !!document.querySelector('#challenge-form, #cf-challenge-running, iframe[src*="challenges.cloudflare.com"]');
      const links = Array.from(document.querySelectorAll("a[href]")).map(a => a.href).filter(h => h.startsWith(location.origin)).slice(0, 400);
      return JSON.stringify({ href: location.href, blocked, links });
    })()"#;
    if let Ok(v) = client
        .send("Runtime.evaluate", json!({ "expression": probe, "returnByValue": true }), s, CMD)
        .await
        && let Some(text) = v["result"]["value"].as_str()
        && let Ok(facts) = serde_json::from_str::<Value>(text)
    {
        out.final_url = facts["href"].as_str().unwrap_or(url).to_string();
        out.blocked = facts["blocked"].as_bool().unwrap_or(false);
        out.links = facts["links"]
            .as_array()
            .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
    }
    if out.final_url.is_empty() {
        out.final_url = url.to_string();
    }
    if !loaded && tools.is_empty() && !out.blocked {
        out.error = Some(format!("the page did not finish loading within {}s", opts.load_timeout.as_secs()));
    }
    out.tools = tools.into_values().collect();
    Ok(())
}

fn remember_origin(event: &Event, map: &mut BTreeMap<String, String>) {
    let ctx = &event.params["context"];
    if ctx["auxData"]["isDefault"].as_bool() == Some(true)
        && let (Some(frame), Some(origin)) = (ctx["auxData"]["frameId"].as_str(), ctx["origin"].as_str())
    {
        map.insert(frame.to_string(), origin.to_string());
    }
}

fn raw_tool(t: &Value, frame_origins: &BTreeMap<String, String>) -> Option<RawTool> {
    let name = t["name"].as_str()?.to_string();
    let frame = t["frameId"].as_str().unwrap_or_default();
    let schema = match &t["inputSchema"] {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        v => v.clone(),
    };
    let a = &t["annotations"];
    Some(RawTool {
        name,
        description: t["description"].as_str().unwrap_or_default().to_string(),
        input_schema: schema,
        annotations: Annotations {
            read_only: a["readOnlyHint"].as_bool().or(a["readOnly"].as_bool()).unwrap_or(false),
            consequential: a["consequentialHint"].as_bool().or(a["consequential"].as_bool()).unwrap_or(false),
            untrusted_content: a["untrustedContentHint"]
                .as_bool()
                .or(a["untrustedContent"].as_bool())
                .unwrap_or(false),
        },
        origin: frame_origins.get(frame).cloned().unwrap_or_default(),
    })
}

/// Pick follow-up pages from the homepage's links: docs, search, pricing, products.
pub fn pick_pages(home: &str, links: &[String], max: usize) -> Vec<String> {
    const HINTS: &[&str] = &["/docs", "/search", "/pricing", "/products", "/product", "/shop", "/api", "/help"];
    let mut picked: Vec<String> = Vec::new();
    for hint in HINTS {
        if picked.len() + 1 >= max {
            break;
        }
        if let Some(link) = links.iter().find(|l| {
            let path = l.split_once("://").and_then(|(_, rest)| rest.find('/').map(|i| &rest[i..])).unwrap_or("");
            path.starts_with(hint) && !l.contains('#') && *l != home && !picked.contains(l)
        }) {
            picked.push(link.clone());
        }
    }
    picked
}

/// Check a site: its start page, then a few likely pages if it loaded.
/// `entry` is a host (`vercel.com`) or a host and path for a site that lives
/// under one (`googlechromelabs.github.io/webmcp-tools/demos/react-flightsearch`).
pub async fn scan_site(client: &Client, entry: &str, opts: &Options) -> Site {
    let host = entry.split('/').next().unwrap_or(entry);
    let home = format!("https://{}/", entry.trim_end_matches('/'));
    let first = scan_page(client, &home, opts).await;
    let mut pages = vec![first];
    if pages[0].error.is_none() && !pages[0].blocked {
        for url in pick_pages(&pages[0].final_url, &pages[0].links, opts.max_pages) {
            pages.push(scan_page(client, &url, opts).await);
        }
    }
    assemble(host, &pages)
}

/// Turn page results into a site record. Tools are merged by name; each keeps
/// the first page it was seen on.
pub fn assemble(host: &str, pages: &[PageResult]) -> Site {
    let mut tools: BTreeMap<String, Tool> = BTreeMap::new();
    for page in pages {
        for t in &page.tools {
            tools.entry(t.name.clone()).or_insert_with(|| Tool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
                page: page.final_url.clone(),
                origin: t.origin.clone(),
                annotations: t.annotations.clone(),
                flags: lint::flags(&t.description),
            });
        }
    }
    let first = pages.first();
    let (status, note) = if !tools.is_empty() {
        (Status::Verified, None)
    } else if first.is_some_and(|p| p.blocked) {
        (Status::Blocked, Some("a bot check blocked the page; it was not solved".to_string()))
    } else if let Some(e) = first.and_then(|p| p.error.clone()) {
        (Status::Error, Some(e))
    } else {
        (Status::NoTools, None)
    };
    Site {
        host: host.trim_start_matches("www.").to_string(),
        status,
        checked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        pages_checked: pages.iter().map(|p| p.final_url.clone()).collect(),
        platform: crate::platform::detect(&tools.values().map(|t| t.name.as_str()).collect::<Vec<_>>()),
        tools: tools.into_values().collect(),
        note,
    }
}
