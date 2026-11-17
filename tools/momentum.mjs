#!/usr/bin/env node
// Daily momentum numbers for the launch. No auth needed for the public parts;
// GitHub traffic and issue counts need `gh auth login` and degrade to "n/a".
//
//   node tools/momentum.mjs
//
// Prints: npm weekly downloads, GitHub stars, open site submissions,
// published index stats (sites/tools/generated_at), and the worker URL.

import { spawnSync } from "node:child_process";

const get = async (url, headers = {}) => {
  const res = await fetch(url, { headers });
  if (!res.ok) throw new Error(`${url} -> HTTP ${res.status}`);
  return res.json();
};

const npm = async () => {
  try {
    const j = await get("https://api.npmjs.org/downloads/point/last-week/@orkestrate/agent-webmcp");
    return `${j.downloads} downloads last week`;
  } catch {
    return "n/a (package not published yet)";
  }
};

const stars = async () => {
  try {
    const j = await get("https://api.github.com/repos/orkestrate-dot-space/agent-webmcp");
    return `${j.stargazers_count} stars`;
  } catch {
    return "n/a";
  }
};

const issues = () => {
  const r = spawnSync("gh", ["issue", "list", "--repo", "orkestrate-dot-space/agent-webmcp", "--limit", "100", "--json", "number,title,labels"], {
    encoding: "utf8",
  });
  if (r.status !== 0) return "n/a (gh not authed)";
  try {
    const all = JSON.parse(r.stdout);
    const subs = all.filter((i) => (i.labels ?? []).some((l) => l.name === "site-submission"));
    return `${subs.length} open site submissions (${all.length} open issues total)`;
  } catch {
    return "n/a";
  }
};

const index = async () => {
  try {
    const p = await get("https://agent-webmcp-index.agent-webmcp.workers.dev/index.json");
    return `${p.sites} sites, ${p.tools} tools, generated ${p.generated_at}`;
  } catch (e) {
    return `index unreachable (${e.message})`;
  }
};

console.log("agent-webmcp momentum");
console.log(`  npm:     ${await npm()}`);
console.log(`  github:  ${await stars()}`);
console.log(`  submits: ${issues()}`);
console.log(`  index:   ${await index()}`);
