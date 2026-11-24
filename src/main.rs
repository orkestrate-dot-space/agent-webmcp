use std::path::PathBuf;

use agent_webmcp::call;
use agent_webmcp::config;
use agent_webmcp::config::DriverChoice;
use agent_webmcp::driver;
use agent_webmcp::index::Search;
use agent_webmcp::inspect;
use agent_webmcp::mcp::Server;
use agent_webmcp::setup;
use agent_webmcp::skills;
use agent_webmcp::store;
use agent_webmcp::text;
use agent_webmcp::upgrade;
use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agent-webmcp",
    bin_name = "agent-webmcp",
    version,
    disable_version_flag = true,
    about = "Discover, inspect, and call WebMCP tools on websites",
    after_help = "Start here (for AI agents): agent-webmcp skills get core\n\nCore:\n  find <site|URL|task>    check a site or search by task\n  inspect <url>           list live tools on a page\n  call <target> '<json>'  run a tool (--plan for steps, --confirm to approve)\n  tools [list]            list local custom tools\n\nSetup & Maintenance:\n  skills [list]           list skills (bundled core + local)\n  tools validate|path     validate custom tools, print tools dir\n  setup                   fetch index, install skill, check drivers\n  status                  index, drivers, session locks\n  config|update|upgrade|mcp  see below\n\nExamples:\n  agent-webmcp find cal.com\n  agent-webmcp inspect https://cal.com/\n  agent-webmcp call cal.com/ask_cal_com '{\"question\":\"hi\"}'\n  agent-webmcp tools validate\n\nEnv: AGENT_WEBMCP_DRIVER, AGENT_WEBMCP_CONFIG, AGENT_WEBMCP_SKILLS_DIR, AGENT_WEBMCP_TOOLS_DIR, AGENT_WEBMCP_INDEX_URL\nDocs: https://web.orkestrate.space/webmcp/docs"
)]
struct Cli {
    /// Print version
    #[arg(short = 'v', visible_short_alias = 'V', long, action = ArgAction::Version)]
    version: Option<bool>,
    #[command(subcommand)]
    command: Option<Command>,
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
        /// Print the steps instead of running (needs --confirm first for gated tools)
        #[arg(long)]
        plan: bool,
        /// Timeout in milliseconds (default from config)
        #[arg(long)]
        timeout: Option<u64>,
        /// Settle budget in milliseconds for late tool registration
        #[arg(long)]
        settle: Option<u64>,
        /// Print machine-readable JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Inspect a live page and list the WebMCP tools it registers
    Inspect {
        /// Page URL or host (e.g. https://cal.com/ or cal.com)
        url: String,
        /// Driver to use (default from config)
        #[arg(long)]
        driver: Option<String>,
        /// Browser session for agent-browser (default from config)
        #[arg(long)]
        session: Option<String>,
        /// Use a throwaway session or tab, closed afterwards
        #[arg(long)]
        fresh: bool,
        /// Show the browser window
        #[arg(long)]
        headed: bool,
        /// Timeout in milliseconds (default from config)
        #[arg(long)]
        timeout: Option<u64>,
        /// Settle budget in milliseconds for late tool registration
        #[arg(long)]
        settle: Option<u64>,
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
    /// First-run setup: fetch the index, install the skill, and check drivers
    Setup,
    /// Show the index in use
    Status,
    /// Skills: bundled core guide plus local skills
    Skills {
        #[command(subcommand)]
        action: Option<SkillsAction>,
        /// Print JSON (list mode when no subcommand is given)
        #[arg(long)]
        json: bool,
    },
    /// Local custom tools (manifest.json + tools.js per host)
    Tools {
        #[command(subcommand)]
        action: Option<ToolsAction>,
        /// Print JSON (list mode when no subcommand is given)
        #[arg(long)]
        json: bool,
    },
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

#[derive(Subcommand)]
enum ToolsAction {
    /// List hosts (bundled validity + tool names)
    List {
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Validate one host or all hosts
    Validate {
        /// Host to validate (e.g. example.com); omit for all hosts
        host: Option<String>,
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Print the tools root dir or one host's dir
    Path {
        /// Host name (e.g. example.com)
        host: Option<String>,
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SkillsAction {
    /// List skills (bundled core + local)
    List {
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Print a skill, a reference, or everything
    Get {
        /// Skill names
        names: Vec<String>,
        /// Reference: relative path, filename, or stem (repeatable)
        #[arg(long = "ref", conflicts_with = "full")]
        refs: Vec<String>,
        /// Inline SKILL.md plus every reference and template
        #[arg(long)]
        full: bool,
        /// Every skill
        #[arg(long)]
        all: bool,
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Print the local skills dir, or a skill's path
    Path {
        /// Skill name
        name: Option<String>,
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Install the stub skill into every detected client
    Sync {
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    use clap::CommandFactory;
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        // No subcommand: print help and exit 0 (clap's
        // `arg_required_else_help` would exit 2 instead).
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    match command {
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
            settle,
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
            settle,
            json,
        ),
        Command::Inspect {
            url,
            driver,
            session,
            fresh,
            headed,
            timeout,
            settle,
            json,
        } => inspect_command(
            &url,
            driver.as_deref(),
            session,
            fresh,
            headed,
            timeout,
            settle,
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
            println!("data dir {}", store::data_dir().display());
            let sessions = store::data_dir().join("sessions");
            if let Some((pid, _)) =
                driver::lock::holder(&sessions, &cfg.drivers.agent_browser.session)
            {
                println!(
                    "session {}: in use by pid {pid}",
                    cfg.drivers.agent_browser.session
                );
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
        Command::Skills { action, json } => match action.unwrap_or(SkillsAction::List { json }) {
            SkillsAction::List { json: list_json } => skills_list(json || list_json),
            SkillsAction::Get {
                names,
                refs,
                full,
                all,
                json: get_json,
            } => skills_get(names, refs, full, all, json || get_json),
            SkillsAction::Path {
                name,
                json: path_json,
            } => skills_path(name, json || path_json),
            SkillsAction::Sync { json: sync_json } => skills_sync(json || sync_json),
        },
        Command::Tools { action, json } => match action.unwrap_or(ToolsAction::List { json }) {
            ToolsAction::List { json: list_json } => tools_list(json || list_json),
            ToolsAction::Validate {
                host,
                json: validate_json,
            } => tools_validate(host, json || validate_json),
            ToolsAction::Path {
                host,
                json: path_json,
            } => tools_path(host, json || path_json),
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
        let mut extra = String::new();
        if h.source.as_deref() == Some("custom") {
            extra.push_str("  [custom]");
        }
        if h.headed == Some(true) {
            extra.push_str("  [headed]");
        }
        if h.auth.as_deref() == Some("session") {
            extra.push_str("  [auth:session]");
        }
        println!(
            "{}  {kind}{shared}{flags}{extra}\n    {}\n    open {}\n    call agent-webmcp call {} '{}'\n",
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
fn prepare_run_config(
    driver: Option<&str>,
    session: Option<String>,
    headed: bool,
    timeout: Option<u64>,
    settle: Option<u64>,
) -> config::Config {
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
    for (name, value, check) in [
        (
            "--timeout",
            timeout,
            config::check_timeout_ms as fn(u64, &str) -> anyhow::Result<()>,
        ),
        ("--settle", settle, config::check_settle_ms),
    ] {
        if let Some(v) = value
            && let Err(e) = check(v, name)
        {
            let e = call::CallError::usage(format!("{e:#}"));
            call::print_failure(&e);
            std::process::exit(call::exit_code(&e));
        }
    }
    cfg.apply_overrides(&config::Overrides {
        driver: choice,
        session,
        headed: headed.then_some(true),
        timeout,
        settle,
        command: None,
    });
    cfg
}

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
    settle: Option<u64>,
    json: bool,
) -> Result<()> {
    let cfg = prepare_run_config(driver, session, headed, timeout, settle);
    let search = Search::build(store::load()?)?;
    let opts = call::CallOptions {
        confirm,
        plan_only,
        fresh,
        retry_hint: call::RETRY_CLI,
    };
    match call::run_call(target, input, params, &opts, &cfg, &search) {
        Ok(done @ (call::Outcome::Blocked { .. } | call::Outcome::NeedsConfirmation { .. })) => {
            // Neither tool ran: report the structured envelope and exit 1.
            if json {
                println!("{}", serde_json::to_string_pretty(&done.envelope())?);
            } else {
                call::print_human(&done);
            }
            std::process::exit(1);
        }
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

#[allow(clippy::too_many_arguments)]
fn inspect_command(
    url: &str,
    driver: Option<&str>,
    session: Option<String>,
    fresh: bool,
    headed: bool,
    timeout: Option<u64>,
    settle: Option<u64>,
    json: bool,
) -> Result<()> {
    let cfg = prepare_run_config(driver, session, headed, timeout, settle);
    let opts = inspect::InspectOptions { fresh };
    match inspect::run_inspect(url, &opts, &cfg) {
        Ok(done) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&done.envelope())?);
            } else {
                inspect::print_human(&done);
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
        "browse" => Ok(DriverChoice::Browse),
        "lightpanda" => Ok(DriverChoice::Lightpanda),
        "none" => Ok(DriverChoice::None),
        _ => Err(call::CallError::usage(format!(
            "unknown driver {name:?}; expected one of auto, agent-browser, browse, lightpanda, none"
        ))),
    }
}

/// Lossy JSON for a skill listing entry: `PathBuf` may be non-UTF-8, and
/// `serde_json::json!` would panic; use `to_string_lossy` instead.
fn skill_summary_json(s: &skills::SkillSummary) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "name": s.name,
        "source": s.source,
        "refs": s.refs,
    });
    if let Some(desc) = &s.description {
        obj["description"] = serde_json::Value::String(desc.clone());
    }
    if let Some(path) = &s.path {
        obj["path"] = serde_json::Value::String(path.to_string_lossy().into_owned());
    }
    obj
}

/// Lossy JSON for path lists (`installed`/`updated`/`unchanged`/`stubs`).
fn paths_json(paths: &[PathBuf]) -> Vec<serde_json::Value> {
    paths
        .iter()
        .map(|p| serde_json::Value::String(p.to_string_lossy().into_owned()))
        .collect()
}

/// List bundled core plus local skills.
fn skills_list(json: bool) -> Result<()> {
    let listed = skills::list_skills();
    if json {
        // `PathBuf` is not reliably UTF-8: serialize paths lossily instead
        // of `json!({ "skills": listed })`, which would panic on non-UTF-8.
        let items: Vec<serde_json::Value> = listed.iter().map(skill_summary_json).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "skills": items }))?
        );
        return Ok(());
    }
    for s in listed {
        match &s.description {
            Some(d) => println!(
                "{} ({}): {}",
                s.name,
                s.source,
                d.lines().next().unwrap_or(d)
            ),
            None => println!("{} ({})", s.name, s.source),
        }
    }
    Ok(())
}

/// Print one or more skills, optionally with references.
fn skills_get(
    names: Vec<String>,
    refs: Vec<String>,
    full: bool,
    all: bool,
    json: bool,
) -> Result<()> {
    if full && !refs.is_empty() {
        eprintln!("error: --full cannot be combined with --ref");
        std::process::exit(2);
    }
    if all && !names.is_empty() {
        eprintln!("error: --all cannot be combined with skill names");
        std::process::exit(2);
    }
    let wanted: Vec<String> = if all {
        skills::list_skills().into_iter().map(|s| s.name).collect()
    } else {
        names
    };
    if wanted.is_empty() {
        eprintln!("error: provide skill names or --all (e.g. agent-webmcp skills get core)");
        std::process::exit(2);
    }
    // Load every skill first so unknown names fail with exit 2 before printing.
    // Lazy: plain `get` (no --ref, no --full) reads only SKILL.md;
    // `--full` reads everything; `--ref` reads the body plus only the
    // selected files from the exact same selected source (local override vs
    // bundled `core`), never mixing a local body with bundled refs.
    // `--full` keeps each loaded skill intact: output borrows `refs` and
    // `templates` directly instead of cloning every attachment body into a
    // combined list (up to ~256 MiB per skill).
    let mut loaded = Vec::new();
    let mut resolved: Vec<Vec<skills::RefContent>> = Vec::new();
    for name in &wanted {
        if full {
            let skill = match skills::get_skill(name) {
                Ok(skill) => skill,
                Err(e @ (skills::SkillError::UnknownSkill(_) | skills::SkillError::Invalid(_))) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };
            loaded.push(skill);
            resolved.push(Vec::new());
            continue;
        }
        if !refs.is_empty() {
            match skills::get_skill_with_refs(name, &refs) {
                Ok((skill, hits)) => {
                    loaded.push(skill);
                    resolved.push(hits);
                }
                Err(
                    e @ (skills::SkillError::UnknownSkill(_)
                    | skills::SkillError::UnknownRef { .. }
                    | skills::SkillError::Invalid(_)),
                ) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
            continue;
        }
        let skill = match skills::get_skill_shallow(name) {
            Ok(skill) => skill,
            Err(e @ (skills::SkillError::UnknownSkill(_) | skills::SkillError::Invalid(_))) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        };
        loaded.push(skill);
        resolved.push(Vec::new());
    }

    if json {
        // Stream borrowed views directly to stdout: building a
        // `serde_json::Value` tree would copy every attachment body (up to
        // ~256 MiB per skill) into owned `Value::String`s plus a second
        // full-string copy from `to_string_pretty`. All skills are already
        // loaded above, so unknown names still fail before any output.
        use serde::Serialize;
        #[derive(Serialize)]
        struct RefView<'a> {
            #[serde(rename = "ref")]
            ref_path: &'a str,
            content: &'a str,
        }
        #[derive(Serialize)]
        struct SkillView<'a> {
            name: &'a str,
            source: &'a str,
            content: &'a str,
            refs: Vec<RefView<'a>>,
            templates: Vec<RefView<'a>>,
        }
        #[derive(Serialize)]
        struct SkillsView<'a> {
            skills: Vec<SkillView<'a>>,
        }
        let view = SkillsView {
            skills: loaded
                .iter()
                .zip(resolved.iter())
                .map(|(skill, refs)| {
                    let (ref_items, template_items) = if full {
                        (
                            skill
                                .refs
                                .iter()
                                .map(|item| RefView {
                                    ref_path: item.ref_path.as_str(),
                                    content: item.content.as_str(),
                                })
                                .collect(),
                            skill
                                .templates
                                .iter()
                                .map(|item| RefView {
                                    ref_path: item.ref_path.as_str(),
                                    content: item.content.as_str(),
                                })
                                .collect(),
                        )
                    } else {
                        (
                            refs.iter()
                                .map(|item| RefView {
                                    ref_path: item.ref_path.as_str(),
                                    content: item.content.as_str(),
                                })
                                .collect(),
                            Vec::new(),
                        )
                    };
                    SkillView {
                        name: skill.name.as_str(),
                        source: skill.source.as_str(),
                        content: skill.content.as_str(),
                        refs: ref_items,
                        templates: template_items,
                    }
                })
                .collect(),
        };
        let out = std::io::stdout().lock();
        let mut out = out;
        serde_json::to_writer_pretty(&mut out, &view)?;
        use std::io::Write as _;
        writeln!(out)?;
        return Ok(());
    }

    let multiple = loaded.len() > 1;
    for (i, skill) in loaded.iter().enumerate() {
        if full {
            if multiple {
                println!("=== {} ===\n{}", skill.name, skill.content);
            } else {
                println!("{}", skill.content);
            }
            for r in skill.refs.iter().chain(skill.templates.iter()) {
                println!("\n--- {}/{} ---\n{}", skill.name, r.ref_path, r.content);
            }
            continue;
        }
        let refs = &resolved[i];
        if refs.is_empty() {
            if multiple {
                println!("=== {} ===\n{}", skill.name, skill.content);
            } else {
                println!("{}", skill.content);
            }
            continue;
        }
        if !multiple && refs.len() == 1 {
            println!("{}", refs[0].content);
            continue;
        }
        for r in refs {
            if multiple {
                println!("--- {}/{} ---\n{}", skill.name, r.ref_path, r.content);
            } else {
                println!("--- {} ---\n{}", r.ref_path, r.content);
            }
        }
    }
    Ok(())
}

/// Print the local skills dir, or one skill's location.
fn skills_path(name: Option<String>, json: bool) -> Result<()> {
    let Some(name) = name else {
        let dir = skills::skills_dir().map_err(|e| anyhow::anyhow!("{e}"))?;
        if json {
            // Lossy: `PathBuf` may be non-UTF-8; `json!({ "path": dir })`
            // would panic.
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({ "path": dir.to_string_lossy() })
                )?
            );
        } else {
            println!("{}", dir.display());
        }
        return Ok(());
    };
    if !skills::valid_skill_name(&name) {
        eprintln!("error: invalid skill name {name:?}");
        std::process::exit(2);
    }
    // Explicit local override (or any other local skill) wins as a path.
    // A real I/O failure (invalid UTF-8, oversized, ...) is exit 1, not
    // "unknown skill". Shallow load: no attachment bodies are read.
    // `Core` (any case) follows the same reserved policy as `core`.
    let is_local =
        !name.eq_ignore_ascii_case(skills::BUNDLED_CORE_NAME) || skills::is_explicit_dir();
    if is_local {
        match skills::get_skill_shallow(&name) {
            Ok(skill) if skill.source == "local" => {
                if let Some(file) = skill.path {
                    let skill_dir = file.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| {
                        skills::skills_dir()
                            .map(|d| d.join(&name))
                            .unwrap_or_else(|_| PathBuf::from(&name))
                    });
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                &serde_json::json!({ "name": name, "source": "local", "path": skill_dir.to_string_lossy() })
                            )?
                        );
                    } else {
                        println!("{}", skill_dir.display());
                    }
                    return Ok(());
                }
            }
            Err(e @ skills::SkillError::Io(_)) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
            _ => {}
        }
    }
    if name.eq_ignore_ascii_case(skills::BUNDLED_CORE_NAME) {
        let stubs = installed_stub_paths();
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({ "name": name, "source": "bundled", "stubs": paths_json(&stubs) })
                )?
            );
        } else {
            println!("bundled");
            for stub in stubs {
                println!("{}", stub.display());
            }
        }
        return Ok(());
    }
    // Local name that does not exist.
    eprintln!("error: unknown skill {name:?}");
    std::process::exit(2);
}

