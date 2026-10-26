# Site agents (agent-webmcp reference)

A site with an AI chat gets the same four verbs as any other seam —
open (reveal) → ask (submit + verify) → read (settle-aware) → close
(report state, never lie). No special engine support, no special marks;
the capability `SKILL.md` teaches the order for that site, the files
carry the selectors. Author the agent when the chat is the task interface;
consume natives for everything else.

## The four verbs

- `open {}` — reveal the panel (idempotent). Verify it is on-screen
  (bounding rect, not just DOM presence) before returning `opened …`.
- `ask {question}` — submit one question, verify it landed (message count
  or transcript length moved). Self-open if closed. A duplicate question
  may not re-submit (chats dedupe) — report `no-submit`, don't fake it.
- `read {}` — block until the latest assistant reply settles (text stable
  across polls, ~2.5s; up to ~2 min cap), return the FULL text. One call
  per question; the settle check means the first call already waited.
- `close {}` — close the panel (idempotent). Report state, never lie:
  `closed`, or `open-pinned …` when the rail persists.

## Quirks worth expecting

- Viewport-gated panels: chat rails hide below ~1280px — widen first,
  never debug selectors in a narrow tab.
- Enter-to-send instead of a Submit button; icon-only buttons with empty
  innerText (match `aria-label` too).
- Persistent desktop rails that never dismiss: report `open-pinned`,
  don't fail — suggest navigating away or closing the tab.
- Sequence: open before ask, ask before read, one question per read.

## Typing the agent

Reads return long strings (`{"type":"string"}` + what FULL means);
ask/open/close return status strings (`asked: …`, `opened x=… w=…`,
`closed`/`open-pinned …`). Keep all four `outputSchema.type: string`
with descriptions carrying the format — callers unwrap
`content[0].text` and need no parse.
