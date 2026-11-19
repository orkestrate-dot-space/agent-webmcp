//! User config, project config, env vars, then flag overrides, merged in
//! that order. Files are JSONC; project files take only an allowlist
//! (call.timeout, drivers.agent-browser.headed, permissions.ask and
//! permissions.deny), so a cloned repo never loosens what runs.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Which driver `call` uses: the first usable run driver, or a plan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriverChoice {
    /// Use the first installed run driver, else print a plan.
    #[default]
    Auto,
    /// Run driver: drives a browser over a local socket.
    AgentBrowser,
    /// Run driver: lighter browser over PandaScript.
    Lightpanda,
    /// Plan driver: steps for the agent's Playwright MCP.
    PlaywrightMcp,
    /// Plan driver: steps for the agent's Chrome DevTools MCP.
    ChromeDevtoolsMcp,
    /// Plan driver: steps for OpenCode Browse.
    Browse,
    /// Never drive a browser; always print a generic plan.
    None,
}

/// Per-driver settings; agent-browser and lightpanda run outside the agent session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Drivers {
    /// Settings for the agent-browser run driver.
    #[serde(rename = "agent-browser")]
    pub agent_browser: AgentBrowserSettings,
    /// Settings for the lightpanda run driver.
    pub lightpanda: LightpandaSettings,
}

/// How to reach agent-browser: command, session, and how to show it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AgentBrowserSettings {
    /// Program to run; user config only, never the project file.
    pub command: String,
    /// Named session to reuse, so logins persist across calls.
    pub session: String,
    /// Show the browser window instead of running headless.
    pub headed: bool,
    /// Extra `--args` values for agent-browser, before the WebMCP one.
    /// User config only; agent-webmcp merges duplicates itself.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Logged-in Chrome profile dir; user config only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Attach to a running Chrome; user config only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdp: Option<String>,
}

impl Default for AgentBrowserSettings {
    fn default() -> Self {
        Self {
            command: "agent-browser".to_string(),
            session: "agent-webmcp".to_string(),
            headed: false,
            profile: None,
            cdp: None,
            extra_args: Vec::new(),
        }
    }
}

/// How to reach lightpanda: just the program to run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct LightpandaSettings {
    /// Program to run; user config only, never the project file.
    pub command: String,
}

impl Default for LightpandaSettings {
    fn default() -> Self {
        Self {
            command: "lightpanda".to_string(),
        }
    }
}

/// Call-wide settings shared by every driver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct CallSettings {
    /// How long one tool call may take, in milliseconds.
    pub timeout: u64,
    /// How long to wait for a late tool registration, in milliseconds.
    /// Clamped to 30 s where it is used; 0 waits not at all.
    #[serde(alias = "settle_ms")]
    pub settle_ms: u64,
}

impl Default for CallSettings {
    fn default() -> Self {
        Self {
            timeout: 60_000,
            settle_ms: 8_000,
        }
    }
}

/// Tool patterns governing whether a call runs, asks, or is refused.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Permissions {
    /// Patterns allowed without asking.
    pub allow: Vec<String>,
    /// Patterns needing confirmation first.
    pub ask: Vec<String>,
    /// Patterns never called; deny wins over ask and allow.
    pub deny: Vec<String>,
}

/// The whole configuration file, plus what the flags override.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Config {
    /// Schema URL for editors; read and ignored.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    /// Which driver to use, or `auto` to pick.
    pub driver: DriverChoice,
    /// Per-driver settings.
    pub drivers: Drivers,
    /// Call-wide settings.
    pub call: CallSettings,
    /// Tool patterns for allow, ask, and deny.
    pub permissions: Permissions,
}

/// Flag overrides fed in by the CLI; each set field wins over files and env.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    /// Override `driver`.
    pub driver: Option<DriverChoice>,
    /// Override `drivers.agent-browser.session`.
    pub session: Option<String>,
    /// Override `drivers.agent-browser.headed`.
    pub headed: Option<bool>,
    /// Override `call.timeout`.
    pub timeout: Option<u64>,
    /// Override `call.settle_ms`.
    pub settle: Option<u64>,
    /// Override `drivers.agent-browser.command`.
    pub command: Option<String>,
}

