//! First-run setup: fetch the index, install the skill, wire the MCP server
//! into OpenCode, print the one-line installs for Claude Code and Codex, and
//! report the per-driver WebMCP setup.
//!
//! Everything here is additive: files are only created or extended, never
//! overwritten, and `setup` reports each change it made.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{self, Config};
use crate::driver::{self, Choice, Detection, Driver, Kind};

const SKILL: &str = include_str!("../skills/agent-webmcp/SKILL.md");

/// How `--install-driver` / `--no-install` decide the Lightpanda offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallFlag {
    /// No flag: ask on an interactive terminal, else print the manual lines.
    None,
    /// `--install-driver lightpanda`: install without asking.
    InstallLightpanda,
    /// `--no-install`: never ask, never install.
    NoInstall,
}

/// What setup should do about the missing run driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Install Lightpanda without asking.
    Install,
    /// Ask once on the terminal.
    Ask,
    /// Never ask, never install.
    Never,
}

/// The offer logic, kept pure so tests cover it without a terminal:
/// a ready run driver wins over everything; `--no-install` wins over
/// `--install-driver`; otherwise the flag decides, and without a flag only
/// an interactive terminal gets asked.
pub fn should_offer(any_run_ready: bool, interactive: bool, flag: InstallFlag) -> Offer {
    if any_run_ready {
        return Offer::Never;
    }
    match flag {
        InstallFlag::NoInstall => Offer::Never,
        InstallFlag::InstallLightpanda => Offer::Install,
        InstallFlag::None => {
            if interactive {
                Offer::Ask
            } else {
                Offer::Never
            }
        }
    }
}

/// True when agent-browser or lightpanda detects as Ready.
fn any_run_ready(cfg: &Config) -> bool {
    driver::all(cfg)
        .iter()
        .any(|d| d.kind() == Kind::Run && matches!(d.detect(cfg), Detection::Ready { .. }))
}

/// Run the whole setup and report what changed.
pub fn run(flag: InstallFlag) -> Result<()> {
    let index = crate::store::refresh()?;
    let (sites, tools) = crate::model::count_index(&index.sites);
    println!(
        "index: {sites} sites, {tools} tools ({})",
        index.generated_at
    );

    let home = home_dir()?;
    let mut installed = Vec::new();
    for dir in skill_dirs(&home) {
        let target = dir.join("agent-webmcp").join("SKILL.md");
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&target, SKILL).with_context(|| format!("writing {}", target.display()))?;
        installed.push(target);
    }
    if installed.is_empty() {
        println!("skill: already installed");
    } else {
        for p in &installed {
            println!("skill installed: {}", p.display());
        }
    }

    match configure_opencode()? {
        Configured::Added(path) => println!(
            "opencode: added the agent-webmcp MCP server to {}",
            path.display()
        ),
        Configured::Present(path) => {
            println!("opencode: {} already lists agent-webmcp", path.display())
        }
        Configured::Skipped(path) => {
            println!(
                "opencode: {} was not changed (it has comments or is not plain JSON); add this under \"mcp\":",
                path.display()
            );
            println!("{}", mcp_snippet(&mcp_command()?));
        }
        Configured::Missing => {
            println!("opencode: no config file found; add this under \"mcp\":");
            println!("{}", mcp_snippet(&mcp_command()?));
        }
    }

    // A bad config must not kill setup; report it and carry on.
    let cfg = match config::load() {
        Ok(loaded) => {
            for warning in &loaded.warnings {
                eprintln!("warning: {warning}");
            }
            loaded.config
        }
        Err(e) => {
            println!("config: {e:#}");
            Config::default()
        }
    };
    for line in driver_report(&cfg) {
        println!("{line}");
    }
    let ready = any_run_ready(&cfg);
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    match should_offer(ready, interactive, flag) {
        Offer::Never => {
            if !ready {
                print_manual_lines(&cfg);
            }
        }
        Offer::Install => install_lightpanda(&cfg),
        Offer::Ask => {
            if prompt_install() {
                install_lightpanda(&cfg);
            } else {
                print_manual_lines(&cfg);
            }
        }
    }
    println!("other clients (not configured automatically):");
    println!(
        "  claude mcp add agent-webmcp -s user -- {} mcp",
        mcp_command()?
    );
    println!("  codex mcp add agent-webmcp -- {} mcp", mcp_command()?);
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot install the skill")
}

/// The well-known local skill directories.
pub fn skill_dirs(home: &Path) -> Vec<PathBuf> {
    [
        ".agents/skills",
        ".claude/skills",
        ".config/opencode/skills",
    ]
    .iter()
    .map(|d| home.join(d))
    .collect()
}

pub enum Configured {
    Added(PathBuf),
    Present(PathBuf),
    Missing,
    /// The config exists but is not plain JSON (comments, JSONC): left
    /// untouched, with the manual snippet printed instead.
    Skipped(PathBuf),
}

