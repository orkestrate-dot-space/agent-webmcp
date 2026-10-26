# agent-webmcp

Connect-only authoring layer for the agentic web (Bun + TypeScript, MIT).
Six MCP verbs; page verbs live inside the `execute` dialect, never as flat
harness tools.

## Rules

- `bun run gate` green before every commit (typecheck, unit tests,
  skill-shape, schema-object, desc-budget). CI runs the same gate.
- Conventional commits: `feat|fix|docs|chore(scope): …`. Breaking in beta
  goes atop the `CHANGELOG.md` entry, with the fix. No migrations until 1.0.
- The engine reads files and owns no browsers; the agent writes files with
  its own tools. Skills are the product — keep one meaning in one place.

## Layout

- `src/` — engine (server, sessions, transport, admission, catalog,
  registry, runner)
- `skills/agent-webmcp/` — the skill half (core + references)
- `scripts/gate.ts` — the gate · `workers/sites-index` — directory API
- `docs/` research · `specs/` launch spec · `space/` boards (previews ignored)

## Never commit

Authored verbs (`spec.json`/`body.ts`/`body.js`/`stamp.json` — they live in
the local registry, not here), secrets or credentials, `.env*` files,
build output (`dist/`, `.cloudflare/`), screenshots.
