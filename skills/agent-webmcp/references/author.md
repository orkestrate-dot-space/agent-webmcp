# Author doctrine (agent-webmcp reference)

When the page lacks the verb the task needs, author it. The engine admits
files; all judgment here is yours.

## 1. Recon (driver, with eyes — because the engine is blind)

The engine never sees the page: no snapshot, no click, no network log.
Your driver is the eyes and hands; recon happens there because selectors,
sequence guards, and auth only exist there. Every guess you skip in recon
costs an inject+prove cycle later — ten minutes with the driver saves five
body rewrites.

Open the page in your driver, snapshot it, act out the real user flow as
the user would: click the actual buttons, submit the actual form. Record
traffic (HAR) and mine first-party JSON endpoints out of the noise:
request/response shapes (these become your schemas), auth pattern (cookies,
XSRF headers, token exchange), sequence guards (what must be read before a
write succeeds). Check the native catalog (`list`) first — consume native
verbs, never rebuild them. Pick the seam: same-origin API wins (stable,
typed); DOM scraping loses (brittle, restyle-broken); cross-origin walls
get named in the skill, not hacked in the body. Write down the flow order,
the guards, and the exact shapes — §3 turns them into schemas, §4 into code.

## 2. Files

Root is fixed: `~/.local/share/agent-webmcp/registry/`
(isolate tests with `HOME`, never with flags).

```
<origin-slug>/{SKILL.md, <capability>/{SKILL.md, <name>/{spec.json, body.ts, stamp.json, evidence/}}}
```

`<origin-slug>` is the lowercased host with non-alphanumerics as `-`.
One origin holds one or more **capabilities** — a chat agent, a docs
agent, a page-verb set — each a directory with its own `SKILL.md`
(doctrine + rules for exactly those verbs). The origin `SKILL.md` says
which capabilities exist and when to reach for each. A verb lives in
exactly one capability; the same name in two capabilities fails loud
at load — rename, don't shadow. `open` serves the origin skill plus the
capability skills behind the re-applied verbs as `briefing` (bounded).
`stamp.json` is engine-owned (proof record); never hand-write it.
`evidence/` holds fixture transcripts and eval notes — trust is the
reader's informed decision.

Install a capability by copying directories; remove one by deleting
them. No registration, no restart — next `open` discovers, first call
admits:

```bash
cp -r ./my-agent/eve-dev/chat ~/.local/share/agent-webmcp/registry/eve-dev/chat
rm -rf ~/.local/share/agent-webmcp/registry/eve-dev/chat/stale_verb
```

Budgets the briefing enforces: origin `SKILL.md` ≤2KB, capability
`SKILL.md` ≤2KB, assembled `briefing` ≤6KB. Write under them.

## 3. Spec (schemas are the codemode types — taste lives here)

`{name, description, inputSchema, origin, outputSchema, annotations?, fixtureInput?,
consequential?}`. The two schemas exist so codemode never guesses: an
agent reads `catalog(path)` and gets `signature` (the input call-shape),
`returns` (the output type-hint), `description`, plus both raw schemas —
then writes `await tools.NAME(typedArgs)` with full confidence. Closed
schemas mean no hallucinated fields; `required` means no missing-field
failures at runtime. If the schema is vague, every caller pays in type
errors — so write it like a type declaration, not a comment.

- `inputSchema`: the args type. Closed object, real `required`,
  `additionalProperties: false`. Every property gets a `type` and, where
  it matters, a `description` (units, bounds, format).
- `outputSchema`: the type of the `text` payload inside the envelope —
  required. String verbs (read, open, ask, close) declare
  `{"type":"string","description":"..."}` describing the string. Object
  data is `JSON.stringify`d into `text` and declared closed
  (`type:"object"` + real `properties` + `additionalProperties:false`).
  It surfaces in `list tool:` and `catalog()` before the first call, so a
  blind agent codes against it; the capability `SKILL.md` documents the
  shape in words too.
- Calls resolve to the **envelope**, not the payload:
  `await tools.NAME(args)` → `{content:[{type:'text',text}]}`. Unwrap with
  `const text = out.content[0].text`, then `JSON.parse(text)` when
  `returns` says `object{...}`. Raw strings need no parse. Never guess the
  shape — `catalog(path)` first, destructure per `returns`, branch on the
  coded failures (§6) instead of retrying blind.
- Names are goals (`notebook_write_cells`, not `call_api`). Descriptions
  say what it does plus what it returns — blind agents decide on this
  alone. `fixtureInput` is a provable example — the proof runs it, so it
  must pass. Consequential verbs (mutations) carry `consequential: true`
  and skip fixtures: your confirmed call is the proof. `origin` is the
  page origin, exactly.

## 4. Body (typed TypeScript, not style)

Write the body the way you write application code: interfaces first,
envelope last. Mirror the schemas as types so mismatches fail in your
head, not in the page:

```ts
interface Input { question: string }
type Output = string; // objects: interface Out { reply: string; settled: boolean }
const ok = (text: string) => ({ content: [{ type: "text" as const, text }] });
(async (input: Input): Promise<{ content: Array<{ type: "text"; text: string }> }> => {
  const q = (input.question ?? "").trim();
  if (!q) throw new Error("ask: empty question — pass {question}");
  // ... drive the DOM / call the page's own API (recon flow order here)
  return ok(q); // objects: ok(JSON.stringify(data satisfies Output))
})
```

Rules the template already follows: `Input` mirrors `inputSchema`
(field for field, required for required); `Output` mirrors
`outputSchema` (string stays raw, objects go through `JSON.stringify`);
one function returning the envelope and nothing else — a raw number or
string fails at the reader, so `ok()` it. `throw new Error("verb: what
broke — what to do")` to refuse: your message becomes the `PAGE_REFUSED`
reason the caller branches on. Self-contained: DOM lib only, no imports,
no network beyond the page's own origins, no `eval`/`new Function`.
Types erase at load (transpile) — they are author-time safety, the page
runs JS. Driver idioms (`page.*`, `$x`) pasted as bodies are flagged
warn-only — page bodies run in the page, drivers act from outside.

## 5. Prove

First `execute` naming the verb admits it. Non-consequential with a
fixture: the run proves and stamps. Consequential: call with
`confirm:true`; success stamps, refusal doesn't. No fixture and no confirm:
the engine tells you both proving paths — pick one. Failure reasons name
the fix; fix the files, call again. Re-open re-applies: stamped + unchanged
bytes inject silently, changed bytes re-prove, bad files stay out with
reasons.

## 6. Debug playbook

- Shape errors → read the reason, fix the file. Never argue with the gate.
- `PAGE_REFUSED` → the page changed; mirror the live flow in your driver
  first, then rewrite the body.
- Sequence guards → read before every write; the server's sequence wins.
- Stale tabs → after you write, a human's open tab is stale: say so,
  never silently overwrite.
- Envelope bugs → if the call returns garbage, the body returned garbage.
  Read the shape contract again.

## 7. Compose, don't proliferate

One verb per human goal. Two verbs that always run together are one verb.
Delete (rm the dir) what the page outgrew — dead files still admit.

## 8. Site agents (a pattern with its own skill)

Chat agents (open → ask → read → close) carry enough doctrine —
order, settle-waiting, viewport quirks, rail-that-never-closes — to deserve
their own file: `skill show name=site-agents` before authoring one. The
engine gives no privilege; the pattern is all skill.
