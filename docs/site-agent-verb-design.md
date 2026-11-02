# Site-agent verb design: A vs B vs C

Date: 2026-10-31. Question: what shape should *site agents* (remote AI assistants,
e.g. the Mintlify docs assistant) take when invocable through the agent-webmcp engine?
Primary sources only. No code changes made.

Options under test:

- **A (per-site verbs):** each contracted site gets its own verb, e.g.
  `ask_mintlify({question})`, materialized only on sessions whose origin matches.
- **B (generic verb):** one `agent({site, question})` verb resolving a roster of site rows.
- **C (anything better found in the wild).**

Related standing decisions (not relitigated here, recorded as constraints):
`SITE-AGENTS.md` locks "ONE tool per site regardless of how many transports exist"
as the standing bias and v1 scope "mintlify complete, no second site until sign-off".

---

## 1. WebMCP proposal + Chrome docs + webmcp.com directory

Sources: spec draft (CG report, explicitly "not a W3C Standard nor on the Standards
Track") at `https://webmachinelearning.github.io/webmcp`; Chrome docs hub at
`https://developer.chrome.com/docs/ai/webmcp` (hub), `/imperative-api`,
`/build-tools`, `/best-practices`; directory at `https://browser-use.com/webmcp`.
Local deep-dive: `docs/webmcp-host-research.md`, `docs/web-to-agent-tool-space.md`.

What each says (quoted or precisely paraphrased):

- **Registration model.** A page registers tools via
  `document.modelContext.registerTool({name, description, inputSchema, execute, annotations})`.
  `name` must be 1–128 chars of `[A-Za-z0-9_.-]`; `description` must be non-empty;
  `inputSchema` is stored as an opaque stringified-JSON blob; `getTools()` returns
  `RegisteredTool{name, title, description, inputSchema, window, origin, annotations}`;
  `executeTool(tool, inputObject, options)` runs on the owning document. There is no
  `unregisterTool` — unregistration is aborting the register-time `AbortSignal`
  (`docs/webmcp-host-research.md` §1.1–1.2; spec IDL index). Same-name re-registration
  across documents is *not* protected from the execution race (spec §3 note referencing
  issue #92).
- **Annotations** are first-class: `readOnlyHint`, `untrustedContentHint`,
  `consequentialHint`, `debugging` (Chrome `/imperative-api`, "Tool annotations").
- **Cross-origin is two-sided opt-in**: tool lists `exposedTo`, caller lists
  `fromOrigins`; permissions-policy `tools` defaults to `'self'`. `toolchange` is an
  optional discovery signal (poll/subscribe instead). Per-invocation cancel is optional
  (`docs/webmcp-host-research.md` §1.2).
- **Multi-tool pages are the idiom, not the exception.** Browser-Use scanned 322,714
  pages: **13,352 sites / 45,763 distinct tools** (scan finished Oct 19, 2026;
  re-check of 400 listings Oct 24: 317 registered all listed tools via the shipped API).
  The **Mintlify platform row: 203 sites, 5 tools**
  (`get_site_overview`, `navigate_page`, `read_page`, `read_skill`, `search_docs`).
  Shopify: 12 tools on 5,190 sites; GitBook 3; ReadMe 3
  (`https://browser-use.com/webmcp`, "Platforms that ship it for you" + "Kinds of site").
- **One function per tool; no overlap.** Chrome `/best-practices`: "Each tool should
  consist of a single function"; "Be careful not to create overlapping tools, as the
  agent may be confused as to what to use"; "While there isn't a maximum number of
  tools allowed, each tool takes up part of the context window… The more tools you
  provide and the more the tools have overlap, the harder it is for the agent to pick
  correctly. Experiment to determine what's right for your application." Also:
  "Register tools all at once… Roll back the whole batch on failure instead of leaving
  half a workflow callable."
- **Tool discovery requires visiting the site**: "Clients and browsers must visit a
  site directly to know if it has callable tools" (Chrome hub, "Limitations").
- **Crucial negative fact**: Mintlify's own 5 platform tools are *retrieval*
  (`search_docs`, `read_page`, …), not the assistant. The assistant is a separate
  product surface (chat panel, `?assistant=` deep link). Repo already learned this the
  hard way: "Index vs agent: mounted the MCP *search* recipe as 'the agent' because it
  ran in `execute`. The poem test killed it: the agent reasons and refuses; the index
  retrieves. Never conflate again." (`SITE-AGENTS.md` §"What I got wrong" ¶3).

Implication for A vs B: the entire WebMCP corpus — spec, Chrome guidance, 13k-site
directory — speaks **A**: named, single-function, descriptively-routed verbs. Nothing
in the spec models a generic dispatcher verb, and nothing forbids an engine from
serving one either. A site agent named `ask_<site>` reads exactly like a site's own
verb, which is also what `skills/agent-webmcp/SKILL.md` already promises
("plus authored site agents (`ask_<site>`) on contracted origins"). Caveat: today's
`ask_mintlify` is engine-authored, not site-registered — it fills a gap Mintlify
didn't register itself.

## 2. Google A2A protocol (v1.0, Linux Foundation)

Sources: `https://a2a-protocol.org/latest/` (home: "A2A is for agent-to-agent
communication… MCP is for agent-to-tool"),
`https://a2a-protocol.org/latest/specification/` (§§3–4, 8–9),
`https://a2a-protocol.org/latest/topics/agent-discovery/`.

What it says:

- **AgentCard** is the discovery document: `name`, `description`, `provider`, service
  `url`, capabilities (`streaming`, `pushNotifications`), auth `schemes`, and `skills[]`
  with `id`, `name`, `description`, `inputModes`, `outputModes`, `examples`
  (discovery page, "The Role of the Agent Card"; spec §4.4/§8).
- **Discovery strategies**: (1) well-known URI
  `https://{domain}/.well-known/agent-card.json` (RFC 8615 style, recommended for
  public agents); (2) curated registries — "The current A2A specification does not
  prescribe a standard API for curated registries"; (3) direct configuration
  (discovery page).
- **Operations**: `message/send`, `message/stream`, `tasks/get|list|cancel`, push
  notification configs, `getExtendedAgentCard`; bindings for JSON-RPC, gRPC,
  HTTP+JSON/REST (spec §§3, 9–11). Async-first, long-running, human-in-the-loop
  (§1.2 guiding principles).
- **Explicit non-goal**: "Not a sub-agent or tool-call protocol. A2A does not specify
  how an agent talks to its own sub-agents or how it invokes tools — use your
  framework's native primitives, or MCP, for those." (home, "What A2A Is Not").
- **MCP relationship**: "Use MCP to equip an individual agent with the specific tools
  it needs… Use A2A to let that specialized agent securely collaborate with other
  agents." A2A Appendix B restates the complementarity.

Implication: A2A is the industry's agent-to-agent answer, and its shape is closest to
**C (adopt a standard agent client)** — but there is **no server to talk to**:
Mintlify publishes no `/.well-known/agent-card.json`, no A2A endpoint; its
machine surfaces are the assistant REST API (key-gated), the hosted MCP retrieval
server, WebMCP retrieval verbs, and the `?assistant=` panel. Adopting A2A for v1 would
mean building an A2A *client transport* in the engine plus lifecycle machinery
(tasks, streaming, push notifications) for a one-shot Q&A currently served by a 90s
panel wait. Revisit trigger is crisp: **the day a contracted site publishes an
AgentCard at its well-known URI**, a generic A2A transport becomes viable and strictly
dominates screen-driving. Until then A2A is evidence *against* over-building, not a
v1 option.

## 3. Context7 (roster-then-act over hundreds of libraries)

Sources: `https://github.com/upstash/context7` (README, "Available Tools" + "Use
Library Id"), `https://context7.com/docs` (intro: "1000 API calls… every public
library").

What it says (README verbatim, trimmed):

- MCP tools: **`resolve-library-id`** — "Resolves a general library name into a
  Context7-compatible library ID" (`query` required — "used to rank results by
  relevance" — plus `libraryName` required); **`query-docs`** — "Retrieves
  documentation for a library using a Context7-compatible library ID"
  (`libraryId`, `query` required, `language` optional). CLI mirrors: `ctx7 library`,
  `ctx7 docs`.
- Roster-bounding mechanism: resolution disambiguates at call time ("The slash syntax
  tells Context7 exactly which library to load docs for" — caller can pre-resolve and
  skip matching); version via prompt mention ("How do I set up Next.js 16… Context7
  will automatically match the appropriate version").

Implication: Context7 is the closest live analog to our problem and it chose
**B-shaped at vendor scope** — two generic verbs fronting hundreds of libraries —
because per-library verbs would explode the tool surface. That is evidence for B
*at scale*. But note the structural difference that dissolves most of it here: our
roster is **compiled-in contracts with origin matching** (`src/agents.ts`
`matchAgent`: `host === domain || host.endsWith('.'+domain)`), so site selection is
deterministic from the session the caller already opened — the model never spends a
call on resolution. Context7 needs two verbs precisely because its roster is
open/dynamic and selection is model-driven. A compiled roster of site agents needs
neither a resolver verb (B's extra call) nor a `site` parameter (B's model-choice
failure mode) — provided the roster stays small and origin-keyed (see verdict
triggers).

## 4. Exa + Firecrawl (generic verbs with parameters vs per-domain tools)

Sources: `https://github.com/exa-labs/exa-mcp-server` (README "Available Tools",
"Authentication"); `https://docs.firecrawl.dev/mcp-server` (hub),
`https://docs.firecrawl.dev/mcp-server/tools` ("Choose a Firecrawl tool", "Important
behavior").

What they say:

- **Exa**: default surface is **2 generic verbs** — `web_search_exa` ("Search the web
  for any topic"), `web_fetch_exa` ("Read a webpage's full content… from one or more
  URLs") — plus opt-in via `?tools=` (`web_search_advanced_exa`, `agent_run`). No
  per-domain verbs anywhere. Auth: anonymous with rate limits; OAuth or API key for
  full surface.
- **Firecrawl**: ~15-tool family of **generic verbs**: `firecrawl_scrape` (one URL +
  optional JSON format), `firecrawl_map` (discover URLs), `firecrawl_search`,
  `firecrawl_parse`, `firecrawl_crawl` + `firecrawl_check_crawl_status` (async pair —
  "The crawl tool polls the job to a terminal state before returning. If that wait
  times out, resume the job with [the status tool]"),
  `firecrawl_agent` + `firecrawl_agent_status` (asynchronous by design), `firecrawl_map`,
  `firecrawl_interact` + `firecrawl_interact_stop`, `firecrawl_research_*`,
  `firecrawl_monitor_*`, feedback tools. Hosted keyless tier exposes only
  `search`/`scrape`/`parse`.
- **Discovery-inside-catalog precedent (Alexandria)**: authenticated `firecrawl_search`
  returns matching *tools* in `data.tools`; `firecrawl_find_tools` browses
  categories→providers→tools with no args; run via `firecrawl_scrape` with
  `alexandria:{provider, capability, options}`. I.e., generic verbs + a finder, not
  per-provider verbs.

Implication: the extraction-MCP industry votes unanimously **B-shaped** (generic verbs
+ parameters). Supporting details transfer directly: (a) long jobs get async
start/status pairs — our engine already has that shape (`execute{background:true}` →
`poll`), and the deep-link driver fits inside it; (b) tiered surfaces by auth
(keyless subset) mirror our transport ladder (key-gated REST parked, keyless
browser/panel path active); (c) when a roster *is* large and dynamic (Alexandria
providers), the answer is finder + generic executor — a **C option** to keep in
reserve, not v1.

## 5. MCP spec itself (tools vs resources vs prompts; annotations; count guidance)

Sources: `https://modelcontextprotocol.io/specification/2025-06-18/server/tools`
(tools page: user interaction model, capabilities, listing, calling, list-changed,
data types, errors, security).

What it says:

- **Tools are "model-controlled"**: "the language model can discover and invoke tools
  automatically based on its contextual understanding." Servers "MUST declare the
  `tools` capability" (`tools: {listChanged}`); discovery is `tools/list` with
  **pagination** support (`cursor`/`nextCursor`); updates via
  `notifications/tools/list_changed`; errors split into protocol errors (JSON-RPC)
  vs tool-execution errors (`isError: true`).
- **Tool shape**: `name` (unique id), `title`, `description`, `inputSchema`,
  optional `outputSchema` (servers "MUST provide structured results that conform to
  this schema"), optional `annotations`. "For trust & safety… clients MUST consider
  tool annotations to be untrusted unless they come from trusted servers."
- **Human-in-the-loop SHOULD**: "there SHOULD always be a human in the loop with the
  ability to deny tool invocations"; "Provide UI that makes clear which tools are
  being exposed"; "Present confirmation prompts… for operations."
- **No tool-count limit anywhere in the page.** Pagination and `listChanged` are the
  spec's scaling answers, not a number.
- Tools vs the other two primitives (neighboring spec pages, same version family):
  resources = app-controlled context, prompts = user-controlled templates. A
  question-in/answer-out site assistant is **model-controlled invocation** → tool, not
  resource, not prompt.

Implication: the spec is neutral on A vs B; it only demands the chosen shape be a
*tool* with a unique name, description, and JSON-schema inputs — both options comply.
Two actionable details regardless of verdict: (a) an `ask_*` verb is read-only and
should eventually carry `readOnlyHint: true` (currently `src/server.ts`
`authoredFor` sets only `inputSchema` via `agentInputSchema()` — no annotations at
all); (b) the spec's human-in-the-loop SHOULD is currently satisfied by the calling
harness owning approval (CHANGELOG: "The calling harness owns approval now"), which
holds for either shape.

## 6. OpenAI + Anthropic function-calling guidance (many-tool limits)

Sources: OpenAI `https://platform.openai.com/docs/guides/function-calling`
("Best practices for defining functions", "Token Usage", namespaces, tool search);
Anthropic `https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview`,
`…/tool-use/tool-search-tool` ("Limits and best practices", "When to use tool
search", "Optimization tips").

What they say:

- **OpenAI**: "Keep the number of initially available functions small… Aim for fewer
  than 20 functions available at the start of a turn at any one time, though this is
  just a soft suggestion." "Functions are injected into the system message… billed
  as input tokens." Remedies: limit loaded functions, shorten descriptions, or **tool
  search** (`gpt-5.4+` defers rarely-used tools); **namespaces** group related tools
  (`crm`, `billing`…) "especially useful when the model must choose between tools
  that serve different systems."
- **Anthropic**: "Claude's ability to pick the right tool degrades once you exceed
  **30–50** available tools"; "A typical multiserver setup… can consume ~55k tokens
  in definitions… Tool search typically reduces this by over 85 percent." Server-side
  tool search: mark tools `defer_loading: true`, keep "3–5 most frequently used tools
  non-deferred"; limits: **10,000 deferred tools** per request, search returns up to 5
  by default. "Use tool search when… You have 10 or more tools… Tool definitions
  consume more than 10k tokens… You aggregate multiple MCP servers (200+ tools)."
  Optimization tip: "Use consistent namespacing in tool names: prefix by service or
  resource (for example, `github_`, `slack_`) so one search matches the whole group."
- Both: strict/structured schemas, clear descriptions, `tool_choice` controls.

Implication: at N=1 contracted site both options cost ~1 definition and all of this
is moot — but the scaling math contains the decisive engine-specific fact (see §7):
**our authored verbs are per-session, not global**. `authoredFor()` adds only the
contract matching the session's origin, so option A never pays the N-definition
context tax that motivates tool search in normal multi-server MCP deployments. B
costs 1 definition always but grows a `site` enum/union with the roster and adds a
model routing decision per call. The `ask_<site>` prefix convention is exactly
Anthropic's recommended "prefix by service" namespacing, which keeps a future
resolver trivially matchable. Net: vendor guidance argues B for *global* rosters and
is neutral-to-A for *session-scoped* rosters — ours is session-scoped.

## 7. agent-webmcp repo constraints (engine, budgets, gate, registry history)

Sources: `src/server.ts`, `src/agents.ts`, `src/runner.ts`, `src/sessions.ts`,
`CHANGELOG.md` ("Unreleased"), `SITE-AGENTS.md`, `skills/agent-webmcp/SKILL.md`,
`src/catalog.ts` (via server imports), gate config (`scripts/gate.ts`, `package.json`).

What they say:

- **Fixed 7-verb engine**: `open, list, execute, poll, close, skill, doctor`
  (`src/server.ts` `VERBS`). `open` returns `{handle, url, toolCount, cdpUrl, owned}`;
  `list` returns name/signature rows, name-sorted "for cache stability"; `execute`
  runs caller JS in a `Bun.Worker` with a proxy-tree `tools` object and reserved
  `search()`/`catalog()` in-dialect discovery (`src/runner.ts`); unknown paths fail
  as `unknown tool path … call search() to discover verbs`.
- **Hard budgets** (`src/server.ts` constants + CHANGELOG): `timeoutMs` ≤ 300000;
  defaults 25 max calls / 16000 max chars-per-read / 64000 max result chars;
  ≤ 8 sessions per run; ≤ 20 live background runs (50 records); failed runs keep a
  20-call `partial` ledger. **Base-prompt budget: skill text + all served descriptions
  ≤ 8000 chars** (~2k tokens; currently ~4.7k) — a global cap that every added
  description spends from.
- **A is already implemented**: `src/agents.ts` holds compiled `CONTRACTS`
  (`mintlify.com` → `ask_mintlify`, `deep-link` transport,
  endpoint `https://www.mintlify.com/docs/<page>?assistant=<question>`),
  `matchAgent()` origin matching, `agentInputSchema()` (`{question}` required),
  `runDeepLink()` (mint tab, wait for panel, settle-detect, close tab).
  `src/server.ts` `authoredFor()` serves it "exactly like page verbs; a native verb
  of the same name always wins", counted in `toolCount`. CHANGELOG documents it as
  "New (non-breaking): authored site agents."
- **Registry is dead, contracts are data**: "the registry is gone… `registry.ts`,
  `admission.ts`, `snippet.ts` (and their tests) deleted; verbs resolve live-only…
  Nothing is read from or written to disk" (CHANGELOG). "Admission is contracts, not
  code: rows carry transport + endpoint + example. Drivers are fixed engine code per
  transport. No arbitrary bodies are ever admitted (that registry is dead and stays
  dead)." (`SITE-AGENTS.md`).
- **Engine surgery needs explicit approval each time** (`SITE-AGENTS.md` "Decided");
  adding *either* a B verb or more A rows is engine-code change — B saves no
  implementation cost over A at N=1.
- **Transport ladder is verified, not hypothetical** (`SITE-AGENTS.md` §§46–74):
  REST assistant API parked (needs `mint_dsc_*` key); v1 = `?assistant=` deep link
  ("verified: 3+ Q&A, citations, out-of-scope refusal"); undocumented panel backend
  PROBED and CLOSED (`leaves.mintlify.com`, requires browser-minted invisible-captcha
  token; null → 400 `captcha_required`; datacenter egress → "unavailable from your
  network"). "No scraping around auth/captcha/login walls. Parked, never bypassed."
- **Gate**: `bun run gate` (typecheck, unit tests, skill-shape, schema-object,
  desc-budget) green before every commit (`AGENTS.md`).

Implication: the repo has already paid for A and fenced off the alternatives'
failure modes (no arbitrary code, one tool per site, no second site pre-sign-off).
The 8000-char description budget is the only real scaling ceiling either option hits —
and A spends it only on the matched session's verb, the cheapest possible placement.

## 8. opencode's own subagent pattern (caller-side precedent)

Sources: `https://opencode.ai/docs/agents/` ("Types": primary vs subagents;
"Subagents… primary agents can invoke… via the Task tool… or @ mentioning";
"description… required"; `permission.task` glob patterns controlling which subagents
an agent may invoke; `@general`/`@explore` built-ins),
`https://opencode.ai/docs/tools/` neighbor pages (task tool family).

What it says: **one generic invocation tool (`task`) + a named roster**, where each
roster entry carries `description` (required, used by the model for dispatch),
`mode`, `model`, `permission` scoping, and `disable_parallel_tool_use`-style
controls. Users can always `@`-mention directly; programmatic invocation goes through
the single tool with the roster resolved by description match. `permission.task`
with glob patterns ("`orchestrator-*`: allow") is the roster-scale access control.

Implication: the caller-side ecosystem precedent is **B-shaped** (generic invoke +
roster), and it works because the roster is user-editable config with per-entry
descriptions doing the routing. Transferability to the engine is partial: our roster
is compiled contracts with sign-off, selected by *session origin* rather than model
choice — the engine already resolves "which site agent" without spending model
judgment. Copying opencode's shape inside the engine would reintroduce a routing
decision the engine has already made deterministically. The correct borrowing is
narrower: keep per-site verbs (A) with **opencode-quality descriptions** (one line,
what it answers, when to use it) since description-match is how both harnesses and
models route — `agents.ts` `description` already follows this
("Ask Mintlify's docs assistant anything; returns its worked answer with citations").

---

## Verdict: A for v1 (one contracted site), with a defined revisit trigger

**Recommend A — `ask_<site>({question})` per contracted origin, served only on
matching sessions — for the v1 with mintlify.com as the single contracted site.**

Reasoning, in load-bearing order:

1. **Native idiom.** WebMCP's spec, Chrome's guidance ("single function per tool, no
   overlap"), and the 13k-site directory all converge on named per-function verbs.
   `ask_mintlify` reads as the site's own verb inside `list`/`execute`, which is
   also what the shipped skill text already promises.
2. **Already built and fenced.** `src/agents.ts` + `authoredFor()` implement A behind
   the repo's hard-won constraints (compiled rows, fixed drivers, native-wins,
   mint-refusing browsers fail loud). B costs the same engine surgery for zero v1
   benefit.
3. **Session scoping neutralizes A's classic weakness.** The vendor case for B
   (OpenAI <20, Anthropic 30–50, Context7/Exa/Firecrawl generic verbs) is about
   *global* tool surfaces taxing every context. Our authored verbs load only on the
   matching session, so A scales in context-cost terms like B while keeping
   deterministic origin routing — no resolver call, no `site` parameter for the
   model to get wrong.
4. **Standing bias.** `SITE-AGENTS.md` locks "ONE tool per site" as a user order;
   A is that order implemented. B's `site` parameter would additionally strain the
   one thing both options share: the 8000-char base-prompt description budget.
5. **No A2A server exists.** The principled C (speak the industry agent protocol)
   has no counterparty; screen-driving the panel is the verified transport, and a
   per-site verb hides exactly that embarrassment behind a stable contract.

**Trigger to revisit (any one flips the verdict):**

- **T1 — second contracted site signed off.** At N=2, re-price B vs A+A. Default
  expectation: still A (add `ask_<second>` row; per-session scoping holds). Revisit
  seriously only if T2 or T3 also fires.
- **T2 — roster pressure on the description budget.** If contracted rows exceed ~5
  sites *or* skill+descriptions approach the 8000-char gate, adopt **C-resolver**:
  keep `ask_<site>` verbs, add a curated-registry-style finder (A2A's registry
  strategy, Alexandria's `find_tools`, Context7's `resolve-library-id`) — additive,
  never replacing A.
- **T3 — origin matching breaks.** A multi-tenant docs host or shared-origin sites
  where `matchAgent(origin)` cannot resolve the site deterministically forces an
  explicit `site` parameter — i.e., B (or a resolver) becomes necessary rather than
  stylistic.
- **T4 — a contracted site publishes `/.well-known/agent-card.json`.** Adopt an A2A
  client transport behind the existing per-site verb (transport ladder gains a tier
  above the panel driver; verb shape unchanged).

None of T1–T4 holds today (N=1, ~4.7k/8k budget, `mintlify.com` suffix-match works,
no AgentCard). Ship A.

## Open questions (primary sources could not answer)

1. **`/agents` page mechanics.** `SITE-AGENTS.md` lists "Exact working of /agents
   (page mechanics beyond the row)" as pending. Neither the WebMCP spec nor A2A
   covers an engine-side "contracted roster" concept — it is our invention, so no
   primary source will validate the row/presence/merge design; it must be settled by
   user sign-off, as the file already requires.
2. **API keys: user-supplied vs keyless-only.** No vendor docs resolve who provisions
   the `mint_dsc_*` key or per-user OAuth for the REST tier; the ladder parks it
   ("Needs dashboard key. PARKED"). Unblocks only via product decision, not research.
3. **Multi-turn thread continuity.** The v1 driver mints a fresh tab per call
   (`runDeepLink` → `Target.createTarget` per invocation) while `SITE-AGENTS.md`
   claims "multi-turn keeps the thread." Whether consecutive `ask_mintlify` calls
   share a thread (cookies/localStorage persist cross-tabs on the same browser
   profile, but panel thread state may be per-page-load) is unverified — needs a
   live probe, not docs.
4. **Panel reliability envelope.** Verified behaviors (settle detection, 3+ Q&A,
   citations, out-of-scope refusal) vs failure modes (captcha-gated backend,
   "unavailable from your network" on datacenter egress, EasyList/assistant-bar
   blocking per Mintlify's own troubleshooting, corporate-network allowlist
   `leaves.mintlify.com`/`*.mintlify.dev`/hcaptcha) bound *where* the driver works,
   not the verb shape. No primary source quantifies headless/datacenter success
   rates.
5. **Annotations + outputSchema on authored verbs.** MCP spec and Chrome docs say
   `readOnlyHint:true` fits a no-side-effect Q&A verb and `outputSchema` tightens
   clients, but neither says who mints annotations for a third-party-authored verb
   (spec warns annotations are untrusted unless from trusted servers). Engine
   decision: set or omit, and whether the "trusted server" here is us.
6. **Citation/format contract.** The description promises "worked answer with
   citations" (matching the assistant's observed behavior), but no Mintlify doc
   specifies the panel's answer format as a stable contract — panel-markup drift is
   the known fragility ("the site may have changed its chat markup — re-verify the
   row"), handled by row re-verification rather than schema.
