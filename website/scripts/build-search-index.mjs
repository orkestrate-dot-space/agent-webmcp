// Search index builder: reads the docs JSX sources, extracts titles,
// headings, and text (code samples preserved), emits public/search-index.json.
// Runs on `prebuild` so deploys never serve a stale index; the committed
// copy keeps `next dev` working. Deterministic — same sources, same JSON.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const appDir = join(here, "..", "app");
const outFile = join(here, "..", "public", "search-index.json");
// TOC data is bundled (not fetched): the rail renders on first paint with
// zero loading state. Same sections, reshaped per route.
const tocFile = join(appDir, "docs", "toc-index.json");

const ENT = {
  "&apos;": "'",
  "&#39;": "'",
  "&quot;": '"',
  "&#34;": '"',
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&nbsp;": " ",
  "&rarr;": "→",
  "&larr;": "←",
};

// Must match slugify() in app/docs/toc.tsx — search anchors link to TOC ids.
const slugify = (s) => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");

// Strip one source slice to searchable text. Backtick template literals
// (code samples — full of <angle> terms like <handle>) are protected
// first so the tag-stripper cannot eat them.
const clean = (raw) => {
  const kept = [];
  const masked = raw.replace(/`(?:\\.|[^`\\])*`/g, (m) => {
    kept.push(m.slice(1, -1));
    return `\u0000${kept.length - 1}\u0000`;
  });
  const noComments = masked.replace(/{\/\*[\s\S]*?\*\/}/g, " ");
  const noTags = noComments.replace(/<[^>]*>/g, " ");
  const restored = noTags.replace(/\u0000(\d+)\u0000/g, (_, i) => ` ${kept[Number(i)]} `);
  const decoded = restored.replace(/&[a-z#0-9]+;/g, (m) => ENT[m] ?? m);
  return decoded.replace(/\{" "\}/g, " ").replace(/\s+/g, " ").trim();
};

const preferMdx = (base) =>
  existsSync(join(appDir, `${base}.mdx`)) ? `${base}.mdx` : `${base}.tsx`;

const pageFiles = [{ file: preferMdx("page"), url: "/" }, { file: preferMdx("docs/page"), url: "/docs" }, { file: "sites/page.tsx", url: "/sites" }];
for (const entry of readdirSync(join(appDir, "docs"), { withFileTypes: true })) {
  if (entry.isDirectory()) {
    for (const candidate of [`docs/${entry.name}/page.mdx`, `docs/${entry.name}/page.tsx`]) {
      try {
        readFileSync(join(appDir, candidate), "utf8");
        pageFiles.push({ file: candidate, url: `/docs/${entry.name}` });
        break;
      } catch {
        // Try the next candidate.
      }
    }
  }
}

// Markdown strip for .mdx sources: fences keep their content, links keep
// their text, markers go. Plain-text prose needs no work.
const cleanMd = (raw) =>
  raw
    .replace(/```[a-z]*\n?/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[*_~]/g, "")
    .replace(/&apos;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();

const index = pageFiles.map(({ file, url }) => {
  const src = readFileSync(join(appDir, file), "utf8");
  if (file.endsWith(".mdx")) {
    // MDX pages carry no metadata (Next #91735) — it lives in the
    // colocated layout.tsx. Body parses as markdown.
    const layoutFile = join(appDir, file.replace(/\/page\.mdx$/, "/layout.tsx"));
    let layout = "";
    try {
      layout = readFileSync(layoutFile, "utf8");
    } catch {
      // No layout — title falls back to the URL.
    }
    const title = (layout.match(/title:\s*"([^"]*)"/) ?? [])[1] ?? url;
    const description = (layout.match(/description:\s*"([^"]*)"/) ?? [])[1] ?? "";
    const body = src
      .replace(/export const metadata = \{[\s\S]*?\n\};/, " ")
      .replace(/^import .*$/gm, " ");
    const heads = [...body.matchAll(/^#{2,3}\s+(.*)$/gm)].map((m) => ({
      level: m[0].startsWith("###") ? 3 : 2,
      text: cleanMd(m[1]),
      start: m.index ?? 0,
      end: (m.index ?? 0) + m[0].length,
    }));
    const sections = [];
    const introEnd = heads.length > 0 ? heads[0].start : body.length;
    const intro = cleanMd(body.slice(0, introEnd));
    if (intro) sections.push({ anchor: "", heading: "", text: intro });
    heads.forEach((h, i) => {
      const next = i + 1 < heads.length ? heads[i + 1].start : body.length;
      const text = cleanMd(body.slice(h.end, next));
      if (h.text) sections.push({ anchor: slugify(h.text), heading: h.text, text, level: h.level });
    });
    return { url, title, description, sections };
  }
  const title = (src.match(/title:\s*"([^"]*)"/) ?? [])[1] ?? url;
  const description = (src.match(/description:\s*"([^"]*)"/) ?? [])[1] ?? "";
  const noMeta = src
    .replace(/export const metadata = \{[\s\S]*?\n\};/, " ")
    .replace(/^import .*$/gm, " ");
  const heads = [...noMeta.matchAll(/<h([23])[^>]*>([\s\S]*?)<\/h\1>/g)].map((m) => ({
    level: Number(m[1]),
    text: clean(m[2]),
    start: m.index ?? 0,
    end: (m.index ?? 0) + m[0].length,
  }));
  const sections = [];
  const introEnd = heads.length > 0 ? heads[0].start : noMeta.length;
  const intro = clean(noMeta.slice(0, introEnd));
  if (intro) sections.push({ anchor: "", heading: "", text: intro });
  heads.forEach((h, i) => {
    const next = i + 1 < heads.length ? heads[i + 1].start : noMeta.length;
    const text = clean(noMeta.slice(h.end, next));
    if (h.text && (text || h.level === 2)) {
      sections.push({ anchor: slugify(h.text), heading: h.text, text, level: h.level });
    }
  });
  return { url, title, description, sections };
});

writeFileSync(outFile, `${JSON.stringify(index, null, 1)}\n`);
console.log(`search index: ${index.length} pages, ${index.reduce((n, p) => n + p.sections.length, 0)} sections → public/search-index.json`);

const toc = {};
for (const p of index) {
  toc[p.url] = p.sections
    .filter((s) => s.anchor !== "")
    .map((s) => ({ anchor: s.anchor, heading: s.heading, level: s.level ?? 2 }));
}
writeFileSync(tocFile, `${JSON.stringify(toc, null, 1)}\n`);
console.log(`toc index: ${Object.keys(toc).length} routes → app/docs/toc-index.json`);
