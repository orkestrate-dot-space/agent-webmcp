# Changelog

## Unreleased

- Polyfill shorthands: `window.webmcp.<tool>(args)` bound at register time,
  unbound on unregister/abort. `get` emits `how_to_run` blocks (Browse
  evaluate-inline, agent-browser `--init-script`, Playwright MCP).
- MCP `save` runs the full pipeline (validate → compile → merge, 64KB cap).
- Registry seed: `upstash.com` (3 native), `allbirds.com` (12 native),
  `docs.stripe.com` (1 custom `ask_ai`, live-proven).
- Verified native contract on Chrome 157: async `registerTool`,
  promise `getTools`, handle-based `executeTool`, string outputs parsed back.

## 0.1.0

Complete rewrite from zero. `agent-webmcp` is now a capability layer, not a
browser driver: flagless `document.modelContext` polyfill + `window.webmcp`
facade (`init.js`), BM25 per-site index (`search`), targeted bundles
(`get`), local authoring (`save`), managed sync (`sync`), stdio MCP
(`skill/search/get/save/init`). No CDP, no tabs, no directory to browse.
