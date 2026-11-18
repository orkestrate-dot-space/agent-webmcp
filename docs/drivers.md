# Driver setup

`agent-webmcp find` tells you which page has the tool you need.
Your browser driver opens that page and calls the tool.
This guide shows how to set up each supported driver.

Run first-run setup once:

```bash
npm i -g @orkestrate/agent-webmcp
agent-webmcp setup
```

`setup` fetches the index, installs the skill, and wires the MCP
server into OpenCode. It also reports which browser flags are missing.

## agent-browser

agent-browser needs Chrome 157 with WebMCP features switched on.
Set this before you launch it:

```bash
export AGENT_BROWSER_ARGS="--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport"
```

Open the page from the `find` result, then list and call:

```bash
agent-browser webmcp list
agent-browser webmcp invoke <tool> --params '{...}'
```

Without the flag the page registers no tools and the list comes back empty.
If a tool is missing, wait for the page to finish loading and list again.

## Playwright MCP

Open the page from the `find` result with Playwright MCP first.
Tools named `webmcp_<tool>` appear after navigation.
Call `webmcp_<tool>` with the arguments from the schema.

Check your driver's docs for passing Chrome launch flags.
The flag you need is
`--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport`.
If tools do not appear, wait for the page to finish loading and look again.

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
