# agent-webmcp

Search websites' WebMCP tools by what you want to do, then call them with the
browser driver you already use.

```console
$ agent-webmcp find "search flights"
googlechromelabs.github.io/searchFlights  [read]
    Searches for flights with the given parameters.
    open https://googlechromelabs.github.io/webmcp-tools/demos/react-flightsearch/
```

Open that page with agent-browser, Playwright MCP or OpenCode Browse, and
`searchFlights` is in the driver's WebMCP tools.

Status: pre-launch. The npm package is not published yet; until it is, build
from source (`cargo build --release`) and run `agent-webmcp setup`, which
fetches the index, installs the skill, and wires the MCP server into OpenCode.
The live index is served from KV by the `agent-webmcp-index` Worker;
`index/index.json` is a snapshot of it. Listings
are automatic: the `agent-webmcp-intake` Worker scans submissions on an
instabox box and publishes clean results. The previous version is on the
`archive/v1-main` branch.

## Privacy

No analytics. The CLI fetches only the public index pointer (hourly, or on
demand with `update`); the index server stores nothing about readers. Site
submissions keep a salted hash of the submitter's IP for rate limiting, plus
the submitted URL — see the sites page privacy policy.

## Commands

| Command | What it does |
|---|---|
| `find <task>` | Search the index. `--json` for machine output. |
| `update` | Check for a newer index now (it also checks hourly). |
| `status` | Show the index in use and the addresses it was fetched from. |
| `setup` | First run: fetch the index, install the skill, wire up OpenCode. |
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
page shows the verdict and anything to fix. **Remove a site** puts a host on the
takedown list, which stops all checks and drops the listing. No ownership proof
is needed: the bot lists only what the site registers with WebMCP on.

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
