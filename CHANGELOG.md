# Changelog (agent-webmcp slim core)

Beta channel: `0.2.0-beta.x`. Breaking changes ship in any beta release —
each one is listed at the top of its entry, with the fix. No migrations
until 1.0: deleted paths fail loud naming the replacement, never silently.

## Unreleased

**Breaking:** the `--consequential` policy is gone (`policy.ts` deleted,
`confirm` arg removed from `execute`). Page verbs call free — the gate
was a leftover from authored/stamped tools, and with live-only verbs
there is no wall behind it to guard. `mcp serve` takes no flags;
`doctor` drops the `policy` field. A passed `confirm:true` is inert
(extra args are ignored, never errors). The calling harness owns
approval now — that was always true in practice.

**Fundamentals (non-breaking):** failed runs keep a `partial` ledger —
the last 20 completed calls with 200-char output heads — on timeout,
call-budget, and mid-run throws, in `execute` and `poll` alike.
Timeouts stop discarding work: continue from the ledger, never redo it.
Base-prompt budget gated: skill text + all served descriptions ≤ 8000
chars (~2k tokens; currently ~4.7k). Skill loop now states plan-up-front
(one program over many calls, `Promise.all` for independent awaits).

**Breaking:** the registry is gone. No `registry/` reads or writes anywhere:
`src/registry.ts`, `src/admission.ts`, `src/snippet.ts` (and their tests)
deleted; verbs resolve live-only. `open` absorbs `attach` (`{url}` mints,
`{targetId}` borrows — one verb, `owned` in the response). `skill` loses
`search` and all `name`/`origin`/`capability` scoping: `skill` with no args
returns the core loop, that is the whole skill surface. New verb `poll`
reads back `execute {background:true}` runs (`{runId}` → running|done);
long jobs no longer die on timeouts and never need polling loops. `doctor`
drops the `registry` field. `list` output is name-sorted for cache
stability. Session handles are allowlisted (`s_[0-9a-f]{12}`) — yesterday's
traversal shape now fails as `bad-handle`.
Old registry files on disk are inert (never read); delete them.

**Breaking:** `skill` loses the `list` action — `search` with no query
returns the grouped map instead (same shape). Codemode `search()`
loses the `namespace` filter (sessions are already handle-scoped).
`outputSchema` is now required in every `spec.json` (was
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
