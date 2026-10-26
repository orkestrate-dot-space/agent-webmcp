# /sites page — proposal and spec

## Purpose

List agent-ready sites: what each site declares in its
`agent-webmcp.json` (skills, agents, hooks) plus what we proved
(verbs, proof time, quarantine state). Tool-vs-territory split:
`/skills` holds executable tools, `/sites` holds where they work.

## Data per entry

`origin, slug (derived), description ≤140ch, agents[] (description,
url, protocol), hooks (flow → selectors), verbs[] (name + proof
time), files + digests, repo @ pinned ref, lastVerified,
state (listed | quarantined + reason)`.

## Layout (top to bottom)

1. Nav: `agent-webmcp / sites`, links Docs, Skills, GitHub, Search.
2. Hero: H1 `Browse Sites`, one-line sub stating the mechanism
   (pulled from each domain's file, proven on open), one CTA
   `Publish your site` → the file guide. No second CTA.
3. Scan box: domain input + Scan. Client-side preview only.
   Hint below: preview rule + claim rule, secondary text.
4. Filter tabs, one axis matching the file: All, Skills, Agents,
   Hooks. No counts until real. No state tabs.
5. Rows (one per domain): domain + trigger line + first verbs
   (`quoteTrip, bookTrip +1 more`); right side protocols
   (`chat + actions`) and proof date (`proof Mar 3`).
6. States: quarantined rows keep full content plus one reason
   line (`Hooks drifted, re-proof pending — last verified Jan
   21`) and consequence (`Files kept, install needs explicit
   pin`). Docs-only rows read `Skills only`. Empty catalog reads
   its size honestly (`1 listed`).
7. Entry detail: domain H1, agents with protocols, hooks per
   flow, verbs with proof time, install as one copy block
   (clone · install · list), verify line (digests, ref, proof
   date), source line. Order fixed.
8. Footer: real count + quarantine rule. No slogans.

## Copy rules

Timestamps, not adjectives (`proof Mar 3`, never `stale`).
Reasons verbatim, stays clickable. No fake counts, demo rows,
badges, scores, or slogans. Every number real or absent.

## Non-goals

Backend, accounts, scores, leaderboard, trust badges,
categories, brand icons, submission queue. Static catalog,
PR persistence, proof or absent.
