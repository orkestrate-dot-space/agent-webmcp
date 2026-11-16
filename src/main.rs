use agent_webmcp::agent_browser::AgentBrowser;
use agent_webmcp::approval::{AllowAll, Terminal};
use agent_webmcp::catalog::{Catalog, Kind, Source};
use agent_webmcp::engine;
use agent_webmcp::ledger::Ledger;
use agent_webmcp::index::Index;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agent-webmcp", version, about = "Find and call website tools from any agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Search for a capability across known tools
    Find {
        /// What you want to do, in plain words
        query: Vec<String>,
        /// Maximum number of results
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Call a tool by id, for example example.com/say_hello
    Call {
        /// Tool id from `find`
        id: String,
        /// Arguments as a JSON object
        #[arg(default_value = "{}")]
        args: String,
        /// Approve a write tool without asking
        #[arg(long)]
        yes: bool,
    },
    /// Run the MCP server on stdio
    Mcp,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Find { query, limit } => find(&query.join(" "), limit)?,
        Command::Call { id, args, yes } => call(&id, &args, yes).await?,
        Command::Mcp => {
            eprintln!("not implemented: mcp");
        }
    }
    Ok(())
}

async fn call(id: &str, args: &str, yes: bool) -> anyhow::Result<()> {
    let catalog = Catalog::load()?;
    let args: serde_json::Value =
        serde_json::from_str(args).map_err(|e| anyhow::anyhow!("arguments are not valid JSON: {e}"))?;
    // One browser session per call, so a call never lands in a daemon that a previous
    // call is still closing.
    let browser = AgentBrowser::connect(&format!("agent-webmcp-{}", std::process::id())).await?;
    let ledger = Ledger::default_location();
    let opts = engine::Options::default();
    let result = if yes {
        engine::call(&browser, &AllowAll, &ledger, &catalog, id, args, &opts).await
    } else {
        engine::call(&browser, &Terminal, &ledger, &catalog, id, args, &opts).await
    };
    browser.shutdown(true).await?;
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}

fn find(query: &str, limit: usize) -> anyhow::Result<()> {
    let catalog = Catalog::load()?;
    let index = Index::build(&catalog.entries)?;
    let hits = index.search(query, limit)?;
    if hits.is_empty() {
        println!("no tools match {query:?}");
        return Ok(());
    }
    for (pos, _score) in hits {
        let e = &catalog.entries[pos];
        let kind = match e.kind {
            Kind::Read => "read",
            Kind::Write => "write",
        };
        let source = match e.source {
            Source::Native => "native",
            Source::Community => "community",
        };
        println!("{}  [{kind}, {source}]\n    {}", e.id, e.description);
    }
    Ok(())
}
