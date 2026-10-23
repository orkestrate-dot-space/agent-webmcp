# agent-webmcp slim-core build plan

## 0. Objective

A connect-only authoring layer for the agentic web, rebuilt clean-room on
this branch. The agent teaches any page a verb, proves it, keeps it — over
any CDP connection (local daemon, cloud browser, sandbox). Seven verbs, one
skill, one dependency. Prior art is preserved on the `experimental` branch;
proven semantics are carried across, code is rewritten, nothing is ported.

## 1. Non-goals (refused on sight)

- No Effect, no framework, no codegen, no `generated/` embeds.
- No engine-map / parallel tool tables. Native WebMCP only; incapable
  browsers fail loud naming the flag.
- No sites index, no `search`/`find` verbs, no workers, no `/sites` catalog,
  no seed/scan/population work. The 68-row D1 ledger stays frozen and
  untouched by this branch.
- No CLI verb surface beyond `serve`/`doctor`. No config files, no env
  parsing, no walk-up beyond the single registry root flag.
- No `unregister` verb (register overwrites, navigation wipes, close drops,
  true deletion is `rm`). No suspect/stale tracking (skip-reasons tell the
  story). No per-origin skill serve in v1 (files on disk + `skill list`).
- No frame-targeted eval (separate track, nothing here depends on it).
- No feature without a stranger run. No behavior change without a changelog
  entry. No push/tag/deploy/release without an explicit user order naming
  the target.

## 2. Boundary rule (load-bearing)

