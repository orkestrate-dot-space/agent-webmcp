use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_webmcp::cdp::Client;
use agent_webmcp::chrome::Chrome;
use agent_webmcp::index::Search;
use agent_webmcp::mcp::Server;
use agent_webmcp::model::{Index, SCHEMA_VERSION, Site, Status};
use agent_webmcp::{scan, store};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agent-webmcp", version, about = "Find websites' WebMCP tools by what you want to do")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Search tools by what you want to do
    Find {
        /// What you want to do, in plain words
        query: Vec<String>,
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Check which WebMCP tools sites register, by loading them in Chrome
    Scan {
        /// Hosts or URLs to check; with --list, read them from a file
        sites: Vec<String>,
        /// A file with one host per line (# comments allowed)
        #[arg(long)]
        list: Option<PathBuf>,
        /// Write an index file here (default: print site records as JSON lines)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Use a running Chrome instead of starting one (ws://... browser URL)
        #[arg(long)]
        cdp: Option<String>,
        /// Sites checked at the same time
        #[arg(long, default_value_t = 6)]
        concurrency: usize,
        /// Pages to try per site, including the homepage
        #[arg(long, default_value_t = 4)]
        pages: usize,
        /// Seconds to keep listening for tools after the page loads
        #[arg(long, default_value_t = 5)]
        settle: u64,
    },
    /// Download the latest index now
    Update,
    /// Show the index in use
    Status,
    /// Run the MCP server on stdio
    Mcp,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Find { query, limit, json } => find(&query.join(" "), limit, json),
        Command::Scan { sites, list, out, cdp, concurrency, pages, settle } => {
            scan_cmd(sites, list, out, cdp, concurrency, pages, settle).await
        }
        Command::Update => {
            let index = tokio::task::spawn_blocking(store::refresh).await??;
            let search = Search::build(index)?;
            println!("index updated: {} sites, {} tools ({})", search.sites(), search.tools(), search.generated_at());
            Ok(())
        }
        Command::Status => {
            let search = Search::build(tokio::task::spawn_blocking(store::load).await??)?;
            let file = std::env::var_os("AGENT_WEBMCP_INDEX").map(PathBuf::from).unwrap_or_else(store::index_path);
            println!(
                "{} sites, {} tools\ngenerated {}\nfile {}",
                search.sites(),
                search.tools(),
                search.generated_at(),
                file.display()
            );
            Ok(())
        }
        Command::Mcp => {
            use rmcp::ServiceExt;
            let search = Search::build(tokio::task::spawn_blocking(store::load).await??)?;
            let running = Server::new(search).serve(rmcp::transport::stdio()).await?;
            running.waiting().await?;
            Ok(())
        }
    }
}

fn find(query: &str, limit: usize, json: bool) -> Result<()> {
    let search = Search::build(store::load()?)?;
    let hits = search.find(query, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "matches": hits }))?);
        return Ok(());
    }
    if hits.is_empty() {
        println!("No indexed site has a tool for {query:?}. Use your browser driver as usual.");
        return Ok(());
    }
    for h in hits {
        let shared = h
            .platform
            .as_ref()
            .map(|p| format!("  (shared by {} {} sites)", p.sites, p.name))
            .unwrap_or_default();
        let flags = if h.flags.is_empty() { String::new() } else { format!("  [flagged: {}]", h.flags.join(", ")) };
        println!("{}  [{}]{shared}{flags}\n    {}\n    open {}\n", h.id, h.kind, h.description, h.open);
    }
    Ok(())
}

fn read_list(sites: Vec<String>, list: Option<PathBuf>) -> Result<Vec<String>> {
    let mut hosts = Vec::new();
    let mut raw = sites;
    if let Some(path) = list {
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        raw.extend(text.lines().map(str::to_string));
    }
    for line in raw {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // A host, or a full URL to check a specific page as the start page.
        let entry = line.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
        let entry = match entry.split_once('/') {
            Some((host, path)) => format!("{}/{}", host.to_ascii_lowercase(), path),
            None => entry.to_ascii_lowercase(),
        };
        if !entry.is_empty() && !hosts.contains(&entry) {
            hosts.push(entry);
        }
    }
    Ok(hosts)
}

async fn scan_cmd(
    sites: Vec<String>,
    list: Option<PathBuf>,
    out: Option<PathBuf>,
    cdp: Option<String>,
    concurrency: usize,
    pages: usize,
    settle: u64,
) -> Result<()> {
    let hosts = read_list(sites, list)?;
    anyhow::ensure!(!hosts.is_empty(), "nothing to scan: pass hosts or --list <file>");
    let chrome = match cdp {
        Some(url) => Chrome::attach(url),
        None => Chrome::launch().await?,
    };
    let client = Client::connect(&chrome.ws_url).await?;
    let opts = scan::Options { settle: Duration::from_secs(settle), max_pages: pages.max(1), ..Default::default() };

    let total = hosts.len();
    let queue = Arc::new(tokio::sync::Mutex::new(hosts.into_iter().enumerate().collect::<Vec<_>>()));
    let results: Arc<tokio::sync::Mutex<Vec<(usize, Site)>>> = Arc::default();
    let mut workers = Vec::new();
    for _ in 0..concurrency.max(1) {
        let (queue, results, client, opts) = (queue.clone(), results.clone(), client.clone(), opts.clone());
        workers.push(tokio::spawn(async move {
            loop {
                let Some((i, host)) = queue.lock().await.pop() else { break };
                let site = scan::scan_site(&client, &host, &opts).await;
                let tag = match site.status {
                    Status::Verified => format!("{} tools", site.tools.len()),
                    Status::NoTools => "no tools".into(),
                    Status::Blocked => "blocked".into(),
                    Status::Error => format!("error: {}", site.note.clone().unwrap_or_default()),
                };
                eprintln!("{host}: {tag}");
                results.lock().await.push((i, site));
            }
        }));
    }
    for w in workers {
        w.await?;
    }
    chrome.close().await;

    let mut sites: Vec<(usize, Site)> = std::mem::take(&mut *results.lock().await);
    sites.sort_by_key(|(i, _)| *i);
    let sites: Vec<Site> = sites.into_iter().map(|(_, s)| s).collect();
    let verified = sites.iter().filter(|s| s.status == Status::Verified).count();
    eprintln!("checked {total} sites: {verified} with tools");

    match out {
        Some(path) => {
            let index = Index {
                schema_version: SCHEMA_VERSION,
                generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                sites,
            };
            std::fs::write(&path, serde_json::to_string_pretty(&index)? + "\n")?;
            eprintln!("wrote {}", path.display());
        }
        None => {
            for s in sites {
                println!("{}", serde_json::to_string(&s)?);
            }
        }
    }
    Ok(())
}
