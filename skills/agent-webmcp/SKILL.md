---
name: agent-webmcp
description: Turn pages into callable toolkits inside your existing browser driver. Mount init.js once, check webmcp before snapshot.
---

# agent-webmcp — toolkits alongside drivers

agent-webmcp never opens tabs. Your driver owns the browser. This skill
carries `init.js` (polyfill + `window.webmcp`) into whatever driver you use.

## 1. Search, then get

```bash
agent-webmcp search "<intent or domain>"   # BM25 over native + custom tools
agent-webmcp get <domain>                  # schemas + entry URL + init script
```

- OpenCode Browse MCP (your paired browser, code-mode): script-tag and
  init-script injection are neutered in the execute realm, so the polyfill
  rides in as an evaluated expression. Paste the whole block:
  ```js
  await page.goto(URL, { waitUntil: 'domcontentloaded' });
  await page.evaluate(INIT_SOURCE);   // init.js bytes from `agent-webmcp init --stdout`
  const tools = await page.evaluate(() => window.webmcp.list());
  ```
  Then `window.webmcp.call(name, args)`; `handoff()` for logins/2FA.
- agent-browser MCP: `open` the URL, then its native `webmcp list` /
  `webmcp invoke <tool>` verbs. The init script populated
  `document.modelContext`, so its engine sees the tools automatically.

## 2. goto -> webmcp.list() before snapshot()

```js
await page.goto(URL);
const tools = await page.evaluate(() => window.webmcp.list());
```

Match → 1-turn `window.webmcp.call(name, args)`. Else snapshot, solve
with driver primitives, then save it (step 3).

Reads run in-page. Writes run as driver plans: the toolkit returns
targets + values + a verification read; the agent executes with trusted
driver primitives and projects the result.

## 3. Use -> save -> free next visit

```bash
agent-webmcp save <domain> <file.js>  # persist site toolkit (was `learn`)
agent-webmcp sync                     # pull managed index (only network verb)
```

Save stubs/paywalls as warnings, not tools.
