#!/usr/bin/env node
// Splice one verified site record into the index and its seed entry into the
// seed list. Used by the publish workflow after a submission scan verifies.
//
//   node tools/submit/splice.mjs --record /tmp/record.json
//        [--index index/index.json] [--seeds seeds/launch.txt]
//
// Rules, all conservative:
// - The record must already have passed validateResult (the caller enforces).
// - Only `verified` records with at least one tool are spliced. Anything else
//   is refused: drops happen through the weekly rebuild's carry rules, never
//   from a single submission scan.
// - Existing records for the same host AND same start page are replaced; the
//   rest of the index is untouched.
// - The seed entry is appended under "# Submissions" only when absent.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};

const record = JSON.parse(readFileSync(resolve(flag("record", "")), "utf8"));
if (record.status !== "verified" || !Array.isArray(record.tools) || !record.tools.length) {
  console.error(`refusing to splice a ${record.status} record with ${record.tools?.length ?? 0} tools`);
  process.exit(1);
}

const indexPath = resolve(flag("index", "index/index.json"));
const seedsPath = resolve(flag("seeds", "seeds/launch.txt"));
const index = JSON.parse(readFileSync(indexPath, "utf8"));

const keyOf = (s) => `${s.host} ${(s.pages_checked ?? [])[0] ?? ""}`;
const key = keyOf(record);
const before = index.sites.length;
index.sites = index.sites.filter((s) => !(s.host === record.host && keyOf(s) === key));
const replaced = before - index.sites.length;
index.sites.push(record);
index.generated_at = new Date().toISOString().replace(/\.\d+Z$/, "");
mkdirSync(dirname(indexPath), { recursive: true });
writeFileSync(indexPath, `${JSON.stringify(index, null, 2)}\n`);

const entry = `${record.host}${record.pages_checked[0] ? new URL(record.pages_checked[0]).pathname.replace(/\/$/, "") : ""}`;
const seeds = readFileSync(seedsPath, "utf8");
const present = seeds.split("\n").some((l) => {
  const t = l.replace(/(^|\s)#\s.*$/, "").trim().replace(/^https?:\/\//, "").replace(/\/$/, "");
  return t === entry || t === entry.replace(/\/$/, "");
});
if (!present) {
  const block = seeds.includes("# Submissions") ? "" : "\n# Submissions (added by the intake bot)\n";
  writeFileSync(seedsPath, `${seeds.replace(/\n?$/, "\n")}${block}${entry}\n`);
}

const tools = index.sites.reduce((n, s) => n + s.tools.length, 0);
console.log(`spliced ${record.host} (${record.tools.length} tools, replaced ${replaced}); index now ${index.sites.length} sites, ${tools} tools; seed entry ${entry} ${present ? "already listed" : "added"}`);
