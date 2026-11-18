# The index

`index/index.json` lists, for each checked site, the WebMCP tools its pages
register: name, description, input schema, annotations, the page the tool was
found on, and the date. Sites that showed no tools, were blocked by a bot
check or failed to load are recorded with that status and are not searchable.

Tools that only appear after you interact with a page (for example, after a
search) are not indexed yet.

## How `find` matches

`find` needs the object of your request, not the verb: "order a pizza" finds
`manage_pizza`. A word that names an indexed site narrows the search to that
site, so "search stripe docs" never answers with another company's docs. When
nothing matches, it says so instead of returning a loose word match.

## Listing and removal

To list a site, use **Add your site** on
[the sites page](https://web.orkestrate.space/webmcp/sites). A bot checks it
in a fresh Chromium and lists it automatically when the result is clean; the
status page shows the verdict and anything to fix. No ownership proof is
needed to list: the bot lists only what the site registers with WebMCP on.
See [CONTRIBUTING.md](../CONTRIBUTING.md) for the full rules. To have a site
removed, its owner emails support@orkestrate.space.

## Publishing and updates

The live index is served from KV by the `agent-webmcp-index` Worker;
`index/index.json` is a snapshot of it. Submissions go to the
`agent-webmcp-intake` Worker, which scans them in an isolated sandbox and
publishes clean results.

The index is published as a small pointer plus an immutable, hash-addressed
file. The CLI checks the pointer hourly, downloads only when the hash changed,
verifies it, and keeps the last good copy if anything fails. The weekly
rebuild rechecks every seed; a site that misses one check keeps its tools and
drops on the second, and a rebuild that finds far fewer sites refuses to
publish.