impl Config {
    /// Apply flag overrides; each set field replaces the merged value.
    pub fn apply_overrides(&mut self, o: &Overrides) {
        if let Some(driver) = o.driver {
            self.driver = driver;
        }
        if let Some(session) = &o.session {
            self.drivers.agent_browser.session.clone_from(session);
        }
        if let Some(headed) = o.headed {
            self.drivers.agent_browser.headed = headed;
        }
        if let Some(timeout) = o.timeout {
            self.call.timeout = timeout;
        }
        if let Some(settle) = o.settle {
            self.call.settle_ms = settle;
        }
        if let Some(command) = &o.command {
            self.drivers.agent_browser.command.clone_from(command);
        }
    }
}

/// Which layer of the effective config a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// The user's own config file.
    User,
    /// The nearest project config at or above the current directory.
    Project,
    /// Environment variables.
    Env,
}

/// One merged layer; env layers have no path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// Which layer this is.
    pub kind: SourceKind,
    /// File read for this layer, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            SourceKind::User => write!(f, "user"),
            SourceKind::Project => write!(f, "project"),
            SourceKind::Env => write!(f, "env"),
        }?;
        if let Some(path) = &self.path {
            write!(f, ": {}", path.display())?;
        }
        Ok(())
    }
}

/// The merged config plus where each layer came from and file warnings.
#[derive(Debug, Clone, Serialize)]
pub struct Loaded {
    /// The effective config after merging every layer.
    pub config: Config,
    /// Layers used, lowest precedence first.
    pub sources: Vec<Source>,
    /// Non-fatal file notes, printed to stderr.
    pub warnings: Vec<String>,
}

/// Config file locations without reading anything, for `config path`.
#[derive(Debug, Clone)]
pub struct Locations {
    /// The user file: explicit, existing, or the default `.jsonc` path.
    pub user: PathBuf,
    /// Whether the user file exists.
    pub user_exists: bool,
    /// The nearest project file from `cwd` upward, if any.
    pub project: Option<PathBuf>,
    /// Discovery notes (both `.json` and `.jsonc` present).
    pub warnings: Vec<String>,
}

/// Load the effective config from the real environment and directory.
pub fn load() -> Result<Loaded> {
    let cwd = std::env::current_dir()?;
    load_from(&|key: &str| std::env::var(key).ok(), &cwd)
}

/// Load the effective config: user file, project file, then env vars.
/// Missing files mean defaults; bad values name the file, variable, or key.
pub fn load_from(env: &dyn Fn(&str) -> Option<String>, cwd: &Path) -> Result<Loaded> {
    let set = |key: &str| env(key).filter(|v| !v.is_empty());
    let home: PathBuf = set("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let mut merged = serde_json::Value::Object(serde_json::Map::new());
    let mut sources = Vec::new();
    let mut warnings = Vec::new();

    if let Some(explicit) = set("AGENT_WEBMCP_CONFIG") {
        let raw = PathBuf::from(&explicit);
        let path = if raw.is_absolute() {
            raw
        } else {
            cwd.join(raw)
        };
        let mut value = read_file(&path)?;
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| cwd.to_path_buf());
        substitute(&mut value, &dir, env, &home, None, &path, false)?;
        check_ranges(&check_file(&value, &path)?, &path)?;
        merge_into(&mut merged, value);
        sources.push(Source {
            kind: SourceKind::User,
            path: Some(path),
        });
    } else {
        let dir = user_dir(env);
        let (found, warning) = pick_file(&dir);
        warnings.extend(warning);
        let user_expected = found
            .clone()
            .unwrap_or_else(|| dir.join("agent-webmcp.jsonc"));
        if let Some(path) = found {
            let mut value = read_file(&path)?;
            substitute(&mut value, &dir, env, &home, None, &path, false)?;
            check_ranges(&check_file(&value, &path)?, &path)?;
            merge_into(&mut merged, value);
            sources.push(Source {
                kind: SourceKind::User,
                path: Some(path),
            });
        }
        let (project, root, project_warnings) = find_project(cwd);
        warnings.extend(project_warnings);
        if let Some(path) = project {
            let mut value = read_file(&path)?;
            let file_dir = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| cwd.to_path_buf());
            substitute(
                &mut value,
                &file_dir,
                env,
                &home,
                root.as_deref(),
                &path,
                true,
            )?;
            check_ranges(&check_file(&value, &path)?, &path)?;
            check_project_keys(&value, &path, &user_expected)?;
            merge_into(&mut merged, value);
            sources.push(Source {
                kind: SourceKind::Project,
                path: Some(path),
            });
        }
    }

    let mut config: Config = serde_json::from_value(merged)
        .map_err(|e| anyhow::anyhow!("invalid config: {e:#}; fix the config file and retry"))?;
    if apply_env(&mut config, env)? {
        sources.push(Source {
            kind: SourceKind::Env,
            path: None,
        });
    }
    Ok(Loaded {
        config,
        sources,
        warnings,
    })
}

