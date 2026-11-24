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
    // Even when the sync fails, still report drivers below so users see
    // their driver status; the sync error is returned at the end so scripts
    // still detect the incomplete sync.
    let sync_result = sync_skills(&skill_dirs(&home, xdg.as_deref()));
    if let Ok(sync) = &sync_result {
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
    // Return the sync error (if any) after reporting drivers, so automation
    // detects the incomplete sync while users still see driver status.
    sync_result?;
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    crate::config::home_dir(&|key| std::env::var(key).ok())
        .context("HOME (or USERPROFILE on Windows) is not set; cannot install the skill")
}

/// Skill directories to populate: always `~/.agents/skills`, plus each of
/// `~/.claude/skills`, `<xdg-or-home-config>/opencode/skills`,
/// `~/.codex/skills`, `~/.openclaw/skills`, `~/.cursor/skills` only when
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
    if home.join(".codex").is_dir() {
        dirs.push(home.join(".codex").join("skills"));
    }
    if home.join(".openclaw").is_dir() {
        dirs.push(home.join(".openclaw").join("skills"));
    }
    if home.join(".cursor").is_dir() {
        dirs.push(home.join(".cursor").join("skills"));
    }
    dirs
}

/// Install or update `agent-webmcp/SKILL.md` in each target skill directory.
/// Every directory is attempted even if an earlier one fails; the first
/// error is returned after the loop so later writable client dirs still get
/// synced.
///
/// Symlink and FIFO safety: `symlink_metadata` is checked first so a
/// symlinked stub is never read through or overwritten (its referent would
/// be clobbered), and non-regular files (FIFO, socket, directory, …) are
/// rejected without opening them (which could block). The existing file is
/// opened with `O_NONBLOCK` (plus `O_NOFOLLOW`) on Unix via
/// `OpenOptionsExt::custom_flags` before re-validating (`is_file`, plus
/// `dev`/`ino`) so a symlink/FIFO swap between check and open is caught;
/// raw bytes are compared so invalid UTF-8 stubs get replaced rather than
/// failing. Regular files are replaced atomically via a temp file in the same
/// directory plus rename, so a failed write keeps the old stub intact.
/// Initial installs publish via a no-replace link so a concurrently created
/// symlink is never overwritten.
pub fn sync_skills(dirs: &[PathBuf]) -> Result<SkillSync> {
    let mut sync = SkillSync::default();
    let mut first_err: Option<anyhow::Error> = None;
    for dir in dirs {
        let target = dir.join("agent-webmcp").join("SKILL.md");
        let step: Result<Option<bool>> = match std::fs::symlink_metadata(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                match publish_new(&target, SKILL) {
                    Ok(true) => Ok(Some(true)),
                    Ok(false) => {
                        // Target appeared concurrently: reclassify through the
                        // existing update-or-reject path instead of overwriting.
                        classify_existing(&target)
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(anyhow::Error::from(e))
                .with_context(|| format!("checking {}", target.display())),
            Ok(lstat) => update_existing(&target, &lstat),
        };
        match step {
            Ok(None) => {}
            Ok(Some(true)) => sync.installed.push(target),
            Ok(Some(false)) => sync.updated.push(target),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    if let Some(err) = first_err {
        return Err(err);
    }
    Ok(sync)
}

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Open an existing stub without blocking on a swapped-in FIFO and without
/// following a swapped-in symlink. Platform-provided flags: hand-rolled
/// octal constants are wrong on some targets (`0o2000` is `O_APPEND`, not
/// `O_NONBLOCK`, on Linux).
#[cfg(unix)]
fn open_existing(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_existing(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Classify an existing target (symlink/non-file rejected; regular files
/// compared and updated when stale). Shared by the normal update path and
/// the concurrent-create fallback for initial installs.
fn classify_existing(target: &Path) -> Result<Option<bool>> {
    let lstat = std::fs::symlink_metadata(target)
        .with_context(|| format!("checking {}", target.display()))?;
    update_existing(target, &lstat)
}

/// Update path for an existing target described by `lstat`.
fn update_existing(target: &Path, lstat: &std::fs::Metadata) -> Result<Option<bool>> {
    if lstat.file_type().is_symlink() {
        anyhow::bail!("{} is a symlink; refusing to overwrite", target.display());
    }
    if !lstat.file_type().is_file() {
        anyhow::bail!(
            "{} is not a regular file; refusing to read or overwrite",
            target.display()
        );
    }
    // Open with O_NONBLOCK|O_NOFOLLOW on Unix so a FIFO/symlink swap after
    // the `symlink_metadata` check cannot block or redirect the read.
    // A swapped-in symlink fails open (O_NOFOLLOW); a FIFO fails the
    // `is_file` check below without blocking.
    let file = open_existing(target).with_context(|| format!("reading {}", target.display()))?;
    let opened = file
        .metadata()
        .with_context(|| format!("reading {}", target.display()))?;
    if !opened.file_type().is_file() {
        anyhow::bail!(
            "{} is not a regular file; refusing to read or overwrite",
            target.display()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if lstat.dev() != opened.dev() || lstat.ino() != opened.ino() {
            anyhow::bail!(
                "{} changed during read; refusing to overwrite",
                target.display()
            );
        }
    }
    // Raw bytes: invalid UTF-8 stubs compare unequal and get
    // replaced instead of failing the sync. Bounded to `SKILL.len() + 1`
    // bytes so a huge `SKILL.md` never allocates unbounded memory: any
    // file longer than the stub must differ.
    use std::io::Read;
    let mut existing = Vec::new();
    file.take((SKILL.len() + 1) as u64)
        .read_to_end(&mut existing)
        .with_context(|| format!("reading {}", target.display()))?;
    if existing == SKILL.as_bytes() {
        return Ok(None);
    }
    write_atomic_replace(target, SKILL)
        .with_context(|| format!("updating {}", target.display()))?;
    Ok(Some(false))
}

/// Publish an initial install without ever overwriting an existing path:
/// write a unique temp file, then `hard_link` it to `target` (no-replace)
/// and remove the temp. Returns `Ok(true)` on install, `Ok(false)` when
/// `target` appeared concurrently (caller must reclassify via
/// `classify_existing`), propagating other errors.
fn publish_new(target: &Path, contents: &str) -> Result<bool> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let parent = target.parent().unwrap_or(Path::new("."));
    let file_name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "SKILL.md".to_string());
    loop {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".{file_name}.tmp.{}-{n}", std::process::id()));
        let file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("writing {}", tmp.display())),
        };
        let wrote: Result<()> = (|| {
            use std::io::Write;
            let mut file = file;
            file.write_all(contents.as_bytes())
                .with_context(|| format!("writing {}", tmp.display()))?;
            file.sync_all()
                .with_context(|| format!("writing {}", tmp.display()))?;
            drop(file);
            Ok(())
        })();
        if let Err(e) = wrote {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        match std::fs::hard_link(&tmp, target) {
            Ok(()) => {
                let _ = std::fs::remove_file(&tmp);
                return Ok(true);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = std::fs::remove_file(&tmp);
                return Ok(false);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                // No unsafe `rename` fallback here: overwriting could clobber
                // a concurrently created symlink. Surface a clear error.
                let ctx = hard_link_context(target, &e);
                return Err(e).context(ctx);
            }
        }
    }
}

/// Context for a failed `hard_link` publish (no `rename` fallback, which
/// could clobber a concurrently created symlink). Notes hard-link support
/// when the filesystem reports `Unsupported`.
fn hard_link_context(target: &Path, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::Unsupported {
        format!(
            "writing {} via hard link: hard-link support is required on this filesystem",
            target.display()
        )
    } else {
        format!("writing {} via hard link", target.display())
    }
}

/// Write `contents` without ever truncating `target` in place: create the
/// parent, write a unique temp file in the same directory (so the rename
/// stays on one filesystem), then atomically replace `target`. A crash
/// mid-write leaves the old stub, never half a file.
///
/// CWE-59 hardening: the temp file is created exclusively (`create_new`), so
/// a pre-existing symlink/file is never truncated; on `AlreadyExists` the
/// counter advances and the next name is tried. The temp file is removed on
/// error only when this call created it. Used only for updates (target
/// already validated as a regular file); initial installs use `publish_new`.
fn write_atomic_replace(target: &Path, contents: &str) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let parent = target.parent().unwrap_or(Path::new("."));
    let file_name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "SKILL.md".to_string());
    loop {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".{file_name}.tmp.{}-{n}", std::process::id()));
        let file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("writing {}", tmp.display())),
        };
        // This call created `tmp`; clean it up on any later failure.
        let written: Result<()> = (|| {
            use std::io::Write;
            let mut file = file;
            file.write_all(contents.as_bytes())
                .with_context(|| format!("writing {}", tmp.display()))?;
            file.sync_all()
                .with_context(|| format!("writing {}", tmp.display()))?;
            drop(file);
            std::fs::rename(&tmp, target)
                .with_context(|| format!("writing {}", target.display()))?;
            Ok(())
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        return written;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_link_context_mentions_support_on_unsupported() {
        let target = Path::new("/tmp/skills/agent-webmcp/SKILL.md");
        let unsupported = std::io::Error::new(std::io::ErrorKind::Unsupported, "op not supported");
        let ctx = hard_link_context(target, &unsupported);
        assert!(ctx.contains("via hard link"), "{ctx}");
        assert!(ctx.contains("hard-link support is required"), "{ctx}");
        assert!(ctx.contains(&target.display().to_string()), "{ctx}");

        let other = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let ctx = hard_link_context(target, &other);
        assert!(ctx.contains("via hard link"), "{ctx}");
        assert!(!ctx.contains("hard-link support is required"), "{ctx}");
    }
}
