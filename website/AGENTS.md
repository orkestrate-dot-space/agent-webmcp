# site — Next.js docs site

Next.js 16 + React 19 + Tailwind 4. `dev`/`build`/`start`/`typecheck` in `package.json`.

## Rules

- Prefer retrieval-led reasoning: check installed `next` version docs before writing App Router code.
- `npm run typecheck` (`tsc --noEmit`) clean before `npm run build`.
- The site is the CLI's docs plus the skills catalog (shipped contract +
  installable origin skills). No hosted service — those concepts live in
  Orkestrate now.

## MDX content

- Docs pages are `.mdx` (prose, no JSX): `#` once, `##` sections (every
  page needs ≥2 — the TOC depends on it), fenced blocks with a lang, no
  line over ~80 cols, `[text](/docs/slug)` cross-links.
- Metadata lives in the colocated `layout.tsx`, NEVER in the MDX:
  `export const metadata` inside MDX trips Next #91735 under webpack.
- `dev`/`build` run `--webpack`: Turbopack cannot load `@next/mdx`
  on Next 16.4 (loader sandbox), and MDX metadata is webpack-broken
  upstream — hence layouts. Do not switch back without re-proving.
- Rehype/remark plugins must be string-form (serializable); content
  tables use markdown pipes (remark-gfm is on).

## Search

- `bun run prebuild` regenerates two derived files from the docs MDX
  sources (also runs automatically on `bun run build`); both are
  committed so `bun run dev` and `bun run typecheck` work fresh:
  `public/search-index.json` (palette, fetched lazily) and
  `app/docs/toc-index.json` (rail items, bundled — first paint).
- Anchors must stay in sync: heading ids are baked by `rehype-slug`
  (github-slugger) at build; `scripts/build-search-index.mjs` reimplements
  its slugify for both indexes. If their outputs ever disagree with the
  DOM, the DOM wins and the script gets fixed.

## Design tokens (single source: `aw-*`)

- `app/globals.css` `:root` holds every value. Components reference tokens,
  never raw colors. shadcn vars bridge onto `aw-*` (same block) — never the
  reverse, never raw values in either layer.
- Dark mode is media-only (`light-dark()` in tokens, default `dark:`
  variant). Never re-add a class-based dark variant: nothing toggles
  `.dark`, and class-based silently kills every `dark:` style on the site
  (this already happened once via CLI scaffolding).
- Radii: hand code uses the Tailwind scale (`rounded-md/lg/xl`); shadcn
  components carry their own `--radius-*` scale. Don't mix the two vocabularies.
- Motion uses the `:root` ease tokens (`--ease-out` for entrances/exits,
  `--ease-in-out` for on-screen movement), UI work stays under 300ms
  (skills-wall step: 280ms travel with its crossfade decoupled at 200ms),
  keyboard steps are instant (never animate keyboard input), `transform`/`opacity`
  only, named transition properties (never `all`).
- New shadcn pieces via CLI (`bunx shadcn@latest add <name>`), then verify
  `bun run build`. Review CLI diffs to `globals.css` before committing —
  scaffolding rewrites tokens and variants aggressively.

<!-- BEGIN:nextjs-agent-rules -->

# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` (resolved from this file's directory; in monorepos the `next` package may not be visible from the repo root) before writing any code. Heed deprecation notices.

This block is written and re-added by `next dev` — verify at `node_modules/next/dist/server/lib/generate-agent-files.js`. Removing it from a diff only re-creates the uncommitted change; committing it with your work keeps the tree clean.

<!-- END:nextjs-agent-rules -->