/// The directory holding the user config: `$XDG_CONFIG_HOME/agent-webmcp`,
/// else `$HOME/.config/agent-webmcp`. Empty values count as unset.
pub fn user_dir(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(dir) = env("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir).join("agent-webmcp");
    }
    let home = env("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".config/agent-webmcp")
}

/// The default user config path for messages: the explicit file, or the
/// `.jsonc` default the loader would read.
pub fn user_path(env: &dyn Fn(&str) -> Option<String>, cwd: &Path) -> PathBuf {
    locate(env, cwd).user
}

/// Find the config files without parsing them.
pub fn locate(env: &dyn Fn(&str) -> Option<String>, cwd: &Path) -> Locations {
    let set = |key: &str| env(key).filter(|v| !v.is_empty());
    let mut warnings = Vec::new();
    // An explicit file replaces discovery entirely: no project file.
    if let Some(explicit) = set("AGENT_WEBMCP_CONFIG") {
        let raw = PathBuf::from(&explicit);
        let path = if raw.is_absolute() {
            raw
        } else {
            cwd.join(raw)
        };
        return Locations {
            user_exists: path.exists(),
            user: path,
            project: None,
            warnings,
        };
    }
    let dir = user_dir(env);
    let (found, warning) = pick_file(&dir);
    warnings.extend(warning);
    let (user, user_exists) = match found {
        Some(path) => (path, true),
        None => (dir.join("agent-webmcp.jsonc"), false),
    };
    let (project, _, project_warnings) = find_project(cwd);
    warnings.extend(project_warnings);
    Locations {
        user,
        user_exists,
        project,
        warnings,
    }
}

/// Pick the config file in one directory: `.jsonc` wins over `.json`.
fn pick_file(dir: &Path) -> (Option<PathBuf>, Option<String>) {
    let jsonc = dir.join("agent-webmcp.jsonc");
    let json = dir.join("agent-webmcp.json");
    let has_jsonc = jsonc.is_file();
    let has_json = json.is_file();
    if has_jsonc && has_json {
        let warning = format!(
            "both agent-webmcp.json and agent-webmcp.jsonc in {}; using .jsonc",
            dir.display()
        );
        (Some(jsonc), Some(warning))
    } else if has_jsonc {
        (Some(jsonc), None)
    } else if has_json {
        (Some(json), None)
    } else {
        (None, None)
    }
}

/// Find the nearest project file from `cwd` up to the git root, plus the
/// root itself for `{file:}` containment. Without a `.git`, only `cwd`.
fn find_project(cwd: &Path) -> (Option<PathBuf>, Option<PathBuf>, Vec<String>) {
    let mut going = cwd.to_path_buf();
    let root = loop {
        if going.join(".git").exists() {
            break Some(going.clone());
        }
        match going.parent() {
            Some(parent) => going = parent.to_path_buf(),
            None => break None,
        }
    };
    let Some(root) = root else {
        let (found, warning) = pick_file(cwd);
        return (
            found,
            Some(cwd.to_path_buf()),
            warning.into_iter().collect(),
        );
    };
    let mut warnings = Vec::new();
    let mut going = cwd.to_path_buf();
    loop {
        let (found, warning) = pick_file(&going);
        warnings.extend(warning);
        if found.is_some() {
            return (found, Some(root), warnings);
        }
        if going == root {
            return (None, Some(root), warnings);
        }
        match going.parent() {
            Some(parent) => going = parent.to_path_buf(),
            None => return (None, Some(root), warnings),
        }
    }
}

/// Read one config file as JSONC, keeping line numbers for error messages.
fn read_file(path: &Path) -> Result<serde_json::Value> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("could not read config file {}: {e:#}", path.display()))?;
    serde_json::from_str(&strip_jsonc(&text))
        .map_err(|e| anyhow::anyhow!("invalid JSON in {}: {e}", path.display()))
}

