//! `agent-webmcp upgrade`: replace this CLI with a newer release the way it
//! was installed. npm global installs and install.sh copies upgrade in
//! place; anything else gets the command to run by hand.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::setup::{is_exact_version, newer_available};

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

/// Classify by path shape alone. `run` additionally checks that the prefix
/// holds the npm shim, so a project under a folder named `lib` is not taken
/// for a global install.
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

/// Accept `latest`, `next`, or an exact version (`0.2.1`, `v0.3.0-rc.1`).
/// Ranges like `0.2` and anything else are refused before they reach npm.
pub fn normalize_target(target: Option<&str>) -> Result<String> {
    let t = target.unwrap_or("latest").trim();
    let t = t.strip_prefix('v').unwrap_or(t);
    if matches!(t, "latest" | "next") || is_exact_version(t) {
        return Ok(t.to_string());
    }
    bail!("not a version: {t:?} (use latest, next, or an exact version like 0.2.1)");
}

fn npm() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

fn shim(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.join("agent-webmcp.cmd")
    } else {
        prefix.join("bin").join("agent-webmcp")
    }
}

/// The install command for a user to run, aimed at the detected prefix.
fn manual(prefix: Option<&Path>, version: &str) -> String {
    match prefix {
        Some(p) => format!("npm i -g --prefix \"{}\" {PACKAGE}@{version}", p.display()),
        None => format!("npm i -g {PACKAGE}@{version}"),
    }
}

/// Resolve a tag or version to one exact version, with the same global
/// config the install will use (run outside any project, `--json` output).
fn resolve(prefix: &Path, target: &str) -> Result<String> {
    let out = Command::new(npm())
        .current_dir(prefix)
        .args(["view", "--json", "-g", "--prefix"])
        .arg(prefix)
        .arg(format!("{PACKAGE}@{target}"))
        .arg("version")
        .output()
        .context("running npm (is it on PATH?)")?;
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    if !out.status.success() {
        let why = value["error"]["summary"]
            .as_str()
            .unwrap_or("npm view failed");
        bail!(
            "cannot resolve {PACKAGE}@{target}: {}",
            why.lines().next().unwrap_or(why)
        );
    }
    // A single match prints a string; npm 10+ wraps it in a one-item array.
    let v = match &value {
        serde_json::Value::String(s) => Some(s.as_str()),
        serde_json::Value::Array(a) if a.len() == 1 => a[0].as_str(),
        _ => None,
    };
    match v {
        Some(v) if is_exact_version(v) => Ok(v.to_string()),
        _ => bail!("npm returned no single version for {PACKAGE}@{target}"),
    }
}

const INSTALL_SH: &str =
    "https://github.com/orkestrate-dot-space/agent-webmcp/releases/latest/download/install.sh";

/// The binary the install script wrote, when this is it. The script leaves
/// a receipt naming the file it installed.
fn script_install(exe: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(crate::store::data_dir().join("install-receipt")).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "script" {
        return None;
    }
    let path = PathBuf::from(lines.next()?);
    let canon = std::fs::canonicalize(&path).unwrap_or(path.clone());
    (canon == exe).then_some(path)
}

