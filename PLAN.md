# Proof of concept: build plan

## The question the POC answers

Can an agent ask for a capability, without naming a site, and have it run as a
typed WebMCP call in a real browser, with a permission check in between?

If yes, the core of agent-webmcp works and everything else (registry, ranking,
workflows, relay) is built on top of it.

## What the agent experiences

```
find("search flights from SFO to JFK")
  -> googlechromelabs.github.io/searchFlights   read   native
     schema: { origin, destination, tripType, outboundDate, inboundDate, passengers }

call("googlechromelabs.github.io/searchFlights", { origin: "SFO", ... })
  -> policy: read-only, allowed
  -> opens the page in agent-browser, invokes the page's own tool
  -> { result, origin: "https://googlechromelabs.github.io", tab: "t1" }

find("say hello on example.com")
  -> example.com/say_hello   read   community script

call("example.com/say_hello", { name: "Ada" })
  -> opens example.com, injects the script, invokes it
  -> "hello Ada"

call("<a write tool>", { ... })
  -> policy: consequential, asks the user (elicitation), runs only on approve
```

## Scope

In:

- One binary, `agent-webmcp`, in Rust.
- `agent-webmcp mcp`: an MCP stdio server with two tools, `find` and `call`.
- `agent-webmcp find <query>` and `agent-webmcp call <id> <json>`: the same
  engine from the CLI, for testing and for agents with a shell.
- A catalog file, `catalog/tools.json`: a handful of tool entries, native and
  community, written by hand.
- A community script directory, `catalog/scripts/`: one or two scripts.
- Search over the catalog with SQLite FTS5 (BM25).
- One backend: agent-browser, used as an MCP client of
  `agent-browser mcp --tools core,webmcp`.
- Policy, hard-coded: read-only tools run; anything else asks the user through
  MCP elicitation, and is denied when the client cannot ask.
- A ledger: every call appended to a local JSON Lines file with origin, tool,
  arguments hash, outcome and duration.

Out (later phases):

- Registries, signing, lockfiles, sync.
- Browse backend.
- Embeddings, login-aware ranking, reliability scores.
- Cedar policies, scopes, toolset pinning.
- Workflows, self-repair, relay, harness plugins.
- npm distribution. The POC is `cargo install --path .`.

## Repository layout

```
Cargo.toml
src/
  main.rs        CLI: find, call, mcp
  catalog.rs     load tools.json and scripts, validate entries
  index.rs       SQLite FTS5 in memory, BM25 search
  backend.rs     Backend trait
  agent_browser.rs  agent-browser over MCP
  policy.rs      allow, ask or deny
  ledger.rs      append-only JSONL
  mcp.rs         MCP server: find, call
catalog/
  tools.json
  scripts/example.com.js
tests/
  catalog.rs  index.rs  policy.rs     unit tests, no browser
  e2e.rs                               real agent-browser, ignored by default
PLAN.md
README.md
```

## Data shapes

Catalog entry:

```json
{
  "id": "example.com/say_hello",
  "site": "example.com",
  "url": "https://example.com/",
  "name": "say_hello",
  "description": "Say hello to someone by name.",
  "kind": "read",
  "source": "community",
  "script": "scripts/example.com.js"
}
```

- `source`: `native` (the site registers it) or `community` (we inject
  `script` first).
- `kind`: `read` or `write`. Community entries declare it; for native tools
  the page's `readOnlyHint` is checked at call time and the stricter of the
  two wins.

`find` result: id, site, description, kind, source, and the input schema
when known. The schema of a native tool is read from the page on first call
and cached in memory.

Backend trait:

```rust
trait Backend {
    async fn open(&self, url: &str) -> Result<Tab>;
    async fn inject(&self, tab: &Tab, script: &str) -> Result<()>;
    async fn list_tools(&self, tab: &Tab) -> Result<Vec<PageTool>>;
    async fn invoke(&self, tab: &Tab, tool: &str, frame: &str, args: Value) -> Result<Value>;
}
```

agent-browser mapping (tool names confirmed with `agent-browser mcp --tools
core,webmcp`, version 0.41.0):

| Trait | agent-browser MCP tool |
|---|---|
| `open` | `agent_browser_open` (`url`, `session`) |
| `inject` | `agent_browser_eval` (`script`), after load |
| `list_tools` | `agent_browser_webmcp_list` |
| `invoke` | `agent_browser_webmcp_invoke` (`tool`, `frameId`, `params`) |

Injection after load is enough for the POC; the spike showed that a
`registerTool` from the page's main world is listed and invokable.

## Dependencies

| Crate | Use |
|---|---|
| `rmcp` | MCP server (stdio) and MCP client (child process) |
| `tokio` | async runtime |
| `serde`, `serde_json` | JSON |
| `rusqlite` (`bundled`) | FTS5 index |
| `clap` | CLI |
| `anyhow` | errors |

Runtime: agent-browser on PATH, with its Chrome installed. WebMCP is on by
default in the Chrome it launches.

## Build steps

Each step ends with something you can run.

1. **Skeleton.** `cargo new`, clap with `find`, `call`, `mcp` subcommands that
   print "not implemented". Check: `cargo run -- find x`.
2. **Catalog and search.** Load `tools.json`, build the FTS5 index, print
   ranked results. Check: `find "flights"` returns `searchFlights` first;
   unit tests for loading, validation and ranking.
3. **agent-browser backend.** Spawn `agent-browser mcp --tools core,webmcp` as
   a child, call `open`, `webmcp_list`, `webmcp_invoke`. Check: an ignored
   e2e test lists `searchFlights` on the Chrome Labs demo.
4. **`call` for native tools.** Resolve id, open, list, match the tool and
   frame, invoke, return result with origin. Check: `call
   googlechromelabs.github.io/searchFlights '{...}'` returns flights.
5. **`call` for community tools.** Inject the script after open, then list
   and invoke. Check: `call example.com/say_hello '{"name":"Ada"}'` returns
   `hello Ada`.
6. **Policy, ledger and argument checks.** Arguments are validated against the
   tool's `input_schema` before a browser opens. Read tools run; write tools
   need approval (CLI: `--yes` or ask on the terminal; MCP: elicitation). A
   question with no answer is recorded as `unanswered`, distinct from
   `approval_required` where nobody could be asked. Every call is appended to
   the ledger. Check: unit tests for the decision table and the validator; a
   write call without approval is denied and logged; bad arguments never reach
   a browser.
7. **MCP server.** Expose `find` and `call` over stdio with `rmcp`, with
   output schemas. Check: add it to OpenCode, ask "search flights from SFO to
   JFK" in a fresh session, and the agent uses `find` then `call` without
   being told the site.

## Done when

- Step 7 works in OpenCode, end to end, in one session.
- A write tool triggers an approval prompt in OpenCode, and denial stops it.
- `cargo test` passes; the e2e test passes locally with agent-browser.
- The ledger shows every call.

## Risks to watch

- agent-browser's MCP server speaks protocol 2025-11-25; `rmcp` must
  negotiate down as a client. Check in step 3.
- WebMCP tool names may collide across frames; match on frame from
  `webmcp_list`.
- The Chrome Labs demo is a third-party page that may change; keep a local
  copy for the e2e test if it does.
- Elicitation support in OpenCode is subagent-reported; confirm in step 7.
