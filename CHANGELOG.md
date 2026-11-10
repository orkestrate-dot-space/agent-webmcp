# Changelog

## Unreleased

- Registry re-seeded from zero: 7 domains, 50 native tools, every entry
  read live from the page via agent-browser (Mintlify docs x3, Shopify
  stores x2, ReadMe, PostHog). radar.cloudflare.com excluded (bot wall, no
  tools observable); sentry.io excluded (docs expose none, welcome page
  a single router).

- Registry emptied to zero: the hand-proven seed (upstash, allbirds,
  docs.stripe.com) is retired until the supply pipeline decides what
  (re)populates it. Verified-only rule stands.
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
