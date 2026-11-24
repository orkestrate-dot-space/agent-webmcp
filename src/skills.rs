//! Bundled `core` skill plus local user skills.
//!
//! The CLI is the source of truth; MCP resources mirror the same API.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::hardened::{canonical_root, read_inside_bytes};

/// Largest skill file accepted (512 KiB). Larger files are refused.
pub const MAX_SKILL_FILE_BYTES: u64 = 512 * 1024;

/// Per-skill traversal budgets: bound directory visits and emitted paths so
/// a DAG of in-root symlinks cannot stall listing or exhaust memory.
const MAX_TRAVERSAL_DIRS: usize = 256;
const MAX_ATTACHMENT_FILES: usize = 512;

/// One traversal budget shared across `references/` + `templates/` of a skill.
#[derive(Debug, Default)]
struct TraversalBudget {
    dirs_visited: usize,
    emitted: usize,
}

/// Decode double-quoted YAML escapes in a single pass: `\\`, `\"`, `\n`,
/// `\r`, `\t`. Unknown escapes keep the backslash. Single-pass matters:
/// `"Use \\new"` (backslash + `n` after `\\` decodes) must become
/// `Use \new`, not `Use <newline>ew` from double-decoding.
fn decode_double_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

const CORE_SKILL: &str = include_str!("../skill-data/core/SKILL.md");
const CORE_DRIVERS: &str = include_str!("../skill-data/core/references/drivers.md");
const CORE_PERMISSIONS: &str = include_str!("../skill-data/core/references/permissions.md");
const CORE_CONFIG: &str = include_str!("../skill-data/core/references/config.md");
const CORE_AUTHORING: &str = include_str!("../skill-data/core/references/authoring.md");

/// Bundled skill name.
pub const BUNDLED_CORE_NAME: &str = "core";

/// Bundled reference paths, in stable order.
pub const BUNDLED_REFS: &[(&str, &str)] = &[
    ("references/drivers.md", CORE_DRIVERS),
    ("references/permissions.md", CORE_PERMISSIONS),
    ("references/config.md", CORE_CONFIG),
    ("references/authoring.md", CORE_AUTHORING),
];

/// Where local skills live: `AGENT_WEBMCP_SKILLS_DIR` when non-empty,
/// else `<config-base>/agent-webmcp/skills`.
///
/// Returns an error instead of falling back to a shared temp directory when
/// `HOME`/`USERPROFILE` and `XDG_CONFIG_HOME` are all unset: loading skills
/// from `std::env::temp_dir()` would be world-readable on multi-user
/// machines.
pub fn skills_dir() -> Result<PathBuf, SkillError> {
    if let Some(dir) = explicit_skills_dir() {
        return Ok(dir);
    }
    let env = |key: &str| std::env::var(key).ok();
    skills_dir_from(&env)
}

/// Resolve the default skills dir from explicit env values (testable).
fn skills_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, SkillError> {
    if let Some(xdg) = env("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(xdg).join("agent-webmcp/skills"));
    }
    if let Some(home) = crate::config::home_dir(env) {
        return Ok(home.join(".config/agent-webmcp/skills"));
    }
    Err(SkillError::Io(
        "HOME (or USERPROFILE on Windows) and XDG_CONFIG_HOME are not set; cannot locate skills directory".to_string(),
    ))
}

/// The explicit override dir, when `AGENT_WEBMCP_SKILLS_DIR` is non-empty.
pub fn explicit_skills_dir() -> Option<PathBuf> {
    std::env::var_os("AGENT_WEBMCP_SKILLS_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// True when the explicit env override is in effect.
pub fn is_explicit_dir() -> bool {
    explicit_skills_dir().is_some()
}

/// Default skills dir ignoring any explicit override (HOME/XDG only), so
/// injected helpers with `None` stay isolated from `AGENT_WEBMCP_SKILLS_DIR`.
fn default_skills_dir() -> Result<PathBuf, SkillError> {
    let env = |key: &str| std::env::var(key).ok();
    skills_dir_from(&env)
}

/// True when `name` is the reserved `core` name, case-insensitively.
///
/// Discovery and lookup share this so a local directory differing from
/// `core` only by case (`Core/SKILL.md`) follows the same policy on every
/// platform (including case-insensitive filesystems where `core` and `Core`
/// resolve to the same directory).
fn is_core_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(BUNDLED_CORE_NAME)
}

/// A skill listing entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillSummary {
    /// Skill name (`core` for the bundled guide).
    pub name: String,
    /// `bundled` or `local`.
    pub source: String,
    /// Frontmatter description, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Local `SKILL.md` path, when local.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Available reference paths (relative, e.g. `references/drivers.md`).
    #[serde(default)]
    pub refs: Vec<String>,
}

/// One reference file's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefContent {
    /// Relative path from the skill root (e.g. `references/drivers.md`).
    #[serde(rename = "ref")]
    pub ref_path: String,
    /// File content.
    pub content: String,
}

/// A fully loaded skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullSkill {
    /// Skill name.
    pub name: String,
    /// `bundled` or `local`.
    pub source: String,
    /// `SKILL.md` content.
    pub content: String,
    /// Reference files (under `references/`).
    pub refs: Vec<RefContent>,
    /// Template files (under `templates/`), when present.
    pub templates: Vec<RefContent>,
    /// Local `SKILL.md` path, when local.
    pub path: Option<PathBuf>,
}

