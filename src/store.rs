//! Where the index lives on disk, and how it is fetched.
//!
//! The server publishes two files: a small pointer (`index.json`) that names
//! the current index file and its sha256, and the immutable index file itself.
//! The client asks the pointer with a conditional request, downloads the index
//! only when its hash changed, verifies it, and swaps it in atomically. Any
//! failure keeps the copy already on disk.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::model::{Index, SCHEMA_VERSION};

/// Check for a newer index when the local copy is older than this.
pub const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// With a copy on disk, give the network this long before using it.
const QUICK_TIMEOUT: Duration = Duration::from_secs(2);
/// With nothing on disk there is nothing to fall back to, so wait longer.
const FIRST_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest index file the client will accept.
const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;

pub const DEFAULT_POINTER_URL: &str = "https://agent-webmcp-index.agent-webmcp.workers.dev/index.json";

#[derive(Debug, Deserialize)]
pub struct Pointer {
    pub sha256: String,
    /// Relative to the pointer's URL, or absolute.
    pub url: String,
}

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("AGENT_WEBMCP_HOME") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(dir).join("agent-webmcp");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    home.join(".local/share/agent-webmcp")
}

pub fn index_path() -> PathBuf {
    data_dir().join("index.json")
}

/// The pointer's last ETag and the sha256 of the index on disk.
fn state_path() -> PathBuf {
    data_dir().join("index.state")
}

pub fn pointer_url() -> String {
    std::env::var("AGENT_WEBMCP_INDEX_URL").unwrap_or_else(|_| DEFAULT_POINTER_URL.to_string())
}

pub fn parse(text: &str) -> Result<Index> {
    let index: Index = serde_json::from_str(text).context("the index file is not valid")?;
    if index.schema_version != SCHEMA_VERSION {
        bail!(
            "the index uses schema {} but this agent-webmcp understands {}; update agent-webmcp",
            index.schema_version,
            SCHEMA_VERSION
        );
    }
    Ok(index)
}

/// Resolve the pointer's `url` against the pointer's own address.
pub fn resolve(pointer_url: &str, target: &str) -> String {
    if target.starts_with("https://") || target.starts_with("http://") {
        return target.to_string();
    }
    let base = pointer_url.rsplit_once('/').map(|(b, _)| b).unwrap_or(pointer_url);
    format!("{base}/{}", target.trim_start_matches('/'))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Load the local index, checking for a newer one when it is missing or
/// older than an hour. A failed check falls back to the copy on disk.
pub fn load() -> Result<Index> {
    if let Some(path) = std::env::var_os("AGENT_WEBMCP_INDEX") {
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", PathBuf::from(&path).display()))?;
        return parse(&text);
    }
    let path = index_path();
    let have_copy = path.exists();
    let stale = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() > STALE_AFTER)
        .unwrap_or(true);
    if stale {
        let timeout = if have_copy { QUICK_TIMEOUT } else { FIRST_TIMEOUT };
        match refresh_with(timeout) {
            Ok(index) => return Ok(index),
            Err(e) if have_copy => {
                eprintln!("agent-webmcp: could not check for a newer index ({e:#}); using the copy on disk");
            }
            Err(e) => return Err(e.context("downloading the index")),
        }
    }
    parse(&std::fs::read_to_string(&path)?)
}

/// Check for a newer index now (the `update` command).
pub fn refresh() -> Result<Index> {
    refresh_with(FIRST_TIMEOUT)
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent(concat!("agent-webmcp/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn refresh_with(timeout: Duration) -> Result<Index> {
    let url = pointer_url();
    let agent = agent(timeout);
    let path = index_path();
    let (stored_etag, stored_sha) = read_state();

    let mut req = agent.get(&url);
    if let (Some(etag), true) = (&stored_etag, path.exists()) {
        req = req.header("If-None-Match", etag);
    }
    let mut resp = req.call().with_context(|| format!("fetching {url}"))?;
    let status = resp.status().as_u16();

    if status == 304 && path.exists() {
        touch(&path);
        return parse(&std::fs::read_to_string(&path)?);
    }
    if status != 200 {
        bail!("{url} answered HTTP {status}");
    }
    let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
    let pointer: Pointer = serde_json::from_str(&resp.body_mut().read_to_string().context("reading the pointer")?)
        .context("the pointer file is not valid")?;

    if path.exists() && stored_sha.as_deref() == Some(pointer.sha256.as_str()) {
        write_state(etag.as_deref(), &pointer.sha256);
        touch(&path);
        return parse(&std::fs::read_to_string(&path)?);
    }

    let file_url = resolve(&url, &pointer.url);
    let mut file = agent.get(&file_url).call().with_context(|| format!("fetching {file_url}"))?;
    if file.status().as_u16() != 200 {
        bail!("{file_url} answered HTTP {}", file.status().as_u16());
    }
    let bytes = file
        .body_mut()
        .with_config()
        .limit(MAX_INDEX_BYTES)
        .read_to_vec()
        .context("reading the index")?;
    let got = sha256_hex(&bytes);
    if !got.eq_ignore_ascii_case(&pointer.sha256) {
        bail!("the downloaded index does not match its hash (expected {}, got {got}); keeping the old copy", pointer.sha256);
    }
    let text = String::from_utf8(bytes).context("the index is not valid UTF-8")?;
    let index = parse(&text)?;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &path)?;
    write_state(etag.as_deref(), &pointer.sha256);
    Ok(index)
}

/// Mark the local copy as just checked, so the next check waits an hour.
fn touch(path: &PathBuf) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

fn read_state() -> (Option<String>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(state_path()) else {
        return (None, None);
    };
    let mut lines = text.lines();
    let etag = lines.next().filter(|l| !l.is_empty()).map(str::to_string);
    let sha = lines.next().filter(|l| !l.is_empty()).map(str::to_string);
    (etag, sha)
}

fn write_state(etag: Option<&str>, sha: &str) {
    let _ = std::fs::write(state_path(), format!("{}\n{sha}\n", etag.unwrap_or("")));
}
