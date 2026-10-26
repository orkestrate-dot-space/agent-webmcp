# agent-webmcp

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Beta](https://img.shields.io/badge/channel-0.2.0--beta-orange.svg)](CHANGELOG.md)

**Give your agent verbs for any page.** Write each verb once as a TypeScript file. The engine proves it inside the page and keeps it. Your agent calls it forever over MCP.

Six verbs. One dependency. MIT licensed, runs local.

## Install

```sh
./install.sh   # builds dist/, installs ~/.local/bin/agent-webmcp + skills
```

You need [Bun](https://bunjs.com) (any recent 1.x) and a reachable browser: an agent-browser daemon, or any Chromium with `--remote-debugging-port` plus WebMCP flags. Desktop chat panels need `--window-size=1366,900`. Run `doctor` to verify.

Wire it into your harness as an MCP server:

```json
{ "mcp": { "agent-webmcp": {
  "type": "local",
  "command": ["/home/you/.local/bin/agent-webmcp", "mcp", "serve"]
} } }
```

Skill first, every session: `skill action=show`.

## Use

```js
// Discover, then call. Calls resolve to the envelope.
await search({ query: "read" });
const out = await tools.eve_read({});
const reply = JSON.parse(out.content[0].text);
```

`open` attaches a tab and returns a handle. `list` shows what that handle can call. `execute` runs your code over those verbs. `close` drops the tab. `skill` serves docs and finds agents. `doctor` tells the truth about the browser and the registry.

The engine never launches a browser. It attaches tabs in one you already run and never kills it.

## Author a verb

No register verbs. Write two files with your own tools. The engine validates, injects, proves, and stamps them:

```
~/.local/share/agent-webmcp/registry/<origin-slug>/<agent>/{spec.json, body.ts}
```

`spec.json` declares the input and output shapes. `body.ts` is a typed function that returns `{content: [{type: "text", text}]}`. First call admits the verb: validate, check against the live page catalog, run the fixture, stamp on success. Bad files never inject. The reason names the fix.

One origin holds one or more agents. Each agent has its own skill file, served on `open` and searchable by name, description, and capability.

## Skills

| Layer | Loads | Answers |
|---|---|---|
| `SKILL.md` | always | drive and compose |
| `show name=drivers` | before first recon | how to see and act |
| `show name=author` | before authoring | files, schemas, prove |
| `show name=site-agents` | before chat agents | open, ask, read, close |
| origin and capability skills | auto on `open` | how this place works |

## Local and remote

Two entries, same six verbs. Local runs over stdio with no key. Remote runs over HTTP with a bearer key against shared browsers and stored logins. Same agents, same call shape. Mint locally, run anywhere.

## How it compares

| Project | What it does | Where we differ |
|---|---|---|
| [browser-use](https://github.com/browser-use/browser-use) (119k stars, MIT) | Drives real browsers from agent instructions | We author verbs once instead of driving pixels every run |
| [Pomerado](https://github.com/Pomerado/pomerado) (MIT) | Model mints site integrations, open core plus cloud | Your agent authors directly into files it keeps |
| [UsefulSoftwareCo/executor](https://github.com/UsefulSoftwareCo/executor) (4.3k stars, MIT) | Calls OpenAPI, MCP, GraphQL, and custom JS in a sandbox | We author verbs where no API exists |
| [Steel](https://github.com/steel-dev/steel-browser) (7.9k stars, Apache-2.0) | Self-hostable browser API | They run browsers. We add the verb layer above any browser |
| [Playwright MCP](https://github.com/microsoft/playwright-mcp) (38k stars) and [chrome-devtools-mcp](https://github.com/GoogleChromeLabs/chrome-devtools-mcp) (54k stars) | Expose browser actions as tools | We expose page verbs, native and authored alike |
| [Stagehand](https://github.com/browserbase/stagehand) (26k stars) | Plain-language browser acts over Playwright | Our acts compile to proven files |

Pages increasingly ship their own verbs (13k+ sites expose WebMCP). Consume natives where they exist. Author the gap as files. Run it all the same way.

## Repo

`src/` is the engine. `skills/agent-webmcp/` is the other half of the product. `scripts/gate.ts` is the green gate: typecheck, unit tests, skill shape, schema checks, description budget. `PLAN.md`, `LAUNCH.md`, and `CHANGELOG.md` hold the plan, the launch list, and the beta log.

Beta rules: breaking changes ship in any beta and are listed at the top of each changelog entry. No migrations until 1.0.

## Contributing

Read `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, and `SECURITY.md` before your first PR. Never paste secrets into issues. License is MIT.