/// Failure loading a skill or reference.
#[derive(Debug)]
pub enum SkillError {
    /// Unknown skill name.
    UnknownSkill(String),
    /// Unknown reference within a known skill.
    UnknownRef { skill: String, query: String },
    /// Invalid usage (empty names, `--all` with names, bad names, …).
    Invalid(String),
    /// Filesystem or read failure.
    Io(String),
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSkill(name) => write!(f, "unknown skill {name:?}"),
            Self::UnknownRef { skill, query } => {
                write!(f, "unknown ref {query:?} for skill {skill:?}")
            }
            Self::Invalid(msg) => write!(f, "{msg}"),
            Self::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SkillError {}

/// True when `name` is a safe skill name: `[a-zA-Z0-9._-]+`, not `.`/`..`.
pub fn valid_skill_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 || name == "." || name == ".." {
        return false;
    }
    name.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// True when `rel` is a safe relative ref path: components of
/// `[a-zA-Z0-9._-]+`, no `..`, no absolute paths, no backslashes.
fn valid_ref_path(rel: &str) -> bool {
    if rel.is_empty() || rel.len() > 256 || rel.starts_with('/') || rel.contains('\\') {
        return false;
    }
    let mut any = false;
    for part in rel.split('/') {
        any = true;
        if part.is_empty() || part == "." || part == ".." {
            return false;
        }
        if part.len() > 128 {
            return false;
        }
        if !part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
        {
            return false;
        }
    }
    any
}

/// Extract the frontmatter `description:` value, when present.
///
/// Handles single-line plain and quoted (`"…"`, `'…'`) values plus YAML
/// block scalars (`>`, `>-`, `>+`, `|`, `|-`, `|+`, with optional trailing
/// comments) so `skills list` shows prose instead of `>-`.
///
/// Quoted values allow a trailing `# comment` after the closing quote; `#`
/// inside the quotes is preserved.
fn parse_double_single_line(v: &str) -> Option<String> {
    // `v` starts with `"`. Find the closing unescaped `"` whose remainder is
    // empty or a `#` comment.
    let bytes = v.as_bytes();
    let mut idx = 1;
    while idx < bytes.len() {
        if bytes[idx] == b'"' {
            // Count preceding backslashes: even => closing, odd => escaped.
            let mut backslashes = 0;
            let mut k = idx;
            while k > 0 && bytes[k - 1] == b'\\' {
                backslashes += 1;
                k -= 1;
            }
            if backslashes % 2 == 0 {
                let rest = v[idx + 1..].trim();
                if rest.is_empty() || rest.starts_with('#') {
                    return Some(v[1..idx].to_string());
                }
                // Not a valid terminator (trailing junk): keep scanning for a
                // later quote that is followed by empty/comment.
            }
        }
        idx += 1;
    }
    None
}

/// Single-quoted variant: `''` is an escaped quote; the closing `'` must be
/// followed by empty or a `#` comment. `#` inside is preserved.
fn parse_single_single_line(v: &str) -> Option<String> {
    // `v` starts with `'`.
    let bytes = v.as_bytes();
    let mut idx = 1;
    while idx < bytes.len() {
        if bytes[idx] == b'\'' {
            // `''` is escaped, skip both.
            if idx + 1 < bytes.len() && bytes[idx + 1] == b'\'' {
                idx += 2;
                continue;
            }
            let rest = v[idx + 1..].trim();
            if rest.is_empty() || rest.starts_with('#') {
                return Some(v[1..idx].to_string());
            }
            // Trailing junk after the quote: keep scanning (a later quote
            // might terminate with empty/comment).
        }
        idx += 1;
    }
    None
}

fn frontmatter_description(content: &str) -> Option<String> {
    let mut lines = content.lines();
    // Delimiters only at column 0: an indented `---` inside a `|` scalar
    // must be preserved as content, not treated as the closing fence.
    if lines.next()? != "---" {
        return None;
    }
    let mut fm: Vec<&str> = Vec::new();
    for line in lines {
        if line == "---" {
            break;
        }
        // `...` also ends YAML frontmatter; stop there too.
        if line == "..." {
            break;
        }
        fm.push(line);
    }
    let mut i = 0;
    while i < fm.len() {
        let line = fm[i];
        let trimmed_start = line.trim_start();
        // Only top-level keys (allow no indent); indented lines are continuations.
        let indent = line.len() - trimmed_start.len();
        let Some(rest) = trimmed_start.strip_prefix("description:") else {
            i += 1;
            continue;
        };
        // Require the key itself to be top-level (indent 0). Deeper
        // `description:` inside nested maps is not the skill description.
        if indent != 0 {
            i += 1;
            continue;
        }
        let rest_trimmed = rest.trim();
        // Block scalar: first token is `>`, `>-`, `>+`, `|`, `|-`, `|+`
        // (anything after it must be a `#` comment or whitespace).
        let first_token = rest_trimmed.split_whitespace().next().unwrap_or("");
        let is_block = matches!(first_token, ">" | ">-" | ">+" | "|" | "|-" | "|+") && {
            let after = rest_trimmed[first_token.len()..].trim();
            after.is_empty() || after.starts_with('#')
        };
        if is_block {
            let folded = first_token.starts_with('>');
            let mut vals: Vec<String> = Vec::new();
            let mut j = i + 1;
            while j < fm.len() {
                let l = fm[j];
                if l.trim().is_empty() {
                    vals.push(String::new());
                    j += 1;
                    continue;
                }
                let cur_indent = l.len() - l.trim_start().len();
                if cur_indent <= indent {
                    break;
                }
                vals.push(l.trim_start().to_string());
                j += 1;
            }
            if vals.is_empty() {
                return None;
            }
            let result = if folded {
                // Folded: single newlines become spaces, blank lines become newlines.
                let mut s = String::new();
                for v in &vals {
                    if v.is_empty() {
                        if !s.ends_with('\n') {
                            s.push('\n');
                        }
                    } else {
                        if !s.is_empty() && !s.ends_with('\n') && !s.ends_with(' ') {
                            s.push(' ');
                        }
                        // A blank line before means paragraph break: keep one newline.
                        s.push_str(v);
                    }
                }
                s
            } else {
                vals.join("\n")
            };
            let trimmed = result.trim().to_string();
            if trimmed.is_empty() {
                return None;
            }
            return Some(trimmed);
        }
        if rest_trimmed.is_empty() {
            return None;
        }
        // Quoted single-line values.
        let v = rest_trimmed;
        if v.starts_with('"') && v.len() >= 2 {
            // Single-line double-quoted, allowing a trailing `# comment` after
            // the closing quote while preserving `#` inside the quotes.
            if let Some(inner) = parse_double_single_line(v) {
                let unescaped = decode_double_escapes(&inner);
                let t = unescaped.trim().to_string();
                if t.is_empty() {
                    return None;
                }
                return Some(t);
            }
            // Multiline double-quoted: single pass — join the remainder once,
            // then scan for the closing quote a single time (allowing a
            // trailing `# comment` on the closing line). Never rescans the
            // accumulated scalar on every continuation line.
            let mut acc = v[1..].to_string();
            if i + 1 < fm.len() {
                acc.push('\n');
                acc.push_str(&fm[i + 1..].join("\n"));
            }
            if let Some(inner) = parse_double_single_line(&format!("\"{acc}")) {
                // `acc` already holds the remainder after the opening quote;
                // a valid close was found (possibly with trailing comment).
                let unescaped = decode_double_escapes(&inner);
                let t = unescaped.trim().to_string();
                if t.is_empty() {
                    return None;
                }
                return Some(t);
            }
            return None;
        }
        if v.starts_with('\'') && v.len() >= 2 {
            if let Some(inner) = parse_single_single_line(v) {
                let t = inner.replace("''", "'").trim().to_string();
                if t.is_empty() {
                    return None;
                }
                return Some(t);
            }
            // Multiline single-quoted: single pass — join once, scan once.
            let mut acc = v[1..].to_string();
            if i + 1 < fm.len() {
                acc.push('\n');
                acc.push_str(&fm[i + 1..].join("\n"));
            }
            if let Some(inner) = parse_single_single_line(&format!("'{acc}")) {
                let t = inner.replace("''", "'").trim().to_string();
                if t.is_empty() {
                    return None;
                }
                return Some(t);
            }
            return None;
        }
        // Plain scalar: strip a trailing ` # comment` (space + `#`).
        let mut plain = v.to_string();
        if let Some(idx) = plain.find(" #") {
            plain.truncate(idx);
        }
        let t = plain.trim().to_string();
        if t.is_empty() || t == ">" || t == "|" || t == ">-" || t == "|-" {
            return None;
        }
        return Some(t);
    }
    None
}

/// Read one file inside `root`, refusing escapes, non-regular files, and
/// oversized files. Hardened reader shared with custom tools; see
/// `crate::hardened`.
fn read_inside(root: &Path, path: &Path) -> Result<String, SkillError> {
    let buf =
        read_inside_bytes(root, path, MAX_SKILL_FILE_BYTES, "skills").map_err(SkillError::Io)?;
    String::from_utf8(buf)
        .map_err(|e| SkillError::Io(format!("could not read {}: {e:#}", path.display())))
}

/// Join `path` (under `base`) into a `/`-separated reference key, validating
/// the real filesystem components: rejects `..`, `.`, root/prefix, and any
/// component containing a literal backslash.
fn rel_key(base: &Path, path: &Path) -> Option<String> {
    use std::path::Component;
    let rel = path.strip_prefix(base).ok()?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(os) => {
                let s = os.to_str()?;
                if s.is_empty() || s.contains('\\') {
                    return None;
                }
                parts.push(s.to_owned());
            }
            _ => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join("/");
    if !valid_ref_path(&joined) {
        return None;
    }
    Some(joined)
}

/// The single `core` override selector, shared by listing, body reads, and
/// reference reads.
///
/// Only counts when `SKILL.md` exists in the explicit override dir (via
/// `symlink_metadata`, so a dangling symlink still counts as an override
/// that later surfaces I/O instead of silently falling back to bundled).
/// Chooses deterministically — exact `core` over any other casing, then
/// lexical — so `core` + `Core` together always resolve to `core`.
/// The candidate skill dir must canonicalize inside the skills root and be
/// a directory; otherwise it is not a valid override and the caller falls
/// back to bundled `core`.
///
/// Enumerating `dir` itself is different: a missing directory means "no
/// override" (`Ok(None)`), but any other `read_dir` failure (permissions,
/// I/O, …) is propagated as `Io` so callers never silently fall back to
/// bundled `core` when the explicit directory exists but cannot be listed.
fn select_local_core(dir: &Path) -> Result<Option<(String, PathBuf)>, SkillError> {
    let canon_root = canonical_root(dir);
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(SkillError::Io(format!(
                "could not read {}: {e:#}",
                dir.display()
            )));
        }
    };
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !is_core_name(&name) {
            continue;
        }
        if !valid_skill_name(&name) {
            continue;
        }
        let skill_dir = dir.join(&name);
        // Containment: the skill dir must resolve inside the root and be a dir.
        let Ok(canon_skill) = std::fs::canonicalize(&skill_dir) else {
            continue;
        };
        if !canon_skill.starts_with(&canon_root) {
            continue;
        }
        if !canon_skill.is_dir() {
            continue;
        }
        let skill_file = skill_dir.join("SKILL.md");
        // Require SKILL.md (dangling symlinks count via symlink_metadata).
        if std::fs::symlink_metadata(&skill_file).is_err() {
            continue;
        }
        candidates.push((name, skill_file));
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    candidates.sort_by(|a, b| {
        let a_exact = a.0 == BUNDLED_CORE_NAME;
        let b_exact = b.0 == BUNDLED_CORE_NAME;
        (!a_exact).cmp(&(!b_exact)).then_with(|| a.0.cmp(&b.0))
    });
    Ok(candidates.into_iter().next())
}

/// Collect relative paths under `dir` (relative to `base`) without reading
/// file bodies. `md_only` restricts to `.md` files (for `references/`).
///
/// Containment is validated BEFORE `read_dir`: `dir` must canonicalize
/// inside `skills_root_canon`. Every subdirectory is canonicalized and
/// checked before recursing, and visited canonical directories on the
/// current recursion stack break symlink cycles without blocking
/// non-cyclic in-root links.
///
/// Best-effort for listing: `read_dir` failures, out-of-root symlinks, and
/// dangling links are skipped. A per-skill `budget` bounds directory visits
/// and emitted paths so a DAG of in-root symlinks cannot stall listing or
/// exhaust memory (truncates when exceeded). Attachments larger than
/// `MAX_SKILL_FILE_BYTES` (via resolved file metadata) are excluded here;
/// explicit reads still surface them as I/O via `read_inside`.
fn collect_paths(
    dir: &Path,
    base: &Path,
    skills_root_canon: &Path,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<String>,
    md_only: bool,
    budget: &mut TraversalBudget,
) {
    // The file budget is enforced only when actually adding a file below;
    // visiting an (even empty) directory while at the limit is fine.
    if budget.dirs_visited >= MAX_TRAVERSAL_DIRS {
        return;
    }
    // Validate this directory itself before listing it.
    let Ok(canon_dir) = std::fs::canonicalize(dir) else {
        return;
    };
    if !canon_dir.starts_with(skills_root_canon) {
        return;
    }
    // Cycle guard: only the current stack matters, so shared in-root
    // subdirectories reached via different non-cyclic links still list.
    if !visited.insert(canon_dir.clone()) {
        return;
    }
    budget.dirs_visited += 1;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            visited.remove(&canon_dir);
            return;
        }
    };
    for entry in entries.flatten() {
        if budget.emitted >= MAX_ATTACHMENT_FILES {
            break;
        }
        let path = entry.path();
        // Classify without following except for the explicit dir check below.
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let is_dir = file_type.is_dir();
        // A symlink to a directory reports as symlink here; follow it only
        // to test `is_dir`, then containment + cycle checks decide.
        let follow_dir = file_type.is_symlink() && path.is_dir();
        if is_dir || follow_dir {
            // Validate the subdirectory before recursing into it.
            if let Ok(canon) = std::fs::canonicalize(&path)
                && canon.starts_with(skills_root_canon)
            {
                collect_paths(
                    &path,
                    base,
                    skills_root_canon,
                    visited,
                    out,
                    md_only,
                    budget,
                );
            }
            continue;
        }
        // Regular files and symlinks-to-files both arrive here; eligibility
        // (escape, non-regular, size, UTF-8) is enforced at read time by
        // `read_inside`. For public listing we only advertise in-root regular
        // files: skip anything that is clearly not a file without blocking,
        // and skip symlinks resolving outside the root (explicit reads still
        // surface those as containment I/O errors via `read_inside`).
        if md_only && path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        // Skip non-file targets without opening (FIFO guard for discovery):
        // `symlink_metadata` never blocks; `metadata` follows the link but
        // only stats, never opens. Directories were handled above.
        let usable = match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => match std::fs::metadata(&path) {
                Ok(tm) => tm.file_type().is_file(),
                Err(_) => false,
            },
            Ok(m) => m.file_type().is_file(),
            Err(_) => false,
        };
        if !usable {
            continue;
        }
        // Exclude out-of-root symlinks from listing (explicit `--ref`/`--full`
        // still reports them as I/O via `read_inside`).
        let canon = match std::fs::canonicalize(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !canon.starts_with(skills_root_canon) {
            continue;
        }
        // Size filter for listing: exclude attachments exceeding the limit
        // using resolved file metadata. Explicit reads keep the bounded
        // read check at `read_inside` time.
        if let Ok(m) = std::fs::metadata(&canon)
            && m.len() > MAX_SKILL_FILE_BYTES
        {
            continue;
        }
        if let Some(rel_str) = rel_key(base, &path)
            && budget.emitted < MAX_ATTACHMENT_FILES
        {
            out.push(rel_str);
            budget.emitted += 1;
        }
    }
    visited.remove(&canon_dir);
}

/// Collect every attachment candidate for explicit reads (`--ref`/`--full`),
/// including FIFOs, out-of-root symlinks, and dangling links (no opens, so
/// no blocking). Listing uses [`collect_paths`] (filtered); explicit reads
/// use this so those entries surface as I/O errors (not `UnknownRef`) via
/// [`read_inside`]. Best-effort with the same traversal budget (truncates).
fn collect_paths_all(
    dir: &Path,
    base: &Path,
    skills_root_canon: &Path,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<String>,
    md_only: bool,
    budget: &mut TraversalBudget,
) {
    // The file budget is enforced only when actually adding a file below;
    // visiting an (even empty) directory while at the limit is fine.
    if budget.dirs_visited >= MAX_TRAVERSAL_DIRS {
        return;
    }
    let Ok(canon_dir) = std::fs::canonicalize(dir) else {
        return;
    };
    if !canon_dir.starts_with(skills_root_canon) {
        return;
    }
    if !visited.insert(canon_dir.clone()) {
        return;
    }
    budget.dirs_visited += 1;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            visited.remove(&canon_dir);
            return;
        }
    };
    for entry in entries.flatten() {
        if budget.emitted >= MAX_ATTACHMENT_FILES {
            break;
        }
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let is_dir = file_type.is_dir();
        let follow_dir = file_type.is_symlink() && path.is_dir();
        if is_dir || follow_dir {
            if let Ok(canon) = std::fs::canonicalize(&path)
                && canon.starts_with(skills_root_canon)
            {
                collect_paths_all(
                    &path,
                    base,
                    skills_root_canon,
                    visited,
                    out,
                    md_only,
                    budget,
                );
            }
            continue;
        }
        // Skip only true directories; everything else (regular, symlink,
        // FIFO, socket, dangling) is a candidate — `read_inside` classifies
        // without opening, so FIFOs never block here.
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        if md_only && path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(rel_str) = rel_key(base, &path) {
            out.push(rel_str);
            budget.emitted += 1;
        }
    }
    visited.remove(&canon_dir);
}