/// Upgrade a copy installed by install.sh by running the script again. The
/// version and directory go in as environment variables, never into the
/// shell text.
fn upgrade_script(path: &Path, target: &str, explicit: bool, current: &str) -> Result<()> {
    let wanted = match target {
        "latest" => crate::store::latest_cli_remote()
            .filter(|v| is_exact_version(v))
            .context(
                "cannot read the latest release from the index; try again, or name a version",
            )?,
        "next" => bail!(
            "`next` builds ship through npm only; name a version or use npm i -g {PACKAGE}@next"
        ),
        v => v.to_string(),
    };
    if wanted == current {
        println!("agent-webmcp {current} is already the {target} release");
        return Ok(());
    }
    if !explicit && !newer_available(current, &wanted) {
        println!("agent-webmcp {current} is newer than {target} ({wanted}); nothing to do");
        return Ok(());
    }
    let dir = path.parent().context("install path has no directory")?;
    println!("upgrading agent-webmcp {current} -> {wanted}");
    let status = Command::new("sh")
        .arg("-c")
        // Save the script first so a failed download fails the upgrade (`sh`
        // has no pipefail). curl or wget, like install.sh itself.
        .arg(format!(
            r#"f=$(mktemp) || exit 1; trap 'rm -f "$f"' EXIT
if command -v curl >/dev/null 2>&1; then curl -fsSL --proto '=https' --tlsv1.2 -o "$f" {INSTALL_SH} || exit 1
elif command -v wget >/dev/null 2>&1; then wget -q -O "$f" {INSTALL_SH} || exit 1
else echo 'agent-webmcp upgrade: needs curl or wget' >&2; exit 1; fi
sh "$f""#
        ))
        .env("AGENT_WEBMCP_VERSION", &wanted)
        .env("AGENT_WEBMCP_INSTALL_DIR", dir)
        .status()
        .context("running the install script")?;
    if !status.success() {
        bail!(
            "the install script failed; run it yourself: curl -fsSL {INSTALL_SH} | sh -s -- --version {wanted}"
        );
    }
    let now = Command::new(path)
        .current_dir(std::env::temp_dir())
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if now != format!("agent-webmcp {wanted}") {
        bail!("the script finished but {} reports {now:?}", path.display());
    }
    Ok(())
}

pub fn run(target: Option<String>) -> Result<()> {
    let explicit = target.is_some();
    let target = normalize_target(target.as_deref())?;
    let current = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);

    if !cfg!(windows)
        && let Some(path) = script_install(&exe)
    {
        return upgrade_script(&path, &target, explicit, current);
    }

    let prefix = match detect(&exe) {
        Install::NpmGlobal(prefix) if shim(&prefix).exists() => prefix,
        _ => {
            println!(
                "agent-webmcp {current} was not installed by npm or the install script ({}).\nReinstall with: {}\n          or: curl -fsSL {INSTALL_SH} | sh",
                exe.display(),
                manual(None, &target)
            );
            return Ok(());
        }
    };

    let wanted = resolve(&prefix, &target)?;
    if wanted == current {
        println!("agent-webmcp {current} is already the {target} release");
        return Ok(());
    }
    // A bare `upgrade` never moves backwards (say, a prerelease user while
    // `latest` still points at the previous version). Naming a version does.
    if !explicit && !newer_available(current, &wanted) {
        println!("agent-webmcp {current} is newer than {target} ({wanted}); nothing to do");
        return Ok(());
    }

    // Windows locks a running .exe, so npm cannot replace this binary while
    // it runs. Hand over the command instead.
    if cfg!(windows) {
        println!(
            "agent-webmcp {wanted} is available. Close agent-webmcp, then run: {}",
            manual(Some(&prefix), &wanted)
        );
        return Ok(());
    }

    println!("upgrading agent-webmcp {current} -> {wanted}");
    let status = Command::new(npm())
        .current_dir(&prefix)
        .args(["i", "-g", "--prefix"])
        .arg(&prefix)
        .arg(format!("{PACKAGE}@{wanted}"))
        .status()
        .context("running npm")?;
    if !status.success() {
        bail!(
            "npm install failed; run it yourself: {}",
            manual(Some(&prefix), &wanted)
        );
    }

    // Verify from a neutral directory, so the wrapper cannot fall back to a
    // project's own copy and report a version the global install lacks.
    let check = Command::new(shim(&prefix))
        .current_dir(std::env::temp_dir())
        .arg("--version")
        .output();
    let now = match &check {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };
    if now != format!("agent-webmcp {wanted}") {
        bail!(
            "npm finished but {} reports {:?}; run: {}",
            shim(&prefix).display(),
            now,
            manual(Some(&prefix), &wanted)
        );
    }
    println!("{now} installed. The MCP server picks it up the next time your client starts it.");
    Ok(())
}