/// Platform-aware home: `$HOME` when non-empty, else `$USERPROFILE`
/// (Windows) when non-empty — matching `crate::config::home_dir`.
fn resolve_home() -> Option<PathBuf> {
    config::home_dir(&|key| std::env::var(key).ok())
}

/// Installed `agent-webmcp/SKILL.md` stubs across detected client dirs.
fn installed_stub_paths() -> Vec<PathBuf> {
    let Some(home) = resolve_home() else {
        return Vec::new();
    };
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    setup::skill_dirs(&home, xdg.as_deref())
        .into_iter()
        .map(|d| d.join("agent-webmcp").join("SKILL.md"))
        .filter(|p| p.is_file())
        .collect()
}

/// Sync the stub skill into every detected client dir.
fn skills_sync(json: bool) -> Result<()> {
    let Some(home) = resolve_home() else {
        anyhow::bail!(
            "HOME (or USERPROFILE on Windows) is not set; cannot locate skill directories"
        );
    };
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let dirs = setup::skill_dirs(&home, xdg.as_deref());
    let sync = setup::sync_skills(&dirs)?;
    let installed_set: std::collections::HashSet<&PathBuf> = sync.installed.iter().collect();
    let updated_set: std::collections::HashSet<&PathBuf> = sync.updated.iter().collect();
    let mut unchanged = Vec::new();
    for dir in &dirs {
        let target = dir.join("agent-webmcp").join("SKILL.md");
        if !installed_set.contains(&target) && !updated_set.contains(&target) {
            unchanged.push(target);
        }
    }
    if json {
        // Lossy: non-UTF-8 paths would make `json!` panic.
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "installed": paths_json(&sync.installed),
                "updated": paths_json(&sync.updated),
                "unchanged": paths_json(&unchanged),
            }))?
        );
        return Ok(());
    }
    for p in &sync.installed {
        println!("skill installed: {}", p.display());
    }
    for p in &sync.updated {
        println!("skill updated: {}", p.display());
    }
    if sync.installed.is_empty() && sync.updated.is_empty() {
        println!("skill: already up to date");
    } else {
        for p in &unchanged {
            println!("skill unchanged: {}", p.display());
        }
    }
    Ok(())
}