/// Strict traversal for `--full`: like `collect_paths_all` but propagates
/// traversal failures instead of silently returning partial results.
/// Unreadable subdirectories, directory symlinks escaping the root, and
/// budget exhaustion are I/O errors. Missing optional `references/` /
/// `templates/` is handled by the caller (empty), not here.
///
/// The file budget is enforced only when actually adding a file: visiting an
/// (even empty) directory while `emitted == MAX` is not an error.
fn collect_paths_strict(
    dir: &Path,
    base: &Path,
    skills_root_canon: &Path,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<String>,
    md_only: bool,
    budget: &mut TraversalBudget,
) -> Result<(), SkillError> {
    if budget.dirs_visited >= MAX_TRAVERSAL_DIRS {
        return Err(SkillError::Io(format!(
            "{} has too many directories to list; refusing",
            base.display()
        )));
    }
    let canon_dir = std::fs::canonicalize(dir)
        .map_err(|e| SkillError::Io(format!("could not read {}: {e:#}", dir.display())))?;
    if !canon_dir.starts_with(skills_root_canon) {
        return Err(SkillError::Io(format!(
            "{} escapes the skills directory",
            dir.display()
        )));
    }
    if !visited.insert(canon_dir.clone()) {
        return Ok(());
    }
    budget.dirs_visited += 1;
    let entries = std::fs::read_dir(dir)
        .map_err(|e| SkillError::Io(format!("could not read {}: {e:#}", dir.display())))?;
    let mut pending_dirs: Vec<PathBuf> = Vec::new();
    for entry_res in entries {
        let entry = entry_res
            .map_err(|e| SkillError::Io(format!("could not read {}: {e:#}", dir.display())))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| SkillError::Io(format!("could not read {}: {e:#}", path.display())))?;
        let is_dir = file_type.is_dir();
        let follow_dir = file_type.is_symlink() && path.is_dir();
        if is_dir || follow_dir {
            // Directory symlinks escaping the root are errors on `--full`.
            let canon = std::fs::canonicalize(&path)
                .map_err(|e| SkillError::Io(format!("could not read {}: {e:#}", path.display())))?;
            if !canon.starts_with(skills_root_canon) {
                visited.remove(&canon_dir);
                return Err(SkillError::Io(format!(
                    "{} escapes the skills directory",
                    path.display()
                )));
            }
            pending_dirs.push(path);
            continue;
        }
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        if md_only && path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(rel_str) = rel_key(base, &path) {
            if budget.emitted >= MAX_ATTACHMENT_FILES {
                visited.remove(&canon_dir);
                return Err(SkillError::Io(format!(
                    "{} has too many attachment files to list; refusing",
                    base.display()
                )));
            }
            out.push(rel_str);
            budget.emitted += 1;
        }
    }
    for sub in pending_dirs {
        collect_paths_strict(&sub, base, skills_root_canon, visited, out, md_only, budget)?;
    }
    visited.remove(&canon_dir);
    Ok(())
}

/// Discover `references/…` paths (skill-relative, e.g. `references/foo.md`)
/// without reading bodies.
fn discover_ref_paths(skill_root: &Path, skills_root: &Path) -> Vec<String> {
    let base = skill_root.join("references");
    let canon_root = canonical_root(skills_root);
    // The base itself must be inside the root before listing.
    if let Ok(canon_base) = std::fs::canonicalize(&base) {
        if !canon_base.starts_with(&canon_root) {
            return Vec::new();
        }
    } else {
        // Missing `references/` is not an error: no refs. A dangling symlink
        // canonicalizes to an error here; treat as no refs for discovery —
        // the exact `--ref` / `--full` body load will surface the I/O error.
        // To distinguish, check symlink_metadata: if something exists but
        // does not canonicalize, still return empty here (lazy); the loader
        // reports the failure when that ref is actually requested.
        if std::fs::symlink_metadata(&base)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Vec::new();
        }
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    let mut budget = TraversalBudget::default();
    collect_paths(
        &base,
        &base,
        &canon_root,
        &mut visited,
        &mut out,
        true,
        &mut budget,
    );
    out.sort();
    out.into_iter().map(|r| format!("references/{r}")).collect()
}

/// Discover `templates/…` paths without reading bodies.
fn discover_template_paths(skill_root: &Path, skills_root: &Path) -> Vec<String> {
    let base = skill_root.join("templates");
    let canon_root = canonical_root(skills_root);
    if let Ok(canon_base) = std::fs::canonicalize(&base) {
        if !canon_base.starts_with(&canon_root) {
            return Vec::new();
        }
    } else {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    let mut budget = TraversalBudget::default();
    collect_paths(
        &base,
        &base,
        &canon_root,
        &mut visited,
        &mut out,
        false,
        &mut budget,
    );
    out.sort();
    out.into_iter().map(|r| format!("templates/{r}")).collect()
}

/// Discover every attachment candidate for explicit reads, including FIFOs,
/// out-of-root symlinks, and dangling links. Listing uses the filtered
/// `discover_*` above; `--ref`/`--full` use these so such entries surface as
/// I/O (not `UnknownRef`) via `read_inside` without opening FIFOs.
fn discover_all_ref_paths(skill_root: &Path, skills_root: &Path) -> Vec<String> {
    let base = skill_root.join("references");
    let canon_root = canonical_root(skills_root);
    // Only walk when the base itself is inside the root; otherwise there is
    // nothing in-root to enumerate (the base problem itself is reported by
    // `attachment_base_error` for explicit reads).
    if let Ok(canon_base) = std::fs::canonicalize(&base) {
        if !canon_base.starts_with(&canon_root) {
            return Vec::new();
        }
    } else {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    let mut budget = TraversalBudget::default();
    collect_paths_all(
        &base,
        &base,
        &canon_root,
        &mut visited,
        &mut out,
        true,
        &mut budget,
    );
    out.sort();
    out.into_iter().map(|r| format!("references/{r}")).collect()
}

/// Discover every template candidate for explicit reads (see above).
fn discover_all_template_paths(skill_root: &Path, skills_root: &Path) -> Vec<String> {
    let base = skill_root.join("templates");
    let canon_root = canonical_root(skills_root);
    if let Ok(canon_base) = std::fs::canonicalize(&base) {
        if !canon_base.starts_with(&canon_root) {
            return Vec::new();
        }
    } else {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    let mut budget = TraversalBudget::default();
    collect_paths_all(
        &base,
        &base,
        &canon_root,
        &mut visited,
        &mut out,
        false,
        &mut budget,
    );
    out.sort();
    out.into_iter().map(|r| format!("templates/{r}")).collect()
}

/// Strict discovery for `--full`: propagates traversal errors (unreadable
/// subdirectories, directory symlinks escaping root) and budget exhaustion
/// instead of silently returning partial results. Missing optional
/// `references/` / `templates/` still counts as empty (caller checks
/// `attachment_base_error` first for the dangling/out-of-root base case).
fn discover_strict_ref_paths(
    skill_root: &Path,
    skills_root: &Path,
    budget: &mut TraversalBudget,
) -> Result<Vec<String>, SkillError> {
    let base = skill_root.join("references");
    if std::fs::symlink_metadata(&base)
        .err()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(Vec::new());
    }
    let canon_root = canonical_root(skills_root);
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    collect_paths_strict(
        &base,
        &base,
        &canon_root,
        &mut visited,
        &mut out,
        true,
        budget,
    )?;
    out.sort();
    Ok(out.into_iter().map(|r| format!("references/{r}")).collect())
}

/// Strict template discovery for `--full` (see above), sharing the same
/// per-skill budget across `references/` + `templates/`.
fn discover_strict_template_paths(
    skill_root: &Path,
    skills_root: &Path,
    budget: &mut TraversalBudget,
) -> Result<Vec<String>, SkillError> {
    let base = skill_root.join("templates");
    if std::fs::symlink_metadata(&base)
        .err()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(Vec::new());
    }
    let canon_root = canonical_root(skills_root);
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    collect_paths_strict(
        &base,
        &base,
        &canon_root,
        &mut visited,
        &mut out,
        false,
        budget,
    )?;
    out.sort();
    Ok(out.into_iter().map(|r| format!("templates/{r}")).collect())
}

/// When an attachment base (`references/` or `templates/`) exists but cannot
/// be listed (dangling, out-of-root, or non-directory), explicit reads
/// (`--ref`/`--full`) must report I/O, not `UnknownRef`/success. Returns the
/// I/O error to surface, or `None` when the base is missing (no attachments)
/// or healthy.
fn attachment_base_error(
    skill_root: &Path,
    skills_root: &Path,
    subdir: &str,
    _skill_name: &str,
) -> Option<SkillError> {
    let base = skill_root.join(subdir);
    let meta = match std::fs::symlink_metadata(&base) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            return Some(SkillError::Io(format!(
                "could not read {}: {e:#}",
                base.display()
            )));
        }
        Ok(m) => m,
    };
    // A plain file named `references`/`templates` (FIFO, regular file, …)
    // is not a directory of attachments: explicit reads report I/O.
    if !meta.file_type().is_dir() && !meta.file_type().is_symlink() {
        return Some(SkillError::Io(format!(
            "{} is not a regular file",
            base.display()
        )));
    }
    let canon_root = canonical_root(skills_root);
    match std::fs::canonicalize(&base) {
        Ok(canon) => {
            if !canon.starts_with(&canon_root) {
                return Some(SkillError::Io(format!(
                    "{} escapes the skills directory",
                    base.display()
                )));
            }
            if !canon.is_dir() {
                return Some(SkillError::Io(format!(
                    "{} is not a regular file",
                    base.display()
                )));
            }
            None
        }
        Err(e) => Some(SkillError::Io(format!(
            "could not read {}: {e:#}",
            base.display()
        ))),
    }
}

/// Read every discovered attachment body, propagating the first I/O failure
/// instead of silently dropping it. Used for `--full` so an unreadable
/// reference (invalid UTF-8, oversized, FIFO, escape, …) reports exit 1.
fn load_all_bodies(
    skill_root: &Path,
    skills_root: &Path,
    rel_paths: &[String],
) -> Result<Vec<RefContent>, SkillError> {
    let mut out = Vec::new();
    for rel in rel_paths {
        let file = skill_root.join(rel);
        let content = read_inside(skills_root, &file)?;
        out.push(RefContent {
            ref_path: rel.clone(),
            content,
        });
    }
    Ok(out)
}

