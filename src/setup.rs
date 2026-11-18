//! First-run setup: fetch the index, install the skill, wire the MCP server
//! into OpenCode, print the one-line installs for Claude Code and Codex, and
//! report the per-driver WebMCP setup.
//!
//! Everything here is additive: files are only created or extended, never
//! overwritten, and `setup` reports each change it made.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const SKILL: &str = include_str!("../skills/agent-webmcp/SKILL.md");

/// Run the whole setup and report what changed.
pub fn run() -> Result<()> {
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

    for line in browser_report() {
        println!("{line}");
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

/// Human-readable notes about each driver's WebMCP setup, verified against
/// the drivers on 18 Nov 2026. agent-browser turns WebMCP on itself; the
/// others need a flag the user adds to their own config.
pub fn browser_report() -> Vec<String> {
    vec![
        "browser: agent-browser 0.43+ enables WebMCP itself; no flags needed.".to_string(),
        "browser: Playwright MCP needs --enable-features=WebMCP in browser.launchOptions.args; tools appear after a snapshot.".to_string(),
        "browser: Chrome DevTools MCP needs --categoryExperimentalWebmcp.".to_string(),
        "browser: OpenCode Browse drives your own Chrome and takes no flags.".to_string(),
    ]
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
