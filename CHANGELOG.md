# Changelog

## 0.3.1 — 2026-11-20

- Address CodeRabbit review findings from `v0.3.0`: reject positional JSON
  arrays in project config allowlist checks, return `is_error: false` on
  successful MCP `inspect` calls, sanitize live-page strings in
  `inspect::print_human`, map Lightpanda `isError` payloads to `tool_error`,
  add unique sequence ids to `--fresh` sessions, preserve queries/localhost
  on URL normalization, and recognize ST (`ESC \`) OSC terminators.

## 0.3.0 — 2026-11-20

- New `inspect` command and MCP `inspect` tool: opens any live URL
  (`https://...`, `localhost`, or bare host) through the active run
  driver and lists its registered WebMCP tools, descriptions, live
  `inputSchema`s, and read/write hints without calling them.
- New `call` command and MCP `call` tool: resolves `site/tool` or
  `https://page#tool`, validates input against the tool's JSON Schema,
  enforces `allow`/`ask`/`deny` permission globs (with `--confirm` and
  MCP form elicitation), and runs through `agent-browser` (>= 0.43.0),
  `browse` (your connected Chrome), or experimental `lightpanda`, with
  `--plan` / `none` for step-by-step output.
- New `config [show|path|validate]` command with JSONC user & project
  configs, `${env:...}` / `${file:...}` substitution, and a restricted
  project-config allowlist.
- `find` results now include ready-to-paste `call` targets and inferred
  read/write hints (`kind_inferred`).
- `setup` now stays non-invasive (fetches the index, installs/updates the
  skill only for detected client directories, reports driver readiness,
  and never edits MCP configs or auto-installs drivers); `status` reports
  detected drivers and active session locks.

## 0.2.3 — 2026-11-18

- `upgrade` on an install-script copy now refuses releases before 0.2.2,
  which ship no script binaries (the archives started with 0.2.2), and
  points at npm for those versions instead of failing on a 404.
- `find` drops fuzzy matches that earn less than one relevance point per
  query word, so gibberish gets the no-match line instead of weak hits.
- `find` scores camelCase tool names the way the index splits them, so a
  fallback match on the name counts.

## 0.2.2 — 2026-11-18

- curl install for Linux and macOS:
  `curl -fsSL https://github.com/orkestrate-dot-space/agent-webmcp/releases/latest/download/install.sh | sh`.
  One binary to `~/.local/bin`, checked against the release's SHA256SUMS.
  `agent-webmcp upgrade` re-runs it for copies it installed.
- The npm package has no install script any more, so npm 12 no longer
  warns about a blocked `postinstall`. A missing platform binary is still
  reported, by the `agent-webmcp` command itself.
- GitHub releases carry standalone binaries for every platform.

## 0.2.1 — 2026-11-18

- New `upgrade [version]` command: upgrades agent-webmcp itself through npm
  (latest by default; `next` or a version like `0.2.1` also work). Installs
  that did not come from `npm i -g` get the command to run instead. On
  Windows it prints the command, because a running `.exe` cannot be replaced.
- The newer-version note from `update` and `status` now says
  `run: agent-webmcp upgrade`.
- The npm package page shows the README.

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