/// Type-check one file on its own so typos name the file they came from.
fn check_file(value: &serde_json::Value, path: &Path) -> Result<Config> {
    serde_json::from_value::<Config>(value.clone())
        .map_err(|e| anyhow::anyhow!("invalid config in {}: {e}", path.display()))
}

/// Project files take an allowlist: a repo may share stricter rules and
/// timeouts, never looser ones or anything that runs.
fn check_project_keys(value: &serde_json::Value, path: &Path, user_path: &Path) -> Result<()> {
    if let Some(key) = first_disallowed(value, "") {
        bail!(
            "{} sets {key}; project config can only set call.timeout, drivers.agent-browser.headed, permissions.ask and permissions.deny. Put it in your user config ({})",
            path.display(),
            user_path.display()
        );
    }
    Ok(())
}

/// First key outside the project allowlist in document order, dotted.
fn first_disallowed(value: &serde_json::Value, parent: &str) -> Option<String> {
    for (key, child) in value.as_object()? {
        let dotted = if parent.is_empty() {
            key.clone()
        } else {
            format!("{parent}.{key}")
        };
        if !allowed_key(parent, key) {
            return Some(dotted);
        }
        if child.is_object()
            && is_container(&dotted)
            && let Some(deep) = first_disallowed(child, &dotted)
        {
            return Some(deep);
        }
    }
    None
}

/// Whether `key` may appear inside the project object at `parent`.
fn allowed_key(parent: &str, key: &str) -> bool {
    match parent {
        "" => matches!(key, "$schema" | "call" | "drivers" | "permissions"),
        "call" => key == "timeout",
        "drivers" => matches!(key, "agent-browser" | "lightpanda"),
        "drivers.agent-browser" => key == "headed",
        "permissions" => matches!(key, "ask" | "deny"),
        _ => false,
    }
}

/// Project objects whose children the allowlist names individually.
fn is_container(dotted: &str) -> bool {
    matches!(
        dotted,
        "call" | "drivers" | "drivers.agent-browser" | "drivers.lightpanda" | "permissions"
    )
}

