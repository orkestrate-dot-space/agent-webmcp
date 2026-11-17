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

Status: pre-launch. `index/index.json` holds the launch index (52 sites
checked, 38 verified with 227 tools); submissions arrive through the
`add-site` issue form and are proposed by a bot, merged by a maintainer. The
previous version is on the `archive/v1-main` branch.

## Commands

| Command | What it does |
|---|---|
| `find <task>` | Search the index. `--json` for machine output. |
| `update` | Check for a newer index now (it also checks hourly). |
| `status` | Show the index in use and the addresses it was fetched from. |
| `mcp` | Run an MCP server with one tool, `find`. |

## The index

`index/index.json` lists, for each checked site, the WebMCP tools its pages
register: name, description, input schema, annotations, the page the tool was
found on, and the date. Sites that showed no tools, were blocked by a bot
check or failed to load are recorded with that status and are not searchable.

Tools that only appear after you interact with a page (for example, after a
search) are not indexed yet.

To list a site, open the `add-site` issue form. A bot checks the site in a
fresh browser and opens a listing PR when it verifies; a maintainer merges it.
Takedown works the same way through the takedown form. No ownership proof is
needed to be listed: the bot lists only what the site registers with WebMCP on.

`find` needs the object of your request, not the verb: "order a pizza" finds
`manage_pizza`. A word that names an indexed site narrows the search to that
site, so "search stripe docs" never answers with another company's docs. When
nothing matches, it says so instead of returning a loose word match.

The index is published as a small pointer plus an immutable, hash-addressed
file. The CLI checks the pointer hourly, downloads only when the hash changed,
verifies it, and keeps the last good copy if anything fails.

## License

MIT
