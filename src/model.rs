//! The index: the WebMCP tools each checked site registers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub schema_version: u32,
    /// When this index file was assembled, RFC 3339.
    pub generated_at: String,
    pub sites: Vec<Site>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// At least one tool was seen at the last check.
    Verified,
    /// The pages loaded and registered no tools.
    NoTools,
    /// A bot wall or challenge stopped the check. Recorded, never solved.
    Blocked,
    /// The check failed for another reason (timeout, network, crash).
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    /// Hostname without `www.`, for example `docs.stripe.com`.
    pub host: String,
    pub status: Status,
    /// When the site was last checked, RFC 3339.
    pub checked_at: String,
    /// Pages that were loaded during the check.
    pub pages_checked: Vec<String>,
    /// A known platform whose tools this site shares, such as `mintlify`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    /// Why the check did not produce tools, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Consecutive checks that saw no tools after the site last had some.
    /// The site keeps its last tools until this reaches the drop limit.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub missed: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
    /// The first page the tool was seen on. Agents open this URL.
    pub page: String,
    /// The origin of the frame that registered the tool.
    pub origin: String,
    #[serde(default)]
    pub annotations: Annotations,
    /// Set when the description was flagged when the index was built (for example, it gives
    /// the agent orders). Flagged tools are kept but ranked last and marked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Annotations {
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub consequential: bool,
    #[serde(default)]
    pub untrusted_content: bool,
}
