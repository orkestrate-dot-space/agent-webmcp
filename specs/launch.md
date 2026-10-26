# Launch TODO — single-browser, tab-scoped agent-webmcp

Goal: strip to the shippable core and ship it. No indexing, no capability
search, no marketplace, no driver framework. Engine + one session model +
registry lifecycle + install path + README table + gate green.

## Model (locked)

- One persistent browser, invisible to the agent. Single durable profile,
  warm logins, shared cookies. No profile names, paths, or launch flags
  in any tool schema. Drivers (agent-browser, any CDP client) connect
  to it; they own no browser.
- A session is a lightweight handle bound to one CDP tab (`targetId`),
  guarded by a per-tab mutex, marked `borrowed` (attached to an existing
  tab) vs `created` (opened a fresh tab in the same window).
- `open(url)` is attach-first with automatic scale-out: reuse an existing
  unlocked tab on that origin (`borrowed: true`); otherwise open a fresh
  tab in the same browser (`borrowed: false`). Parallel agents on one
  origin get parallel tabs automatically.
- Session record is `{ handle, targetId, origin, borrowed }`. No pids,
  no per-session profile dirs, no `/tmp` sprawl.

## 1. Trim the bloat (deletions, one commit)

- [ ] `open`: delete `port`, `profile`, `target`, `headed` from tool schema,
      CLI flags, USAGE, and SKILL.md Drive line. Schema is `{ url }` only.
- [ ] Delete `ToolCtx` (`definition.ts`) and every `_ctx` param.
- [ ] `close` with no handle fails loud (schema + verb, no success-envelope
      error).
- [ ] Delete `/skills/search` prototype page + `skillArt()` (only it uses).
- [ ] Delete `/og/[...path]` route (param ignored, duplicates OG card).
- [ ] Delete `calls.log` append path + `tools.json` snapshot comment in
      `store.ts` (never read, races on write).
- [ ] Fix stale headers in passing: registry "WRITES fail loud",
      reapply "never fixture-invoke", `effect.md` line refs.
- [ ] Verify: `bun run gate` green. Commit: `chore(scope): trim dead surface`.

## 2. Single-browser runtime (one commit, tested) — done `a3f073c`

- [x] `Browser` service owns exactly one persistent Chromium (durable hidden
      profile under the user-data root, survives reboot). Exposes the shared
      connection + `Target.getTargets` / create / attach / close helpers.
- [x] Remove per-open launch, `findFreePort` probe races, `isOurs` pid guard,
      `/proc` kill path, throwaway profile dirs. Kill-by-pid dies entirely.
- [x] First launch creates the browser + default tab; every bare `open`
      reuses the same browser. Daemon/idle policy stated in one comment.
- [x] Verify: `bun test src/sessions src/transport` + gate green.

## 3. Tab-scoped sessions (one commit, tested) — done

- [x] `open(url)`: list targets, bind unlocked matching origin tab
      (`borrowed: true`) else `Target.createTarget` in the same browser
      (`borrowed: false`). Run the pack's declarative `expect` probes only
      (read-only, never mutating fixtures). Write `{ handle, targetId,
      origin, borrowed }`.
- [x] `execute`: acquire per-tab mutex per handle; presence-check the page
      runtime namespace before invoking; auto re-inject pack tools when a
      navigation wiped them.
- [x] `close(handle)`: fail loud on unknown handle. `borrowed: false`
      closes the tab via `Target.closeTarget`; `borrowed: true` leaves the
      tab and drops the record only.
- [x] `list`/resolve auto-prunes records whose `targetId` vanished from
      `Target.getTargets` (user closed the tab manually — silent delete).
- [x] Fixture-mutability rule enforced: mutating fixtures run at register
      or scratch-tab time only; open-time re-proof is preconditions plus
      read-only tools. Unproven strangers quarantine, never auto-execute.
- [x] Verify: new + updated session tests (borrow-reuse, scale-out on lock,
      navigation-wipe heal, borrowed-close leaves tab, dead-tab prune).

## 4. Contract + docs (one commit)

- [ ] SKILL.md: Drive (`open { url }`, borrowed/created, attach-first),
      Memory (record shape, re-apply + quarantine, fixtures rule),
      Law (profiles invisible, handles tab-bound, kill only created tabs).
      Then `bun run gen:skill`, commit both.
- [ ] README: session model paragraph + plugins table (3 rows max).
- [ ] Website docs touched only where they contradict the new model
      (sessions page, commands page). No new pages.

## 5. Ship

- [ ] Full `bun run gate` green. Stranger-ordered runs only after.
- [ ] Push / tag / release on explicit user order only (standing law).

## Explicitly not launch (refuse on sight)

Indexing service, capability-search backend, publish gate, rank/scores,
marketplace, driver-skill system, prose loader, second pack + two-tab demo,
precondition-probe framework beyond `expect`, typed CDP envelope, PLAYBOOK
doc. Each earns its build from a stranger run or console demand. Not before.
