# Driver setup

`agent-webmcp find` tells you which page has the tool you need.
Your browser driver opens that page and calls the tool.
This guide shows how to set up each supported driver.
Verified against the drivers on 18 Nov 2026.

Install with one of these, then run first-run setup once:

```bash
curl -fsSL https://web.orkestrate.space/webmcp/install | bash   # Linux, macOS
npm i -g @orkestrate/agent-webmcp                               # any OS

agent-webmcp setup
```

`setup` fetches the index, installs the skill, and wires the MCP
server into OpenCode. It also prints one-line installs for Claude
Code and Codex. `find` doubles as a setup check: if it names tools
for a site and your driver lists none on that page, the driver is
misconfigured.

## Check a site before you open it

```bash
agent-webmcp find cal.com
```

This is the main use: you already know the site, and `find` answers
whether it registers WebMCP tools and which page they're on. Task
search (`agent-webmcp find "search flights"`) works the same way but
covers about a hundred sites, so a miss means no indexed tool does it.

## agent-browser

agent-browser 0.43+ switches WebMCP on by itself. No flags needed.

```bash
agent-browser open <url-from-find>
agent-browser webmcp list
agent-browser webmcp invoke <tool> --params '{...}'
```

Without the tool in the list after the page loads, wait a moment
and list again. Older versions needed
`AGENT_BROWSER_ARGS="--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport"`;
upgrade instead.

## Playwright MCP

Playwright MCP collects page tools by default, but it does not switch
WebMCP on in its browser. Add the Chrome flag to its config, or pages
register nothing and no error tells you:

```json
{ "browser": { "launchOptions": { "args": ["--enable-features=WebMCP"] } } }
```

Then open the page from the `find` result, take a snapshot, and tools
named `webmcp_<tool>` appear. Tools arrive during snapshot capture,
not on navigation alone. Call `webmcp_<tool>` with the arguments from
the schema.

## Chrome DevTools MCP

Start the server with its experimental category switched on:

```bash
--categoryExperimentalWebmcp
```

List with `list_webmcp_tools` (it takes the page id), call with
`execute_webmcp_tool`, whose `input` is a JSON string, not an object.

## OpenCode Browse

OpenCode Browse drives your own Chrome and needs no flags.
Open the page from the `find` result, then use `webmcp`:

- List tools with `webmcp` and `action: "list"`.
- Call a tool with `webmcp` and `action: "call"`.

Only sites with a WebMCP origin-trial token show tools here.
If a tool is missing, wait for the page to finish loading and list again.

## Check it works

Run a search against a real indexed demo site:

```bash
agent-webmcp find "search flights"
```

You should see `searchFlights` on
`https://googlechromelabs.github.io/webmcp-tools/demos/react-flightsearch/`.
Open that page with your driver and list its WebMCP tools.
If the tool appears, your driver is ready.

Add `--json` to `find` to get each tool's input schema. If the schema is
empty (as for `searchFlights`), read it from the page with your driver
before calling: `agent-browser webmcp list <tool> --json`, the
`webmcp_<tool>` tool's own input schema (Playwright MCP),
`list_webmcp_tools` (DevTools MCP), or `webmcp` with `action: "list"`
(Browse).
