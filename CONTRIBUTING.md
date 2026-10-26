# Contributing to agent-webmcp

Beta channel (`0.2.0-beta.x`): breaking changes ship in any beta — each one
listed at the top of its `CHANGELOG.md` entry, with the fix. No migrations
until 1.0.

## Ground rules

- Plan, get approval, execute, verify. No code before an approved plan on
  multi-area work.
- One review round is standard; a second only on BLOCKING findings.

## Before you open a PR

1. `bun run gate` must be green — typecheck, unit tests, skill shape,
   schema-object, desc-budget. CI runs the same gate.
2. Behavior change → `CHANGELOG.md` entry. Breaking change → listed at the
   top of the entry, with the fix.
3. Skill edits: the skill is the product. Keep one meaning in one place,
   no dead counts, front-load the trigger in descriptions.

## Commit style

Conventional, scoped, dense: `feat(scope): …`, `fix(scope): …`,
`docs(scope): …`, `chore(scope): …`. One meaning per commit.

## What we refuse

Effect/frameworks, codegen, registry flags or overlays, authoring verbs
beyond files, engine-owned browsers, secrets in specs or evidence. The
boundary rule binds: code owns what must be identical every run; language
owns all judgment.
