<p align="center">
  <img src="https://raw.githubusercontent.com/orkestrate-dot-space/agent-webmcp/main/.github/assets/banner.png" alt="agent-webmcp: WebMCP for AI agents" width="100%">
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/@orkestrate/agent-webmcp"><img src="https://img.shields.io/npm/v/@orkestrate/agent-webmcp?color=000&label=npm" alt="npm version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-000" alt="MIT license"></a>
  <a href="https://web.orkestrate.space/webmcp/sites"><img src="https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fagent-webmcp-index.agent-webmcp.workers.dev%2Findex.json&query=%24.sites&suffix=%20sites&label=directory&color=000" alt="Sites in the directory"></a>
</p>

# agent-webmcp

Your agent is clicking through pages that already have typed tools.
agent-webmcp tells it which ones before it opens them.

Sites can now register [WebMCP](https://github.com/webmachinelearning/webmcp)
tools: named functions with a description and an input schema, callable from
the page. An agent that calls `searchFlights({origin, destination, ...})`
skips the screenshots, the guessed selectors and the retries. The catch is
that nothing tells the agent which sites have tools, so it drives the page
by hand anyway.

agent-webmcp is that missing lookup. One call, before the agent opens a
page:

```console
$ agent-webmcp find "search flights"
googlechromelabs.github.io/searchFlights  [unspecified]
    Searches for flights with the given parameters.
    open https://googlechromelabs.github.io/webmcp-tools/demos/react-flightsearch/
```

| Without agent-webmcp | With agent-webmcp |
|---|---|
| Open the page, snapshot, find the form, type into each field, click search, snapshot again to check | `find` names the page and tool, open it, call `searchFlights` once with typed arguments |
| Breaks when the layout changes | Calls the interface the site publishes for agents |
| Never learns the site had a tool | Knows before the page loads |

It works with three real run drivers (`agent-browser`, `browse` for your
logged-in Chrome, and experimental `lightpanda`), plus `--plan` (`none`)
for driver-agnostic steps. When nothing matches, `find` says so, and the
agent uses its browser as usual.

## Quickstart

Hand your agent one prompt:

```text
Set up agent-webmcp for yourself by reading the docs at https://web.orkestrate.space/webmcp/agent.md.
```

Or install manually:

| | |
|---|---|
| curl (Linux, macOS) | `curl -fsSL https://web.orkestrate.space/webmcp/install \| bash` |
| npm (any OS) | `npm i -g @orkestrate/agent-webmcp` |

Then run first-run setup:

```console
$ agent-webmcp setup
```

`setup` downloads the index, installs or updates the `agent-webmcp` skill (`~/.agents/skills`, plus `~/.claude/skills` and `~/.config/opencode/skills` when those client directories exist), and reports detected browser drivers. You can also install the skill via `npx skills add orkestrate-dot-space/agent-webmcp`.

The curl script installs one checksum-verified binary to `~/.local/bin` and never edits your shell profile; it tells you if that folder is not on your PATH. Both installs upgrade with `agent-webmcp upgrade`.

**MCP server (optional):** the CLI and skill work out of the box with no client restart. If you prefer MCP tools (`find`, `inspect`, `call`), wire `agent-webmcp mcp` into your client:

```console
$ claude mcp add agent-webmcp -s user -- agent-webmcp mcp
$ codex mcp add agent-webmcp -- agent-webmcp mcp
```

For OpenCode, Cursor, or other MCP clients, configure a local stdio server running `["agent-webmcp", "mcp"]`. See [docs/drivers.md](docs/drivers.md) for browser driver details.

## Check a site, a URL or a task

```console
$ agent-webmcp find cal.com
cal.com/ask_cal_com  [unspecified]
    Ask a question about Cal.com (plans, features, integrations, self-hosting, the API) and get an answer grounded in the documentation.
    open https://cal.com/

$ agent-webmcp find stripe.com
No indexed site has a tool for "stripe.com". Use your browser driver as usual.
```

Site lookup is the main use: your agent already knows where it is going.
Task search covers the sites in the index. Add `--json` for each tool's
input schema.

## The directory

Every indexed site and its tools are listed at
[web.orkestrate.space/webmcp/sites](https://web.orkestrate.space/webmcp/sites).
A bot rechecks every site weekly in a fresh Chromium, so listings reflect
what pages register today.

**Own a site with WebMCP tools?** Paste its URL into **Add your site**. The
bot checks it and lists it within a couple of minutes, with no account or
ownership proof needed. [CONTRIBUTING.md](CONTRIBUTING.md) has the rules.
Owners can ask for removal at support@orkestrate.space.

## Commands

| Command | What it does |
|---|---|
| `find <site, URL or task>` | Check a site first, or search by task. `--json` for machine output. |
| `inspect <url>` | Open a live page and list the WebMCP tools and schemas it registers. |
| `call <target> [input]` | Validate input, check permissions, and call a WebMCP tool (or `--plan` for driver steps). |
| `config [show\|path\|validate]` | Inspect or validate JSONC user/project config (`--json` supported). |
| `setup` | First run: fetch the index, install or update the skill for detected clients, and report drivers. |
| `mcp` | Run an MCP server exposing `find`, `inspect`, and `call` (with elicitation for confirmations). |
| `update` | Check for a newer index now (it also checks hourly). |
| `upgrade [version]` | Upgrade agent-webmcp itself the way you installed it (latest by default). |
| `status` | Show the index in use, detected drivers, and active session locks. |

How matching, listing and index updates work: [docs/index.md](docs/index.md).
Driver setup: [docs/drivers.md](docs/drivers.md). Configuration & permissions:
[docs/config.md](docs/config.md).

## Privacy

No analytics. The CLI fetches only the public index (hourly, or on demand
with `update`), and the index server stores nothing about who reads it. Site
submissions keep the submitted URL and a salted hash of the submitter's IP
for rate limiting.

## Contributing

Build from source with `cargo build --release`; the result is the same
binary npm ships. Bugs go in [issues](https://github.com/orkestrate-dot-space/agent-webmcp/issues),
security reports to [SECURITY.md](SECURITY.md). v1 is preserved on the
`archive/v1-main` branch.

## License

MIT
