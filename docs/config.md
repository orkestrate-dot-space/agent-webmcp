# Configuration

`agent-webmcp` reads optional JSONC (JSON with `//` / `/* */` comments and trailing commas) configuration files for `call`, driver selection, and tool permissions.

## Files and precedence

Settings merge in this order (later wins):

1. **Built-in defaults**
2. **User config** — `$XDG_CONFIG_HOME/agent-webmcp/agent-webmcp.jsonc` (or `.json`; defaults to `~/.config/agent-webmcp/agent-webmcp.jsonc` on Linux/macOS). If `AGENT_WEBMCP_CONFIG` is set, that file is used instead.
3. **Project config** — `agent-webmcp.jsonc` (or `.json`) in the current working directory or any parent directory.
4. **Environment variables**
5. **CLI flags** (`--driver`, `--session`, `--headed`, `--timeout`, `--settle`)

If both `.jsonc` and `.json` exist in the same directory, `.jsonc` is used and a warning is printed.

Use `agent-webmcp config` to inspect and verify your setup:

```bash
agent-webmcp config show       # print effective config and source per key
agent-webmcp config path       # print config file paths
agent-webmcp config validate   # check config files for errors
```

Add `--json` to any `config` subcommand for machine-readable output.

## Project config allowlist

A project `agent-webmcp.jsonc` may come from an untrusted repository, so it is restricted to four keys:

- `call.timeout`
- `drivers.agent-browser.headed`
- `permissions.ask`
- `permissions.deny`

Setting any other key (such as `drivers.*.command`, `permissions.allow`, `profile`, or `cdp`) in a project config fails validation.

## Schema and options

Point your editor at [schema/config.json](../schema/config.json) with `"$schema": "https://web.orkestrate.space/webmcp/config.json"`.

```jsonc
{
  "$schema": "https://web.orkestrate.space/webmcp/config.json",
  "driver": "auto",
  "drivers": {
    "agent-browser": {
      "command": "agent-browser",
      "session": "agent-webmcp",
      "headed": false,
      "profile": null,
      "cdp": null,
      "extraArgs": []
    },
    "browse": {
      "command": "browse"
    },
    "lightpanda": {
      "command": "lightpanda"
    }
  },
  "call": {
    "timeout": 60000,
    "settleMs": 8000
  },
  "permissions": {
    "allow": [],
    "ask": [],
    "deny": []
  }
}
```

### `driver`

Default `"auto"`. One of:

- **Run drivers** (execute the tool directly, and also support `--plan`): `"auto"`, `"agent-browser"`, `"browse"`, `"lightpanda"`
- **Plan-only fallback**: `"none"`

With `"auto"`, `call` tries `agent-browser` (`>= 0.43.0`), then `browse` (if its Chrome extension is connected), then `lightpanda`, otherwise returns `no_driver` with setup help. Naming a specific driver never silently falls back to another.

### `call`

- `timeout` (integer `>= 1`, default `60000`): maximum milliseconds for a tool call.
- `settleMs` (integer `0..=30000`, default `8000`): how long to wait for late tool registration after page load (`0` skips waiting).

### `permissions`

Glob patterns matched against `<host>/<tool>` (for example `cal.com/*`, `*.example.com/delete_*`):

- `deny`: never called (`deny` wins over `ask` and `allow`).
- `ask`: requires explicit confirmation (`--confirm` on the CLI, `confirm: true` or MCP elicitation in `mcp`).
- `allow`: allowed without asking (unless matched by `deny`).

By default (when no pattern matches), tools annotated as `readOnly`—or inferred as reads from their name—run without confirmation, while write/consequential tools require confirmation.

## Variable substitution

String values in config files support:

- `${env:VAR_NAME}` — replaced with the value of environment variable `VAR_NAME`.
- `${file:relative/or/~/path}` — replaced with the trimmed contents of that file (resolved relative to the config file's directory, or `~/`).

## Environment variables

| Variable | Overrides |
|---|---|
| `AGENT_WEBMCP_CONFIG` | User config file path |
| `AGENT_WEBMCP_DRIVER` | `driver` |
| `AGENT_WEBMCP_SESSION` | `drivers.agent-browser.session` |
| `AGENT_WEBMCP_HEADED` | `drivers.agent-browser.headed` (`1`/`true`/`0`/`false`) |
| `AGENT_WEBMCP_TIMEOUT` | `call.timeout` (ms) |
| `AGENT_WEBMCP_SETTLE` | `call.settleMs` (ms) |
| `AGENT_WEBMCP_AGENT_BROWSER` | `drivers.agent-browser.command` |
| `AGENT_WEBMCP_BROWSE` | `drivers.browse.command` |
| `AGENT_WEBMCP_LIGHTPANDA` | `drivers.lightpanda.command` |
| `AGENT_WEBMCP_EXTRA_ARGS` | `drivers.agent-browser.extraArgs` |
