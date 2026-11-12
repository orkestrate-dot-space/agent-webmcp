# Driver playbooks

Follow the `how_to_run` block from `get`. These are the full versions.
Load only the section for the driver in use.

Two rules hold on every driver:

- **Always await inside the evaluation.** `window.webmcp.list()` and
  `window.webmcp.call()` are async. Capturing `list()` without await
  serializes to `{}` and looks like a failed mount — it isn't.
- **Prefer `call(name, args)` over the shorthand.** Pre-existing native
  tools have no `window.webmcp.<tool>` shorthand (shorthands bind only
  for tools registered through the header). `call` works for all tools.

## OpenCode Browse (execute realm)

Script-tag and init-script injection are dead in this realm — mount the
polyfill as an evaluated expression, in the same session as the call.

**Happy path (one terminal + one execute):**

```bash
# terminal: stage the bundle in the realm's PRIVATE dir.
# files.dir comes from one execute: `return { dir: files.dir }`.
# Shared /tmp paths are rejected — use that dir, not /tmp directly.
cp "<INIT_SCRIPT_FROM_GET>" "<files.dir>/<DOMAIN>.init.js"
```

```js
// single execute block (timeout_ms comes from the get tool entry;
// askers settle on the page's schedule — 180s, never the 60s default):
const src = await files.read(files.path("<DOMAIN>.init.js"), "utf8"); // await: read is async
await page.goto("<ENTRY_URL_FROM_GET>", { waitUntil: "domcontentloaded" });
// settle: mounting before hydration strands the toolkit on a bare registry.
try { await page.waitForFunction(() => typeof document.modelContext !== "undefined" && typeof document.modelContext.getTools === "function", null, { timeout: 15000 }); } catch (e) { /* custom-only surface: mount anyway */ }
await page.waitForTimeout(2000); // natives register during hydration
await page.evaluate(src); // mount: evaluate-inline; re-mount after every nav
const tools = await page.evaluate(() => window.webmcp.list()); // await: list() is async
return await page.evaluate(
  (args) => window.webmcp.call("<TOOL_NAME>", args),
  { /* schema args, from the get inputSchema */ }
);
```

**Debug path (when the call fails):** split the block — goto + native
baseline first (`page.evaluate` checking `document.modelContext` with no
bundle loaded), then mount, then `list()` to verify the expected names,
then `call`. A working toolkit going silent right after a nav means
re-evaluate the bundle, not a broken bundle.

`handoff()` for logins/2FA/confirmation, then re-call.

**Headed check:** Mintlify hosts are bot-gated. Confirm the session runs
in a headed, non-bot browser (`status` shows the connected browsers;
a store/extension-driven Chrome is headed). Signs of a gate: a bot
challenge or block page in the execute `aftermath`, or an asker stuck
at `no-panel`. Fall back to native `search_docs` headless, or re-drive
headed — never retry-loop a challenge.

Done when: the call returns the tool result.

## agent-browser

`--init-script` registers before the 1-second discovery window.

```bash
agent-browser --init-script "<INIT_SCRIPT_FROM_GET>" open "<ENTRY_URL_FROM_GET>"
agent-browser webmcp invoke <TOOL_NAME> --params '{"key": "value"}'
```

Tab already open: `agent-browser eval "$(cat <INIT_SCRIPT>)"`, then
`agent-browser webmcp list --json`, then invoke as above.

Done when: invoke prints the tool result.

## Playwright MCP

1. `browser_navigate` to the entry URL.
2. `browser_evaluate` the init script bytes, then in the same evaluation:
   `await window.webmcp.call("<TOOL_NAME>", { /* schema args */ })`.

Done when: the evaluation returns the tool result. Proven live via
`browser_navigate` + `browser_evaluate` against the MCP server
(bundle mounts, `list()` returns schemas, `call` executes).
Caveat: interactive custom tools (e.g. docs assistants) may not stream
answers to headless automation browsers — same script, headed browser,
real answers. Mount, do a cheap read-only call first to confirm.
