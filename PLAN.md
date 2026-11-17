# Plan

## What it is

`agent-webmcp find "<task>"` tells an agent which website has a WebMCP tool for
the task, and which page to open. The agent opens that page with its own browser
driver and calls the tool there. agent-webmcp never opens a page or calls a tool
for the agent.

## What is built

| Piece | State |
|---|---|
| `find` | Search over verified sites: the object of a request is required, the verb is not; a word naming an indexed site narrows the search to it; results rerank on where the words land. `--json` adds `relevance`. |
| Index | `index/index.json`: 52 sites checked, 38 verified with 227 tools (Chrome 157 with the WebMCP feature flag). Built by `tools/build-index` from `seeds/launch.txt` with agent-browser (Lightpanda available for cross-checks); each site records the page each tool was found on and when it was checked. A site that misses one check keeps its tools, marked with the date they were last seen; two misses drop it. |
| Delivery | `worker/agent-webmcp-index` publishes a pointer (`index.json`) plus an immutable, hash-addressed index file (the full 52-site file; the pointer counts the 38 verified / 227 tools). The CLI checks hourly with a conditional GET, verifies the sha256, swaps atomically, and keeps the last good copy on any failure. `agent-webmcp status` shows the addresses tried. |
| Submissions | Issue forms (`add-site`, takedown) → `scan.yml` on a disposable GitHub runner (no secrets, installs agent-browser + Chromium, `check-site` with agent-browser only) → result artifact → `publish.yml` validates strictly and opens a listing PR (or comments + `needs-review`). Bots propose, humans merge, then `publish-index --deploy`. Weekly `rebuild.yml` fans out to 5 ephemeral instabox boxes (needs `INSTABOX_API_KEY`; keep box quota free: no persistent boxes alive at rebuild time). Scripts: `tools/submit/validate.mjs`, `decide.mjs`, `splice.mjs`, all with self-tests. |
| Skill | `skills/agent-webmcp/SKILL.md`: find, then open and call with your own driver. Chrome 157 needs `AGENT_BROWSER_ARGS="--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport"` or the page registers nothing. |
| MCP server | One tool, `find`. Refreshes the index in the background every hour. |
| Skill | `skills/agent-webmcp/SKILL.md`: find, then open and call with your own driver. |
| Publish | `node tools/publish-index.mjs --deploy`. |

## Not built yet

1. **Registry.** Static item JSON, `add`/`remove`/`list`/`outdated`/`update`, a
   lockfile with hashes, the generated extension folder, `setup`, and the first
   `@orkestrate` items. Reviewed `@orkestrate` only at first.
2. **Distribution.** CI, binaries per platform, npm `@orkestrate/agent-webmcp@0.2.0`
   (v1 `0.1.0` deprecated), skill install.
3. **Website.** `/webmcp` rewrite, "Add your site", registry pages, docs, flags
   for the other surfaces, Vercel Web Analytics.
4. **Launch.** Archive push, website deploy, force-push `main`, tag, publish.

## How the index stays honest

- Only what a browser actually saw is listed; a tool that needs interaction to
  appear is not indexed yet.
- Blocked sites are recorded as blocked, never solved.
- A site that misses one check keeps its tools, marked with the date they were
  last seen; two misses in a row drop it.
- Descriptions that try to steer the agent are flagged and ranked last.

## Commands

| Command | What it does |
|---|---|
| `find <task>` | Search the index. `--json` for machine output. |
| `update` | Check for a newer index now. |
| `status` | Show the index in use and the addresses tried. |
| `mcp` | Run the MCP server with one tool, `find`. |