fn opencode_config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        let p = dir.join("opencode/opencode.json");
        if p.exists() {
            return Some(p);
        }
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".config/opencode/opencode.json"))
        .filter(|p| p.exists())
}

/// The command that runs this program: the plain `agent-webmcp` from PATH
/// when one is there (the npm wrapper survives `npm i -g` upgrades while the
/// inner binary path changes with each platform-package layout), else this
/// binary's own canonical path (argv[0] may be relative or a symlink).
fn mcp_command() -> Result<String> {
    if path_resolves_to_us() {
        return Ok("agent-webmcp".to_string());
    }
    let exe = std::env::current_exe()?;
    Ok(std::fs::canonicalize(&exe)
        .unwrap_or(exe)
        .to_string_lossy()
        .into_owned())
}

fn path_resolves_to_us() -> bool {
    let exe = if cfg!(windows) {
        "agent-webmcp.exe"
    } else {
        "agent-webmcp"
    };
    let current = match std::env::current_exe() {
        Ok(exe) => std::fs::canonicalize(&exe).unwrap_or(exe),
        Err(_) => return false,
    };
    let ours = format!("agent-webmcp {}", env!("CARGO_PKG_VERSION"));
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    // Mirror shell resolution: only the first match on PATH matters. A stale
    // install earlier in PATH wins over a good one later, so later entries
    // must not qualify.
    let cand = match std::env::split_paths(&paths)
        .map(|dir| dir.join(exe))
        .find(|cand| cand.is_file())
    {
        Some(cand) => cand,
        None => return false,
    };
    // The same file (a symlinked or hardlinked install).
    if std::fs::canonicalize(&cand).is_ok_and(|canon| canon == current) {
        return true;
    }
    // The same release under a different path (e.g. the npm wrapper shim,
    // which survives `npm i -g` upgrades while the inner binary path does
    // not). A stale install with another version does not qualify.
    std::process::Command::new(&cand)
        .arg("--version")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == ours)
}

pub fn configure_opencode() -> Result<Configured> {
    let Some(path) = opencode_config_path() else {
        return Ok(Configured::Missing);
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let exe = mcp_command()?;
    match merge_opencode_mcp(&text, &exe) {
        Ok(None) => Ok(Configured::Present(path)),
        Ok(Some(merged)) => {
            crate::store::write_atomically(&path, &merged)
                .with_context(|| format!("writing {}", path.display()))?;
            Ok(Configured::Added(path))
        }
        Err(e) => {
            // Valid JSON with the wrong shape is a real error; text that is
            // not JSON at all (comments, JSONC) just means hands off.
            if serde_json::from_str::<serde_json::Value>(&text).is_ok() {
                return Err(e);
            }
            Ok(Configured::Skipped(path))
        }
    }
}

/// Merge the agent-webmcp MCP server into an opencode.json document.
/// Returns `Ok(None)` when the server is already listed. A successful merge
/// is re-serialised, so formatting and comments are normalised; callers leave
/// non-JSON configs (JSONC with comments) untouched instead.
pub fn merge_opencode_mcp(existing: &str, exe: &str) -> Result<Option<String>> {
    let mut doc: serde_json::Value =
        serde_json::from_str(existing).context("the opencode config is not valid JSON")?;
    let mcp = doc
        .as_object_mut()
        .context("the opencode config is not a JSON object")?
        .entry("mcp")
        .or_insert_with(|| serde_json::Value::Object(Default::default()));
    let servers = mcp
        .as_object_mut()
        .context("the opencode config's \"mcp\" is not an object")?;
    if servers.contains_key("agent-webmcp") {
        return Ok(None);
    }
    servers.insert(
        "agent-webmcp".to_string(),
        serde_json::json!({
            "type": "local",
            "command": [exe, "mcp"],
            "enabled": true,
        }),
    );
    Ok(Some(serde_json::to_string_pretty(&doc)?))
}

fn mcp_snippet(exe: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "mcp": { "agent-webmcp": { "type": "local", "command": [exe, "mcp"], "enabled": true } }
    }))
    .unwrap_or_default()
}

