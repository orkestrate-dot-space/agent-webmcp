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
    /// How well this matched, higher is better. Only comparable within one
    /// search, and only useful for spotting a weak match.
    pub relevance: f64,
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
            // No stemming: porter would stem the query term too, so "deploy"
            // becomes "deploi" and stops matching "deployment". Prefix terms
            // (see `term`) cover the inflections that matter instead.
            "CREATE VIRTUAL TABLE tools USING fts5(site, name, description, params, tokenize = 'unicode61');",
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
        Ok(Self {
            conn,
            rows,
            index,
            platform_sites,
        })
    }

    pub fn sites(&self) -> usize {
        crate::model::count_index(&self.index.sites).0
    }

    pub fn tools(&self) -> usize {
        crate::model::count_index(&self.index.sites).1
    }

    pub fn generated_at(&self) -> &str {
        &self.index.generated_at
    }

    /// Best matches first. A platform's shared tool appears once (its
    /// best-ranked site), with the number of sites that share it. Flagged
    /// tools come after clean ones.
    pub fn find(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let words = content_words(query);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        // A word that names an indexed site narrows the search to that site, so
        // "search stripe docs" never answers with another company's docs.
        let site = self.site_filter(&words);
        // The verb in a request describes what the caller wants to do, not what
        // the tool is called: "order a pizza" is answered by `manage_pizza`, so
        // only the object words have to be present.
        let subjects: Vec<String> = words
            .iter()
            .filter(|w| !ACTION_WORDS.contains(&w.as_str()))
            .cloned()
            .collect();
        let required = if subjects.is_empty() {
            &words
        } else {
            &subjects
        };
        let mut ids = self.match_rows(required, site.as_deref())?;
        if ids.is_empty() && words.len() > 1 {
            // Nothing has the object. Accept a match on any two words, which
            // still needs more than one coincidental term.
            ids = self.any_two_rows(&words, site.as_deref())?;
        }

        // Order by where the words land: a tool named for the task beats one
        // that mentions it in passing. Every word counts, verbs included, so
        // "add to cart" prefers a tool that says add.
        let mut scored: Vec<(f64, f64, i64)> = ids
            .into_iter()
            .map(|(id, rank)| {
                let row = &self.rows[(id - 1) as usize];
                let site = &self.index.sites[row.site];
                let tool = &site.tools[row.tool];
                let score = relevance(
                    &words,
                    &site.host,
                    &tool.name,
                    &tool.description,
                    &tool.input_schema,
                );
                (score, rank, id)
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        });

        let mut seen_ids: Vec<String> = Vec::new();
        let mut seen_platform_tools: Vec<(String, String)> = Vec::new();
        let mut clean = Vec::new();
        let mut flagged = Vec::new();
        for (relevance, _rank, id) in scored {
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
            // One host can hold several records (the Chrome Labs demos), and
            // the same tool can appear in more than one of them.
            let id = format!("{}/{}", site.host, tool.name);
            if seen_ids.contains(&id) {
                continue;
            }
            seen_ids.push(id.clone());
            let hit = Hit {
                id,
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
                relevance,
            };
            if hit.flags.is_empty() {
                clean.push(hit)
            } else {
                flagged.push(hit)
            }
            if clean.len() >= limit {
                break;
            }
        }
        clean.extend(flagged);
        clean.truncate(limit);
        Ok(clean)
    }

    /// Rows matching every content word, best first.
    fn match_rows(&self, words: &[String], site: Option<&str>) -> Result<Vec<(i64, f64)>> {
        let expr = words
            .iter()
            .map(|w| term(w))
            .collect::<Vec<_>>()
            .join(" AND ");
        self.ranked(&expr, site)
    }

    /// Rows matching at least two of the content words.
    fn any_two_rows(&self, words: &[String], site: Option<&str>) -> Result<Vec<(i64, f64)>> {
        let mut pairs = Vec::new();
        for (i, a) in words.iter().enumerate() {
            for b in &words[i + 1..] {
                pairs.push(format!("({} AND {})", term(a), term(b)));
            }
        }
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        self.ranked(&pairs.join(" OR "), site)
    }

    fn ranked(&self, expr: &str, site: Option<&str>) -> Result<Vec<(i64, f64)>> {
        let fts = match site {
            Some(s) => format!("site:\"{s}\" AND ({expr})"),
            None => format!("({expr})"),
        };
        let mut stmt = self.conn.prepare(
            "SELECT rowid, bm25(tools, 8.0, 5.0, 2.0, 1.0) AS rank FROM tools WHERE tools MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let wanted = 400i64;
        let rows = stmt
            .query_map(params![fts, wanted], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// The word in the query that names a site, when exactly one brand does.
    /// A word qualifies when every site family it matches begins with it:
    /// "stripe" covers stripe.com, docs.stripe.com and stripe.dev, while
    /// "docs" matches docs.whop.com only through a subdomain and never
    /// qualifies. Sites we checked without finding tools count too: naming one
    /// means "not that site's neighbours either".
    fn site_filter(&self, words: &[String]) -> Option<String> {
        let mut best: Option<(usize, String)> = None;
        for word in words {
            let mut families: Vec<String> = Vec::new();
            for site in self.index.sites.iter() {
                if site.host.split(['.', '-']).any(|label| label == word) {
                    let family = host_family(&site.host);
                    if !families.contains(&family) {
                        families.push(family);
                    }
                }
            }
            if families.is_empty() {
                continue;
            }
            let names_the_brand = families
                .iter()
                .all(|f| f.split('.').next() == Some(word.as_str()));
            if !names_the_brand {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|(count, _)| families.len() < *count)
            {
                best = Some((families.len(), word.clone()));
            }
        }
        best.map(|(_, word)| word)
    }
}

/// One query word as an FTS5 term. Bare, not quoted: quoting turns stemming
/// off in FTS5, and a prefix from three characters up covers the inflections
/// that matter ("deploy" finds "deployment").
fn term(word: &str) -> String {
    if word.chars().count() >= 3 {
        format!("{word}*")
    } else {
        word.to_string()
    }
}

/// The registered-domain-ish part of a host, for grouping sites that belong
/// together (`docs.stripe.com` and `stripe.com`).
fn host_family(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    parts[parts.len().saturating_sub(2)..].join(".")
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
    "up", "what", "when", "where", "which", "with", "you", "want", "need", "some", "site",
    "website", // FTS5 query operators: never treat them as search terms.
    "and", "or", "not", "near",
];

/// The words that describe a capability: stopwords and punctuation dropped,
/// camelCase split, order kept, duplicates removed.
fn content_words(query: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for w in words(query).split_whitespace() {
        let w = w.to_lowercase();
        if !STOPWORDS.contains(&w.as_str()) && !seen.contains(&w) {
            seen.push(w);
        }
    }
    seen
}

/// How well a tool answers a query: every query word adds weight depending on
/// whether it appears in the tool's name, its site, its description or its
/// input fields. Words match by prefix, so "deploy" finds "deployment".
fn relevance(words: &[String], host: &str, name: &str, description: &str, schema: &Value) -> f64 {
    let fields = schema["properties"]
        .as_object()
        .map(|p| p.keys().cloned().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let name = name.replace(['_', '.'], " ");
    let mut score = 0.0;
    for w in words {
        if contains_word(&name, w) {
            score += 4.0;
        }
        if contains_word(host, w) {
            score += 3.0;
        }
        if contains_word(description, w) {
            score += 2.0;
        }
        if contains_word(&fields, w) {
            score += 1.0;
        }
    }
    let score: f64 = score;
    (score * 100.0).round() / 100.0
}

/// Whether `text` has a word that starts with `word`, case-insensitively.
fn contains_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .any(|t| {
            let t = t.to_lowercase();
            t.starts_with(word) || camel_pieces(t.as_str()).any(|p| p.starts_with(word))
        })
}

/// "openDocumentation" also offers "open" and "documentation".
fn camel_pieces(word: &str) -> impl Iterator<Item = String> + '_ {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in word.chars() {
        if c.is_uppercase() && prev_lower && !current.is_empty() {
            pieces.push(current.to_lowercase());
            current = String::new();
        }
        current.push(c);
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    if !current.is_empty() {
        pieces.push(current.to_lowercase());
    }
    pieces.into_iter()
}

/// Words that say what the caller wants to do. A tool rarely carries them in
/// its name, so they rank but do not have to match.
const ACTION_WORDS: &[&str] = &[
    "add", "ask", "book", "buy", "cancel", "check", "create", "delete", "deploy", "edit", "find",
    "get", "list", "look", "make", "manage", "open", "order", "read", "remove", "reserve",
    "search", "send", "set", "share", "show", "start", "submit", "track", "update", "view",
    "write",
];
