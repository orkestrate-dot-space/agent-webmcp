# agent-webmcp

Connect-only authoring layer for the agentic web. Your agent teaches any page a verb, proves it, keeps it — as files. Six verbs, one skill, one dependency. MIT-licensed, local-first.

**Status:** `0.2.0-beta.1`. Breaking changes may ship in any beta (listed atop `CHANGELOG.md`); no migrations until 1.0; removed paths fail loud.

## How it works

The engine never launches a browser — it attaches tabs in a reachable one (your driver skill knows the endpoint: per-call `cdp` → daemon discovery → loopback 9222/9229). Page verbs live *inside* the `execute` dialect, never as flat harness tools. Native page verbs and your authored verbs are indistinguishable at call time: the same list, the same call shape.

```
skill → open → list → execute → close
```

```js
// Compose over live page verbs. Calls resolve to the envelope;
// unwrap content[0].text (JSON.parse when returns says object{...}).
const out = await tools.eve_read({});
const reply = JSON.parse(out.content[0].text);
```

## Install

```sh
./install.sh   # builds dist/, installs ~/.local/bin/agent-webmcp + skills
```

Wire it into your harness as an MCP server:

```json
{ "mcp": { "agent-webmcp": {
  "type": "local",
  "command": ["/home/you/.local/bin/agent-webmcp", "mcp", "serve"]
} } }
```

Then: `skill action=show` first, every session. `doctor` tells the truth about browser reachability and registry path.

## Authoring (files, not verbs)

No register/unregister verbs — the agent writes two files with its own tools, the engine validates, injects, proves, and stamps:

```
~/.local/share/agent-webmcp/registry/
  <origin-slug>/{SKILL.md, <capability>/{SKILL.md, <name>/{spec.json, body.ts, stamp.json}}}
```

- `spec.json`: `{name, description, inputSchema, origin, outputSchema}` (+ `fixtureInput?`, `consequential?`, `annotations?`). `outputSchema` is required — the schema of the `text` payload, so codemode never guesses.
- `body.ts`: typed TypeScript `(input: Input) => envelope`; transpiled at load, proved by fixture (or your confirmed call for consequential verbs), stamped on success.
- A verb lives in exactly one capability; duplicates fail loud. Capabilities group an origin's verbs (chat, docs, page tools), each with its own `SKILL.md`, surfaced in `open`'s briefing and searchable via `skill search` / in-dialect `search()`.

First `execute` naming the verb admits it (validate → native-collision check → inject → prove → stamp). Bad files never inject; the reason names the fix.

## Skills (progressive disclosure)

| Layer | Loads | Answers |
|---|---|---|
| `SKILL.md` | always | drive → compose |
| `show name=drivers` | before first recon | how to see/act (driver contract + your driver's own skill) |
| `show name=author` | before authoring | files, schemas-as-types, body template, prove |
| `show name=site-agents` | before chat agents | open → ask → read → close pattern |
| per-origin / per-capability | auto on `open` briefing | how THIS place works |

## Local + remote

Both at once: two MCP entries, same six verbs. Local runs stdio keyless (the OS user is the auth) against your browsers and your filesystem registry. Remote runs streamable HTTP with a bearer key against shared browsers and vault-held logins. Same agents, same call shape — local for minting and daily driving, remote for unattended runs. The engine already abstracts the attach point (`open {url, cdp?}`).

## Neighbors (what each does, where we differ)

| Project | What it is | Difference |
|---|---|---|
| [browser-use](https://github.com/browser-use/browser-use) (119k★, MIT) | OSS agent library driving real browsers via CDP | They drive pixels per task; we author verbs once, call them cheaply forever |
| [Pomerado](https://github.com/Pomerado/pomerado) (MIT) | Model-minted site integrations, open core + cloud | Their mint agent authors behind `mint`; our agent authors directly into files it owns |
| [UsefulSoftwareCo/executor](https://github.com/UsefulSoftwareCo/executor) (4.3k★, MIT) | Integration layer: OpenAPI/MCP/GraphQL/custom JS in a secure env | They broker existing APIs; we author verbs where no API exists |
| [Steel](https://github.com/steel-dev/steel-browser) (7.9k★, Apache-2.0) | Self-hostable browser API (sessions, stealth, proxies) | They're the browser fleet; we're the verb layer above any fleet |
| [Playwright MCP](https://github.com/microsoft/playwright-mcp) (38k★) / [chrome-devtools-mcp](https://github.com/GoogleChromeLabs/chrome-devtools-mcp) (54k★) | Vendor tool servers: your agent calls browser actions | They expose *browser* actions; we expose *page* verbs, native + authored alike |
| [Stagehand](https://github.com/browserbase/stagehand) (26k★) | act/extract/observe over Playwright, Browserbase-hosted | Natural-language acts per run; our acts compile to proven files |
| [Skyvern](https://github.com/Skyvern-AI/skyvern) (23k★) | Agent + cloud for browser workflows | Workflow service; our unit is the versioned agent you keep |

The bet: pages ship verbs (13k+ sites on WebMCP and counting). Consume natives where they exist, author the gap as files, run anywhere. No owned browsers, no model key, no third party holding your logins.

## Repo

- `src/` — engine (server, sessions, transport, admission, catalog, registry, runner)
- `skills/agent-webmcp/` — the other half of the product (core + references)
- `scripts/gate.ts` — green gate: typecheck, unit tests, skill-shape, schema-object, desc-budget
- `PLAN.md`, `LAUNCH.md`, `CHANGELOG.md` — build plan, launch checklist, beta log
- `workers/sites-index` — directory API scaffold (Cloudflare)
