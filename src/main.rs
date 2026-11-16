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
        Command::Find { query } => {
            println!("not implemented: find {:?}", query.join(" "));
        }
        Command::Call { id, args, yes } => {
            println!("not implemented: call {id} {args} (yes={yes})");
        }
        Command::Mcp => {
            eprintln!("not implemented: mcp");
        }
    }
    Ok(())
}
