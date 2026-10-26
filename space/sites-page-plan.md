# /sites final design + implementation plan

Proposal for approval. No code changes made. Mocks:
`space/preview-sites-mock.png` (board: `space/sites-mock.excalidraw.json`),
HTML render at `/tmp/opencode/agentbrowser-sites.png`.

## 1. Research: how integrations.sh indexes

- Aggregates public feeds (APIs.guru, Anthropic/OpenAI MCP dirs, public
  GraphQL lists, CLI seeds) → normalize → catalog. Fixes the pipeline,
  not records.
- Owner declarations read live from the domain: their own
  `/.well-known/integrations.json` (v3: surfaces + credentials, inline,
  strict JSON) plus the open standards we also read (llms.txt, api-catalog,
  OpenAPI, MCP server card, oauth-resource, A2A card, agent-skills index).
- Discovery is on-demand: open `/{domain}/` → "Map integration surface" runs
  a Worker discover (LLM-backed, rate-limited) → listing live immediately.
- Every fact carries basis: `detected` (machine signal) vs `discovered`
  (docs/LLM). Owner declarations respected as intent; registry may add
  verified caveats.
- Row tags = surfaces with counts (`MCP`, `API 15`, `GraphQL`, `CLI`);
  auth lives on detail (credentials, setup prose).
- Conventions scorecard per domain (n/8 files published).

### Take / reject for us

Take: closed surface vocabulary on rows; declaration-file-first claim;
basis honesty (our scan = detected; proof-on-open gates); request flow
anchored at the domain you scanned.

Reject: hosted LLM discovery runs (no hosted service — deliberate); auth
tags (we carry no auth facts by decision); counts on tags at our scale
(one entry; counts become noise); category sidebar (not in our model).

## 2. Tags: the final set

Tags answer one question per row: how does an agent get in. Closed set of
six, each machine-checkable, no counts, no adjectives:

| Tag | Meaning | Basis |
| --- | --- | --- |
| WebMCP | callable tools on the page — open and call | live probe (webmcp.com lookup; re-probed on open) |
| Chat | assistant endpoint to dial | file: agents[] protocol chat |
| Agent-native | the site's own actions endpoint | file: agents[] protocol agent-native |
| A2A | A2A agent card | file: agents[] protocol a2a |
| MCP | MCP server | file: agents[] protocol mcp |
| Skills | published agent instructions | file: skills URL + agent-skills index |

Cut, with reasons:

- `Hooks` — authoring scaffolding, not a way in; stays on detail.
- Categories (AI/Analytics/…) — not in the model (declaration spec: no
  categories); every record would read "Other" = otiose. If topical
  grouping is ever wanted it becomes a real record field first.
- Auth tags (OAuth/API key) — mixes axes; auth facts deliberately live
  in RFC 9728 pointers + detail, never catalog tags.
- Counts on chips — honest but noise at our size (integrations.sh has 5k
  specs, we have 1).
- Quarantine as a tag — it is a status with a reason, shown in STATUS,
  never a badge to filter.

Record shape: `has` object becomes `tags: []` — subset of the six,
canonical order (webmcp, chat, agent-native, a2a, mcp, skills).
agent-browser.dev: `[]` (serves llms.txt only; no declaration file).

## 3. Page design (final)

- Hero: left title + lede unchanged. Right: `Publish your site` (primary)
  + `Read the docs` (ghost). The SUPPORTED CONNECTIONS block is removed —
  a legend floating in the hero explained nothing; the vocabulary now
  lives where it is usable (filter chips) and under the table (tag key).
- Search row: one bar + Scan (56px, 16px gap). Unchanged behavior: words
  filter (domain + description + tag labels), a domain scans.
- Toolbar: hex chips `All · WebMCP · Chat · Agent-native · A2A · MCP ·
  Skills` (left), `N sites` + sort (right). Category chips are gone (no
  data behind them).
- Table: `SITE · DESCRIPTION · TAGS · STATUS`. Identity is one line
  (favicon + domain — the duplicated host subline is dropped). TAGS cell
  renders pills in canonical order, `—` when none. STATUS unchanged
  (green dot, Verified, date, chevron).
- Under the table: the tag key — six one-line definitions, muted.
- Scan result card: probes unchanged; label `native verbs` renamed
  `WebMCP tools`. New footer:
  - file present → `Request listing →` (prefilled GitHub issue:
    title `Listing request: {host}`, body = host, file description,
    scan checklist, repo/ref lines for the install vector).
  - file absent → `Serve /.well-known/agent-webmcp.json to claim this
    domain →` linking the publishing guide.

### Adding-a-page flow (the whole loop)

1. Serve `/.well-known/agent-webmcp.json` (template in the guide).
2. Scan the domain on /sites — the scan is the proof the file is real.
3. Request listing (one prefilled issue, no account, no queue).
4. Operator records it (sites.json + tag flags from the scan); agents
   re-verify on every open — drift quarantines with its reason.

honest note: step 4 is ours today (static catalog, no hosted write
service); the copy must not promise instant self-serving.

## 4. Implementation plan (on approval)

Phase 1 — record + validator
- `website/sites.json`: `has` → `tags: []`; agent-browser.dev → `[]`.
- `website/scripts/verify-sites.mjs`: validate `tags` (array, subset of
  the six, no dupes); drop `has` checks; live mode unchanged.

Phase 2 — page (`website/app/sites/sites-catalog.tsx`)
- Remove connections block from hero.
- Replace category chips with tag chips; keep count + sort.
- Header CAPABILITIES → TAGS; pills from record tags; identity one line.
- Search haystack includes tag labels; pill `title` = key line.
- Tag key under the table.
- Scan card: label rename; CTA branch (request listing / serve-file).
- Issue URL base: `https://github.com/orkestrate-dot-space/agent-webmcp/issues/new`.

Phase 3 — detail (`website/app/sites/[slug]/page.tsx`)
- Render the same pills under the description. Nothing else.

Phase 4 — docs (`website/app/docs/publishing/page.mdx`)
- Fix stale tail (`website/sites/` → `website/sites.json`; single file).
- Describe the real listing flow (scan → request → proof gates); drop
  queue-less phrasing that oversells.

Phase 5 — verify
- `npm run typecheck` clean → `npm run build` (verify-sites runs in
  prebuild). Screenshots 1680 and 390. Live sanity: scan our own domain
  once the deploy serves the file.
- Commits (one concern each, not pushed):
  1. `feat(site): tag record shape plus validator`
  2. `feat(site): tag directory with listing requests`
  3. `docs(site): publishing flow matches the listing`

Deferred, explicitly: index worker / D1 shape (separate surface, untouched);
auth tags; chip counts; topical categories.

Found while researching (fix in passing):
- Our own `/.well-known/agent-webmcp.json` 404s LIVE — repo serves it
  (`website/public/.well-known/`), Vercel is stale. Needs a push/deploy
  on order; not part of these phases.
- Scan probe label drift, fixed in Phase 2.
