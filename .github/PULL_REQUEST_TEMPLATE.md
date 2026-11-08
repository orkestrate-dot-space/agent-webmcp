# Pull Request

## What

One meaning per PR. What changed and why it belongs in agent-webmcp.

## Proof

- [ ] `bun run gate` green (paste the tail or link the CI run)
- [ ] Live-verified where it touches the page, a driver, or the index (paste the evidence, not a summary)
- [ ] CHANGELOG.md entry (behavior changes only)

## Checklist

- [ ] Small and focused — one meaning, no drive-bys
- [ ] No browser ownership, no CDP, no per-driver logic in the engine
- [ ] No secrets, credentials, or session tokens anywhere in the diff
- [ ] Conventional title: `feat|fix|docs|chore(scope): …`
- [ ] Registry/site additions include the live proof (see CONTRIBUTING.md)