Code owns what must be identical every time (protocol, validation, budgets,
locks, envelopes). Language owns everything requiring judgment (what to
build, what's wrong, what next). Interface between them is narrow: schemas
in, verdicts plus human-readable reasons out. Every deterministic gate must
name the fix, or the boundary is broken.

## 3. Outcomes and build sequence

Build in this order. Each item states its outcome, the code/language split,
and its acceptance. Nothing starts until the previous item's acceptance is
green.

### 3.1 Transport — "bytes move over CDP"

- Code: WebSocket connect, id-routed send with per-call timeout, close.
  `GET /json/version` resolve with portless-localhost inheritance fix.
  Minimal domains: `Target.createTarget` / `attachToTarget` / `closeTarget`,
  `Runtime.evaluate` (await + returnByValue), native WebMCP trio
  (`WebMCP.enable` → `toolsAdded` burst, `invokeTool` → `toolResponded`,
  `cancelInvocation` on stall). Auth material per-connection, never stored.
- Language: none.
- Accept: attach to a local daemon, a plain `chrome --remote-debugging-port`
  instance, and one cloud CDP URL; evaluate round-trips; missing domain
  degrades to empty catalog, never a hang.

### 3.2 Sessions — "the agent has a live tab"

- Code: `open {url, cdp?}` mints a fresh tab, records
  `{handle, targetId, origin}` as one JSON file, returns
  `{handle, toolCount, cdpUrl}`. `close {handle}` kills our tab, drops the
  record; unknown handle fails loud. `list` shows live handles. Per-tab
  mutex (counter + queue, same process; lock files only if cross-process
  harnesses demand it). Dead-tab drops the record on next touch; dead
  browser fails loud and keeps it.
- Language: agent decides which browser and when to open/close.
- Accept: open → attach → close leaves no tab; concurrent executes on one
  tab serialize; dead tab drops, dead browser keeps.

### 3.3 Registry + validation — "files are the library"

- Code: layout `<root>/<origin-slug>/{origin.txt, <name>/{spec.json,
  body.ts}}`. Roots: `--registry DIR` pin, else project overlay over the
  user-global main store (reads merge project-first, writes go resolved
  root). `origin.txt` slug-collision guard, symlink escape refusal.
  `checkSpec`: closed object schema, honest names, non-empty description,
  no `eval`/`new Function`, size cap — pure, same bytes → same verdict.
  Bodies are TS; transpile at load (Bun transpiler, plain JS passes
  through); untranspilable = corrupt file, named, never injected.
- Language: agent authors all three files guided by the skill.
- Accept: unit suite on fixtures (good shape passes, each bad shape names
  its reason); legacy `body.js` fallback reads.

### 3.4 Engine verbs, native-only — "missing verbs exist and are proven"

- Code: ONE snippet family on `document.modelContext` (register / list-via-
  burst / invoke). `reapply` on open: skip-if-live (never overwrite a
  native verb), else validate → inject → presence-check → stamp
  `{verified, bodyHash}` for non-consequential, hand-prove path for
  consequential. Bad shapes skip with reasons in `skipped[]`, files kept.
  `register` verb: pre-check native catalog FIRST — name collision fails
  loud naming both claimants — then validate → inject → fixture-prove →
  persist. Fixture failure rolls back live + files, reason names the fix.
- Language: agent does recon, seam choice, body logic, spec naming,
  fixture input. The fixture is the handshake: agent writes it, code runs
  it, neither does the other's half.
- Accept: register → prove → invoke → close → re-open re-applies silently;
  changed bytes re-prove; planted `eval()` body skips with reason, nothing
  injected; native-collision attempt fails loud.

### 3.5 Execute — "code over verbs returns a value, safely"

- Code: `execute {handle, code, confirm?, budgets?}` runs agent JS in a
  `Bun.Worker` behind a `tools.NAME` proxy (flat) / `alias.NAME`
  (multi-session). Host bridges each call to CDP invocation. Budgets:
  wall-clock, max calls, max input chars, max result chars. Consequential
  gate: `ask` requires `confirm:true` in the call, `deny` blocks, `allow`
  runs free — verb-level, the one layer that sees inside the code string.
  Every failure is a coded reason + human fix (`NO_SUCH_TOOL`,
  `NEEDS_CONFIRM`, `PAGE_REFUSED` with recovered page text, `STALE_DRAFT`
  after nav wipe + heal attempt). Envelope always carries `untrusted:true`.
- Language: agent composes, sequences, repairs from reasons.
- Accept: multi-verb composition in one run; budget breaches end the run,
  never hang it; each error reason demonstrably self-healed by the agent
  in a live run.

### 3.6 Skill — "a stranger is competent in one read"

- Language (the product): core `SKILL.md` (drive loop: open → list →
  compose; author loop: recon → write files → register → prove; compose
  shape), plus `author.md`, `drivers.md` (one appendix table, no
  modelContext prerequisite language), `memory.md` (files-first registry).
  Open standard format (`agentskills.io` frontmatter: `name` matching
  dirname, `description` with trigger keywords) so the same files load in
  opencode, pi, and Claude unmodified. Core under ~2k tokens; per-origin
  `SKILL.md` files live beside tools; references load on demand
  (progressive disclosure).
- Code: `skill show` serves core + refs from disk (no codegen, no embeds);
  `skill list` inventories origins; shape-check in gate that served text
  equals the files.
- Accept: a stranger with only the skill text completes the Section 7
  stranger run. Skill edits never require a code release.

### 3.7 Install, gate, doctor — "green means shippable"

- Code: `install.sh` (interactive when TTY, env-driven otherwise; browser
  stages only, no config writes, dirty-tree refusal). `gate`: typecheck +
  unit tests + skill-shape check + schema-object check + desc-budget.
  `doctor`: reachability + `release: beta` + policy source.
- Accept: `gate` green on a clean checkout; install script proven
  end-to-end in a scratch env.

## 4. Verb surface (final, seven)

`open`, `list`, `execute`, `register`, `close`, `skill`, `doctor`.
Schemas stay minimal; every high-value input field carries a `description`
annotation that flows into the served `inputSchema`. No other verbs ship.

## 5. Line budgets (aspiration, not law)

main+args+serve ~80 · transport ~220 · sessions ~150 · registry ~220 ·
register+prove ~120 · execute+runner+gate ~270 · skill+doctor ~110.
≈1,200 lines, one dependency (`@modelcontextprotocol/sdk`), ~50-line
`Result` kit replacing Effect. If a module can't be rewritten in one
session, it's too big.

## 6. Website (same knife)

Teach-the-loop pages only: installation, quick-start, commands,
custom-tools, sessions, security, troubleshooting, changelog. No catalog,
no search-index machinery, no OG routes, no `/sites`. Every page re-proven
against the slim binary before it ships. Great means true.

## 7. Launch gates (in order)

1. `gate` green on clean checkout.
2. Acceptance replay: the two live prove scripts (flagged + plain-capable
   browser) pass against the slim binary.
3. Stranger verification from TWO harnesses (opencode + pi) using only the
   skill: connect, author, prove, compose, persist across sessions.
4. Fresh `CHANGELOG.md`: one beta entry, breaking changes listed atop.
5. Push / tag / release / deploy — each on an explicit user order naming
   the target. Production stays stale until then.
