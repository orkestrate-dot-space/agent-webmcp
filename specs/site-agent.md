# Site-agent invocation: outcome spec

Status: PROPOSAL — no code. Awaiting build approval (engine surgery rule).
Scope: v1, one site (mintlify.com), one transport (deep-link + panel read).

## Goal (one sentence)

Any harness speaking agent-webmcp can ask a contracted site's
*reasoning* agent a question and get back its worked, cited answer —
single call, no credentials, no stored state, honest failures.

## Non-goals (explicit, to stop scope creep at review)

- No auth-walled sites, no login flows, no credential storage.
- No headless/datacenter stealth: bot-gated backends stay gated.
- No multi-site: second row needs its own sign-off + spec delta.
- No A2A transport: no counterparty publishes an agent card.
- No keyed REST tier: parked until the key decision lands.
- No latency SLA: the site streams at its own pace; the caller
  controls `timeoutMs` and gets a bounded wait, not a fast answer.

## Outcomes (acceptance-tested, no mechanism in the wording)

### O1 — Cold caller gets a worked, cited answer through engine verbs only
**Given** a harness that knows nothing of mintlify except the engine,
**when** it opens a mintlify.com/docs URL, lists, and calls
`ask_mintlify({question})` with a docs question,
**then** it receives the assistant's answer *with citations to docs
pages*, having supplied nothing but the question.
**Verify by:** the row's own example query, run verbatim, returning
cited content. (Holds today for first-turn asks.)

### O2 — Follow-ups keep the thread
**Given** an open session that already asked one question,
**when** the caller asks a follow-up that depends on it
(e.g. “what is X” → “how do I install it” with “it” = X),
**then** the answer resolves the reference correctly.
**Design (the spec's one judgment call): mint the assistant tab ONCE
per session and reuse it** — first call creates it, later calls
reattach, type into the live panel, and send. Close drops it with
the session. No per-call tabs.
**Verify by:** the two-question pronoun test above, asserted on
`answer-2 contains X's install path`.
**Status:** NOT implemented (current driver mints per call =
single-turn). This spec's main new requirement.

### O3 — Honest failure, never a confident nothing
**Given** the panel is missing/changed, the browser refuses minting,
or the answer never settles,
**then** the caller gets a `ToolFailure` naming the cause *and* the
fix (`mint` → use a minting browser; `page` → re-verify the row;
timeout → raise `timeoutMs`, partial kept) — never an empty string,
never a hallucinated answer, never a search-index substitute.
**Verify by:** deliberately breaking each precondition once
(kill the tab mid-run; close the ?assistant= path; shrink the
timeout to 1s) and asserting the error taxonomy.

### O4 — Zero credentials, zero login, zero stored state
**Given** a fresh browser profile and no keys in env,
**when** O1 runs,
**then** it succeeds, and afterwards no driver artifact exists on
disk (user-tab session records are engine-normal; the assistant
tab leaves no record, no cookie jar, no transcript file).
**Verify by:** clean-profile run + `ls` of the session dir showing
only the caller's own handle.

### O5 — No silent conflation of index and agent
**Given** both retrieval verbs (`search_docs`, hosted MCP) and
`ask_*` are visible in one session,
**then** a caller can tell which is which from name + description
alone, and `ask_*` never returns raw index snippets as its answer.
**Verify by:** the poem test — an out-of-scope creative request
must come back a *refusal with redirect*, never search results.

### O6 — Row-traceability
**Given** any listed row,
**then** its `example` runs verbatim through its verb and returns a
cited answer, and its `proof.verified` names a real run date.
**Verify by:** re-running the row example on sign-off day.
(This is the sign-off gate, mechanized.)

## Driver contract (what “fixed per-transport driver” means)

- One code path per transport, audited, no page-supplied code ever.
- v1 deep-link driver: mint tab → wait for panel mount → type
  question (native value setter + input event) → click send →
  settle-detect (same text twice, >200 chars) → close on session
  close, re-mint on dead tab.
- Timeouts: every wait is bounded by the caller's `timeoutMs`
  (minus a mint/close margin); nothing waits forever.
- Multilingual/filters: out of v1 scope (fields exist in the API
  but are not exposed until a row needs them).

## Open items (decisions, not research)

1. Build approval for the session-pinned-tab driver (engine change).
2. Mintlify row sign-off (row is live, sign-off outstanding).
3. Keyed REST tier: parked until the key question is answered.
4. `/agents` page mechanics beyond the row (intake, re-verify loop).
5. Whether failure taxonomy needs a dedicated `reason` enum beyond
   ToolFailure message conventions.
