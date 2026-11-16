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

Status: rebuilding for the first release. See [PLAN.md](PLAN.md). The
previous version is on the `archive/v1-main` branch.

## Commands

| Command | What it does |
|---|---|
| `find <task>` | Search the index. `--json` for machine output. |
| `update` | Download the latest index now (it also refreshes itself weekly). |
| `status` | Show the index in use. |
| `scan <hosts>` or `scan --list <file>` | Load sites in Chrome and record the tools they register. |
| `mcp` | Run an MCP server with one tool, `find`. |

## How the index is made

`agent-webmcp scan` loads each site's homepage and a few likely pages (docs,
search, pricing, products) in headless Chrome with WebMCP on, and records
every tool the pages register: name, description, input schema, annotations,
the page it was found on, and when. It only reads. It never calls a tool,
submits a form, or solves a bot check; blocked sites are recorded as blocked.

Tools that only appear after you interact with a page (for example, after a
search) are not indexed yet.

## License

MIT
