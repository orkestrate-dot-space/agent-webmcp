use agent_webmcp::catalog::{Catalog, Kind, Source};
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
        Command::Call { id, args, yes } => {
            println!("not implemented: call {id} {args} (yes={yes})");
        }
        Command::Mcp => {
            eprintln!("not implemented: mcp");
        }
    }
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
