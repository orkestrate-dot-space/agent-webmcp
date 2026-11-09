# Driver playbooks

Follow the `how_to_run` block from `get`. These are the full versions.
Load only the section for the driver in use.

## OpenCode Browse (execute realm)

Script-tag and init-script injection are dead in this realm — mount the
polyfill as an evaluated expression, in the same block as the call.

```js
await page.goto(URL, { waitUntil: 'domcontentloaded' });
await page.evaluate(INIT_SOURCE); // init.js bytes of the init_script path (`agent-webmcp init --stdout`)
const tools = await page.evaluate(() => window.webmcp.list());
return await page.evaluate((args) => window.webmcp.TOOL(args), { /* schema args */ });
```

`window.webmcp.call(name, args)` works where a shorthand is awkward.
`handoff()` for logins/2FA/confirmation, then re-call.

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
2. `browser_evaluate` the init script bytes, then
   `window.webmcp.TOOL(args)` in the same evaluation.

Done when: the evaluation returns the tool result. Proven live via
`browser_navigate` + `browser_evaluate` against the MCP server
(bundle mounts, `list()` returns schemas, `call` executes).
Caveat: interactive custom tools (e.g. docs assistants) may not stream
answers to headless automation browsers — same script, headed browser,
real answers. Mount, do a cheap read-only call first to confirm.
