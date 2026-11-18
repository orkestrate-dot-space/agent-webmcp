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
        Configured::Missing => {
            println!("opencode: no config file found; add this under \"mcp\":");
            println!("{}", mcp_snippet(&current_exe_string()?));
        }
    }

    for line in browser_report() {
        println!("{line}");
    }
    println!("other clients (not configured automatically):");
    println!(
        "  claude mcp add agent-webmcp -s user -- {} mcp",
        current_exe_string()?
    );
    println!(
        "  codex mcp add agent-webmcp -- {} mcp",
        current_exe_string()?
    );
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

enum Configured {
    Added(PathBuf),
    Present(PathBuf),
    Missing,
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

fn current_exe_string() -> Result<String> {
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

fn configure_opencode() -> Result<Configured> {
    let Some(path) = opencode_config_path() else {
        return Ok(Configured::Missing);
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let exe = current_exe_string()?;
    let Some(merged) = merge_opencode_mcp(&text, &exe)? else {
        return Ok(Configured::Present(path));
    };
    std::fs::write(&path, merged).with_context(|| format!("writing {}", path.display()))?;
    Ok(Configured::Added(path))
}

/// Merge the agent-webmcp MCP server into an opencode.json document.
/// Returns `Ok(None)` when the server is already listed.
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
        "browser: Playwright MCP needs --enable-features=WebMCP in browser.launchOptions.args,"
            .to_string(),
        "  and lists page tools after a snapshot, not on navigation alone.".to_string(),
        "browser: Chrome DevTools MCP needs --categoryExperimentalWebmcp.".to_string(),
        "browser: OpenCode Browse drives your own Chrome and takes no flags.".to_string(),
    ]
}

/// True when `latest` names a newer release than `installed` ("x.y.z", numeric).
pub fn newer_available(installed: &str, latest: &str) -> bool {
    match (parse_version(installed), parse_version(latest)) {
        (Some(a), Some(b)) => b > a,
        _ => false,
    }
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}
