# Authoring reference (agent-webmcp v0.4.0)

Author a custom tool only when `find` plus live `inspect` prove no
native WebMCP tool covers the task. Native tools always win: they are
versioned, permission-checked, and maintained by the site owner.

## Where custom tools live

- Default dir: `~/.config/agent-webmcp/tools/<host>/` (two files per host).
- Override with `AGENT_WEBMCP_TOOLS_DIR`. Print the active dir with:

```sh
agent-webmcp tools path
```

- One directory per exact host. Non-default ports use `<host>__<port>`:

```sh
~/.config/agent-webmcp/tools/example.com/
~/.config/agent-webmcp/tools/localhost__3000/
```

- Exact host only: no wildcards (`*.example.com` never matches).
- Never create tools inside a cloned repo's `.agent-webmcp/` directory.

## The 2-file contract

Each host dir holds exactly `manifest.json` + `tools.js`.

`manifest.json` (`schemaVersion: 1`):

```json
{
  "schemaVersion": 1,
  "host": "example.com",
  "page": "https://example.com/",
  "tools": [
    {
      "name": "check_order",
      "description": "Look up an order status by id.",
      "page": "https://example.com/orders",
      "inputSchema": {
        "type": "object",
        "properties": { "orderId": { "type": "string" } },
        "required": ["orderId"]
      },
      "readOnly": true,
      "headed": false,
      "auth": "session",
      "allowBrowse": false
    }
  ]
}
```

- `auth` is `"none"` (public page) or `"session"` (logged-in state).
  Session tools never run on `lightpanda` (fresh sessionless browser with
  no redirect-hop guard): they need `--driver agent-browser` or `--driver browse`.
- `allowBrowse` stays `false` unless the user explicitly asks to run
  the tool in their personal Chrome via `browse`.

`tools.js` (registers with `__webmcpCustom`):

```js
__webmcpCustom({
  check_order: {
    async available() {
      if (!location.hostname.endsWith("example.com")) {
        return { ok: false, reason: "route_mismatch", remediation: "Open https://example.com/orders first." };
      }
      return { ok: true };
    },
    async execute(input) {
      const res = await fetch(`/api/orders/${encodeURIComponent(input.orderId)}`, {
        credentials: "same-origin"
      });
      if (res.status === 401) {
        return { error: "login_required", remediation: "Log in, then retry." };
      }
      const data = await res.json();
      return { orderId: input.orderId, status: data.status, eta: data.eta };
    }
  }
});
```

## Best practices

1. Prefer same-origin `fetch()` JSON endpoints discovered from network
   traffic over fragile DOM scraping. Inspect DevTools XHR/fetch calls
   and call the same endpoint with `credentials: "same-origin"`.
2. Always write a real `available()` probe for state/auth-dependent
   tools. Return `{ ok: false, reason: "<login_required|cart_empty|route_mismatch|needs_headed>", remediation: "..." }`
   so `inspect` reports `available: false` with a fix instead of failing at call time.
3. Never hardcode cookies, API keys, passwords, or tokens into
   `tools.js`. Rely on the browser session (`--session` or `browse`);
   the page's own cookies carry auth.
4. Set `headed: true` only if the workflow needs a visible UI or chat
   widget. Leave `allowBrowse: false` unless the user explicitly asks
   to run in their personal Chrome.
5. Keep outputs compact, structured JSON. Return fields the caller
   asked for; never dump raw HTML.

## Verify

```sh
agent-webmcp tools validate example.com
agent-webmcp inspect https://example.com/
agent-webmcp call example.com/check_order '{"orderId":"123"}' --confirm
```

1. `tools validate` checks schema, host match, and JS syntax.
2. `inspect` must list the tool as `[custom]` with `available: true`
   (or a `reason` + `remediation` when state is missing).
3. `call` runs it. Custom tools default to `ask`, so the first call
   needs `--confirm`, and approvals pin to `sha256`.

## Graduate to native WebMCP

When the site owner adopts the tool, drop the same logic into the page:

```js
document.modelContext.registerTool({
  name: "check_order",
  description: "Look up an order status by id.",
  inputSchema: { type: "object", properties: { orderId: { type: "string" } } },
  execute: async (input) => ({ status: (await (await fetch(`/api/orders/${input.orderId}`)).json()).status })
});
```

Delete the custom host dir once `find`/`inspect` show the native tool.
