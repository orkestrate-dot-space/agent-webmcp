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

/// The public address, then the Worker behind it. If the website's rewrite is
/// ever broken or moved, the CLI still reaches the index.
pub const POINTER_URLS: &[&str] = &[
    "https://web.orkestrate.space/webmcp/index.json",
    "https://agent-webmcp-index.agent-webmcp.workers.dev/index.json",
];

#[derive(Debug, Deserialize)]
pub struct Pointer {
    pub sha256: String,
    /// Relative to the pointer's URL, or absolute.
    pub url: String,
    /// Newest released CLI; absent until the first release publishes one.
    #[serde(default)]
    pub latest_cli: Option<String>,
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

/// Addresses to try, in order: an explicit override, or the public address
/// followed by the Worker behind it.
pub fn pointer_urls() -> Vec<String> {
    match std::env::var("AGENT_WEBMCP_INDEX_URL") {
        Ok(url) if !url.is_empty() => vec![url],
        _ => POINTER_URLS.iter().map(|u| u.to_string()).collect(),
    }
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
    let candidates = pointer_urls();
    let mut first_error = None;
    for url in &candidates {
        match refresh_from(url, timeout) {
            Ok(index) => return Ok(index),
            Err(e) => {
                first_error.get_or_insert_with(|| e.context(format!("fetching {url}")));
            }
        }
    }
    Err(first_error.unwrap_or_else(|| anyhow::anyhow!("no index address configured")))
}

fn refresh_from(url: &str, timeout: Duration) -> Result<Index> {
    let agent = agent(timeout);
    let path = index_path();
    let state = read_state();

    // An ETag is only meaningful for the origin that issued it.
    let mut req = agent.get(url);
    if let (Some(etag), Some(origin), true) = (&state.etag, &state.origin, path.exists())
        && origin == url
    {
        req = req.header("If-None-Match", etag);
    }
    let mut resp = req.call()?;
    let status = resp.status().as_u16();

    if status == 304 && path.exists() {
        touch(&path);
        return parse(&std::fs::read_to_string(&path)?);
    }
    if status != 200 {
        bail!("HTTP {status}");
    }
    let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
    let pointer: Pointer = serde_json::from_str(&resp.body_mut().read_to_string().context("reading the pointer")?)
        .context("the pointer file is not valid")?;

    if path.exists() && state.sha.as_deref() == Some(pointer.sha256.as_str()) {
        write_state(etag.as_deref(), &pointer.sha256, url);
        touch(&path);
        return parse(&std::fs::read_to_string(&path)?);
    }

    let file_url = resolve(url, &pointer.url);
    let mut file = agent.get(&file_url).call()?;
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
    write_state(etag.as_deref(), &pointer.sha256, url);
    Ok(index)
}

/// The newest released CLI version, when the pointer names one. A failure
/// means "unknown", never an error: the index itself is what matters.
pub fn latest_cli_remote() -> Option<String> {
    let timeout = if index_path().exists() { QUICK_TIMEOUT } else { FIRST_TIMEOUT };
    let agent = agent(timeout);
    for url in pointer_urls() {
        let Ok(mut resp) = agent.get(&url).call() else {
            continue;
        };
        if resp.status().as_u16() != 200 {
            continue;
        }
        let Ok(text) = resp.body_mut().read_to_string() else {
            continue;
        };
        if let Ok(pointer) = serde_json::from_str::<Pointer>(&text)
            && let Some(latest) = pointer.latest_cli
            && !latest.is_empty()
        {
            return Some(latest);
        }
    }
    None
}

/// Mark the local copy as just checked, so the next check waits an hour.
fn touch(path: &PathBuf) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

#[derive(Default)]
struct State {
    etag: Option<String>,
    sha: Option<String>,
    origin: Option<String>,
}

fn read_state() -> State {
    let Ok(text) = std::fs::read_to_string(state_path()) else {
        return State::default();
    };
    let mut lines = text.lines();
    let field = |l: Option<&str>| l.filter(|v| !v.is_empty()).map(str::to_string);
    State { etag: field(lines.next()), sha: field(lines.next()), origin: field(lines.next()) }
}

fn write_state(etag: Option<&str>, sha: &str, origin: &str) {
    let _ = std::fs::write(state_path(), format!("{}\n{sha}\n{origin}\n", etag.unwrap_or("")));
}
