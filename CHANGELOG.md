# Changelog

## 0.2.0 — 2026-11-18

The CLI is now a single Rust binary shipped through npm as
`@orkestrate/agent-webmcp` (per-platform packages, no runtime needed).

- New `setup` command: fetches the index, installs the skill into
  `~/.agents/skills`, `~/.claude/skills` and `~/.config/opencode/skills`,
  adds the MCP server to an existing OpenCode config (or prints the snippet),
  prints one-line `claude mcp add` / `codex mcp add` installs, and prints
  per-driver WebMCP notes.
- `find` checks a site or URL first (the main use); task search covers the
  ~100 indexed sites.
- Counts fixed: sites counted by host, tools by name, everywhere.
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
