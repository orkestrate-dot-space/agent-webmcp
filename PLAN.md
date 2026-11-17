# First release plan

## What it is

`agent-webmcp find "<task>"` tells an agent which website has a WebMCP tool
for the task, and which page to open. The agent opens it with its own browser
driver and calls the tool there. agent-webmcp never opens a page or calls a
tool for the agent.

## Pieces

1. **Index.** Built by loading a curated seed list (`seeds/launch.txt`) in a
   browser with WebMCP on, using an existing driver (no scanner of our own). Records per tool: name, description, schema,
   annotations, the page found on, check date. Status per site: verified,
   no_tools, blocked, error.
2. **find.** FTS5 search. Platform tools shared by many sites (Mintlify)
   appear once with a site count. Descriptions that steer the agent are
   flagged and ranked last.
3. **Distribution.** CLI and skill; MCP server with `find` only. The index is
   downloaded on first use and refreshed weekly.
4. **Website.** `/webmcp/sites` and per-site pages built from the same index
   file.
5. **Registries** (community toolkits for sites without tools): designed next,
   with the user.

## Done when

- A fresh agent, given only the skill, finds a tool with `find` and calls it
  through agent-browser without help.
- The launch scan has run and the index is published.
- `/webmcp/sites` shows only scanned sites, with dates.

## Out of scope for the first release

- Calling tools, approvals, call logs (drivers do this).
- Tools that appear only after interaction.
- Usage counts, claims, badges (after launch).
