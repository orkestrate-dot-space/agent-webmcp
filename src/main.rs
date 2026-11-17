use std::path::PathBuf;

use agent_webmcp::index::Search;
use agent_webmcp::mcp::Server;
use agent_webmcp::store;
use anyhow::Result;
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
                "{} sites, {} tools\ngenerated {}\nfile {}\nchecks {}",
                search.sites(),
                search.tools(),
                search.generated_at(),
                file.display(),
                store::pointer_url()
            );
            Ok(())
        }
        Command::Mcp => {
            use rmcp::ServiceExt;
            let search = Search::build(tokio::task::spawn_blocking(store::load).await??)?;
            let server = Server::new(search);
            server.spawn_refresh();
            let running = server.serve(rmcp::transport::stdio()).await?;
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