/// The driver report, built from real detection: one line per driver plus
/// what `call` will use. Skips the generic fallback, which is not a driver.
pub fn driver_report(cfg: &Config) -> Vec<String> {
    let mut lines = vec!["drivers:".to_string()];
    for driver in driver::all(cfg) {
        if driver.id() == "generic" {
            continue;
        }
        match driver.kind() {
            Kind::Run => {
                let experimental = driver
                    .note()
                    .is_some_and(|note| note.contains("experimental"));
                let kind = if experimental {
                    "run, experimental"
                } else {
                    "run"
                };
                let state = match driver.detect(cfg) {
                    Detection::Ready { version } => format!("ready {version}"),
                    Detection::Missing { install, .. } => {
                        format!("missing; install: {install}")
                    }
                    Detection::TooOld {
                        version,
                        min,
                        install,
                    } => {
                        format!("too old {version} (need {min}); install: {install}")
                    }
                    Detection::Unknown => "unknown".to_string(),
                };
                lines.push(format!("  {} ({kind}): {state}", driver.id()));
            }
            Kind::Plan => match driver.note() {
                Some(note) => lines.push(format!("  {} (plan): {note}", driver.id())),
                None => lines.push(format!("  {} (plan)", driver.id())),
            },
        }
    }
    match driver::choose(cfg) {
        Choice::Run(driver, _) => lines.push(format!("call will use: {}", driver.id())),
        Choice::Plan(driver) => lines.push(format!("call will use: {}", driver.id())),
        Choice::Unusable { .. } => {
            lines.push("call will use: nothing yet (no run driver installed)".to_string());
        }
    }
    lines
}

/// The manual install lines for the run drivers plus the plan-driver notes,
/// the same data `call`'s setup help carries.
fn print_manual_lines(cfg: &Config) {
    for driver in driver::all(cfg) {
        if driver.id() == "generic" {
            continue;
        }
        match driver.kind() {
            Kind::Run => {
                if let Some(install) = driver.install() {
                    println!("  {}: install with `{install}`", driver.id());
                }
            }
            Kind::Plan => match driver.note() {
                Some(note) => println!("  {} (plan): {note}", driver.id()),
                None => println!("  {} (plan)", driver.id()),
            },
        }
    }
    println!(
        "set \"driver\" in {} to pick one",
        user_config_path().display()
    );
}

/// The user config path for messages, without failing setup.
fn user_config_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    config::user_path(&|key: &str| std::env::var(key).ok(), &cwd)
}

/// Ask once whether to install Lightpanda; the default is no.
fn prompt_install() -> bool {
    use std::io::Write;
    print!(
        "No browser driver can run WebMCP tools here. Install Lightpanda now (74 MB, one download from pkg.lightpanda.io)? [y/N] "
    );
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Run the documented Lightpanda installer, then re-detect. A failure
/// prints its exit code and the manual commands; it never fails setup.
fn install_lightpanda(cfg: &Config) {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(crate::driver::lightpanda::INSTALL)
        .status();
    match status {
        Ok(status) if status.success() => {
            let driver = crate::driver::lightpanda::Lightpanda::from_config(cfg);
            match driver.detect(cfg) {
                Detection::Ready { version } => {
                    println!("lightpanda: ready {version}");
                }
                _ => {
                    println!(
                        "lightpanda is still missing; if ~/.local/bin is not on your PATH, add it or set drivers.lightpanda.command to the full path in {}",
                        user_config_path().display()
                    );
                }
            }
        }
        Ok(status) => {
            println!("the lightpanda installer failed with {status}; install it manually:");
            print_manual_lines(cfg);
        }
        Err(e) => {
            println!("could not run the lightpanda installer ({e}); install it manually:");
            print_manual_lines(cfg);
        }
    }
}

/// True when `latest` names a newer release than `installed`, by SemVer
/// precedence: `0.3.0` is newer than `0.3.0-rc.1`, which is newer than
/// `0.2.9`. Anything unparsable means "no".
pub fn newer_available(installed: &str, latest: &str) -> bool {
    match (Version::parse(installed), Version::parse(latest)) {
        (Some(a), Some(b)) => b > a,
        _ => false,
    }
}

/// An exact `x.y.z` version, optionally with a prerelease and build suffix.
pub fn is_exact_version(v: &str) -> bool {
    Version::parse(v).is_some()
}

#[derive(PartialEq, Eq)]
struct Version<'a> {
    core: (u64, u64, u64),
    pre: Vec<&'a str>,
}

impl<'a> Version<'a> {
    fn parse(v: &'a str) -> Option<Self> {
        let (v, build) = v.split_once('+').map_or((v, None), |(a, b)| (a, Some(b)));
        if build.is_some_and(|b| !ident_ok(b)) {
            return None;
        }
        let (core, pre) = v.split_once('-').map_or((v, None), |(a, b)| (a, Some(b)));
        let mut parts = core.split('.');
        let mut num = || -> Option<u64> {
            let p = parts.next()?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let core = (num()?, num()?, num()?);
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            Some(p) if ident_ok(p) => p.split('.').collect(),
            Some(_) => return None,
            None => Vec::new(),
        };
        Some(Self { core, pre })
    }
}

fn ident_ok(s: &str) -> bool {
    !s.is_empty()
        && s.split('.')
            .all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

impl PartialOrd for Version<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        self.core
            .cmp(&other.core)
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Equal,
                (true, false) => Greater,
                (false, true) => Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let ord = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Less,
                            (Err(_), Ok(_)) => Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if ord != Equal {
                            return ord;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            })
    }
}
