<p align="center">
  <img src="https://raw.githubusercontent.com/orkestrate-dot-space/agent-webmcp/main/.github/assets/banner.png" alt="agent-webmcp: tools for every site your agent visits" width="100%">
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/@orkestrate/agent-webmcp"><img src="https://img.shields.io/npm/v/@orkestrate/agent-webmcp?color=000&label=npm" alt="npm version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-000" alt="MIT license"></a>
  <a href="https://web.orkestrate.space/webmcp/sites"><img src="https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fagent-webmcp-index.agent-webmcp.workers.dev%2Findex.json&query=%24.sites&suffix=%20sites&label=directory&color=000" alt="Sites in the directory"></a>
</p>

# agent-webmcp

Check whether a site offers WebMCP tools before you open it, then use them
with the browser driver you already use.

```console
$ agent-webmcp find cal.com
cal.com/ask_cal_com  [unspecified]
    Ask a question about Cal.com (plans, features, integrations, self-hosting, the API) and get an answer grounded in the documentation.
    open https://cal.com/
```

`find` also takes a full URL, or a task when you don't know the site yet:

```console
$ agent-webmcp find "search flights"
googlechromelabs.github.io/searchFlights  [unspecified]
    Searches for flights with the given parameters.
    open https://googlechromelabs.github.io/webmcp-tools/demos/react-flightsearch/
```

Open that page with agent-browser, Playwright MCP, Chrome DevTools MCP or
OpenCode Browse, and `searchFlights` is in the driver's WebMCP tools. See
[docs/drivers.md](docs/drivers.md) for driver setup. Browse every indexed site
at [web.orkestrate.space/webmcp/sites](https://web.orkestrate.space/webmcp/sites).

Install from npm, then run first-run setup:

```console
$ npm i -g @orkestrate/agent-webmcp
$ agent-webmcp setup
```

`setup` fetches the index, installs the skill into `~/.agents/skills`,
`~/.claude/skills` and `~/.config/opencode/skills`, adds the MCP server to
an existing OpenCode config (or prints the snippet to add yourself), prints
one-line `claude mcp add` / `codex mcp add` installs, and prints per-driver
WebMCP notes.
The live index is served from KV by the `agent-webmcp-index` Worker;
`index/index.json` is a snapshot of it. Listings
are automatic: the `agent-webmcp-intake` Worker scans submissions on an
instabox box and publishes clean results. The previous version is on the
`archive/v1-main` branch.

Contributors: build from source with `cargo build --release`; the release
binary is identical to what npm ships.

## Privacy

No analytics. The CLI fetches only the public index pointer (hourly, or on
demand with `update`); the index server stores nothing about readers. Site
submissions keep a salted hash of the submitter's IP for rate limiting, plus
the submitted URL.

## Commands

| Command | What it does |
|---|---|
| `find <site, URL, or task>` | Check a site first, or search by task. `--json` for machine output. |
| `update` | Check for a newer index now (it also checks hourly). |
| `status` | Show the index in use and the addresses it was fetched from. |
| `setup` | First run: fetch the index, install the skill, wire up OpenCode, print Claude/Codex installs and driver notes. |
| `mcp` | Run an MCP server with one tool, `find`. |

## The index

`index/index.json` lists, for each checked site, the WebMCP tools its pages
register: name, description, input schema, annotations, the page the tool was
found on, and the date. Sites that showed no tools, were blocked by a bot
check or failed to load are recorded with that status and are not searchable.

Tools that only appear after you interact with a page (for example, after a
search) are not indexed yet.

To list a site, use **Add your site** on the sites page. A bot checks it in a
fresh Chromium and lists it automatically when the result is clean; the status
page shows the verdict and anything to fix. No ownership proof is needed to
list: the bot lists only what the site registers with WebMCP on. To have a site
removed, its owner emails support@orkestrate.space.

`find` needs the object of your request, not the verb: "order a pizza" finds
`manage_pizza`. A word that names an indexed site narrows the search to that
site, so "search stripe docs" never answers with another company's docs. When
nothing matches, it says so instead of returning a loose word match.

The index is published as a small pointer plus an immutable, hash-addressed
file. The CLI checks the pointer hourly, downloads only when the hash changed,
verifies it, and keeps the last good copy if anything fails. The weekly rebuild
rechecks every seed; a site that misses one check keeps its tools and drops on
the second, and a rebuild that finds far fewer sites refuses to publish.

## License

MIT
