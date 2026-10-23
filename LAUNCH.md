# Launch order (slim core → beta)

Sequenced. Each step starts only when the previous is green. Steps marked
[DECISION] wait on an explicit user order naming the target.

## 1. Core remainder
- Admission fake-transport unit suite (validate → collision → inject →
  prove → stamp, plus every skip reason).
- Live proof on slim: multi-session execute, consequential confirm flow.
- Gate green + acceptance replay (`/tmp/prove_slim.py`, `/tmp/probe_slim.py`).

## 2. Canonical install cutover [DECISION: a/b/c]
- (a) quiet window, retry `install.sh`; (b) restart harness servers, cut
  over immediately after; (c) ship beta under a new binary name, leave old
  servers undisturbed. Backup exists at `~/.local/bin/agent-webmcp-exp-bak`.

## 3. Stranger runs (the launch gate)
- Clean-room subagent, skill text only, fresh `HOME`, slim binary.
- Five tasks: connect, author, prove, compose, persist-across-sessions.
- Twice: once driving via agent-browser, once via a second driver.
- Pass, or the skill gets rewritten. This step can still veto the launch.

## 4. Docs (after the stranger passes)
- README: real usage/install/verbs section (describe what was proven).
- CHANGELOG: confirm the beta entry matches the shipped behavior.
- Website: explicitly deferred to post-beta (no site on `main`).

## 5. Final vetting
- Secrets scan across the branch; scratch cleanup.
- Full gate + acceptance replay from a clean checkout.
- Leftover `.agent-webmcp` registry decision: keep for continuity or wipe.

## 6. OSS readiness
- LICENSE ✓, `.gitignore` ✓, README (step 4), CHANGELOG (step 4).
- Minimal CONTRIBUTING or explicit refusal; one bug-only issue template.
- Call on whether `experimental` stays public.

## 7. Launch [DECISION each]
- Push `main`, tag `v0.2.0-beta.1`, announce — each on an explicit user
  order. Production stays stale until then.