/// Names of local skills in `dir`: subdirectories with a `SKILL.md` that
/// stays inside `dir`. Only existence + containment is checked here (no body
/// reads); readability is decided by the caller so `list` (which needs the
/// description) and `get` (which needs the content) share one source.
fn discover_local(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !valid_skill_name(&name) {
            continue;
        }
        let skill_root = dir.join(&name);
        // Refuse symlinked skill dirs escaping the root.
        let Ok(canon_skill) = std::fs::canonicalize(&skill_root) else {
            continue;
        };
        if !canon_skill.starts_with(canonical_root(dir)) {
            continue;
        }
        if !canon_skill.is_dir() {
            continue;
        }
        let skill_file = skill_root.join("SKILL.md");
        // Existence (not readability) decides discovery; `read_inside`
        // decides readability later. A dangling symlink still counts as
        // "exists" via `symlink_metadata` so `get` reports I/O (exit 1)
        // instead of masking it as unknown (exit 2).
        if std::fs::symlink_metadata(&skill_file).is_ok() {
            names.push((name, skill_file));
        }
    }
    names.sort_by(|a, b| a.0.cmp(&b.0));
    names
}

/// Read one local skill's `SKILL.md` only, distinguishing missing
/// (`UnknownSkill`, exit 2) from real I/O failures (`Io`, exit 1).
fn read_skill_file(dir: &Path, skill_file: &Path, name: &str) -> Result<String, SkillError> {
    match read_inside(dir, skill_file) {
        Ok(content) => Ok(content),
        Err(original) => {
            let missing = std::fs::symlink_metadata(skill_file)
                .err()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound);
            if missing {
                return Err(SkillError::UnknownSkill(name.into()));
            }
            Err(original)
        }
    }
}

/// Load one local skill with ALL attachment bodies, propagating I/O errors.
/// Used for `--full`. Uses the all-candidates discovery so FIFOs,
/// out-of-root symlinks, and dangling links surface as I/O (not silent
/// success) via `read_inside` without opening FIFOs.
fn load_local_skill(dir: &Path, name: &str) -> Result<FullSkill, SkillError> {
    if !valid_skill_name(name) {
        return Err(SkillError::Invalid(format!("invalid skill name {name:?}")));
    }
    // Resolve the actual on-disk spelling via the single shared selector so
    // `Core/SKILL.md` and `core/SKILL.md` follow one deterministic policy.
    let (actual_name, skill_file) = if is_core_name(name) {
        match select_local_core(dir)? {
            Some((actual, file)) => (actual, file),
            None => return Err(SkillError::UnknownSkill(name.into())),
        }
    } else {
        (name.to_string(), dir.join(name).join("SKILL.md"))
    };
    let content = read_skill_file(dir, &skill_file, name)?;
    let skill_root = skill_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dir.join(&actual_name));
    // Explicit `--full` must surface base problems (dangling/out-of-root
    // `references/` or `templates/`) as I/O, not silent success.
    if let Some(err) = attachment_base_error(&skill_root, dir, "references", name) {
        return Err(err);
    }
    if let Some(err) = attachment_base_error(&skill_root, dir, "templates", name) {
        return Err(err);
    }
    // Strict traversal: unreadable subdirectories, directory symlinks escaping
    // root, and budget exhaustion are errors, not partial results. One shared
    // budget across `references/` + `templates/` bounds a DAG of in-root
    // symlinks. Missing optional subdirs still count as empty (handled by
    // `discover_strict_*` returning empty on NotFound).
    let mut budget = TraversalBudget::default();
    let ref_paths = discover_strict_ref_paths(&skill_root, dir, &mut budget)?;
    let template_paths = discover_strict_template_paths(&skill_root, dir, &mut budget)?;
    let refs = load_all_bodies(&skill_root, dir, &ref_paths)?;
    let templates = load_all_bodies(&skill_root, dir, &template_paths)?;
    Ok(FullSkill {
        name: name.to_string(),
        source: "local".to_string(),
        content,
        refs,
        templates,
        path: Some(skill_file),
    })
}

/// Load one local skill's `SKILL.md` without attachment bodies.
/// Used for plain `skills get <name>` (no `--ref`, no `--full`),
/// `skills path`, and MCP skill-body reads.
fn load_local_skill_shallow(dir: &Path, name: &str) -> Result<FullSkill, SkillError> {
    if !valid_skill_name(name) {
        return Err(SkillError::Invalid(format!("invalid skill name {name:?}")));
    }
    let (actual_name, skill_file) = if is_core_name(name) {
        match select_local_core(dir)? {
            Some((actual, file)) => (actual, file),
            None => return Err(SkillError::UnknownSkill(name.into())),
        }
    } else {
        (name.to_string(), dir.join(name).join("SKILL.md"))
    };
    let content = read_skill_file(dir, &skill_file, name)?;
    let _ = actual_name;
    Ok(FullSkill {
        name: name.to_string(),
        source: "local".to_string(),
        content,
        refs: Vec::new(),
        templates: Vec::new(),
        path: Some(skill_file),
    })
}

/// The bundled `core` skill.
pub(crate) fn bundled_core() -> FullSkill {
    FullSkill {
        name: BUNDLED_CORE_NAME.to_string(),
        source: "bundled".to_string(),
        content: CORE_SKILL.to_string(),
        refs: BUNDLED_REFS
            .iter()
            .map(|(rel, content)| RefContent {
                ref_path: rel.to_string(),
                content: content.to_string(),
            })
            .collect(),
        templates: Vec::new(),
        path: None,
    }
}

/// List every skill: bundled `core` plus local skills.
///
/// Discovery only: reads each local `SKILL.md` once for its description,
/// but never reads attachment bodies. The default user dir can never shadow
/// bundled `core`; an explicit `AGENT_WEBMCP_SKILLS_DIR` may override it for
/// local dev and testing. `Core` (any case) is reserved like `core`.
///
/// When `skills_dir()` cannot be resolved (no HOME/XDG/explicit), only the
/// bundled guide is listed — never anything from a shared temp directory.
/// When an explicit `core` override exists but is unreadable (dangling
/// symlink, invalid UTF-8, …), the listing advertises `local`, never the
/// bundled fallback, so `list` and `get` choose the same source.
pub fn list_skills() -> Vec<SkillSummary> {
    list_skills_with(explicit_skills_dir().as_deref())
}

/// Same as [`list_skills`] but with an injected explicit override dir (no
/// `AGENT_WEBMCP_SKILLS_DIR` read). Pass `None` for a clean bundled view plus
/// the default dir (HOME/XDG) without consulting the override env.
pub(crate) fn list_skills_with(explicit_dir: Option<&Path>) -> Vec<SkillSummary> {
    let mut out = Vec::new();
    // Resolve the effective dir: explicit override when injected, otherwise
    // the default dir ignoring any process-env override.
    let dir_res: Result<PathBuf, SkillError> = match explicit_dir {
        Some(d) => Ok(d.to_path_buf()),
        None => {
            // If the process env has an explicit override but the caller
            // injected `None` ("no override"), ignore it and use the default
            // dir only. This keeps the bundled-resource test isolated.
            match default_skills_dir() {
                Ok(d) => Ok(d),
                Err(e) => Err(e),
            }
        }
    };
    // For production parity: when `explicit_dir` is `Some`, `explicit` is true
    // (override may shadow core). When `None`, core is never shadowed — the
    // default dir can never shadow bundled `core`.
    let explicit = explicit_dir.is_some();
    // When injected `Some`, also consider the real env? No: injected wins.
    // To stay isolated, never read `is_explicit_dir()` here; `explicit` above
    // is the sole source.
    let local: Vec<(String, PathBuf)> = dir_res
        .as_ref()
        .ok()
        .map(|d| discover_local(d))
        .unwrap_or_default();
    // Single shared selector for the `core` source: explicit override wins,
    // otherwise bundled. Fall back to bundled ONLY when no valid override
    // exists; I/O failures still advertise `local`.
    // `Ok(None)` = no override; `Err` = explicit dir exists but cannot be
    // enumerated (permissions, I/O, …) — never silently fall back to bundled.
    let local_core_res: Option<Result<Option<(String, PathBuf)>, SkillError>> =
        dir_res.as_ref().ok().map(|d| select_local_core(d));

    // Core first: explicit local override wins, otherwise bundled.
    // Fall back to bundled ONLY when no local core exists (UnknownSkill);
    // I/O failures still advertise `local`.
    if explicit {
        match local_core_res {
            Some(Err(_)) => {
                // Unreadable explicit dir: advertise local, never bundled, so
                // `list` and `get` (which surfaces I/O) agree on the source.
                if let Ok(dir) = dir_res.as_ref() {
                    out.push(SkillSummary {
                        name: BUNDLED_CORE_NAME.to_string(),
                        source: "local".to_string(),
                        description: None,
                        path: Some(dir.join("core").join("SKILL.md")),
                        refs: Vec::new(),
                    });
                } else {
                    push_bundled(&mut out);
                }
            }
            Some(Ok(Some((actual, skill_file)))) => {
                let dir = dir_res.as_ref().unwrap();
                let skill_root = skill_file
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| dir.join(&actual));
                match dir_res
                    .as_ref()
                    .ok()
                    .and_then(|d| read_skill_file(d, &skill_file, BUNDLED_CORE_NAME).ok())
                {
                    Some(content) => {
                        out.push(SkillSummary {
                            name: BUNDLED_CORE_NAME.to_string(),
                            source: "local".to_string(),
                            description: frontmatter_description(&content),
                            path: Some(skill_file),
                            refs: discover_ref_paths(&skill_root, dir),
                        });
                    }
                    None => {
                        // Unreadable override (dangling, invalid UTF-8, …):
                        // advertise local, never bundled.
                        if std::fs::symlink_metadata(&skill_file).is_ok() {
                            out.push(SkillSummary {
                                name: BUNDLED_CORE_NAME.to_string(),
                                source: "local".to_string(),
                                description: None,
                                path: Some(skill_file),
                                refs: discover_ref_paths(&skill_root, dir),
                            });
                        } else {
                            push_bundled(&mut out);
                        }
                    }
                }
            }
            Some(Ok(None)) | None => {
                push_bundled(&mut out);
            }
        }
    } else {
        push_bundled(&mut out);
    }

    let Some(dir) = dir_res.ok() else {
        return out;
    };
    for (name, path) in local {
        if is_core_name(&name) {
            // Reserved (any case): explicit already handled, default never shadows.
            continue;
        }
        let description = read_skill_file(&dir, &path, &name)
            .ok()
            .and_then(|c| frontmatter_description(&c));
        let skill_root = dir.join(&name);
        out.push(SkillSummary {
            name: name.clone(),
            source: "local".to_string(),
            description,
            path: Some(path),
            refs: discover_ref_paths(&skill_root, &dir),
        });
    }
    out
}

fn push_bundled(out: &mut Vec<SkillSummary>) {
    out.push(SkillSummary {
        name: BUNDLED_CORE_NAME.to_string(),
        source: "bundled".to_string(),
        description: frontmatter_description(CORE_SKILL),
        path: None,
        refs: BUNDLED_REFS.iter().map(|(r, _)| r.to_string()).collect(),
    });
}

