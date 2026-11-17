#!/usr/bin/env node
// Publish an index file to the `agent-webmcp-index` Worker.
//
//   node tools/publish-index.mjs [--index index/index.json] [--denylist index/denylist.json]
//        [--kv] [--deploy]
//
// Default (--kv): writes the versioned file, the pointer and the denylist into
// the INDEX KV namespace, which the Worker serves. That is how the intake bot
// publishes without a deploy; a human can run the same command.
//
// Without --kv it only writes worker/public (static assets), which stays as the
// fallback the Worker serves when KV has no key. --deploy then ships those
// assets. The pointer is the only mutable file; a version is written once, so a
// client never sees a half update.

import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const NAMESPACE = "c489f55fb6d04a0a97a9143c15ec2f33";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const args = process.argv.slice(2);
const arg = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};

// --adopt <pointer-url>: pull the live index into index/index.json first, so a
// human edits the same file the bot is writing. Refuses to overwrite a local
// file that has changes git does not have.
if (args.includes("--adopt")) {
  const pointerUrl = arg("adopt", "");
  const res = await fetch(pointerUrl);
  if (!res.ok) throw new Error(`pointer answered ${res.status}`);
  const pointer = await res.json();
  const base = pointerUrl.slice(0, pointerUrl.lastIndexOf("/"));
  const fileUrl = /^https?:\/\//.test(pointer.url) ? pointer.url : `${base}/${pointer.url.replace(/^\//, "")}`;
  const file = await fetch(fileUrl);
  if (!file.ok) throw new Error(`${fileUrl} answered ${file.status}`);
  const text = await file.text();
  const sha = createHash("sha256").update(text).digest("hex");
  if (sha !== pointer.sha256) throw new Error(`hash mismatch: expected ${pointer.sha256}, got ${sha}`);
  const target = resolve(repo, arg("index", "index/index.json"));
  const before = existsSync(target) ? readFileSync(target, "utf8") : "";
  if (before && before !== text) {
    const rel = relative(repo, target);
    const dirty = spawnSync("git", ["status", "--porcelain", "--", rel], { cwd: repo, encoding: "utf8" });
    const changed = (dirty.stdout ?? "").trim();
    if (changed && !args.includes("--force")) {
      throw new Error(`${target} has uncommitted changes; commit or pass --force`);
    }
  }
  writeFileSync(target, text);
  console.log(`adopted ${pointer.sites} sites, ${pointer.tools} tools (${sha.slice(0, 12)}) into ${target}`);
  process.exit(0);
}

const source = resolve(repo, arg("index", "index/index.json"));
const denylistPath = resolve(repo, arg("denylist", "index/denylist.json"));
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
const pointerText = `${JSON.stringify(pointer, null, 2)}\n`;
writeFileSync(join(publicDir, "index.json"), pointerText);

const denylist = existsSync(denylistPath) ? readFileSync(denylistPath, "utf8") : '{\n  "hosts": []\n}\n';
writeFileSync(join(publicDir, "denylist.json"), denylist);

console.log(`${file}  ${pointer.sites} sites, ${pointer.tools} tools, ${pointer.bytes} bytes`);
console.log(`pointer: ${join(publicDir, "index.json")}`);

if (args.includes("--kv")) {
  // The KV index is the live one: the intake bot adds sites to it between
  // deploys. Publishing a local file over it would silently drop those, so
  // refuse when the local copy is clearly behind, unless --force.
  const current = spawnSync(
    "cf",
    ["kv", "keys", "get", "pointer", "--namespace-id", NAMESPACE],
    { encoding: "utf8" },
  );
  let live = null;
  try {
    live = JSON.parse(current.stdout);
  } catch {
    live = null;
  }
  if (live && typeof live.sites === "number" && !args.includes("--force")) {
    const drop = live.sites - pointer.sites;
    if (drop > Math.max(1, Math.floor(live.sites * 0.05))) {
      console.error(
        `refusing: this file has ${pointer.sites} verified sites but the live index has ${live.sites}.\n` +
          `The bot may have listed sites since this file was written. Pull it first:\n` +
          `  node tools/publish-index.mjs --adopt <live pointer url>\n` +
          `or pass --force to overwrite anyway.`,
      );
      process.exit(1);
    }
  }

  const tmp = join(publicDir, ".kv-upload.tmp");
  const put = (key, body) => {
    // --body takes the value on argv, which is too small for the index; the
    // --file form streams it instead.
    writeFileSync(tmp, body);
    const out = spawnSync(
      "cf",
      ["kv", "keys", "put", key, "--namespace-id", NAMESPACE, "--file", tmp],
      { encoding: "utf8" },
    );
    if (out.status !== 0) {
      throw new Error(`kv put ${key} failed: ${(out.stderr || out.stdout || "").trim().slice(0, 300)}`);
    }
  };
  try {
    // Version first, pointer last: a reader that sees the new pointer must find
    // its file already there.
    put(`version:${file}`, text);
    put("denylist", JSON.stringify(JSON.parse(denylist), null, 2) + "\n");
    // The weekly rebuild reads the seed list from here, so publishing the
    // index also publishes what the next rebuild will scan.
    const seedsPath = resolve(repo, arg("seeds", "seeds/launch.txt"));
    if (existsSync(seedsPath)) put("seeds", readFileSync(seedsPath, "utf8"));
    put("pointer", pointerText);
  } finally {
    rmSync(tmp, { force: true });
  }
  console.log(`kv: version:${file}, denylist, seeds, pointer`);
}

if (args.includes("--deploy")) {
  const out = spawnSync("cf", ["deploy"], { cwd: worker, stdio: "inherit" });
  process.exit(out.status ?? 1);
}
