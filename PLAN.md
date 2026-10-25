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
- No `register` / `unregister` verbs. Authoring is skill + files the agent
  writes with its own tools; the engine only reads, validates, injects, and
  proves (lazy admission). This is the agent-browser `webmcp-gen` pattern:
  the loop lives in the skill, the core only loads.
- No registry configuration of any kind: no `--registry` flag, no roots,
  no pin, no project overlay, no `origin.txt`, no symlink walks. One fixed
  directory (Section 3.3). Tests isolate via `HOME`, not flags.
- No sites index, no `search`/`find` verbs, no workers, no `/sites` catalog,
  no seed/scan/population work. The 68-row D1 ledger stays frozen and
  untouched by this branch.
- No CLI verb surface beyond `serve`/`doctor`. No config files, no env
  parsing.
- No suspect/stale tracking (skip-reasons tell the story). No per-origin
  skill serve in v1 (files on disk + `skill list`).
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

### 3.3 Registry — "files are the library, in one fixed place"

- Code (read-mostly): ONE directory, always
  `~/.local/share/agent-webmcp/registry/` (from `HOME`; tests isolate by
  setting `HOME`, never by flag). Layout:
  `<origin-slug>/{SKILL.md, <capability>/{SKILL.md, <name>/{spec.json, body.ts}}}` plus optional `evidence/`.
  Capabilities group an origin's verbs (chat agent, docs pack, page verbs);
  a verb lives in exactly one capability — duplicates across capabilities
  fail loud naming both. `SKILL.md` at each level teaches that level: the
  origin skill names capabilities, each capability skill teaches its verbs. The origin lives inside `spec.json` and is
  verified on load — no `origin.txt`. The engine READS (load, validate,
  inject, stamp); the agent WRITES (with its own file tools, taught by the
  skill) — so there is no save path, no write root, no symlink walk, just a
  normalization check on reads. One stamp function writes back
  `{verified, bodyHash}` after proof (~15 lines).