/// Expand `{env:NAME}` and `{file:PATH}` in every string value.
#[allow(clippy::too_many_arguments)]
fn substitute(
    value: &mut serde_json::Value,
    file_dir: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    root: Option<&Path>,
    path: &Path,
    is_project: bool,
) -> Result<()> {
    match value {
        serde_json::Value::String(s) => {
            *s = expand(s, file_dir, env, home, root, path, is_project)?;
        }
        serde_json::Value::Array(items) => {
            for item in items {
                substitute(item, file_dir, env, home, root, path, is_project)?;
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                substitute(value, file_dir, env, home, root, path, is_project)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Expand placeholders in one string; a missing `}` leaves text literal.
#[allow(clippy::too_many_arguments)]
fn expand(
    text: &str,
    file_dir: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    root: Option<&Path>,
    path: &Path,
    is_project: bool,
) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let next_env = rest.find("{env:");
        let next_file = rest.find("{file:");
        let (start, is_env) = match (next_env, next_file) {
            (Some(a), Some(b)) => {
                if a <= b {
                    (a, true)
                } else {
                    (b, false)
                }
            }
            (Some(a), None) => (a, true),
            (None, Some(b)) => (b, false),
            (None, None) => break,
        };
        let key = &rest[start + if is_env { 5 } else { 6 }..];
        let Some(end) = key.find('}') else { break };
        let name = &key[..end];
        out.push_str(&rest[..start]);
        if is_env {
            out.push_str(&env(name).unwrap_or_default());
        } else {
            let resolved = if let Some(relative) = name.strip_prefix("~/") {
                home.join(relative)
            } else {
                let candidate = PathBuf::from(name);
                if candidate.is_absolute() {
                    candidate
                } else {
                    file_dir.join(candidate)
                }
            };
            let canonical = std::fs::canonicalize(&resolved).map_err(|_| {
                anyhow::anyhow!(
                    "could not read file {} (from {{file:{name}}} in {}): no such file",
                    resolved.display(),
                    path.display()
                )
            })?;
            if is_project && let Some(root) = root {
                let canonical_root =
                    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
                if !canonical.starts_with(&canonical_root) {
                    bail!(
                        "{} uses {{file:{name}}} which resolves outside the project ({}); project configs cannot read files outside the project",
                        path.display(),
                        root.display()
                    );
                }
            }
            let mut contents = std::fs::read_to_string(&canonical).map_err(|e| {
                anyhow::anyhow!(
                    "could not read file {} (from {{file:{name}}} in {}): {e:#}",
                    canonical.display(),
                    path.display()
                )
            })?;
            if contents.ends_with('\n') {
                contents.pop();
                if contents.ends_with('\r') {
                    contents.pop();
                }
            }
            out.push_str(&contents);
        }
        rest = &key[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Merge `over` into `base`: objects recurse and later scalars win, except
/// the `permissions` lists, which append without duplicates.
fn merge_into(base: &mut serde_json::Value, over: serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(over_map)) => {
            for (key, over_value) in over_map {
                match base_map.get_mut(&key) {
                    Some(base_value) => {
                        if key == "permissions" {
                            merge_permissions(base_value, over_value);
                        } else {
                            merge_into(base_value, over_value);
                        }
                    }
                    None => {
                        base_map.insert(key, over_value);
                    }
                }
            }
        }
        (base, over) => {
            *base = over;
        }
    }
}

/// Merge two `permissions` objects: allow/ask/deny append, rest recurses.
fn merge_permissions(base: &mut serde_json::Value, over: serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(over_map)) => {
            for (key, over_value) in over_map {
                let appendable = matches!(key.as_str(), "allow" | "ask" | "deny")
                    && matches!(base_map.get(&key), Some(serde_json::Value::Array(_)))
                    && matches!(over_value, serde_json::Value::Array(_));
                if appendable {
                    if let Some(serde_json::Value::Array(base_list)) = base_map.get_mut(&key)
                        && let serde_json::Value::Array(over_list) = over_value
                    {
                        for item in over_list {
                            if !base_list.contains(&item) {
                                base_list.push(item);
                            }
                        }
                    }
                } else if let Some(base_value) = base_map.get_mut(&key) {
                    merge_into(base_value, over_value);
                } else {
                    base_map.insert(key, over_value);
                }
            }
        }
        (base, over) => {
            *base = over;
        }
    }
}

/// Millisecond bounds for `call.timeout`: zero never runs, huge never returns.
pub const TIMEOUT_RANGE: (u64, u64) = (1, 3_600_000);
/// Millisecond bounds for `call.settle_ms`: zero waits not at all.
pub const SETTLE_RANGE: (u64, u64) = (0, 30_000);

/// Check a millisecond setting against its range; errors name the source.
fn check_ms(name: &str, value: u64, range: (u64, u64), source: &str) -> Result<()> {
    if value < range.0 || value > range.1 {
        bail!(
            "{name} must be {min}..{max} ms, got {value} from {source}",
            min = range.0,
            max = range.1
        );
    }
    Ok(())
}

/// Range-check `call.timeout` from any layer; errors name the source.
pub fn check_timeout_ms(value: u64, source: &str) -> Result<()> {
    check_ms("timeout", value, TIMEOUT_RANGE, source)
}

/// Range-check `call.settle_ms` from any layer; errors name the source.
pub fn check_settle_ms(value: u64, source: &str) -> Result<()> {
    check_ms("settle_ms", value, SETTLE_RANGE, source)
}

/// Range-check the timeouts in one file; errors name the file.
fn check_ranges(cfg: &Config, path: &Path) -> Result<()> {
    let source = &path.display().to_string();
    check_timeout_ms(cfg.call.timeout, source)?;
    check_settle_ms(cfg.call.settle_ms, source)?;
    Ok(())
}

/// Apply env vars over the merged files; true when any var was set.
fn apply_env(config: &mut Config, env: &dyn Fn(&str) -> Option<String>) -> Result<bool> {
    let set = |key: &str| env(key).filter(|v| !v.is_empty());
    let mut used = false;
    if let Some(value) = set("AGENT_WEBMCP_DRIVER") {
        config.driver = match value.as_str() {
            "auto" => DriverChoice::Auto,
            "agent-browser" => DriverChoice::AgentBrowser,
            "lightpanda" => DriverChoice::Lightpanda,
            "playwright-mcp" => DriverChoice::PlaywrightMcp,
            "chrome-devtools-mcp" => DriverChoice::ChromeDevtoolsMcp,
            "browse" => DriverChoice::Browse,
            "none" => DriverChoice::None,
            _ => bail!(
                "invalid AGENT_WEBMCP_DRIVER value {value:?}; expected one of auto, agent-browser, lightpanda, playwright-mcp, chrome-devtools-mcp, browse, none"
            ),
        };
        used = true;
    }
    if let Some(value) = set("AGENT_WEBMCP_SESSION") {
        config.drivers.agent_browser.session = value;
        used = true;
    }
    if let Some(value) = set("AGENT_WEBMCP_HEADED") {
        config.drivers.agent_browser.headed = match value.to_lowercase().as_str() {
            "1" | "true" => true,
            "0" | "false" => false,
            _ => bail!(
                "invalid AGENT_WEBMCP_HEADED value {value:?}; expected one of 1, true, 0, false"
            ),
        };
        used = true;
    }
    if let Some(value) = set("AGENT_WEBMCP_SETTLE") {
        let parsed: u64 = value.parse::<u64>().map_err(|_| {
            anyhow::anyhow!(
                "invalid AGENT_WEBMCP_SETTLE value {value:?}; expected milliseconds as a number"
            )
        })?;
        check_settle_ms(parsed, "AGENT_WEBMCP_SETTLE")?;
        config.call.settle_ms = parsed;
        used = true;
    }
    if let Some(value) = set("AGENT_WEBMCP_TIMEOUT") {
        let parsed: u64 = value.parse::<u64>().map_err(|_| {
            anyhow::anyhow!(
                "invalid AGENT_WEBMCP_TIMEOUT value {value:?}; expected milliseconds as a number"
            )
        })?;
        check_timeout_ms(parsed, "AGENT_WEBMCP_TIMEOUT")?;
        config.call.timeout = parsed;
        used = true;
    }
    if let Some(value) = set("AGENT_WEBMCP_AGENT_BROWSER") {
        config.drivers.agent_browser.command = value;
        used = true;
    }
    if let Some(value) = set("AGENT_WEBMCP_LIGHTPANDA") {
        config.drivers.lightpanda.command = value;
        used = true;
    }
    if let Some(value) = set("AGENT_WEBMCP_EXTRA_ARGS") {
        config.drivers.agent_browser.extra_args = value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        used = true;
    }
    Ok(used)
}

/// Strip `//` and `/* */` comments plus trailing commas, keeping line and
/// column numbers by replacing removed characters with spaces.
fn strip_jsonc(text: &str) -> String {
    let mut out: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;
    while i < out.len() {
        let c = out[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
        } else if c == '"' {
            in_string = true;
            i += 1;
        } else if c == '/' && i + 1 < out.len() && out[i + 1] == '/' {
            out[i] = ' ';
            out[i + 1] = ' ';
            i += 2;
            while i < out.len() && out[i] != '\n' {
                out[i] = ' ';
                i += 1;
            }
        } else if c == '/' && i + 1 < out.len() && out[i + 1] == '*' {
            out[i] = ' ';
            out[i + 1] = ' ';
            i += 2;
            while i < out.len() {
                if out[i] == '*' && i + 1 < out.len() && out[i + 1] == '/' {
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    i += 2;
                    break;
                }
                if out[i] != '\n' {
                    out[i] = ' ';
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < out.len() {
        let c = out[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        } else if c == ',' {
            let mut j = i + 1;
            while j < out.len() && matches!(out[j], ' ' | '\t' | '\n' | '\r') {
                j += 1;
            }
            if j < out.len() && (out[j] == '}' || out[j] == ']') {
                out[i] = ' ';
            }
        }
        i += 1;
    }
    out.into_iter().collect()
}