/// Load one skill by name, honoring the bundled-core precedence rule.
///
/// The explicit local `core` loader is called directly; the bundled guide
/// is the fallback ONLY on `UnknownSkill` (missing override), never on
/// I/O failures (dangling symlink, invalid UTF-8, oversized, …) which
/// surface as `SkillError::Io`. Case-insensitive: `Core` follows the same
/// policy as `core`.
pub fn get_skill(name: &str) -> Result<FullSkill, SkillError> {
    if !valid_skill_name(name) {
        return Err(SkillError::Invalid(format!("invalid skill name {name:?}")));
    }
    if is_core_name(name) {
        if is_explicit_dir() {
            let dir = skills_dir()?;
            match load_local_skill(&dir, name) {
                Ok(local) => {
                    let mut local = local;
                    local.name = BUNDLED_CORE_NAME.to_string();
                    return Ok(local);
                }
                // A real I/O failure must surface, not fall back.
                Err(e @ SkillError::Io(_)) => return Err(e),
                Err(SkillError::UnknownSkill(_)) => {}
                Err(e) => return Err(e),
            }
        }
        let mut core = bundled_core();
        core.name = BUNDLED_CORE_NAME.to_string();
        return Ok(core);
    }
    let dir = skills_dir()?;
    load_local_skill(&dir, name)
}

/// Load one skill's `SKILL.md` without attachment bodies (lazy).
/// Same source decision as [`get_skill`]: explicit `core` override wins,
/// fallback to bundled ONLY on `UnknownSkill`.
pub fn get_skill_shallow(name: &str) -> Result<FullSkill, SkillError> {
    get_skill_shallow_with(name, explicit_skills_dir().as_deref())
}

/// Same as [`get_skill_shallow`] but with an injected explicit override dir
/// (no `AGENT_WEBMCP_SKILLS_DIR` read). After selecting a local `core`
/// override, its body is validated (I/O propagates). Falls back to bundled
/// only when no valid override exists.
pub(crate) fn get_skill_shallow_with(
    name: &str,
    explicit_dir: Option<&Path>,
) -> Result<FullSkill, SkillError> {
    if !valid_skill_name(name) {
        return Err(SkillError::Invalid(format!("invalid skill name {name:?}")));
    }
    if is_core_name(name) {
        if let Some(dir) = explicit_dir {
            match load_local_skill_shallow(dir, name) {
                Ok(mut local) => {
                    local.name = BUNDLED_CORE_NAME.to_string();
                    return Ok(local);
                }
                Err(e @ SkillError::Io(_)) => return Err(e),
                Err(SkillError::UnknownSkill(_)) => {}
                Err(e) => return Err(e),
            }
            let mut core = bundled_core();
            core.refs = Vec::new();
            core.name = BUNDLED_CORE_NAME.to_string();
            return Ok(core);
        }
        let mut core = bundled_core();
        core.refs = Vec::new();
        core.name = BUNDLED_CORE_NAME.to_string();
        return Ok(core);
    }
    if let Some(dir) = explicit_dir {
        return load_local_skill_shallow(dir, name);
    }
    let dir = default_skills_dir()?;
    load_local_skill_shallow(&dir, name)
}

/// True when a listed attachment exceeds `MAX_SKILL_FILE_BYTES` via resolved
/// file metadata (follows symlinks). Used by MCP `resources/list` to exclude
/// oversized files while `resources/read` keeps the bounded read check via
/// `read_inside`. Bundled `core` refs are in-memory and never exceed.
pub(crate) fn attachment_exceeds_limit(
    name: &str,
    ref_path: &str,
    explicit_dir: Option<&Path>,
) -> bool {
    let Some(dir) = explicit_dir else {
        return false;
    };
    let skill_root: PathBuf = if is_core_name(name) {
        match select_local_core(dir) {
            Ok(Some((actual, _))) => dir.join(&actual),
            // No override: bundled (never exceeds). Unreadable dir: no local
            // root to check — keep listed; the explicit read surfaces I/O.
            Ok(None) | Err(_) => return false,
        }
    } else {
        dir.join(name)
    };
    let file = skill_root.join(ref_path);
    // Resolved metadata: follows symlinks, never opens (no blocking).
    match std::fs::metadata(&file) {
        Ok(m) => m.len() > MAX_SKILL_FILE_BYTES,
        // Dangling/unreadable: keep listed; explicit read surfaces I/O.
        // Oversized regular files are excluded here but still error on read.
        Err(_) => false,
    }
}

/// All attachment paths (skill-relative, e.g. `references/foo.md`,
/// `templates/bar.json`) for one skill without reading bodies.
pub fn attachment_paths(name: &str) -> Result<Vec<String>, SkillError> {
    attachment_paths_with(name, explicit_skills_dir().as_deref())
}

/// Same as [`attachment_paths`] but with an injected explicit override dir
/// (no `AGENT_WEBMCP_SKILLS_DIR` read), so the bundled-resource unit test can
/// pass `None` for a clean bundled view regardless of process env.
pub(crate) fn attachment_paths_with(
    name: &str,
    explicit_dir: Option<&Path>,
) -> Result<Vec<String>, SkillError> {
    if !valid_skill_name(name) {
        return Err(SkillError::Invalid(format!("invalid skill name {name:?}")));
    }
    if is_core_name(name) {
        if let Some(dir) = explicit_dir {
            // `select_local_core` returns `Err` (not `None`) for an unreadable
            // dir, and `?` propagates it instead of silently using bundled.
            if let Some((actual, _)) = select_local_core(dir)? {
                let skill_root = dir.join(&actual);
                let mut all = discover_ref_paths(&skill_root, dir);
                all.extend(discover_template_paths(&skill_root, dir));
                all.sort();
                return Ok(all);
            }
        }
        let mut all: Vec<String> = BUNDLED_REFS.iter().map(|(r, _)| r.to_string()).collect();
        all.sort();
        return Ok(all);
    }
    // Non-core with injected override: use it when present, else use the
    // default dir (HOME/XDG) without consulting the override env.
    if let Some(dir) = explicit_dir {
        let skill_file = dir.join(name).join("SKILL.md");
        read_skill_file(dir, &skill_file, name)?;
        let skill_root = dir.join(name);
        let mut all = discover_ref_paths(&skill_root, dir);
        all.extend(discover_template_paths(&skill_root, dir));
        all.sort();
        return Ok(all);
    }
    let dir = default_skills_dir()?;
    let skill_file = dir.join(name).join("SKILL.md");
    read_skill_file(&dir, &skill_file, name)?;
    let skill_root = dir.join(name);
    let mut all = discover_ref_paths(&skill_root, &dir);
    all.extend(discover_template_paths(&skill_root, &dir));
    all.sort();
    Ok(all)
}

/// Load one `--ref` body by skill name plus query, reading ONLY the
/// selected file (lazy). Exact path, filename, and stem matches share one
/// policy; the matched file is read via `read_inside` so I/O failures
/// (invalid UTF-8, oversized, FIFO, escape, …) surface as `Io` (exit 1)
/// instead of being silently dropped or masked as unknown.
pub fn get_ref_content(skill_name: &str, query: &str) -> Result<RefContent, SkillError> {
    get_ref_content_with(skill_name, query, explicit_skills_dir().as_deref())
}

/// Same as [`get_ref_content`] but with an injected explicit override dir
/// (no `AGENT_WEBMCP_SKILLS_DIR` read). After selecting a local `core`
/// override, its `SKILL.md` body is validated first (propagating I/O) before
/// any reference body is served. Falls back to bundled `core` only when no
/// valid override exists. Uses all-candidates discovery so FIFOs and
/// out-of-root symlinks surface as I/O (not `UnknownRef`) without opening.
pub(crate) fn get_ref_content_with(
    skill_name: &str,
    query: &str,
    explicit_dir: Option<&Path>,
) -> Result<RefContent, SkillError> {
    if !valid_skill_name(skill_name) {
        return Err(SkillError::Invalid(format!(
            "invalid skill name {skill_name:?}"
        )));
    }
    if query.is_empty() || query.starts_with('/') || query.contains('\\') || query.contains(':') {
        return Err(SkillError::Invalid(format!("invalid ref {query:?}")));
    }
    for part in query.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(SkillError::Invalid(format!("invalid ref {query:?}")));
        }
    }
    // Core with no valid override: bundled, in-memory, no disk reads.
    if is_core_name(skill_name) {
        if let Some(dir) = explicit_dir {
            // Unreadable dir propagates via `?`, never silently falls back.
            if let Some((actual, skill_file)) = select_local_core(dir)? {
                // Validate the override body first: dangling/invalid UTF-8
                // must surface as I/O before any ref is served.
                read_skill_file(dir, &skill_file, BUNDLED_CORE_NAME)?;
                let skill_root = dir.join(&actual);
                // Exact paths resolve directly (no scan budget): a validated
                // exact URI that exists reads here, even past 512 oversized
                // files that listing would skip.
                if let Some(exact) = read_exact_attachment(&skill_root, dir, query) {
                    return exact;
                }
                let mut all_paths = discover_all_ref_paths(&skill_root, dir);
                all_paths.extend(discover_all_template_paths(&skill_root, dir));
                if let Some(matched) = match_path(&all_paths, query) {
                    let file = skill_root.join(&matched);
                    let content = read_inside(dir, &file)?;
                    return Ok(RefContent {
                        ref_path: matched,
                        content,
                    });
                }
                // No match: a broken base is I/O, not UnknownRef.
                if let Some(err) =
                    attachment_base_error(&skill_root, dir, "references", BUNDLED_CORE_NAME)
                {
                    return Err(err);
                }
                if let Some(err) =
                    attachment_base_error(&skill_root, dir, "templates", BUNDLED_CORE_NAME)
                {
                    return Err(err);
                }
                return Err(SkillError::UnknownRef {
                    skill: BUNDLED_CORE_NAME.to_string(),
                    query: query.to_string(),
                });
            }
            // No valid override: fall back to bundled.
            let core = bundled_core();
            return resolve_ref(&core, query).cloned();
        }
        // Injected "no override": bundled without touching process env.
        let core = bundled_core();
        return resolve_ref(&core, query).cloned();
    }
    // Non-core with injected override.
    if let Some(dir) = explicit_dir {
        let skill_file = dir.join(skill_name).join("SKILL.md");
        read_skill_file(dir, &skill_file, skill_name)?;
        let skill_root = dir.join(skill_name);
        // Exact paths resolve directly (no scan budget).
        if let Some(exact) = read_exact_attachment(&skill_root, dir, query) {
            return exact;
        }
        let mut all_paths = discover_all_ref_paths(&skill_root, dir);
        all_paths.extend(discover_all_template_paths(&skill_root, dir));
        if let Some(matched) = match_path(&all_paths, query) {
            let file = skill_root.join(&matched);
            let content = read_inside(dir, &file)?;
            return Ok(RefContent {
                ref_path: matched,
                content,
            });
        }
        if let Some(err) = attachment_base_error(&skill_root, dir, "references", skill_name) {
            return Err(err);
        }
        if let Some(err) = attachment_base_error(&skill_root, dir, "templates", skill_name) {
            return Err(err);
        }
        return Err(SkillError::UnknownRef {
            skill: skill_name.to_string(),
            query: query.to_string(),
        });
    }
    // Production path when no override was injected: core was already handled
    // above (bundled, no env). For non-core, use the default dir ignoring any
    // explicit override so the helper stays isolated from process env.
    let dir = default_skills_dir()?;
    let skill_file = dir.join(skill_name).join("SKILL.md");
    read_skill_file(&dir, &skill_file, skill_name)?;
    let skill_root = dir.join(skill_name);
    // Exact paths resolve directly (no scan budget).
    if let Some(exact) = read_exact_attachment(&skill_root, &dir, query) {
        return exact;
    }
    let mut all_paths = discover_all_ref_paths(&skill_root, &dir);
    all_paths.extend(discover_all_template_paths(&skill_root, &dir));
    if let Some(matched) = match_path(&all_paths, query) {
        let file = skill_root.join(&matched);
        let content = read_inside(&dir, &file)?;
        return Ok(RefContent {
            ref_path: matched,
            content,
        });
    }
    if let Some(err) = attachment_base_error(&skill_root, &dir, "references", skill_name) {
        return Err(err);
    }
    if let Some(err) = attachment_base_error(&skill_root, &dir, "templates", skill_name) {
        return Err(err);
    }
    Err(SkillError::UnknownRef {
        skill: skill_name.to_string(),
        query: query.to_string(),
    })
}

