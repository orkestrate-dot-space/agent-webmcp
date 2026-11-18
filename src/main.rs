use std::path::PathBuf;

use agent_webmcp::call;
use agent_webmcp::config;
use agent_webmcp::config::DriverChoice;
use agent_webmcp::driver;
use agent_webmcp::index::Search;
use agent_webmcp::mcp::Server;
use agent_webmcp::setup;
use agent_webmcp::store;
use agent_webmcp::text;
use agent_webmcp::upgrade;
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agent-webmcp",
    bin_name = "agent-webmcp",
    version,
    about = "Check a site's WebMCP tools before you open it, or search tools by task"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Search tools by site, URL, or task
    Find {
        /// A site, a URL, or what you want to do, in plain words
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Call a WebMCP tool, or print the steps for your driver
    Call {
        /// site/tool from find, or https://page#tool
        target: String,
        /// Tool input as JSON (or use --params)
        input: Option<String>,
        /// Tool input as JSON or @file (@- reads stdin)
        #[arg(long)]
        params: Option<String>,
        /// Driver to use (default from config)
        #[arg(long)]
        driver: Option<String>,
        /// Browser session for agent-browser (default from config)
        #[arg(long)]
        session: Option<String>,
        /// Use a throwaway session, closed after the call
        #[arg(long)]
        fresh: bool,
        /// Show the browser window
        #[arg(long)]
        headed: bool,
        /// Approve a tool that needs confirmation
        #[arg(long)]
        confirm: bool,
        /// Print the steps instead of running
        #[arg(long)]
        plan: bool,
        /// Timeout in milliseconds (default from config)
        #[arg(long)]
        timeout: Option<u64>,
        /// Print machine-readable JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Download the latest index now
    Update,
    /// Upgrade agent-webmcp itself to the latest or a given version
    Upgrade {
        /// Version to install (with or without a leading v), or `next`
        target: Option<String>,
    },
    /// First-run setup: fetch the index, install the skill, wire up OpenCode
    Setup,
    /// Show the index in use
    Status,
    /// Show or check the effective configuration
    Config {
        /// Print machine-readable JSON instead of text
        #[arg(long, global = true)]
        json: bool,
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Run the MCP server on stdio
    Mcp,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print the effective config and where each value came from
    Show,
    /// Print the config file locations
    Path,
    /// Check the config files for errors
    Validate,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Find { query, limit, json } => find(&query.join(" "), limit, json),
        Command::Call {
            target,
            input,
            params,
            driver,
            session,
            fresh,
            headed,
            confirm,
            plan,
            timeout,
            json,
        } => call_command(
            &target,
            input.as_deref(),
            params.as_deref(),
            driver.as_deref(),
            session,
            fresh,
            headed,
            confirm,
            plan,
            timeout,
            json,
        ),
        Command::Update => {
            let (index, changed) = tokio::task::spawn_blocking(store::refresh_changed).await??;
            let search = Search::build(index)?;
            println!(
                "{}: {} sites, {} tools ({})",
                if changed {
                    "index updated"
                } else {
                    "index is up to date"
                },
                search.sites(),
                search.tools(),
                search.generated_at()
            );
            if let Some(note) = cli_update_note() {
                println!("{note}");
            }
            Ok(())
        }
        Command::Upgrade { target } => upgrade::run(target),
        Command::Setup => setup::run(),
        Command::Status => {
            let search = Search::build(tokio::task::spawn_blocking(store::load).await??)?;
            let file = std::env::var_os("AGENT_WEBMCP_INDEX")
                .map(PathBuf::from)
                .unwrap_or_else(store::index_path);
            println!(
                "{} sites, {} tools\ngenerated {}\nfile {}\nchecks {}",
                search.sites(),
                search.tools(),
                search.generated_at(),
                file.display(),
                store::pointer_urls().join(" then ")
            );
            if let Some(note) = cli_update_note() {
                println!("{note}");
            }
            // A bad config must not kill status; report it and carry on.
            let cfg = match config::load() {
                Ok(loaded) => {
                    for warning in &loaded.warnings {
                        eprintln!("warning: {warning}");
                    }
                    loaded.config
                }
                Err(e) => {
                    println!("config: {e:#}");
                    config::Config::default()
                }
            };
            for driver in driver::all(&cfg) {
                println!("{}", driver::status_line(driver.as_ref(), &cfg));
            }
            match driver::choose(&cfg) {
                driver::Choice::Run(driver, _) => {
                    println!("selected: {} (run)", driver.id());
                }
                driver::Choice::Plan(driver) => {
                    println!("selected: {} (plan)", driver.id());
                }
                driver::Choice::Unusable { driver, .. } => {
                    println!("selected: {driver} (unusable)");
                }
            }
            Ok(())
        }
        Command::Config { json, action } => match action.unwrap_or(ConfigAction::Show) {
            ConfigAction::Show => config_show(json),
            ConfigAction::Path => config_path(),
            ConfigAction::Validate => config_validate(),
        },
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

fn cli_update_note() -> Option<String> {
    let latest = store::latest_cli_remote()?;
    if setup::newer_available(env!("CARGO_PKG_VERSION"), &latest) {
        Some(format!(
            "agent-webmcp {latest} is available; run: agent-webmcp upgrade"
        ))
    } else {
        None
    }
}

/// Print the merged config and, unless `--json`, the sources below it.
fn config_show(json: bool) -> Result<()> {
    let loaded = config::load()?;
    for warning in &loaded.warnings {
        eprintln!("warning: {warning}");
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "config": loaded.config,
                "sources": loaded.sources,
                "warnings": loaded.warnings,
            }))?
        );
        return Ok(());
    }
    println!(
        "{}\n\nsources:",
        serde_json::to_string_pretty(&loaded.config)?
    );
    if loaded.sources.is_empty() {
        println!("(none, using defaults)");
    }
    for source in &loaded.sources {
        println!("{source}");
    }
    Ok(())
}

