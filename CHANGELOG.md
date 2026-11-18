# Changelog

## Unreleased — 0.2.0

The CLI is now a single Rust binary shipped through npm as
`@orkestrate/agent-webmcp` (per-platform packages, no runtime needed).

- New `setup` command: fetches the index, installs the skill, adds the MCP
  server to your OpenCode config, and reports the Chrome flags WebMCP needs.
- `update` and `status` print one line when a newer CLI is released.
- The index pointer carries `latest_cli`; the intake Worker preserves it
  across rebuilds.

### Migrating from v1 (0.1.0)

| v1 | 0.2.0 |
|---|---|
| `search` | `find` |
| `get <site>` | `find` (results include the page and schema) |
| `sync` | `update` |
| `mcp serve` | `mcp` |
| `save`, `init` | removed with the registry; returns in 0.3.0 |

The on-disk index moved to `~/.local/share/agent-webmcp/index.json`
(`$XDG_DATA_HOME` or `$AGENT_WEBMCP_HOME` when set); the old JS cache is not
read. `AGENT_WEBMCP_INDEX_URL` still overrides the pointer address.
