//! Full-text search over the index, and the `find` result shape.

use std::collections::BTreeMap;

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::Serialize;
use serde_json::Value;

use crate::model::{Index, Status};

/// One search hit, as agents see it.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    /// `<host>/<tool>`, for example `docs.stripe.com/search_docs`.
    pub id: String,
    pub site: String,
    pub tool: String,
    pub description: String,
    /// Open this page with your browser driver; the tool is registered there.
    pub open: String,
    pub kind: &'static str,
    pub input_schema: Value,
    pub checked_at: String,
    /// When the tool is a platform's shared tool, the platform and how many
    /// indexed sites share it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Platform {
    pub name: String,
    pub sites: usize,
}

struct Row {
    site: usize,
    tool: usize,
}

pub struct Search {
    conn: Connection,
    rows: Vec<Row>,
    index: Index,
    platform_sites: BTreeMap<String, usize>,
}

impl Search {
    /// Build the search over verified sites only.
    pub fn build(index: Index) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE VIRTUAL TABLE tools USING fts5(site, name, description, params, tokenize = 'porter unicode61');",
        )?;
        let mut rows = Vec::new();
        let mut platform_sites: BTreeMap<String, usize> = BTreeMap::new();
        {
            let mut insert =
                conn.prepare("INSERT INTO tools (rowid, site, name, description, params) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for (si, site) in index.sites.iter().enumerate() {
                if site.status != Status::Verified {
                    continue;
                }
                if let Some(p) = &site.platform {
                    *platform_sites.entry(p.clone()).or_default() += 1;
                }
                for (ti, tool) in site.tools.iter().enumerate() {
                    rows.push(Row { site: si, tool: ti });
                    insert.execute(params![
                        rows.len() as i64,
                        words(&site.host),
                        words(&tool.name),
                        tool.description,
                        param_words(&tool.input_schema),
                    ])?;
                }
            }
        }
        Ok(Self { conn, rows, index, platform_sites })
    }

    pub fn sites(&self) -> usize {
        self.index.sites.iter().filter(|s| s.status == Status::Verified).count()
    }

    pub fn tools(&self) -> usize {
        self.rows.len()
    }

    pub fn generated_at(&self) -> &str {
        &self.index.generated_at
    }

    /// Best matches first. A platform's shared tool appears once (its
    /// best-ranked site), with the number of sites that share it. Flagged
    /// tools come after clean ones.
    pub fn find(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT rowid FROM tools WHERE tools MATCH ?1 ORDER BY bm25(tools, 8.0, 5.0, 2.0, 1.0) LIMIT ?2",
        )?;
        let wanted = (limit * 20).max(200) as i64;
        let ids: Vec<i64> = stmt
            .query_map(params![fts, wanted], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        let mut seen_platform_tools: Vec<(String, String)> = Vec::new();
        let mut clean = Vec::new();
        let mut flagged = Vec::new();
        for id in ids {
            let row = &self.rows[(id - 1) as usize];
            let site = &self.index.sites[row.site];
            let tool = &site.tools[row.tool];
            // Only a platform's own shared tools collapse into one hit. A site's
            // extra tools (PostHog's `ask_max` next to Mintlify's set) stay its own.
            let shared = site
                .platform
                .as_ref()
                .filter(|p| crate::platform::is_shared(p, &tool.name));
            let platform = shared.map(|p| Platform {
                name: p.clone(),
                sites: self.platform_sites.get(p).copied().unwrap_or(1),
            });
            if let Some(p) = shared {
                let key = (p.clone(), tool.name.clone());
                if seen_platform_tools.contains(&key) {
                    continue;
                }
                seen_platform_tools.push(key);
            }
            let hit = Hit {
                id: format!("{}/{}", site.host, tool.name),
                site: site.host.clone(),
                tool: tool.name.clone(),
                description: tool.description.clone(),
                open: tool.page.clone(),
                kind: if tool.annotations.consequential {
                    "consequential"
                } else if tool.annotations.read_only {
                    "read"
                } else {
                    "unspecified"
                },
                input_schema: tool.input_schema.clone(),
                checked_at: site.checked_at.clone(),
                platform,
                flags: tool.flags.clone(),
            };
            if hit.flags.is_empty() { clean.push(hit) } else { flagged.push(hit) }
            if clean.len() >= limit {
                break;
            }
        }
        clean.extend(flagged);
        clean.truncate(limit);
        Ok(clean)
    }
}

/// Property names from a JSON Schema, as searchable words.
fn param_words(schema: &Value) -> String {
    schema["properties"]
        .as_object()
        .map(|p| p.keys().map(|k| words(k)).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// Split camelCase, snake_case and dotted names into separate words.
fn words(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower {
                out.push(' ');
            }
            out.push(c);
            prev_lower = c.is_lowercase() || c.is_ascii_digit();
        } else {
            out.push(' ');
            prev_lower = false;
        }
    }
    out
}

/// Words that say nothing about a capability.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "any", "are", "as", "at", "be", "by", "can", "do", "for", "from", "how", "i",
    "in", "is", "it", "me", "my", "of", "on", "or", "please", "so", "that", "the", "this", "to",
    "up", "what", "when", "where", "which", "with", "you", "want", "need", "some", "site", "website",
];

/// Free text to a safe FTS5 query: every word quoted, joined with OR.
fn fts_query(query: &str) -> Option<String> {
    let mut seen = Vec::new();
    for w in words(query).split_whitespace() {
        let w = w.to_lowercase();
        if !STOPWORDS.contains(&w.as_str()) && !seen.contains(&w) {
            seen.push(w);
        }
    }
    if seen.is_empty() {
        return None;
    }
    Some(seen.iter().map(|w| format!("\"{w}\"")).collect::<Vec<_>>().join(" OR "))
}
