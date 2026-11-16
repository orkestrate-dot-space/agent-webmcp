use anyhow::Result;
use rusqlite::{Connection, params};

use crate::catalog::Entry;

/// In-memory full-text index over catalog entries. Ranking is BM25.
pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn build(entries: &[Entry]) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE VIRTUAL TABLE tools USING fts5(site, name, description, tokenize = 'porter unicode61');",
        )?;
        {
            let mut insert =
                conn.prepare("INSERT INTO tools (rowid, site, name, description) VALUES (?1, ?2, ?3, ?4)")?;
            for (i, e) in entries.iter().enumerate() {
                insert.execute(params![
                    i as i64 + 1,
                    words(&e.site),
                    words(&e.name),
                    e.description
                ])?;
            }
        }
        Ok(Self { conn })
    }

    /// Positions in the entry slice passed to `build`, best match first, with a score
    /// where higher is better.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(usize, f64)>> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT rowid, bm25(tools, 1.0, 5.0, 2.0) AS rank FROM tools WHERE tools MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts, limit as i64], |row| {
            let rowid: i64 = row.get(0)?;
            let rank: f64 = row.get(1)?;
            Ok(((rowid - 1) as usize, -rank))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
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

/// Words that say nothing about a capability. Dropped from queries so "what is the
/// weather" cannot match a tool just because its description contains "the".
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "any", "are", "as", "at", "be", "by", "can", "do", "for", "from", "how",
    "i", "in", "is", "it", "me", "my", "of", "on", "or", "please", "so", "that", "the", "this",
    "to", "up", "what", "when", "where", "which", "with", "you",
];

/// Turn free text into a safe FTS5 query: every word quoted, joined with OR.
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
    Some(
        seen.iter()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}
