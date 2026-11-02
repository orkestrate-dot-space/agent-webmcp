# Site agents — standing contract

How /agents rows become agents invocable through agent-webmcp.
Supersedes all earlier pipeline/registry thinking. No code runs on
prose here; this file binds decisions, not implementation.

## Goal (one line)

Each /agents row is a subagent: one call in, worked answer out,
reasoning sealed inside. Invocable by any harness speaking
agent-webmcp's verbs — never "a recipe to copy-paste".

## Mental model (locked)

- My subagents are the spec: `invoke(input) → reasoning → response`.
  A listed origin agent behaves identically from the caller's side.
- When a site is opened, its /agents tool (`ask_<site>`) loads
  *among* the page's own WebMCP tools. One tool per site, named
  `ask_<first-label>` (e.g. `ask_mintlify`). The row IS the tool
  definition; the engine materializes it from the row.
- Admission is contracts, not code: rows carry transport + endpoint +
  example. Drivers are fixed engine code per transport. No arbitrary
  bodies are ever admitted (that registry is dead and stays dead).

## Decided (user orders, do not relitigate without asking)

- Consumer: any harness via the engine (not my-harness-only).
- Engine surgery: ASK EACH TIME engine code would change.
- v1 scope: mintlify complete first. No second site until signed off.
- Row proof: USER SIGN-OFF ONLY. No row lists without it.
  (Status 2026-10-31: mintlify row is live WITHOUT sign-off — flagged twice.)
- Capture-and-replay probe on the panel POST: APPROVED and RUN
  (result: bot-gated, closed — see ladder tier 5).
- Orkestrate local checkout: purged to landing-only. Stale SPEC/HANDOFF
  are design history, never governing. Parked until raised.
- Push/tag/release/deploy: explicit per-target order only.
- No scraping around auth/captcha/login walls. Parked, never bypassed.
- Verb shape: A (per-site `ask_<site>` verbs, session-origin-scoped).
  Implemented in `src/agents.ts` + `src/server.ts::authoredFor`,
  gate green, showcased live through the real binary.
  Revisit triggers (any one reopens it): T1 second contracted site
  (re-price; default stays A) · T2 ~5 sites or base-budget pressure
  (add a finder, keep A verbs) · T3 multi-tenant origin breaks
  suffix-matching (a `site` param becomes necessary) · T4 a site
  publishes `/.well-known/agent-card.json` (add A2A transport,
  keep the verb).

## Pending discussion (do not implement until resolved)

- Exact working of /agents (page mechanics beyond the row).
- API keys: user supplies vs keyless-only. Research position
  (docs/harness-connections.md §9, §12): keyless public MCP is the
  only artifact that is truly one-command everywhere; keyed needs
  per-harness plumbing (OAuth+DCR per harness callback set, or
  static keys via env-indirection — never literals in committed
  config). PROPOSED: keyless-first always; OAuth-over-static-keys
  if a keyed tier is ever admitted. Needs explicit sign-off.
- Distribution: a row is invocable today by any MCP harness that
  connects agent-webmcp itself (`mcp serve` → open contracted
  origin → `ask_<site>` appears). Per-harness shims (Claude
  marketplace plugin, Cursor deeplink, etc.) are a later
  distribution question, not a v1 blocker. Portable SKILL.md +
  engine verbs already cover all seven surveyed harnesses'
  MCP paths. (Source: docs/harness-connections.md §12.)
- Annotations on authored verbs (`readOnlyHint: true`,
  `outputSchema`): proposed by the verb research, NOT implemented
  (engine change → needs per-change approval).

## Transport ladder (verified live, mintlify 2026-10-30/31)

Ordered best-first. First *working* tier wins per site.

1. Reasoning API over HTTP — assistant REST
   (`POST api.mintlify.com/discovery/v2/assistant/{domain}/message`,
   key `mint_dsc_*`). Needs dashboard key. PARKED (no key).
2. Reasoning agent, browser-held — `?assistant=` deep link opens the
   panel AND submits; read settled panel text. Same-tab follow-ups
   keep the thread (verified live in a borrowed tab). NOTE: the
   engine driver mints a FRESH tab per call, so engine-invoked
   turns are single-turn until thread continuity across tabs is
   proven (open research question — do not claim multi-turn for
   `ask_mintlify`). This is the v1 mintlify transport.
