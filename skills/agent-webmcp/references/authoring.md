# Authoring a site toolkit

`save` sandboxes the file, captures `registerTool` calls, and never runs
`execute()`. One file can register several tools.

Contract (enforced at save time):

- `document.modelContext.registerTool({...})` runs synchronously at top level.
- Each tool needs `name` (string) and `execute` (function). Missing
  either fails the save.
- `inputSchema` is optional (defaults to open object). `description` and
  `annotations` are optional but indexed, so write them.
- Max file size 200KB.

Skeleton:

```js
document.modelContext.registerTool({
  name: "tool_name",
  description: "What it does and what it returns.",
  inputSchema: {
    type: "object",
    required: ["query"],
    properties: { query: { type: "string" } },
  },
  annotations: { readOnlyHint: true },
  execute: async ({ query }) => {
    // Plain DOM only. All waiting lives inside execute().
    const el = document.querySelector("textarea, input[type=text]");
    if (!el) return { ok: false, error: "input not found" };
    el.focus();
    const setter = Object.getPrototypeOf(el).constructor === HTMLTextAreaElement
      ? Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set
      : Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    setter.call(el, query);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    return { ok: true, query };
  },
});
```

Then:

```bash
agent-webmcp save <domain> <file.js> [--entry-url <url>]
```

Done when: save prints `saved <domain> (<tools>) -> <bundle>` and
`get <domain>` lists the new tool. Save stubs and paywalls as warnings
returned by execute, never as tools.
