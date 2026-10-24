# Author doctrine (agent-webmcp reference)

When the page lacks the verb the task needs, author it. The engine admits
files; all judgment here is yours.

## 1. Recon (driver, with eyes)

Open the page in your driver, snapshot it, act out the real user flow.
Record traffic (HAR). Mine first-party JSON endpoints out of the noise:
request/response shapes, auth pattern (cookies, XSRF headers, token
exchange), sequence guards. Check the native catalog (`list`) first —
consume native verbs, never rebuild them. Pick the seam: same-origin API
wins; DOM scraping loses; cross-origin walls get named, not hacked.

## 2. Files

Root is fixed: `~/.local/share/agent-webmcp/registry/`
(isolate tests with `HOME`, never with flags).

```
<origin-slug>/{SKILL.md, <name>/{spec.json, body.ts, stamp.json, evidence/}}
```

`<origin-slug>` is the lowercased host with non-alphanumerics as `-`.
`SKILL.md` (per origin, ≤4KB) teaches future sessions: when to use these
verbs, in what order, what rules apply — it is served back to you on every
`open` as `briefing`. `stamp.json` is engine-owned (proof record); never
hand-write it. `evidence/` holds fixture transcripts and eval notes —
trust is the reader's informed decision.

## 3. Spec (taste lives here)

`{name, description, inputSchema, origin, annotations?, fixtureInput?,
consequential?, outputSchema?}`. Names are goals (`notebook_write_cells`, not
`call_api`). Descriptions say what it does plus what it returns — blind
agents decide on this alone. `inputSchema` is a closed object: real
`required`, `additionalProperties: false`. `outputSchema` declares the
return contract (closed object recommended) — it surfaces in `list` and
`catalog` so callers see the shape before the first call. `fixtureInput`
is a provable example — the proof runs it, so it must pass. Consequential verbs
(mutations) carry `consequential: true` and skip fixtures: your confirmed
call is the proof. `origin` is the page origin, exactly.

## 4. Body (contract, not style)

One function: `fn(input)` returns `{content:[{type:'text',text}]}` and
nothing else. Self-contained: no imports, no network beyond the page's own
origins, no `eval`/`new Function`. Return the envelope — a raw number or
string fails the shape check at the reader, not at author time, so get it
right now. Bodies may use TypeScript; plain JS passes through untouched.
Driver idioms (`page.*`, `$x`) pasted as bodies are flagged warn-only —
page bodies run in the page, drivers act from outside.

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

## 8. Site agents (a pattern, not a privilege)

A site with an AI chat gets the same four verbs as any other seam:
open (reveal the panel) → ask (submit + verify) → read (settle-aware) →
close (report state, never lie). No special engine support, no special
marks — the per-origin `SKILL.md` teaches the order, the files carry the
selectors. Chat quirks worth expecting: viewport-gated panels (widen
first), Enter-to-send instead of a Submit button, icon-only buttons with
empty innerText (match `aria-label` too), persistent desktop rails that
never dismiss (report `open-pinned`, don't fail).