3. Native retrieval over HTTP — hosted MCP (`/docs/mcp`:
   search + filesystem + feedback), WebMCP verbs
   (`search_docs`, `read_page`, …), llms.txt / `.md` twins, skill.md.
   Runs inside `execute` today. Retrieval, NOT the agent —
   never present it as the agent again.
4. Embed JS API (`window.MintlifyAssistant.ask/init/open`) — real,
   but only on third-party embeds with a dashboard widget ID.
   Absent on mintlify-powered docs themselves (verified 2 sites).
   Auth-walled → parked.
5. Undocumented panel backend — PROBED 2026-10-31, CLOSED:
   `POST leaves.mintlify.com/api/assistant/mintlify/v2/message`
   `{fp, filter, currentPath, id, messages[{id,role,parts}], trigger,
   _, captchaToken}`. Server REQUIRES a browser-minted invisible-
   captcha token (null → 400 `captcha_required`; absent → 420;
   valid token + datacenter egress → “unavailable from your
   network”). Bot-gated by design. DO NOT work around it:
   no token harvesting, no fingerprint spoofing. Browser or key only.

## Research index (2026-10-31, all primary-sourced, all in docs/)

- `docs/site-agent-verb-design.md` — A vs B vs C. Verdict: A
  (session-scoped per-site verbs). Open: page mechanics, key
  provisioning, cross-tab thread continuity, panel reliability
  envelope, annotations/outputSchema, citation format.
- `docs/product-landscape.md` — falsifiable product def, innovation
  audit (ladder + row-shape are the novelty; consume-native is the
  spec's own premise), 6 dated why-now factors, no-moat-today
  verdict, 6 risks, 7 open questions, falsifiers.
- `docs/harness-connections.md` — 7-harness install matrix (MCP /
  skill / agent / plugin / ACP / marketplace), credential-custody
  table, keyless-vs-keyed implications, minimal artifact set
  (keyless MCP + portable SKILL.md + per-harness shims).
- `docs/mintlify-agent-surface.md` — Mintlify platform agent-to-agent surface (Discovery API, anon MCP tools, widget/deep-link, per-site variance), primary-sourced 2026-11-01.
- Standing corrections from research: Jev is a TypeSafe *decision*
  model, not a browser agent — never cite it as one. TinyFish is a
  search/fetch toolkit, not browser infra.

## v1 row shape (locked, 4 fields)

`origin` (identity + routing key) · `description` (one line, what it
answers) · `invoke {transport, endpoint, example}` (THE contract) ·
`proof {verified, ref}` (evidence note, date kept in data, not display).
No tags, counts, search, infinite scroll, signals matrix, rescan.
Presence = listed. Worker is presence store; sites.json is the contract
store; the page merges them. Worker source is absent from this
checkout — never depend on worker behavior beyond what /sites returns.

## What I got wrong (root causes, on record)

1. Brochure vs tool: shipped pages *about* invocation instead of
   invocation. A row nobody can call is not an agent.
2. Harness substitution: drove everything in desktop-browser tools,
   presented harness-local results as engine progress. RESOLVED
   2026-10-31: the engine now drives the reasoning path itself
   (`src/agents.ts` deep-link driver, showcased through the real
   binary). Standing constraint unchanged: anything the engine
   cannot do stays out of it — no harness demos presented as
   engine capability, ever again.
3. Index vs agent: mounted the MCP *search* recipe as "the agent"
   because it ran in `execute`. The poem test killed it: the agent
   reasons and refuses; the index retrieves. Never conflate again.
4. Single-viewport detection: narrow hides vercel's button, narrow
   unmounts mintlify's entirely. Viewport is evidence, record it.
5. Text-without-hrefs: "Try with ChatGPT" share stubs read as agent
   entries. Probes must capture hrefs + visibility, not text.
6. Imagery vs live: mintlify's homepage Ask UI is product-preview SVG
   pixels; the live button only mounts wide. Screenshots show
   visibility, DOM shows liveness — verdicts need both.

## Verification discipline (standing)

- Prove live before claiming. Tracebacks over memory.
- No-DOM verdicts are labeled DOM-only; no-pixel verdicts likewise.
- Single-option Choice heads, session-owned browsers, and every
  other live-learned constraint stays in force wherever it applies.
- Key material: env only, per-command export, unset after. Never in
  repo, chat logs aside.