/// List local custom tool hosts.
fn tools_list(json: bool) -> Result<()> {
    let root = match agent_webmcp::tools::tools_dir() {
        Ok(root) => root,
        Err(e) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "hosts": [],
                        "error": e.to_string(),
                    }))?
                );
                return Ok(());
            }
            println!("tools dir: error: {e}");
            return Ok(());
        }
    };
    let hosts = agent_webmcp::tools::list_hosts_in(&root);
    if json {
        let items: Vec<serde_json::Value> = hosts
            .iter()
            .map(|h| {
                let mut obj = serde_json::json!({
                    "host": h.host,
                    "dir": h.dir.to_string_lossy(),
                    "valid": h.valid,
                    "tools": h.tools,
                });
                if let Some(err) = &h.error {
                    obj["error"] = serde_json::Value::String(err.clone());
                }
                if !h.warnings.is_empty() {
                    obj["warnings"] = serde_json::json!(h.warnings);
                }
                obj
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "hosts": items }))?
        );
        return Ok(());
    }
    if hosts.is_empty() {
        println!("no custom tools (dir {})", root.display());
        return Ok(());
    }
    for h in hosts {
        if h.valid {
            let tools = if h.tools.is_empty() {
                "(no tools)".to_string()
            } else {
                h.tools.join(", ")
            };
            println!("{}: {}", h.host, tools);
            for w in &h.warnings {
                println!("  warning: {}", text::clean(w));
            }
        } else {
            println!(
                "{}: invalid: {}",
                h.host,
                h.error.as_deref().unwrap_or("invalid")
            );
        }
    }
    Ok(())
}

