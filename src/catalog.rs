use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const EMBEDDED_TOOLS: &str = include_str!("../catalog/tools.json");
const EMBEDDED_SCRIPTS: &[(&str, &str)] = &[(
    "scripts/example.com.js",
    include_str!("../catalog/scripts/example.com.js"),
)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The site registers the tool itself.
    Native,
    /// A script from the catalog is injected first.
    Community,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub site: String,
    pub url: String,
    pub name: String,
    pub description: String,
    pub kind: Kind,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

#[derive(Debug)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    scripts: HashMap<String, String>,
}

impl Catalog {
    /// The catalog compiled into the binary.
    pub fn embedded() -> Result<Self> {
        let scripts = EMBEDDED_SCRIPTS
            .iter()
            .map(|(name, body)| (name.to_string(), body.to_string()))
            .collect();
        Self::parse(EMBEDDED_TOOLS, scripts)
    }

    /// A catalog directory holding `tools.json` and the scripts it names.
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(dir.join("tools.json"))
            .with_context(|| format!("reading {}/tools.json", dir.display()))?;
        let raw: Vec<Entry> = serde_json::from_str(&json).context("parsing tools.json")?;
        let mut scripts = HashMap::new();
        for entry in &raw {
            if let Some(name) = &entry.script
                && !scripts.contains_key(name)
            {
                let body = std::fs::read_to_string(dir.join(name))
                    .with_context(|| format!("reading script {name}"))?;
                scripts.insert(name.clone(), body);
            }
        }
        Self::parse(&json, scripts)
    }

    /// `AGENT_WEBMCP_CATALOG` points at a directory; otherwise the embedded catalog.
    pub fn load() -> Result<Self> {
        match std::env::var_os("AGENT_WEBMCP_CATALOG") {
            Some(dir) => Self::from_dir(Path::new(&dir)),
            None => Self::embedded(),
        }
    }

    pub fn parse(json: &str, scripts: HashMap<String, String>) -> Result<Self> {
        let entries: Vec<Entry> = serde_json::from_str(json).context("parsing tools.json")?;
        validate(&entries, &scripts)?;
        Ok(Self { entries, scripts })
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn script(&self, entry: &Entry) -> Option<&str> {
        entry
            .script
            .as_ref()
            .and_then(|name| self.scripts.get(name))
            .map(String::as_str)
    }
}

fn validate(entries: &[Entry], scripts: &HashMap<String, String>) -> Result<()> {
    let mut seen = HashSet::new();
    for e in entries {
        if e.id != format!("{}/{}", e.site, e.name) {
            bail!("entry {:?}: id must be \"<site>/<name>\"", e.id);
        }
        if !seen.insert(e.id.as_str()) {
            bail!("duplicate entry {:?}", e.id);
        }
        if !e.url.starts_with("https://") && !e.url.starts_with("http://") {
            bail!("entry {:?}: url must start with http:// or https://", e.id);
        }
        if e.description.trim().is_empty() {
            bail!("entry {:?}: description is empty", e.id);
        }
        match (e.source, &e.script) {
            (Source::Community, None) => {
                bail!("entry {:?}: community tools need a script", e.id)
            }
            (Source::Community, Some(name)) if !scripts.contains_key(name) => {
                bail!("entry {:?}: script {name:?} not found", e.id)
            }
            (Source::Native, Some(_)) => {
                bail!("entry {:?}: native tools must not have a script", e.id)
            }
            _ => {}
        }
    }
    Ok(())
}