/// Validate one `--ref` query (shared by single- and multi-ref paths).
fn validate_ref_query(query: &str) -> Result<(), SkillError> {
    if query.is_empty() || query.starts_with('/') || query.contains('\\') || query.contains(':') {
        return Err(SkillError::Invalid(format!("invalid ref {query:?}")));
    }
    for part in query.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(SkillError::Invalid(format!("invalid ref {query:?}")));
        }
    }
    Ok(())
}

/// Load one skill body plus selected `--ref` bodies from the exact same
/// selected source (local override vs bundled `core`), never mixing a local
/// body with bundled refs if the override disappears mid-call.
///
/// The source is selected once: `select_local_core` runs a single time and
/// the body + all refs come from that same `skill_root`/`dir`. If the
/// override vanishes after selection, the body read fails as I/O (not a
/// silent fallback to bundled refs).
pub fn get_skill_with_refs(
    name: &str,
    queries: &[String],
) -> Result<(FullSkill, Vec<RefContent>), SkillError> {
    get_skill_with_refs_with(name, queries, explicit_skills_dir().as_deref())
}

/// Same as [`get_skill_with_refs`] but with an injected explicit override dir
/// (no `AGENT_WEBMCP_SKILLS_DIR` read).
pub(crate) fn get_skill_with_refs_with(
    name: &str,
    queries: &[String],
    explicit_dir: Option<&Path>,
) -> Result<(FullSkill, Vec<RefContent>), SkillError> {
    if !valid_skill_name(name) {
        return Err(SkillError::Invalid(format!("invalid skill name {name:?}")));
    }
    for q in queries {
        validate_ref_query(q)?;
    }
    if is_core_name(name) {
        if let Some(dir) = explicit_dir {
            // Single selection: local override vs bundled. An unreadable dir
            // propagates via `?`, never silently falls back to bundled.
            if let Some((actual, skill_file)) = select_local_core(dir)? {
                // Body from the selected override. A disappearance after
                // selection is I/O, never a fallback to bundled refs.
                let content = match read_skill_file(dir, &skill_file, BUNDLED_CORE_NAME) {
                    Ok(c) => c,
                    Err(SkillError::UnknownSkill(_)) => {
                        return Err(SkillError::Io(format!(
                            "could not read {}: override disappeared",
                            skill_file.display()
                        )));
                    }
                    Err(e) => return Err(e),
                };
                let skill_root = skill_file
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| dir.join(&actual));
                let shallow = FullSkill {
                    name: BUNDLED_CORE_NAME.to_string(),
                    source: "local".to_string(),
                    content,
                    refs: Vec::new(),
                    templates: Vec::new(),
                    path: Some(skill_file.clone()),
                };
                let mut all_paths = discover_all_ref_paths(&skill_root, dir);
                all_paths.extend(discover_all_template_paths(&skill_root, dir));
                let mut out = Vec::new();
                for q in queries {
                    // Exact paths resolve directly (no scan budget).
                    if let Some(exact) = read_exact_attachment(&skill_root, dir, q) {
                        out.push(exact?);
                        continue;
                    }
                    if let Some(matched) = match_path(&all_paths, q) {
                        let file = skill_root.join(&matched);
                        let content = read_inside(dir, &file)?;
                        out.push(RefContent {
                            ref_path: matched,
                            content,
                        });
                    } else {
                        if let Some(err) =
                            attachment_base_error(&skill_root, dir, "references", BUNDLED_CORE_NAME)
                        {
                            return Err(err);
                        }
                        if let Some(err) =
                            attachment_base_error(&skill_root, dir, "templates", BUNDLED_CORE_NAME)
                        {
                            return Err(err);
                        }
                        return Err(SkillError::UnknownRef {
                            skill: BUNDLED_CORE_NAME.to_string(),
                            query: q.clone(),
                        });
                    }
                }
                return Ok((shallow, out));
            }
            let core = bundled_core();
            let shallow = FullSkill {
                name: BUNDLED_CORE_NAME.to_string(),
                source: "bundled".to_string(),
                content: core.content.clone(),
                refs: Vec::new(),
                templates: Vec::new(),
                path: None,
            };
            let mut out = Vec::new();
            for q in queries {
                out.push(resolve_ref(&core, q)?.clone());
            }
            return Ok((shallow, out));
        }
        let core = bundled_core();
        let shallow = FullSkill {
            name: BUNDLED_CORE_NAME.to_string(),
            source: "bundled".to_string(),
            content: core.content.clone(),
            refs: Vec::new(),
            templates: Vec::new(),
            path: None,
        };
        let mut out = Vec::new();
        for q in queries {
            out.push(resolve_ref(&core, q)?.clone());
        }
        return Ok((shallow, out));
    }
    // Non-core: single dir snapshot, no bundled fallback to mix.
    let owned: Option<PathBuf> = explicit_dir.map(|d| d.to_path_buf());
    let dir: PathBuf = match owned {
        Some(d) => d,
        None => default_skills_dir()?,
    };
    let skill_file = dir.join(name).join("SKILL.md");
    let content = read_skill_file(&dir, &skill_file, name)?;
    let shallow = FullSkill {
        name: name.to_string(),
        source: "local".to_string(),
        content,
        refs: Vec::new(),
        templates: Vec::new(),
        path: Some(skill_file),
    };
    let skill_root = dir.join(name);
    let mut all_paths = discover_all_ref_paths(&skill_root, &dir);
    all_paths.extend(discover_all_template_paths(&skill_root, &dir));
    let mut out = Vec::new();
    for q in queries {
        // Exact paths resolve directly (no scan budget).
        if let Some(exact) = read_exact_attachment(&skill_root, &dir, q) {
            out.push(exact?);
            continue;
        }
        if let Some(matched) = match_path(&all_paths, q) {
            let file = skill_root.join(&matched);
            let content = read_inside(&dir, &file)?;
            out.push(RefContent {
                ref_path: matched,
                content,
            });
        } else {
            if let Some(err) = attachment_base_error(&skill_root, &dir, "references", name) {
                return Err(err);
            }
            if let Some(err) = attachment_base_error(&skill_root, &dir, "templates", name) {
                return Err(err);
            }
            return Err(SkillError::UnknownRef {
                skill: name.to_string(),
                query: q.clone(),
            });
        }
    }
    Ok((shallow, out))
}

/// Try an exact skill-relative attachment path directly, without consuming
/// the bounded catalog scan: validated containment + file checks via
/// [`read_inside`]. Returns `Some` when the query names an existing file
/// (content or I/O error); `None` when the path is absent so the caller
/// falls through to the filename/stem scan. Exact paths always contain
/// `/` (`references/foo.md`); bare filename/stem aliases skip this and keep
/// the bounded scan.
///
/// Only eligible attachment paths resolve here: `references/*.md`
/// (Markdown-only, like listing) and `templates/*` (any extension, like
/// listing, including nested subdirectories of each). Anything else —
/// `SKILL.md` itself, sibling directories, extensionless or non-Markdown
/// `references/` files — falls through to the alias scan, which reports
/// `UnknownRef`.
fn read_exact_attachment(
    skill_root: &Path,
    skills_root: &Path,
    query: &str,
) -> Option<Result<RefContent, SkillError>> {
    if !query.contains('/') {
        return None;
    }
    if query.is_empty()
        || query.starts_with('/')
        || query.contains('\\')
        || query.contains(':')
    {
        return None;
    }
    for part in query.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
    }
    let (base, rest) = query.split_once('/')?;
    match base {
        "references" => {
            // Listing only admits `.md` files here; keep the same rule so
            // an exact read cannot receive text the skill never exposes.
            if Path::new(rest).extension().and_then(|e| e.to_str()) != Some("md") {
                return None;
            }
        }
        "templates" => {}
        _ => return None,
    }
    let file = skill_root.join(query);
    // Absent (or unreadable metadata): not an exact hit — fall through to
    // the alias scan, which reports `UnknownRef` when nothing matches.
    // Present (including dangling symlinks): read directly so oversized,
    // FIFO, escape, and invalid-UTF-8 files surface as I/O without the scan
    // budget deciding whether they are visible.
    if std::fs::symlink_metadata(&file).is_err() {
        return None;
    }
    Some(
        read_inside(skills_root, &file).map(|content| RefContent {
            ref_path: query.to_string(),
            content,
        }),
    )
}

/// Match `--ref` against already-discovered relative paths: exact path
/// wins, then filename, then stem (any extension).
fn match_path(paths: &[String], query: &str) -> Option<String> {
    if paths.iter().any(|p| p == query) {
        return Some(query.to_string());
    }
    let hits: Vec<&String> = paths
        .iter()
        .filter(|p| p.rsplit('/').next().is_some_and(|f| f == query))
        .collect();
    if !hits.is_empty() {
        return Some(hits[0].clone());
    }
    let stem_hits: Vec<&String> = paths
        .iter()
        .filter(|p| {
            Path::new(p)
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == query)
        })
        .collect();
    if !stem_hits.is_empty() {
        return Some(stem_hits[0].clone());
    }
    None
}

/// Match `--ref` by relative path, filename, or stem.
fn match_ref<'a>(refs: &'a [RefContent], query: &str) -> Option<&'a RefContent> {
    // Exact relative path wins.
    if let Some(hit) = refs.iter().find(|r| r.ref_path == query) {
        return Some(hit);
    }
    // Filename (`drivers.md`).
    let hits: Vec<&RefContent> = refs
        .iter()
        .filter(|r| r.ref_path.rsplit('/').next().is_some_and(|f| f == query))
        .collect();
    if hits.len() == 1 {
        return Some(hits[0]);
    }
    if hits.len() > 1 {
        return Some(hits[0]);
    }
    // Stem (`drivers`): stem of the final path component, for any
    // reference/template extension (`drivers.md`, `scaffold.json`, ...).
    let stem_hits: Vec<&RefContent> = refs
        .iter()
        .filter(|r| {
            Path::new(&r.ref_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == query)
        })
        .collect();
    if stem_hits.len() == 1 {
        return Some(stem_hits[0]);
    }
    if stem_hits.len() > 1 {
        return Some(stem_hits[0]);
    }
    // Templates are matched the same way (exact path only above covers them).
    None
}

