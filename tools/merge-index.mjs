#!/usr/bin/env node
// Merge index files scanned in parallel shards into one index.
//
//   node tools/merge-index.mjs --inputs a.json,b.json --out index/index.json
//        [--seeds seeds/launch.txt]
//
// Shards are expected in seed order. A site that appears twice is kept once:
// the record with more tools wins, and the other's pages are added to it.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};

const inputs = String(flag("inputs", "")).split(",").filter(Boolean);
if (!inputs.length) {
  console.error("usage: node tools/merge-index.mjs --inputs a.json,b.json --out index/index.json");
  process.exit(1);
}
const out = resolve(flag("out", "index/index.json"));

const sites = [];
const byKey = new Map();
const keyOf = (s) => s.pages_checked[0] ?? s.host;

let generatedAt = "";
for (const file of inputs) {
  const index = JSON.parse(readFileSync(resolve(file), "utf8"));
  if (index.generated_at > generatedAt) generatedAt = index.generated_at;
  for (const site of index.sites) {
    const key = keyOf(site);
    const seen = byKey.get(key);
    if (!seen) {
      byKey.set(key, site);
      sites.push(site);
      continue;
    }
    // Same start page seen by two shards: keep the richer record.
    if (site.tools.length > seen.tools.length) {
      seen.tools = site.tools;
      seen.status = site.status;
      seen.checked_at = site.checked_at;
      seen.note = site.note;
      seen.missed = site.missed;
      seen.engines = [...new Set([...(seen.engines ?? []), ...(site.engines ?? [])])];
    }
    for (const page of site.pages_checked) {
      if (!seen.pages_checked.includes(page)) seen.pages_checked.push(page);
    }
    seen.platform ??= site.platform;
  }
}

const totals = sites.reduce((m, s) => ({ ...m, [s.status]: (m[s.status] ?? 0) + 1 }), {});
const tools = sites.reduce((n, s) => n + s.tools.length, 0);
const index = { schema_version: 1, generated_at: generatedAt || new Date().toISOString(), sites };

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, `${JSON.stringify(index, null, 2)}\n`);
console.error(`merged ${inputs.length} shards: ${sites.length} sites ${JSON.stringify(totals)}, ${tools} tools -> ${out}`);
