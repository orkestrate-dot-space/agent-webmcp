# Config reference (agent-webmcp v0.4.0)

Config files are JSONC (JSON with `//` / `/* */` comments and trailing
commas). Inspect with `agent-webmcp config show|path|validate`.

## Files and precedence (later wins)

1. Built-in defaults.
2. User config: `$XDG_CONFIG_HOME/agent-webmcp/agent-webmcp.jsonc`
   (or `.json`; defaults to `~/.config/agent-webmcp/agent-webmcp.jsonc`).
   If `AGENT_WEBMCP_CONFIG` is set, that file replaces discovery.
3. Project config: `agent-webmcp.jsonc` (or `.json`) at or above the
   current directory (up to the git root).
4. Environment variables.
5. CLI flags (`--driver`, `--session`, `--headed`, `--timeout`, `--settle`).

If `.jsonc` and `.json` exist side by side, `.jsonc` wins with a warning.

## Project-config safe allowlist

Untrusted repos may only set:

- `call.timeout`
- `drivers.agent-browser.headed`
- `permissions.ask`
- `permissions.deny`

Anything else (`drivers.*.command`, `permissions.allow`, `profile`,
`cdp`, …) fails validation. Put it in your user config instead.

## Variable substitution

String values support:

- `{env:VAR_NAME}` — value of `VAR_NAME` (empty when unset).
- `{file:relative/or/~/path}` — trimmed file contents, resolved
  relative to the config file (or `~/`). Project configs cannot read
  outside the project.

## Environment variables

| Variable | Overrides |
|---|---|
| `AGENT_WEBMCP_CONFIG` | user config file path |
| `AGENT_WEBMCP_DRIVER` | `driver` |
| `AGENT_WEBMCP_SESSION` | `drivers.agent-browser.session` |
| `AGENT_WEBMCP_HEADED` | `drivers.agent-browser.headed` (`1`/`true`/`0`/`false`) |
| `AGENT_WEBMCP_TIMEOUT` | `call.timeout` (ms) |
| `AGENT_WEBMCP_SETTLE` | `call.settleMs` (ms) |
| `AGENT_WEBMCP_AGENT_BROWSER` | `drivers.agent-browser.command` |
| `AGENT_WEBMCP_BROWSE` | `drivers.browse.command` |
| `AGENT_WEBMCP_LIGHTPANDA` | `drivers.lightpanda.command` |
| `AGENT_WEBMCP_EXTRA_ARGS` | `drivers.agent-browser.extraArgs` (comma-separated) |
| `AGENT_WEBMCP_SKILLS_DIR` | local skills directory (overrides default) |
| `AGENT_WEBMCP_INDEX_URL` | index pointer URL override |
| `AGENT_WEBMCP_INDEX` | pinned index file (disables refresh) |
