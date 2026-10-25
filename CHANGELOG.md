# Changelog (agent-webmcp slim core)

Beta channel: `0.2.0-beta.x`. Breaking changes ship in any beta release —
each one is listed at the top of its entry, with the fix. No migrations
until 1.0: deleted paths fail loud naming the replacement, never silently.

## Unreleased

**Breaking:** `outputSchema` is now required in every `spec.json` (was
optional). Old files without it fail as `bad-shape` naming the fix — add
`{"type":"string", ...}` for text verbs or a closed object schema for
`JSON.stringify`d data. All ten shipped verbs now declare it. `opencode.json`
drops the dead `AGENT_WEBMCP_REGISTRY` env (the root is fixed from `HOME`;
it was always ignored) and the nested duplicate `servers` block.

## 0.2.0-beta.1 (clean room)

This is a rewrite on a new `main`, not an upgrade. The `experimental`
branch preserves the full 0.1.0-beta.1 tree (engine-map, index vertical,
Effect, codegen); nothing migrates automatically. Registry files
(`spec.json` + `body.ts` per tool) carry across untouched — point the old
store at the new binary and re-open re-admits them.

**Breaking vs 0.1.x:** six verbs (`open`, `list`, `execute`, `close`,
`skill`, `doctor`). Removed: `register`, `unregister`, `search`, `find`
(authoring is skill + files now; the index vertical is frozen out of this
branch). Removed: `--registry` flag, project overlays, `origin.txt`
(single fixed dir, origin verified from inside `spec.json`). Removed:
engine-map and dual invoke paths — native WebMCP only; incapable browsers
fail loud naming `--enable-features=WebMCPTesting,DevToolsWebMCPSupport`.
Removed: Effect, codegen/`generated/`, config system remnants. `stamp.json`
replaces in-`spec.json` stamping (engine-owned, beside agent-owned files).

**Kept semantics (re-written, re-proven):** content envelope, budgets,
consequential gate (`ask` default), skip-with-reason, tab locks,
port-qualified origin slugs.