/// Resolve one `--ref` query within an already-loaded skill.
pub fn resolve_ref<'a>(skill: &'a FullSkill, query: &str) -> Result<&'a RefContent, SkillError> {
    if query.is_empty() || query.starts_with('/') || query.contains('\\') || query.contains(':') {
        return Err(SkillError::Invalid(format!("invalid ref {query:?}")));
    }
    // Reject `..` components before matching.
    for part in query.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(SkillError::Invalid(format!("invalid ref {query:?}")));
        }
    }
    // All loaded refs (references plus templates) are candidates.
    let mut combined: Vec<&RefContent> = Vec::new();
    combined.extend(skill.refs.iter());
    combined.extend(skill.templates.iter());
    // Exact path match first (covers `references/x` and `templates/x`).
    if let Some(hit) = combined.iter().find(|r| r.ref_path == query) {
        return Ok(hit);
    }
    // Borrowed slice for the filename/stem matcher.
    let owned: Vec<RefContent> = combined.into_iter().cloned().collect();
    match_ref(&owned, query)
        .and_then(|hit| {
            skill
                .refs
                .iter()
                .chain(skill.templates.iter())
                .find(|r| r.ref_path == hit.ref_path)
        })
        .ok_or_else(|| SkillError::UnknownRef {
            skill: skill.name.clone(),
            query: query.to_string(),
        })
}

