//! Where the index lives on disk, and how it is fetched.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};

use crate::model::{Index, SCHEMA_VERSION};

/// Refresh the local index when it is older than this.
pub const STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub const DEFAULT_INDEX_URL: &str =
    "https://raw.githubusercontent.com/orkestrate-dot-space/agent-webmcp/main/index/index.json";

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

pub fn index_url() -> String {
    std::env::var("AGENT_WEBMCP_INDEX_URL").unwrap_or_else(|_| DEFAULT_INDEX_URL.to_string())
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

/// Load the local index, downloading it first when missing or stale. A failed
/// refresh falls back to the copy on disk.
pub fn load() -> Result<Index> {
    if let Some(path) = std::env::var_os("AGENT_WEBMCP_INDEX") {
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", PathBuf::from(&path).display()))?;
        return parse(&text);
    }
    let path = index_path();
    let stale = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() > STALE_AFTER)
        .unwrap_or(true);
    if stale {
        match refresh() {
            Ok(index) => return Ok(index),
            Err(e) if path.exists() => {
                eprintln!("agent-webmcp: could not refresh the index ({e:#}); using the copy on disk");
            }
            Err(e) => return Err(e.context("downloading the index")),
        }
    }
    parse(&std::fs::read_to_string(&path)?)
}

/// Download the index and save it.
pub fn refresh() -> Result<Index> {
    let url = index_url();
    let text = ureq::get(&url)
        .header("User-Agent", concat!("agent-webmcp/", env!("CARGO_PKG_VERSION")))
        .call()
        .with_context(|| format!("fetching {url}"))?
        .body_mut()
        .read_to_string()
        .context("reading the index")?;
    let index = parse(&text)?;
    let path = index_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(index)
}
