//! First-run setup: fetch the index, install or update the skill for
//! detected clients, and report available browser drivers.
//!
//! Setup never edits client MCP configs or installs drivers automatically.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{self, Config};
use crate::driver::{self, Choice, Detection, Kind};

const SKILL: &str = include_str!("../skills/agent-webmcp/SKILL.md");

/// Summary of files written by [`sync_skills`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SkillSync {
    pub installed: Vec<PathBuf>,
    pub updated: Vec<PathBuf>,
}

/// True when any run driver (`agent-browser`, `browse`, or `lightpanda`) detects as Ready.
fn any_run_ready(cfg: &Config) -> bool {
    driver::all(cfg)
        .iter()
        .any(|d| d.kind() == Kind::Run && matches!(d.detect(cfg), Detection::Ready { .. }))
}

/// Run the first-run setup and report what changed.
pub fn run() -> Result<()> {
    let index = crate::store::refresh()?;
    let (sites, tools) = crate::model::count_index(&index.sites);
    println!(
        "index: {sites} sites, {tools} tools ({})",
        index.generated_at
    );

    let home = home_dir()?;
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let sync = sync_skills(&skill_dirs(&home, xdg.as_deref()))?;
    if sync.installed.is_empty() && sync.updated.is_empty() {
        println!("skill: already up to date");
    } else {
        for p in &sync.installed {
            println!("skill installed: {}", p.display());
        }
        for p in &sync.updated {
            println!("skill updated: {}", p.display());
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
    if !any_run_ready(&cfg) {
        print_manual_lines(&cfg);
    }
    println!(
        "mcp (optional): run `agent-webmcp mcp` as a stdio server in your client if you prefer MCP over the CLI"
    );
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot install the skill")
}

/// Skill directories to populate: always `~/.agents/skills`, plus
/// `~/.claude/skills` and `<xdg-or-home-config>/opencode/skills` only when
/// those client directories already exist.
pub fn skill_dirs(home: &Path, xdg_config_home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![home.join(".agents").join("skills")];
    if home.join(".claude").is_dir() {
        dirs.push(home.join(".claude").join("skills"));
    }
    let opencode_dir = match xdg_config_home.filter(|p| !p.as_os_str().is_empty()) {
        Some(xdg) => xdg.join("opencode"),
        None => home.join(".config").join("opencode"),
    };
    if opencode_dir.is_dir() {
        dirs.push(opencode_dir.join("skills"));
    }
    dirs
}

/// Install or update `agent-webmcp/SKILL.md` in each target skill directory.
pub fn sync_skills(dirs: &[PathBuf]) -> Result<SkillSync> {
    let mut sync = SkillSync::default();
    for dir in dirs {
        let target = dir.join("agent-webmcp").join("SKILL.md");
        if let Ok(existing) = std::fs::read_to_string(&target) {
            if existing == SKILL {
                continue;
            }
            std::fs::write(&target, SKILL)
                .with_context(|| format!("updating {}", target.display()))?;
            sync.updated.push(target);
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&target, SKILL).with_context(|| format!("writing {}", target.display()))?;
        sync.installed.push(target);
    }
    Ok(sync)
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

/// Print non-invasive manual install instructions when no run driver is ready.
fn print_manual_lines(cfg: &Config) {
    println!("no run driver ready; install one to run `inspect` and `call`:");
    for driver in driver::all(cfg) {
        if driver.id() == "generic" || driver.kind() != Kind::Run {
            continue;
        }
        if let Some(install) = driver.install() {
            let label = match driver.id() {
                "agent-browser" => "agent-browser (recommended)",
                "browse" => "browse (your Chrome)",
                "lightpanda" => "lightpanda (experimental)",
                other => other,
            };
            println!("  {label}: {install}");
        }
    }
    println!(
        "or use `--plan` / `--driver none` with another browser driver (Playwright, Chrome DevTools, etc.)"
    );
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
