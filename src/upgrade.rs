//! `agent-webmcp upgrade`: replace this CLI with a newer release through the
//! package manager that installed it. Only npm global installs are upgraded
//! in place; anything else gets the command to run by hand.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const PACKAGE: &str = "@orkestrate/agent-webmcp";

/// How this binary was installed, read from its own path.
#[derive(Debug, PartialEq, Eq)]
pub enum Install {
    /// `npm i -g`: the npm prefix that holds `lib/node_modules` (or
    /// `node_modules` on Windows).
    NpmGlobal(PathBuf),
    /// Anything else: a cargo build, a copied binary, a project-local install.
    Other,
}

pub fn detect(exe: &Path) -> Install {
    let parts: Vec<_> = exe.components().collect();
    let Some(i) = parts.iter().position(|c| c.as_os_str() == "node_modules") else {
        return Install::Other;
    };
    let before: PathBuf = parts[..i].iter().collect();
    // Unix global: <prefix>/lib/node_modules. Windows global:
    // <prefix>/node_modules, where <prefix> holds the npm shims.
    if before.file_name().is_some_and(|n| n == "lib")
        && let Some(prefix) = before.parent()
    {
        return Install::NpmGlobal(prefix.to_path_buf());
    }
    if cfg!(windows) && before.join("agent-webmcp.cmd").exists() {
        return Install::NpmGlobal(before);
    }
    Install::Other
}

/// Accept `latest`, `next`, or a version with or without a leading `v`.
/// Anything else is refused before it reaches npm.
pub fn normalize_target(target: Option<&str>) -> Result<String> {
    let t = target.unwrap_or("latest").trim();
    let t = t.strip_prefix('v').unwrap_or(t);
    let ok = matches!(t, "latest" | "next")
        || (!t.is_empty()
            && t.chars().next().is_some_and(|c| c.is_ascii_digit())
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')));
    if !ok {
        bail!("not a version: {t:?} (use latest, next, or a version like 0.2.1)");
    }
    Ok(t.to_string())
}

fn npm() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

fn manual(target: &str) -> String {
    format!("npm i -g {PACKAGE}@{target}")
}

pub fn run(target: Option<String>) -> Result<()> {
    let explicit = target.is_some();
    let target = normalize_target(target.as_deref())?;
    let current = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);

    let prefix = match detect(&exe) {
        Install::NpmGlobal(prefix) => prefix,
        Install::Other => {
            println!(
                "agent-webmcp {current} was not installed with npm i -g ({}).\nTo switch to the npm release, run: {}",
                exe.display(),
                manual(&target)
            );
            return Ok(());
        }
    };

    let resolved = Command::new(npm())
        .args(["view", &format!("{PACKAGE}@{target}"), "version"])
        .output()
        .context("running npm (is it on PATH?)")?;
    let wanted = String::from_utf8_lossy(&resolved.stdout)
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string();
    if !resolved.status.success() || wanted.is_empty() {
        bail!("npm has no {PACKAGE}@{target}");
    }
    if wanted == current {
        println!("agent-webmcp {current} is already the {target} release");
        return Ok(());
    }
    // A bare `upgrade` never moves backwards (say, a prerelease user while
    // `latest` still points at the previous version). Naming a version does.
    if !explicit && !crate::setup::newer_available(current, &wanted) {
        println!("agent-webmcp {current} is newer than {target} ({wanted}); nothing to do");
        return Ok(());
    }

    // Windows locks a running .exe, so npm cannot replace this binary while
    // it runs. Hand over the command instead.
    if cfg!(windows) {
        println!(
            "agent-webmcp {wanted} is available. Close agent-webmcp and run: {}",
            manual(&wanted)
        );
        return Ok(());
    }

    println!("upgrading agent-webmcp {current} -> {wanted}");
    let status = Command::new(npm())
        .args(["i", "-g", "--prefix"])
        .arg(&prefix)
        .arg(format!("{PACKAGE}@{wanted}"))
        .status()
        .context("running npm")?;
    if !status.success() {
        bail!("npm install failed; run it yourself: {}", manual(&wanted));
    }

    let shim = prefix.join("bin").join("agent-webmcp");
    let now = Command::new(&shim)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if now != format!("agent-webmcp {wanted}") {
        bail!(
            "npm finished but {} reports {:?}; run: {}",
            shim.display(),
            now,
            manual(&wanted)
        );
    }
    println!("{now} installed. The MCP server picks it up the next time your client starts it.");
    Ok(())
}
