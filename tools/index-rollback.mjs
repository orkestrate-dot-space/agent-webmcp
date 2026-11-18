#!/usr/bin/env node
// Point the live pointer at an earlier immutable index version.
//
//   node tools/index-rollback.mjs            list published versions
//   node tools/index-rollback.mjs <version>   e.g. index-10e36c6ff48a.json
//
// Versions are never overwritten, so a rollback is just a pointer write. The
// pointer is rebuilt from the version's own bytes (sha256, sites, tools), so it
// is exactly what a fresh publish would have produced.

import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const NAMESPACE = "c489f55fb6d04a0a97a9143c15ec2f33";
const args = process.argv.slice(2);
const kv = (argv) => spawnSync("cf", ["kv", ...argv, "--namespace-id", NAMESPACE], { encoding: "utf8" });

// Sites and tools as every reader reports them: distinct hosts among
// verified records with at least one tool, and distinct tool names per host,
// summed. One host can hold several records; a tool on two of them counts once.
function countIndex(sites) {
  const perHost = new Map();
  for (const s of sites ?? []) {
    if (s.status !== "verified" || !s.tools?.length) continue;
    let names = perHost.get(s.host);
    if (!names) {
      names = new Set();
      perHost.set(s.host, names);
    }
    for (const t of s.tools) names.add(t.name);
  }
  let tools = 0;
  for (const names of perHost.values()) tools += names.size;
  return { sites: perHost.size, tools };
}

const raw = args.find((a) => !a.startsWith("--"));
if (args.includes("--list") || !raw) {
  const out = kv(["keys", "list"]);
  if (out.status !== 0) {
    console.error((out.stderr || out.stdout || "").trim());
    process.exit(1);
  }
  const keys = JSON.parse(out.stdout)
    .map((k) => k.name)
    .filter((n) => n.startsWith("version:"));
  if (!keys.length) {
    console.log("no published versions");
    process.exit(0);
  }
  for (const key of keys) {
    const get = kv(["keys", "get", key]);
    let summary = "(unreadable)";
    try {
      const index = JSON.parse(get.stdout);
      const counts = countIndex(index.sites);
      summary = `${index.generated_at}  ${counts.sites} sites, ${counts.tools} tools`;
    } catch {}
    console.log(`${key.slice("version:".length)}  ${summary}`);
  }
  process.exit(0);
}

const file = raw.startsWith("index-") ? raw : `index-${raw}.json`;
if (!/^index-[0-9a-f]{12}\.json$/.test(file)) {
  console.error(`not a version file: ${file}`);
  process.exit(2);
}

const get = kv(["keys", "get", `version:${file}`]);
if (get.status !== 0 || !get.stdout.trim()) {
  console.error(`${file} is not in KV`);
  process.exit(1);
}
const text = get.stdout;
const index = JSON.parse(text);
const counts = countIndex(index.sites);
const pointer = {
  schema_version: index.schema_version,
  generated_at: index.generated_at,
  sha256: createHash("sha256").update(text).digest("hex"),
  url: `indexes/${file}`,
  bytes: Buffer.byteLength(text),
  sites: counts.sites,
  tools: counts.tools,
};

const tmp = "/tmp/agent-webmcp-pointer.json";
writeFileSync(tmp, `${JSON.stringify(pointer, null, 2)}\n`);
const put = kv(["keys", "put", "pointer", "--file", tmp]);
if (put.status !== 0) {
  console.error((put.stderr || put.stdout || "").trim());
  process.exit(1);
}
console.log(
  `pointer -> ${pointer.url} (${pointer.sites} sites, ${pointer.tools} tools, ${pointer.sha256.slice(0, 12)})`,
);
