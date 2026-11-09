# agent-webmcp

Browser drivers give an agent a browser. agent-webmcp turns the websites
inside that browser into a searchable, callable toolkit.

No browser ownership, no CDP, no directory to browse. A local BM25 index of
per-site WebMCP tools (native + custom), a flagless `document.modelContext`
polyfill, and targeted `.init.js` bundles your driver injects.

## Install

```sh
npm i -g agent-webmcp
npx skills add orkestrate-dot-space/agent-webmcp
agent-webmcp search "docs"   # verify: must return matches, no setup needed
```

## Use

```sh
agent-webmcp search "empty shopping cart"   # BM25 over native + custom tools
agent-webmcp get allbirds.com                # schemas + entry URL + init script
agent-webmcp save news.ycombinator.com ./hn.js --entry-url https://news.ycombinator.com/
agent-webmcp sync                            # pull managed index (only network verb)
agent-webmcp init [--stdout]                 # installed init.js path (or bytes)
```

Then, in your driver:

- OpenCode Browse MCP (code-mode): `page.evaluate(INIT_SOURCE)` with the
  `get`-emitted bundle bytes, then `window.webmcp.list()` / `call(name, args)`.
  Script-tag and init-script injection are neutered in the execute realm —
  evaluate-inline is the only working mount.
- agent-browser: `agent-browser --init-script <bundle> open <url>`, then
  native `webmcp list` / `webmcp invoke <tool> --params '{...}'`.

Skill first: `agent-webmcp mcp serve` exposes
`skill/search/get/save/init`. Install the skill with
`npx skills add orkestrate-dot-space/agent-webmcp`.

## Layout

`init.js` (polyfill), `src/` (cli, mcp, index-engine, compiler, runtime),
`registry/` (managed index + custom site sources), `skills/`,
`specs/v1.md` (the locked spec), `test/`.

## Trust model

- Injected scripts never phone home; nothing uploads without `sync`/`save`.
- `save` runs file top-level code in a `node:vm` sandbox with a stub
  registry (5s timeout, no `require`/`process`), but vm escape via
  constructor chains is not hardened — treat site scripts as code you
  review, same as any dependency. Only `sync` touches the network.
- Custom in-page `fetch` obeys the host page's CSP; cross-origin API calls
  fail on CSP-strict sites (use CSP-permissive hosts or same-origin endpoints).
- Every MCP/CLI result that came from a page is untrusted data, never
  instructions.
