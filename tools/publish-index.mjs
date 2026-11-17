#!/usr/bin/env node
// Publish an index file to the `agent-webmcp-index` Worker.
//
//   node tools/publish-index.mjs [--index index/index.json] [--deploy]
//
// Writes the versioned file and the pointer into worker/public, then (with
// --deploy) uploads them from ../worker. The pointer is the only mutable file;
// the versioned one is cached forever, so a client never sees a half update.

import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const args = process.argv.slice(2);
const arg = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};

const source = resolve(repo, arg("index", "index/index.json"));
const worker = resolve(repo, arg("worker", "worker"));
const publicDir = join(worker, "public");
const indexesDir = join(publicDir, "indexes");

const text = readFileSync(source, "utf8");
const index = JSON.parse(text);
const sha = createHash("sha256").update(text).digest("hex");
const file = `index-${sha.slice(0, 12)}.json`;

mkdirSync(indexesDir, { recursive: true });
copyFileSync(source, join(indexesDir, file));

const verified = index.sites.filter((s) => s.status === "verified");
const pointer = {
  schema_version: index.schema_version,
  generated_at: index.generated_at,
  sha256: sha,
  url: `indexes/${file}`,
  bytes: Buffer.byteLength(text),
  sites: verified.length,
  tools: verified.reduce((n, s) => n + s.tools.length, 0),
};
writeFileSync(join(publicDir, "index.json"), `${JSON.stringify(pointer, null, 2)}\n`);

console.log(`${file}  ${pointer.sites} sites, ${pointer.tools} tools, ${pointer.bytes} bytes`);
console.log(`pointer: ${join(publicDir, "index.json")}`);

if (args.includes("--deploy")) {
  const out = spawnSync("cf", ["deploy"], { cwd: worker, stdio: "inherit" });
  process.exit(out.status ?? 1);
}
