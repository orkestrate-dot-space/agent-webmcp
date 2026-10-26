# Launch plan FINAL (plain)

Principle: read theirs, add ours (proof). Their files say
what exists; our file plus our proof say what an agent can
actually do with it, with proof attached.

## Release channel: beta

`0.1.0-beta.x`. Breaking changes allowed per release,
listed at the top of each changelog entry, no migration
scripts until 1.0, no silent breaks (removed verbs/flags
fail loud naming the replacement). Explicit everywhere:
hero badge, `--version`, `doctor` (`release: "beta"`),
SKILL.md, quick-start, changelog policy.

Update path: `agent-webmcp upgrade --yes` (source
upgrade in place: pull → gate → compile → reinstall
with `.prev` backup; refuses dirty trees). No checkout
→ re-run install.sh. Per-release binaries attach to
GitHub releases from 1.0 on.

## v1 scope (what beta promises)

Open/list/search/find/execute/close/register/unregister
over live tabs; file-backed per-origin registry with
fixture proof and quarantine; index consult
(open advisory + `find`, honors `index.enabled`);
user config (browser/index/permissions); docs + recipes
that all go green. Everything else is post-beta.

## Standards we read (all of them)

`llms.txt` · `/.well-known/api-catalog` RFC 9727 ·
`/openapi.json` + conventional paths ·
`/.well-known/mcp/server-card.json` (SEP-2127 Final) ·
`/.well-known/oauth-protected-resource` RFC 9728 ·
`/.well-known/agent-card.json` A2A ·
`/.well-known/agent-skills/index.json`. Plus our
`/.well-known/agent-webmcp.json` (description, skills
pointer, agents[] with protocols + optional auth, hooks).
We read every one; we duplicate none.

## Done already

- Engine reads the site file on open (tab reads itself,
  engine fetches nothing). Live-proven, gate green.
- Quarantine removes stranger verbs page-side.
- `/sites` + scan (all seven signals), publishing guide +
  served schema, our own file served, build-time validator
  (offline always, live drift mode on flag).
- Skill split, MCP skill-first, `/skills` removed entirely.
- Gate green, binary HEAD. All committed locally, unpushed.

## Now, in order

1. Cloudflare index worker. Site records, no scores, no
   marketplace.
2. Seed the index. Official standards files plus webmcp
   sites. After the worker exists.
3. Engine consults the index. Open reads the record,
   search ranks descriptions. Read-only; proof on open
   still gates everything. After the worker exists.
4. Verify end-to-end. Publish files on a domain, scan,
   map, list, drift, quarantine with reason. Our domain
   plus N proven sites. Launch gate: no green run here,
   no release.

## Last, together

5. Authoring guide, webmcp-gen-aligned (fixtures,
   invalid-state, contaminated-output, eval notes) —
   NEXT, crafted with operator (tool authoring shapes
   the register contract).
6. Tool + skill installation: GitHub installs
   (`skill install owner/repo`), per-site SKILL.md
   packs. (Distribution for what 5 teaches.)
7. Docs pass: quick-start, commands, configuration,
   recipes — every page green against the beta.

## Then (on explicit order)

8. Push main, tag v0.1.0-beta.x, release binary,
   announce file plus sites live.

## Never at launch (post-beta)

Scores, marketplace, backend search beyond the index,
a2a/mcp clients, agent-native caller, integrity substrate
beyond digests plus pins, TS tool bodies, local-model
tool calls, `upgrade` from release assets.