/// Validate one host or all hosts; exits 1 when any host has errors.
fn tools_validate(host: Option<String>, json: bool) -> Result<()> {
    let root = match agent_webmcp::tools::tools_dir() {
        Ok(root) => root,
        Err(e) => {
            let msg = format!("{e}");
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": false,
                        "error": msg,
                    }))?
                );
            } else {
                eprintln!("error: {msg}");
            }
            std::process::exit(1);
        }
    };
    if let Some(host) = host {
        let report = agent_webmcp::tools::validate_host_in(&root, &host);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "host": report.host,
                    "dir": report.dir.to_string_lossy(),
                    "ok": report.ok,
                    "errors": report.errors,
                    "warnings": report.warnings,
                    "tools": report.tools,
                }))?
            );
        } else if report.ok {
            println!("{}: ok ({})", report.host, report.tools.join(", "));
            for w in &report.warnings {
                println!("  warning: {}", text::clean(w));
            }
        } else {
            for err in &report.errors {
                println!("{}: error: {}", report.host, err);
            }
        }
        if !report.ok {
            std::process::exit(1);
        }
        return Ok(());
    }
    let hosts = match agent_webmcp::tools::list_hosts_strict_in(&root) {
        Ok(hosts) => hosts,
        Err(e) => {
            let msg = format!("{e}");
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": false,
                        "error": msg,
                    }))?
                );
            } else {
                eprintln!("error: {msg}");
            }
            std::process::exit(1);
        }
    };
    let mut ok = true;
    if json {
        let items: Vec<serde_json::Value> = hosts
            .iter()
            .map(|h| {
                let mut obj = serde_json::json!({
                    "host": h.host,
                    "dir": h.dir.to_string_lossy(),
                    "valid": h.valid,
                    "tools": h.tools,
                });
                if let Some(err) = &h.error {
                    obj["error"] = serde_json::Value::String(err.clone());
                }
                if !h.warnings.is_empty() {
                    obj["warnings"] = serde_json::json!(h.warnings);
                }
                obj
            })
            .collect();
        let all_ok = hosts.iter().all(|h| h.valid);
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "hosts": items, "ok": all_ok }))?
        );
        if !all_ok {
            std::process::exit(1);
        }
        return Ok(());
    }
    if hosts.is_empty() {
        println!("no custom tools (dir {})", root.display());
        return Ok(());
    }
    for h in &hosts {
        if h.valid {
            println!(
                "{}: ok ({})",
                h.host,
                if h.tools.is_empty() {
                    "(no tools)".to_string()
                } else {
                    h.tools.join(", ")
                }
            );
            for w in &h.warnings {
                println!("  warning: {}", text::clean(w));
            }
        } else {
            ok = false;
            println!(
                "{}: error: {}",
                h.host,
                h.error.as_deref().unwrap_or("invalid")
            );
        }
    }
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

/// Print the tools root dir or one host's dir (non-UTF-8 safe).
fn tools_path(host: Option<String>, json: bool) -> Result<()> {
    let root = match agent_webmcp::tools::tools_dir() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    if let Some(host) = host {
        let dir_name = match agent_webmcp::tools::host_dir_name(&host) {
            Ok(name) => name,
            Err(e) => {
                eprintln!("error: invalid host {host:?}: {e}");
                std::process::exit(2);
            }
        };
        let dir = root.join(dir_name);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "host": host,
                    "path": dir.to_string_lossy(),
                }))?
            );
        } else {
            println!("{}", dir.display());
        }
        return Ok(());
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "path": root.to_string_lossy() }))?
        );
    } else {
        println!("{}", root.display());
    }
    Ok(())
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
