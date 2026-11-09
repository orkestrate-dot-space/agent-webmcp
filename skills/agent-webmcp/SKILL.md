---
name: agent-webmcp
description: >
  Turn the web into a callable toolkit for your browser driver. Use when a task needs an external service, docs, store, or web app: search the index, load site tools via get, execute in one call, save the miss.
---

# agent-webmcp — toolkit-first web access

`agent-webmcp` is a capability index plus a `document.modelContext` /
`window.webmcp` polyfill. It never touches the browser. Your driver owns
tabs; this skill decides which tool to call before any snapshot.

Rule: toolkit-first. Run search, then get. Snapshot only on a toolkit miss.

CLI: `agent-webmcp`. Fallback when the binary is not on PATH: prefix every
command with `npx -y @orkestrate/agent-webmcp`.

## 1. Search the index

```bash
agent-webmcp search "<intent or domain>" [--domain D] [--source native|custom] [--limit N]
```

Done when: search returns a match for the task, or returns empty and the
task still needs the site. Empty is a decision, not a failure — continue
to get.

## 2. Fetch the site toolkit

```bash
agent-webmcp get <domain>
```

Done when: get returns `entry_url`, `init_script`, a `tools` list, and
`how_to_run`. The `inputSchema` values are the only valid arguments.

Unindexed domains still return `init_script` plus a `hint`. The polyfill
exposes live native tools on the page even with zero indexed tools.

## 3. Execute through your driver

Follow the `how_to_run` block that get returned for the active driver.
Full playbooks live in `references/drivers.md` — load only the active
driver's section.

Done when: the tool returns its result, or the driver reports
auth/2FA/confirmation and a human completes it first. The toolkit reads
the page; the driver performs anything consequential.

## 4. Persist the miss

When no tool covered the task and driver primitives solved it, save the
flow so the next visit is one call. Contract and skeleton live in
`references/authoring.md`.

```bash
agent-webmcp save <domain> <file.js> [--entry-url <url>]
```

Done when: save prints `saved <domain> (<tools>) -> <bundle>` and a
repeat `get <domain>` lists the new tool.

Only `sync` touches the network. Everything else reads local files.
