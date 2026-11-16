use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Error,
    /// The call needs approval and there was nobody to ask.
    ApprovalRequired,
    /// Approval was asked for, but there was no usable answer.
    Unanswered,
    /// The user was asked and said no.
    Declined,
}

/// One line of the ledger. Arguments are stored as a hash, never in the clear.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub ts_ms: u128,
    pub id: String,
    pub outcome: Outcome,
    pub approved: bool,
    pub args_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Record {
    pub fn new(id: &str, outcome: Outcome, approved: bool, args: &serde_json::Value) -> Self {
        Self {
            ts_ms: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0),
            id: id.to_string(),
            outcome,
            approved,
            args_sha256: hash_args(args),
            origin: None,
            duration_ms: None,
            error: None,
        }
    }
}

pub fn hash_args(args: &serde_json::Value) -> String {
    let digest = Sha256::digest(args.to_string().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Append-only JSON Lines file.
#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$AGENT_WEBMCP_HOME/ledger.jsonl`, else `~/.local/share/agent-webmcp/ledger.jsonl`.
    pub fn default_location() -> Self {
        Self::at(data_dir().join("ledger.jsonl"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, record: &Record) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        // One write call per line, in append mode, so concurrent processes do not interleave.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("opening {}", self.path.display()))?;
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<Record>> {
        let text = match fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).with_context(|| format!("bad ledger line: {l}")))
            .collect()
    }
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