/// Parse `skill://agent-webmcp/<name>` and `skill://agent-webmcp/<name>/<ref>`.
pub fn parse_resource_uri(uri: &str) -> Option<(String, Option<String>)> {
    let rest = uri.strip_prefix("skill://agent-webmcp/")?;
    if rest.is_empty() || rest.starts_with('/') {
        return None;
    }
    let mut parts = rest.splitn(2, '/');
    let name = parts.next()?;
    if !valid_skill_name(name) {
        return None;
    }
    match parts.next() {
        None => Some((name.to_string(), None)),
        Some(ref_part) => {
            if ref_part.is_empty() {
                return None;
            }
            Some((name.to_string(), Some(ref_part.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_core_loads() {
        let skill = bundled_core();
        assert_eq!(skill.name, "core");
        assert!(skill.content.contains("agent-webmcp"));
        assert_eq!(skill.refs.len(), 4);
    }

    #[test]
    fn ref_matches_path_filename_and_stem() {
        let skill = bundled_core();
        assert!(resolve_ref(&skill, "references/drivers.md").is_ok());
        assert!(resolve_ref(&skill, "drivers.md").is_ok());
        assert!(resolve_ref(&skill, "drivers").is_ok());
        assert!(resolve_ref(&skill, "authoring").is_ok());
        assert!(resolve_ref(&skill, "nope").is_err());
    }

    #[test]
    fn invalid_names_rejected() {
        assert!(!valid_skill_name("../evil"));
        assert!(!valid_skill_name("a/b"));
        assert!(valid_skill_name("my-skill_1.2"));
    }

    #[test]
    fn template_stem_matches_any_extension() {
        let skill = FullSkill {
            name: "demo".to_string(),
            source: "local".to_string(),
            content: "# demo".to_string(),
            refs: vec![RefContent {
                ref_path: "references/drivers.md".to_string(),
                content: "drivers".to_string(),
            }],
            templates: vec![
                RefContent {
                    ref_path: "templates/scaffold.json".to_string(),
                    content: "{}".to_string(),
                },
                RefContent {
                    ref_path: "templates/run.sh".to_string(),
                    content: "#!/bin/sh".to_string(),
                },
            ],
            path: None,
        };
        // Exact paths still win.
        assert_eq!(
            resolve_ref(&skill, "templates/scaffold.json")
                .unwrap()
                .ref_path,
            "templates/scaffold.json"
        );
        // Filename matches for any extension.
        assert_eq!(
            resolve_ref(&skill, "scaffold.json").unwrap().ref_path,
            "templates/scaffold.json"
        );
        // Stems match without hardcoding `.md`.
        assert_eq!(
            resolve_ref(&skill, "scaffold").unwrap().ref_path,
            "templates/scaffold.json"
        );
        assert_eq!(
            resolve_ref(&skill, "run").unwrap().ref_path,
            "templates/run.sh"
        );
        // `.md` stems keep working.
        assert_eq!(
            resolve_ref(&skill, "drivers").unwrap().ref_path,
            "references/drivers.md"
        );
    }

    #[test]
    fn rel_key_rejects_backslash_and_dotdot() {
        let base = Path::new("/tmp/skills-base");
        // Normal nested path joins with `/`.
        assert_eq!(
            rel_key(base, &base.join("a").join("b.md")).as_deref(),
            Some("a/b.md")
        );
        // Parent and absolute escapes are rejected (all platforms).
        assert_eq!(rel_key(base, &base.join("..").join("x.md")), None);
        assert_eq!(rel_key(base, Path::new("/etc/passwd")), None);
    }

    #[cfg(unix)]
    #[test]
    fn rel_key_rejects_literal_backslash() {
        let base = Path::new("/tmp/skills-base");
        // A literal backslash in a component is rejected (no naive replace).
        // Unix-only: on Windows `\` is a separator, not a literal.
        assert_eq!(rel_key(base, &base.join("foo\\bar.md")), None);
        assert_eq!(rel_key(base, &base.join("a\\b").join("c.md")), None);
    }

    #[test]
    fn invalid_utf8_is_io_not_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("bad");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("SKILL.md"), [0xff, 0xfe, 0x00, b'a']).unwrap();
        let err = load_local_skill(dir.path(), "bad").unwrap_err();
        assert!(
            matches!(err, SkillError::Io(_)),
            "invalid UTF-8 must be Io, got {err:?}"
        );
    }

    #[test]
    fn missing_skill_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let err = load_local_skill(dir.path(), "does-not-exist").unwrap_err();
        assert!(
            matches!(err, SkillError::UnknownSkill(_)),
            "missing file must be UnknownSkill, got {err:?}"
        );
    }

    #[test]
    fn oversized_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.md");
        let big = vec![b'a'; (MAX_SKILL_FILE_BYTES + 10) as usize];
        std::fs::write(&path, &big).unwrap();
        let err = read_inside(dir.path(), &path).unwrap_err();
        assert!(
            matches!(err, SkillError::Io(_)),
            "oversized file must be Io, got {err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn fifo_rejected_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let fifo = root.join("fifo.md");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo must run");
        assert!(status.success());
        let err = read_inside(root, &fifo).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("not a regular file"),
            "FIFO must be rejected as non-regular, got {msg}"
        );
        // A symlink to a regular file inside the root still reads.
        let real = root.join("real.md");
        std::fs::write(&real, "# real\n").unwrap();
        let link = root.join("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(read_inside(root, &link).unwrap(), "# real\n");
    }

    #[test]
    fn skills_dir_refuses_temp_fallback() {
        let empty = |key: &str| match key {
            "HOME" | "USERPROFILE" | "XDG_CONFIG_HOME" | "AGENT_WEBMCP_SKILLS_DIR" => None,
            _ => None,
        };
        let err = skills_dir_from(&empty).unwrap_err();
        assert!(matches!(err, SkillError::Io(_)), "must be Io, got {err:?}");
        let msg = format!("{err}");
        assert!(msg.contains("HOME"), "{msg}");

        // XDG alone is enough (no HOME needed, no temp fallback).
        let xdg_only = |key: &str| match key {
            "XDG_CONFIG_HOME" => Some("/tmp/xdg-test".to_string()),
            _ => None,
        };
        let dir = skills_dir_from(&xdg_only).unwrap();
        assert_eq!(
            dir,
            PathBuf::from("/tmp/xdg-test").join("agent-webmcp/skills")
        );

        // HOME alone is enough.
        let home_only = |key: &str| match key {
            "HOME" => Some("/tmp/home-test".to_string()),
            _ => None,
        };
        let dir = skills_dir_from(&home_only).unwrap();
        assert_eq!(
            dir,
            PathBuf::from("/tmp/home-test").join(".config/agent-webmcp/skills")
        );
    }

    #[test]
    fn frontmatter_block_scalars_and_quotes() {
        let folded = "---\nname: x\ndescription: >-\n  Line one\n  Line two\n---\n\n# x\n";
        assert_eq!(
            frontmatter_description(folded).as_deref(),
            Some("Line one Line two")
        );
        let folded_clip = "---\nname: x\ndescription: >\n  Line one\n  Line two\n---\n";
        assert_eq!(
            frontmatter_description(folded_clip).as_deref(),
            Some("Line one Line two")
        );
        let literal = "---\nname: x\ndescription: |\n  Line A\n  Line B\n---\n";
        assert_eq!(
            frontmatter_description(literal).as_deref(),
            Some("Line A\nLine B")
        );
        let strip = "---\nname: x\ndescription: |-\n  Line A\n  Line B\n---\n";
        assert_eq!(
            frontmatter_description(strip).as_deref(),
            Some("Line A\nLine B")
        );
        // Block indicator must not leak as prose.
        let bare = "---\nname: x\ndescription: >-\n---\n";
        assert_eq!(frontmatter_description(bare), None);

        let quoted = "---\nname: x\ndescription: \"Quoted prose\"\n---\n";
        assert_eq!(
            frontmatter_description(quoted).as_deref(),
            Some("Quoted prose")
        );
        let single = "---\nname: x\ndescription: 'Single quoted'\n---\n";
        assert_eq!(
            frontmatter_description(single).as_deref(),
            Some("Single quoted")
        );
        let plain = "---\nname: x\ndescription: Plain prose here\n---\n";
        assert_eq!(
            frontmatter_description(plain).as_deref(),
            Some("Plain prose here")
        );
    }

    #[test]
    fn core_name_is_case_insensitive() {
        assert!(is_core_name("core"));
        assert!(is_core_name("Core"));
        assert!(is_core_name("CORE"));
        assert!(!is_core_name("corey"));
        assert!(!is_core_name("mydemo"));
    }

    #[test]
    fn dangling_symlink_is_io_not_unknown() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("dangle");
            std::fs::create_dir_all(&root).unwrap();
            std::os::unix::fs::symlink("/nonexistent-target-xyz", root.join("SKILL.md")).unwrap();
            let err = load_local_skill(dir.path(), "dangle").unwrap_err();
            assert!(
                matches!(err, SkillError::Io(_)),
                "dangling symlink must be Io, got {err:?}"
            );
        }
    }

    #[test]
    fn ref_bodies_propagate_io_for_full() {
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("lazy");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: lazy\ndescription: Lazy.\n---\n\n# lazy\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
        std::fs::write(skill_root.join("references/bad.md"), [0xff, 0xfe]).unwrap();

        // Shallow (plain get) never reads bodies.
        let shallow = load_local_skill_shallow(dir.path(), "lazy").unwrap();
        assert!(shallow.refs.is_empty());

        // Discovery lists both without reading.
        let paths = discover_ref_paths(&skill_root, dir.path());
        assert!(paths.contains(&"references/ok.md".to_string()));
        assert!(paths.contains(&"references/bad.md".to_string()));

        // Full propagates the bad file.
        let err = load_local_skill(dir.path(), "lazy").unwrap_err();
        assert!(
            matches!(err, SkillError::Io(_)),
            "full must be Io, got {err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_cycle_does_not_hang() {
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("cyc");
        std::fs::create_dir_all(skill_root.join("references/a")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: cyc\ndescription: Cycle.\n---\n\n# cyc\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
        std::os::unix::fs::symlink("..", skill_root.join("references/a/loop")).unwrap();

        let paths = discover_ref_paths(&skill_root, dir.path());
        assert!(paths.contains(&"references/ok.md".to_string()));
        // Full still succeeds (only regular files are read, no infinite loop).
        let full = load_local_skill(dir.path(), "cyc").unwrap();
        assert!(full.refs.iter().any(|r| r.ref_path == "references/ok.md"));
    }

    #[test]
    fn double_quoted_escapes_decode_in_one_pass() {
        // `"Use \\new"` (two backslashes + `new`) must become `Use \new`
        // (backslash + `new`), not a newline from double-decoding `\\n`.
        assert_eq!(decode_double_escapes("Use \\\\new"), "Use \\new");
        assert_eq!(decode_double_escapes("a\\nb"), "a\nb");
        assert_eq!(decode_double_escapes("a\\rb"), "a\rb");
        assert_eq!(decode_double_escapes("a\\tb"), "a\tb");
        assert_eq!(decode_double_escapes("a\\\"b"), "a\"b");
        assert_eq!(decode_double_escapes("a\\\\b"), "a\\b");

        let fm = "---\nname: x\ndescription: \"Use \\\\new\"\n---\n\n# x\n";
        assert_eq!(frontmatter_description(fm).as_deref(), Some("Use \\new"));
    }

    #[test]
    fn listing_excludes_oversized_but_read_errors() {
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("bigskill");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: bigskill\ndescription: Big.\n---\n\n# big\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
        let big = vec![b'a'; (MAX_SKILL_FILE_BYTES + 10) as usize];
        std::fs::write(skill_root.join("references/big.md"), &big).unwrap();

        let listed = discover_ref_paths(&skill_root, dir.path());
        assert!(listed.contains(&"references/ok.md".to_string()));
        assert!(
            !listed.contains(&"references/big.md".to_string()),
            "oversized must be excluded from listing, got {listed:?}"
        );
        let err = read_inside(dir.path(), &skill_root.join("references/big.md")).unwrap_err();
        assert!(matches!(err, SkillError::Io(_)));
    }

    #[cfg(unix)]
    #[test]
    fn full_propagates_escaping_dir_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap();
        let outside = outside_dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("evil.md"), "# evil\n").unwrap();
        let skill_root = dir.path().join("evskill");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: evskill\ndescription: Ev.\n---\n\n# ev\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
        std::os::unix::fs::symlink(&outside, skill_root.join("references/evil")).unwrap();

        // Listing is best-effort: skips the escaping symlink.
        let listed = discover_ref_paths(&skill_root, dir.path());
        assert!(listed.contains(&"references/ok.md".to_string()));

        // Full propagates the escaping directory symlink as I/O.
        let err = load_local_skill(dir.path(), "evskill").unwrap_err();
        let msg = format!("{err}");
        assert!(
            matches!(err, SkillError::Io(_)) && msg.contains("escapes"),
            "full must be escapes Io, got {err:?}"
        );
    }

    #[test]
    fn full_budget_cap_returns_error_not_partial() {
        // Pre-filled budget trips the cap without creating 512 files.
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("budg");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: budg\ndescription: B.\n---\n\n# b\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();

        let mut budget = TraversalBudget {
            dirs_visited: 0,
            emitted: MAX_ATTACHMENT_FILES,
        };
        let err = discover_strict_ref_paths(&skill_root, dir.path(), &mut budget).unwrap_err();
        assert!(
            matches!(err, SkillError::Io(_)),
            "budget exhaustion must be Io, got {err:?}"
        );
        let msg = format!("{err}");
        assert!(msg.contains("too many"), "{msg}");

        // Listing truncates best-effort instead of erroring.
        let listed = discover_ref_paths(&skill_root, dir.path());
        assert!(listed.contains(&"references/ok.md".to_string()));
    }

    #[test]
    fn exact_attachment_resolves_without_catalog_scan() {
        // An exact attachment URI reads directly (containment + file checks)
        // without consuming the bounded catalog scan: even when the scan
        // would truncate before it, the exact path still resolves.
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("exact");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: exact\ndescription: E.\n---\n\n# e\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();

        let hit = read_exact_attachment(&skill_root, dir.path(), "references/ok.md")
            .expect("exact path must bypass the scan")
            .unwrap();
        assert_eq!(hit.ref_path, "references/ok.md");
        assert_eq!(hit.content, "# ok\n");

        // Bare filename/stem aliases keep the bounded scan (None here).
        assert!(read_exact_attachment(&skill_root, dir.path(), "ok.md").is_none());
        assert!(read_exact_attachment(&skill_root, dir.path(), "ok").is_none());
        // Absent exact paths fall through to the alias scan.
        assert!(read_exact_attachment(&skill_root, dir.path(), "references/nope.md").is_none());

        // End to end: the exact read path serves the attachment.
        let hit =
            get_ref_content_with("exact", "references/ok.md", Some(dir.path())).unwrap();
        assert_eq!(hit.ref_path, "references/ok.md");
        assert_eq!(hit.content, "# ok\n");
    }

    #[test]
    fn consistent_source_between_body_and_refs() {
        // Local override selected once: body + refs come from the same dir.
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("core");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: core\ndescription: Local.\n---\n\n# local body\n",
        )
        .unwrap();
        std::fs::write(skill_root.join("references/local.md"), "# local ref\n").unwrap();

        let (body, refs) =
            get_skill_with_refs_with("core", &["local".to_string()], Some(dir.path())).unwrap();
        assert_eq!(body.source, "local");
        assert!(body.content.contains("local body"));
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].ref_path, "references/local.md");
    }

    #[test]
    fn frontmatter_delimiters_only_at_column_zero() {
        // Indented `---` inside a literal block is content, not a fence.
        let fm = "---\nname: x\ndescription: |\n  Line A\n  ---\n  Line B\n---\n\n# x\n";
        assert_eq!(
            frontmatter_description(fm).as_deref(),
            Some("Line A\n---\nLine B")
        );
        // Indented `...` is also content.
        let fm_dots = "---\nname: x\ndescription: |\n  Line A\n  ...\n  Line B\n---\n\n# x\n";
        assert_eq!(
            frontmatter_description(fm_dots).as_deref(),
            Some("Line A\n...\nLine B")
        );
        // A closing fence with leading spaces does not terminate frontmatter
        // under column-0 rules; the description block then runs to the end
        // and still parses (the indented fence is content).
        let indented_close =
            "---\nname: x\ndescription: |\n  Line A\n  ---\n  Line B\n  ---\n\n# x\n";
        assert_eq!(
            frontmatter_description(indented_close).as_deref(),
            Some("Line A\n---\nLine B\n---")
        );
        // Opening fence must also be at column 0.
        assert_eq!(frontmatter_description("  ---\nname: x\n---\n"), None);
    }

    #[test]
    fn multiline_quoted_closes_late_single_pass() {
        // Double-quoted across three lines with a trailing comment on close.
        let fm = "---\nname: x\ndescription: \"line one\nline two\nline three\" # note\n---\n";
        assert_eq!(
            frontmatter_description(fm).as_deref(),
            Some("line one\nline two\nline three")
        );
        // Single-quoted across three lines.
        let fm_single = "---\nname: x\ndescription: 'alpha\nbeta\ngamma'\n---\n";
        assert_eq!(
            frontmatter_description(fm_single).as_deref(),
            Some("alpha\nbeta\ngamma")
        );
        // Unclosed quotes never produce a description.
        let unclosed = "---\nname: x\ndescription: \"never closes\nstill open\n---\n";
        assert_eq!(frontmatter_description(unclosed), None);
        let unclosed_single = "---\nname: x\ndescription: 'never closes\n---\n";
        assert_eq!(frontmatter_description(unclosed_single), None);
    }

    #[test]
    fn strict_file_budget_only_when_adding() {
        // Empty templates dir with a full budget is not an error: the limit
        // applies only when actually adding a 513th attachment.
        let dir = tempfile::tempdir().unwrap();
        let skill_root = dir.path().join("cap");
        std::fs::create_dir_all(skill_root.join("references")).unwrap();
        std::fs::create_dir_all(skill_root.join("templates")).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: cap\ndescription: C.\n---\n\n# c\n",
        )
        .unwrap();
        let mut full = TraversalBudget {
            dirs_visited: 0,
            emitted: MAX_ATTACHMENT_FILES,
        };
        let empty = discover_strict_template_paths(&skill_root, dir.path(), &mut full)
            .expect("empty dir at cap must be Ok");
        assert!(empty.is_empty());

        // A non-empty dir at the cap still refuses when adding the 513th.
        std::fs::write(skill_root.join("templates/one.md"), "# one\n").unwrap();
        let mut full2 = TraversalBudget {
            dirs_visited: 0,
            emitted: MAX_ATTACHMENT_FILES,
        };
        let err = discover_strict_template_paths(&skill_root, dir.path(), &mut full2).unwrap_err();
        assert!(matches!(err, SkillError::Io(_)), "must be Io, got {err:?}");
        assert!(format!("{err}").contains("too many"));
    }

    #[test]
    fn select_local_core_missing_vs_unreadable() {
        // Missing directory means "no override".
        let missing = Path::new("/nonexistent-agent-webmcp-skills-xyz");
        assert!(
            matches!(select_local_core(missing), Ok(None)),
            "missing dir must be Ok(None)"
        );

        // An existing path that cannot be enumerated (a file, so `read_dir`
        // fails with `NotADirectory`, not `NotFound`) must propagate I/O
        // instead of silently falling back to bundled `core`.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"not a dir").unwrap();
        let err = select_local_core(&file).unwrap_err();
        assert!(
            matches!(err, SkillError::Io(_)),
            "file-as-dir must be Io, got {err:?}"
        );
        assert!(format!("{err}").contains("could not read"));

        // And the shallow loader surfaces that I/O instead of UnknownSkill.
        let err = load_local_skill_shallow(&file, "core").unwrap_err();
        assert!(
            matches!(err, SkillError::Io(_)),
            "shallow must be Io, got {err:?}"
        );

        // Listing with an unreadable explicit dir advertises `local`, never
        // bundled, so `list` and `get` agree on the source.
        let listed = list_skills_with(Some(&file));
        let core = listed
            .iter()
            .find(|s| s.name == "core")
            .expect("core listed");
        assert_eq!(core.source, "local", "unreadable dir must list local");
    }
}
