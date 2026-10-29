// verify-sites: build-time gate for sites.json. Slim record shape:
// origin, slug, description<=140, has{webmcp,skills,api,mcp,auth,ask}
// booleans (what an agent can do there, each probe-verified),
// state listed|quarantined (+verbatim reason), proof
// {verified date, ref pin (empty = probed, not pinned)},
// repo, path (empty = no install vector). Live drift mode on --flag.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const live = process.argv.includes("--live");
const fails = [];
const fail = (slug, msg) => fails.push(`${slug}: ${msg}`);

const file = JSON.parse(readFileSync(join(root, "sites.json"), "utf8"));
if (!Array.isArray(file.sites)) {
  console.error("FAIL sites.json: sites is not an array");
  process.exit(1);
}
const seen = new Set();
const foldedSeen = new Set();
for (const s of file.sites) {
  const tag = s.slug ?? s.origin ?? "?";
  if (typeof s.origin !== "string" || !/^https:\/\//.test(s.origin)) fail(tag, "origin must be an https URL");
  const host = s.origin.replace(/^https?:\/\//, "").split("/")[0] ?? "";
  if (host !== host.toLowerCase() || !/^[a-z0-9.-]+\.[a-z]{2,}$/.test(host)) fail(tag, "origin host must be a canonical lowercase domain");
  if (s.slug !== host) fail(tag, "slug must be the raw domain (dash slugs redirect, they are not rows)");
  if (seen.has(host)) fail(tag, "duplicate domain");
  seen.add(host);
  const folded = host.startsWith("www.") ? host.slice(4) : host;
  if (foldedSeen.has(folded)) fail(tag, "www/apex duplicate — one row per site");
  foldedSeen.add(folded);
  if (typeof s.description !== "string" || s.description === "" || s.description.length > 140)
    fail(tag, "description must be 1-140 chars");
  for (const k of ["webmcp", "skills", "api", "mcp", "auth", "ask"]) {
    if (typeof s.has?.[k] !== "boolean") fail(tag, `has.${k} must be a boolean`);
  }
  if (!["listed", "quarantined"].includes(s.state)) fail(tag, "state must be listed or quarantined");
  if (s.state === "quarantined" && (typeof s.reason !== "string" || s.reason === ""))
    fail(tag, "quarantined entries need a verbatim reason");
  if ("signals" in s && s.signals !== undefined) {
    if (
      !Array.isArray(s.signals) ||
      !s.signals.every(
        (x) =>
          typeof x === "object" &&
          x !== null &&
          typeof x.key === "string" &&
          typeof x.ok === "boolean"
      )
    )
      fail(tag, "signals must be an array of {key, ok, ...} rows");
    else {
      // Same mapping as the worker's hasFromSignals and seed's
      // hasOf: flags and matrix must never disagree (node can't
      // import the TS source, so the 6 lines live here too).
      const ok = new Set(s.signals.filter((x) => x.ok).map((x) => x.key));
      const expect = {
        webmcp: ok.has("native"),
        skills: ok.has("skillsIndex") || ok.has("skillMd"),
        api: ok.has("apiCatalog") || ok.has("openapi"),
        mcp: ok.has("mcp"),
        auth: ok.has("oauth"),
        ask: ok.has("ask"),
      };
      for (const k of Object.keys(expect)) {
        if ((s.has?.[k] === true) !== expect[k]) fail(tag, `has.${k} disagrees with signals matrix`);
      }
    }
  }
  if (typeof s.proof?.verified !== "string" || Number.isNaN(Date.parse(s.proof.verified)))
    fail(tag, "proof.verified must be a date");
  if (typeof s.repo !== "string") fail(tag, "repo must be a string");
  if (typeof s.path !== "string") fail(tag, "path must be a string");
  if ((s.repo === "") !== (s.path === "")) fail(tag, "repo and path stand together or stay empty (no install vector)");
  if (typeof s.proof?.ref !== "string") fail(tag, "proof.ref must be a string (empty means probed, not pinned)");
  if (live) {
    const host = s.origin.replace(/^https?:\/\//, "").split("/")[0];
    try {
      const r = await fetch(`https://${host}/.well-known/agent-webmcp.json`);
      if (!r.ok) fail(tag, `live file HTTP ${r.status} — drift, quarantine with reason`);
      else {
        const j = await r.json();
        if (j.domain !== host) fail(tag, "live file domain mismatch — drift, quarantine with reason");
      }
    } catch {
      fail(tag, "live file unreachable — drift, quarantine with reason");
    }
  }
}
if (fails.length > 0) {
  console.error(`FAIL verify-sites:\n${fails.map((f) => ` - ${f}`).join("\n")}`);
  process.exit(1);
}
console.log(`verify-sites: ${file.sites.length} site(s) ok${live ? " (live)" : ""}`);
