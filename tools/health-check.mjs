#!/usr/bin/env node
// Poll the intake /health endpoint and exit nonzero when something needs a
// human. Run from any cron (a laptop, a box, a scheduled worker):
//   */10 * * * * /usr/bin/env node /path/to/health-check.mjs || notify...
const BASE = process.argv[2] ?? "https://agent-webmcp-intake.agent-webmcp.workers.dev";

const res = await fetch(`${BASE}/health`);
if (!res.ok) {
  console.error(`health answered HTTP ${res.status}`);
  process.exit(2);
}
const h = await res.json();
const problems = [];
const ageMin = (iso) => (Date.now() - new Date(iso).getTime()) / 60000;

// A site scan takes a minute or two; the pump runs every minute. Anything
// waiting over half an hour means the cron or the box is stuck. (A rebuild
// scans for close to an hour; that is normal, not an alert.)
if (h.queued?.oldest && ageMin(h.queued.oldest) > 30) {
  problems.push(`queued ${h.queued.n}, oldest waiting ${ageMin(h.queued.oldest).toFixed(0)} min`);
}
if (h.scanning?.oldest && ageMin(h.scanning.oldest) > 30 && h.last_rebuild?.state !== "scanning") {
  problems.push(`scan stuck for ${ageMin(h.scanning.oldest).toFixed(0)} min`);
}
if (h.last_rebuild && h.last_rebuild.state === "done" && h.last_rebuild.outcome === "error") {
  problems.push(`last rebuild failed: ${(h.last_rebuild.message ?? "").slice(0, 160)}`);
}

if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(
  `ok: queued ${h.queued?.n ?? 0}, scanning ${h.scanning?.n ?? 0}, last site scan ${h.last_site_scan ?? "never"}`,
);
