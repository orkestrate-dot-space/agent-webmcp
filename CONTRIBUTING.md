# Contributing to agent-webmcp

The changes most likely to be accepted: bug fixes with live proof,
verified native index entries, small focused improvements, docs.

Core changes (polyfill semantics, index schema, verb surface) need design
review before implementation — open an issue first describing the problem
and the proposed shape. PRs that ignore this will likely be closed.

## Before you open a PR

1. `bun run gate` must be green (typecheck + unit tests). CI runs the same gate.
2. Behavior change → `CHANGELOG.md` entry. Breaking change → listed at the top
   of the entry, with the fix.
3. The skill is the product surface. Keep one meaning in one place; no dead
   counts; front-load the trigger in descriptions.

## Commit style

Conventional, scoped, dense: `feat(scope): …`, `fix(scope): …`,
`docs(scope): …`, `chore(scope): …`. One meaning per commit. Base on `main`.

## Site curation (`registry/`)

Every `registry/index.json` domain entry and every `registry/sites/*.js`
script must have a live proof behind it: the tools were read back from the
site's own `document.modelContext` (native) or the script was mounted and
called in a real browser session (custom). No proof, no merge.
Unverifiable platform tool names without a domain list are not index entries.

## What we refuse

Global/universal tools, `@provider` abstractions, custom registries, an
operated crawler, silent telemetry, per-driver logic in the engine, and
secrets in specs or evidence. The engine never touches a browser.