- Spec fields, minimal: `name, description, inputSchema, outputSchema` + optional
  `annotations?, fixtureInput?, consequential?`.
  (`verified?`/`bodyHash?` live in engine-owned `stamp.json`, beside the
  agent's files, never inside them.) `outputSchema` is required: the schema
  of the `text` payload (string verbs: `{"type":"string"}`; objects:
  closed + `JSON.stringify`d into text) — surfaced in `list`/`catalog`,
  shape-gated at write.
  `checkSpec`: closed object schema, honest names, non-empty description,
  no `eval`/`new Function`, size cap — pure, same bytes → same verdict.
  Bodies are TS; transpile at load (Bun transpiler, plain JS passes
  through); untranspilable = corrupt file, named, never injected.
- Language: agent authors all files guided by the skill. Sharing between
  projects is by pack directories (copy), not overlays — one store means
  memory is cross-project by default.
- Accept: unit suite on fixtures (each bad shape names its reason); legacy
  `body.js` fallback reads; a second `HOME` sees a disjoint store.

### 3.4 Admission — "missing verbs exist and are proven" (no authoring verbs)

There is no `register` verb. The agent writes `spec.json` + `body.ts`
directly (skill-taught, agent-browser `webmcp-gen` pattern); the engine
admits files lazily at first use:

- Code, on `execute` naming an unknown verb (or `open` re-applying):
  load → `checkSpec` → native-catalog collision check FIRST (same name
  fails loud naming both claimants) → inject → prove → stamp.
  Prove: `fixtureInput` present and non-consequential → run it, success
  stamps `{verified, bodyHash}`; consequential → the `confirm:true` call
  itself is the proof. No fixture and no confirm → fail naming both
  proving paths, files kept. `open` fast-path: stamped + unchanged bytes
  re-inject silently; changed bytes re-prove; unstamped admittance is
  marked `unverified` in `list`.
- Language: agent does recon, seam choice, body logic, spec naming,
  fixture input, and all repairs from skip/proof reasons. The fixture is
  the handshake: agent writes it, code runs it, neither does the other's
  half.
- Accept: write files → first `execute` proves → close → re-open
  re-applies silently; changed bytes re-prove; planted `eval()` body
  skips with reason, nothing injected; native-collision attempt fails
  loud; unprovable tool fails naming the two proving paths.

### 3.5 Execute — "code over verbs returns a value, safely" (the codemode interface)

`execute` IS the harness-codemode exposure: one MCP tool taking a string of
plain JS, no harness-specific constructs — identical from opencode, pi,
Claude Code, or anything speaking MCP. Discovery is `list` (per-session
verbs + full schemas via `list tool:<name>`); composition is the
`tools.NAME(args)` dialect, flat for one session, `tools.ALIAS.NAME` for several (one root object).
In-dialect discovery: bare `search({query, namespace?, limit?})` filters
the run's verbs mid-code, `catalog(path?)` renders TS signatures;
`console.log/warn/error` returns as `logs` in the envelope.
Page verbs live INSIDE this dialect, never as top-level harness tools:
flat MCP entries would push every schema into the model context on every
turn, bypass budgets and the envelope, and depend on mid-session catalog
refreshes no harness guarantees.
No search verb: the skill teaches open → list → compose, and anything
worth composing twice becomes a file (Section 3.4).

- Code: `execute {handle, code, confirm?, budgets?}` runs agent JS in a
  `Bun.Worker` behind the `tools` proxy. Host bridges each call to CDP
  invocation, including lazy admission (Section 3.4) for names that are on
  disk but not yet live. Budgets: wall-clock, max calls, max input chars,
  max result chars. Consequential gate: `ask` requires `confirm:true` in
  the call, `deny` blocks, `allow` runs free — verb-level, the one layer
  that sees inside the code string.
  Every failure is a coded reason + human fix (`NO_SUCH_TOOL`,
  `NEEDS_CONFIRM`, `PAGE_REFUSED` with recovered page text, `STALE_DRAFT`
  after nav wipe + heal attempt). Envelope always carries `untrusted:true`.
- Language: agent composes, sequences, repairs from reasons.
- Accept: multi-verb composition in one run; budget breaches end the run,
  never hang it; each error reason demonstrably self-healed by the agent
  in a live run.

### 3.6 Skill — "a stranger is competent in one read" (the other half of the product)

Anything instructable lives here, not in code — following the
agent-browser pattern (tiny discovery stub + version-matched served
content + specialized skills + whole authoring loops as skill text):

- Language (the product): core `SKILL.md` as discovery stub — drive loop
  (open → list → compose), compose dialect, budgets, envelope meaning —
  under ~100 lines with trigger-rich description. The AUTHOR skill is the
  star: full authoring doctrine (recon via driver, traffic mining, seam
  choice, file layout, closed schemas, envelope contract, fixture
  discipline, debug playbook with stale-tab and sequence-guard rules,
  evidence as trust). One driver appendix table (snapshot/eval/HAR per
  driver). Open standard format (`agentskills.io` frontmatter) so the same
  files load in opencode, pi, and Claude unmodified — distribution by
  conformance, not invention.
- Code: `skill show` serves core + refs from disk (no codegen, no embeds);
  `skill list` inventories origins; `skill search` answers "do I already
  have an agent for X?" across every saved verb. Gate shape-checks served
  text equals the files. No `install` action in v1 (packs move by directory copy).
- Accept: a stranger with only the skill text completes the Section 7
  stranger run — including authoring a file-tool with NO authoring verb.
  Skill edits never require a code release.

### 3.7 Install, gate, doctor — "green means shippable"

- Code: `install.sh` (interactive when TTY, env-driven otherwise; browser
  stages only, no config writes, dirty-tree refusal). `gate`: typecheck +
  unit tests + skill-shape check + schema-object check + desc-budget.
  `doctor`: reachability + `release: beta` + policy source.
- Accept: `gate` green on a clean checkout; install script proven
  end-to-end in a scratch env.

## 4. Verb surface (final, six)

`open`, `list`, `execute`, `close`, `skill`, `doctor` — and the reasoning:
`open`/`close` bracket the tab we mint (we reap what we create);
`list` is codemode discovery (names + schemas to compose with);
`execute` is codemode itself plus lazy admission; `skill` teaches both;
`doctor` reports reachability. Schemas stay minimal; every high-value
input field carries a `description` annotation that flows into the served
`inputSchema`. No other verbs ship.

## 5. Line budgets (aspiration, not law)

main+args+serve ~80 · transport ~220 · sessions ~150 (record is
`{handle, targetId, origin}` — authored lists live on disk, not in the
record) · registry ~150 (read + checkSpec + inject + stamp; no save path,
no roots, no guards) · admission ~80 (folded into execute/open paths) ·
execute+runner+gate ~270 · skill+doctor ~100.
≈1,050 lines, one dependency (`@modelcontextprotocol/sdk`), ~50-line
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
