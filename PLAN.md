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
| Index | `index/index.json`, built by `tools/build-index` from `seeds/launch.txt` with agent-browser (Chrome 157 plus the WebMCP feature flag; Lightpanda stays available for cross-checks). Each record says which page a tool was found on and when it was checked. A site that misses one check keeps its tools, marked with the date they were last seen; two misses drop it. |
| Delivery | KV holds the live index; the Worker `agent-webmcp-index` serves it as a pointer (`index.json`) plus immutable, hash-addressed versions (`indexes/index-<sha>.json`) and the takedown list. The CLI checks hourly with a conditional GET, verifies the sha256, swaps atomically, and keeps the last good copy on any failure. `agent-webmcp status` shows the addresses tried. Static assets in the Worker are the fallback if KV has no key. |
| Submissions | Automatic. The site form posts to `agent-webmcp-intake`, which validates the URL (https, public DNS, rate limit, takedown list), queues it in D1, and starts a scan on an instabox box through the HTTP API. The box scans as an unprivileged user, posts the record back, and the Worker validates it, decides, and splices it into the index. A clean verified result is listed in a minute or two; anything else is reported on the request's status page. |
| Takedowns | Automatic: the host goes on the takedown list in KV and every listing for it is removed in the same publish. New submissions for it are refused. |
| Weekly rebuild | A cron on the intake Worker starts `build.mjs` on the box over the whole seed list; the result is published automatically unless verified sites collapse (then the old index is kept and the reason recorded). The seed list and the snapshot of `index/index.json` are published with the index. |
| Rollback | `node tools/index-rollback.mjs <sha>` points the live pointer at any earlier immutable version. `node tools/publish-index.mjs --adopt <pointer-url>` pulls the live index into the repo before a human edits it. |
| Skill | `skills/agent-webmcp/SKILL.md`: find, then open and call with your own driver. Chrome 157 needs `AGENT_BROWSER_ARGS="--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport"` or the page registers nothing. |
| MCP server | One tool, `find`. Refreshes the index in the background every hour. |

## Not built yet

1. **Registry.** Static item JSON, `add`/`remove`/`list`/`outdated`/`update`, a
   lockfile with hashes, the generated extension folder, `setup`, and the first
   `@orkestrate` items. Reviewed `@orkestrate` only at first.
2. **Distribution.** CI, binaries per platform, npm `@orkestrate/agent-webmcp@0.2.0`
   (v1 `0.1.0` deprecated), skill install.
3. **Website.** The site form, status page, and takedown form; registry pages.
4. **Launch.** Archive push, website deploy, force-push `main`, tag, publish.

## How the index stays honest

- Only what a browser actually saw is listed; a tool that needs interaction to
  appear is not indexed yet.
- Blocked sites are recorded as blocked, never solved.
- A browser without the page API reports an error, not "no tools": a broken
  scanner cannot delist anyone.
- A site that misses one check keeps its tools, marked with the date they were
  last seen; two misses in a row drop it.
- Descriptions that try to steer the agent are flagged and never listed
  automatically.
- The rebuild refuses to publish a collapse, and any version can be restored.

## Commands

| Command | What it does |
|---|---|
| `find <task>` | Search the index. `--json` for machine output. |
| `update` | Check for a newer index now. |
| `status` | Show the index in use and the addresses tried. |
| `mcp` | Run the MCP server with one tool, `find`. |
| `node tools/publish-index.mjs --kv` | Publish `index/index.json` to KV (human path; refuses to drop live sites). |
| `node tools/publish-index.mjs --adopt <url>` | Pull the live index into the repo. |
| `node tools/index-rollback.mjs <sha>` | Point the live pointer at an earlier version. |