/// Print where the user and project configs live, whether they exist or not.
fn config_path() -> Result<()> {
    let env = |key: &str| std::env::var(key).ok();
    let cwd = std::env::current_dir()?;
    let found = config::locate(&env, &cwd);
    for warning in &found.warnings {
        eprintln!("warning: {warning}");
    }
    if found.user_exists {
        println!("user: {}", found.user.display());
    } else {
        println!("user: {} (not found)", found.user.display());
    }
    if let Some(project) = found.project {
        println!("project: {}", project.display());
    }
    Ok(())
}

/// Load the configs and report success or the first error.
fn config_validate() -> Result<()> {
    match config::load() {
        Ok(loaded) => {
            for warning in &loaded.warnings {
                eprintln!("warning: {warning}");
            }
            println!("ok");
            for source in &loaded.sources {
                println!("{source}");
            }
            if loaded.sources.is_empty() {
                println!("(no config files, using defaults)");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    }
}

fn find(query: &str, limit: usize, json: bool) -> Result<()> {
    // The MCP tool clamps to 1..=20; the CLI matches it so `--limit 0` cannot
    // print a false "no tool" and huge values stay bounded.
    let limit = limit.clamp(1, 20);
    let search = Search::build(store::load()?)?;
    let hits = search.find(query, limit)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "matches": hits }))?
        );
        return Ok(());
    }
    if hits.is_empty() {
        println!(
            "No indexed site has a tool for {}. Use your browser driver as usual.",
            short_query(query)
        );
        return Ok(());
    }
    for h in hits {
        let shared = h
            .platform
            .as_ref()
            .map(|p| format!("  (shared by {} {} sites)", p.sites, text::clean(&p.name)))
            .unwrap_or_default();
        let flags = if h.flags.is_empty() {
            String::new()
        } else {
            format!(
                "  [flagged: {}]",
                h.flags
                    .iter()
                    .map(|f| text::clean(f))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };
        let example =
            serde_json::to_string(&call::example_input(&h.input_schema)).unwrap_or_default();
        let kind = if h.kind_inferred {
            "[read, inferred]".to_string()
        } else {
            format!("[{}]", h.kind)
        };
        println!(
            "{}  {kind}{shared}{flags}\n    {}\n    open {}\n    call agent-webmcp call {} '{}'\n",
            text::clean(&h.id),
            text::clean(&h.description),
            text::clean(&h.open),
            text::clean(&h.id),
            example
        );
    }
    Ok(())
}

/// Run one `call`: config, index, then the shared pipeline.
#[allow(clippy::too_many_arguments)]
fn call_command(
    target: &str,
    input: Option<&str>,
    params: Option<&str>,
    driver: Option<&str>,
    session: Option<String>,
    fresh: bool,
    headed: bool,
    confirm: bool,
    plan_only: bool,
    timeout: Option<u64>,
    json: bool,
) -> Result<()> {
    let mut cfg = match config::load() {
        Ok(loaded) => {
            for warning in &loaded.warnings {
                eprintln!("warning: {warning}");
            }
            loaded.config
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    };
    let choice = match driver.map(parse_driver_choice) {
        None => None,
        Some(Ok(choice)) => Some(choice),
        Some(Err(e)) => {
            call::print_failure(&e);
            std::process::exit(call::exit_code(&e));
        }
    };
    cfg.apply_overrides(&config::Overrides {
        driver: choice,
        session,
        headed: headed.then_some(true),
        timeout,
        command: None,
    });
    let search = Search::build(store::load()?)?;
    let opts = call::CallOptions {
        confirm,
        plan_only,
        fresh,
        retry_hint: call::RETRY_CLI,
    };
    match call::run_call(target, input, params, &opts, &cfg, &search) {
        Ok(done) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&done.envelope())?);
            } else {
                call::print_human(&done);
            }
            Ok(())
        }
        Err(e) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&e.envelope())?);
            }
            call::print_failure(&e);
            std::process::exit(call::exit_code(&e));
        }
    }
}

/// A `--driver` name, or a usage error naming the valid ids.
fn parse_driver_choice(name: &str) -> Result<DriverChoice, call::CallError> {
    match name {
        "auto" => Ok(DriverChoice::Auto),
        "agent-browser" => Ok(DriverChoice::AgentBrowser),
        "playwright-mcp" => Ok(DriverChoice::PlaywrightMcp),
        "chrome-devtools-mcp" => Ok(DriverChoice::ChromeDevtoolsMcp),
        "browse" => Ok(DriverChoice::Browse),
        "none" => Ok(DriverChoice::None),
        _ => Err(call::CallError::usage(format!(
            "unknown driver {name:?}; expected one of auto, agent-browser, playwright-mcp, chrome-devtools-mcp, browse, none"
        ))),
    }
}

/// The no-match line echoes the query; keep it to one short line.
fn short_query(query: &str) -> String {
    const MAX: usize = 79;
    let flat: String = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX {
        return format!("{flat:?}");
    }
    let cut: String = flat.chars().take(MAX).collect();
    format!("{:?}", format!("{cut}…"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_query_keeps_short_queries_whole() {
        assert_eq!(short_query("cal.com"), "\"cal.com\"");
    }

    #[test]
    fn short_query_truncates_with_ellipsis() {
        let long = "x".repeat(2000);
        let short = short_query(&long);
        assert!(short.len() < 2000, "{short}");
        assert!(short.ends_with("…\""), "{short}");
        assert!(short.lines().count() == 1);
    }
}
