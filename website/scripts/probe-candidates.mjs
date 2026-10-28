// probe-candidates: bulk probe for the population pipeline.
// Reads candidates.txt (one domain per line, # comments), probes
// each with the worker's content checks at concurrency 8, prints
// one JSON row per domain to stdout for the human description pass.
// Usage: bun scripts/probe-candidates.mjs candidates.txt > probed.json
import { readFileSync } from "node:fs";
import { canonicalHost, hasFromSignals, probeAll } from "../../workers/sites-index/src/index.ts";

const file = process.argv[2] ?? "candidates.txt";
const raw = readFileSync(file, "utf8").split("\n");
const seen = new Set();
const hosts = [];
for (const line of raw) {
  const v = line.split("#")[0]?.trim().toLowerCase() ?? "";
  if (v === "") continue;
  const host = canonicalHost(v);
  if (!host) {
    console.error(`skip (junk): ${v}`);
    continue;
  }
  // www folds to apex unless the candidate lists both on purpose.
  const folded = host.startsWith("www.") ? host.slice(4) : host;
  if (seen.has(folded)) {
    console.error(`skip (dup): ${v}`);
    continue;
  }
  seen.add(folded);
  hosts.push(host);
}

const out = [];
// Fast first pass, then one slow retry for all-false rows: bulk
// probes flap under rate limits, and a single attempt turns good
// domains into false drops. The retry runs at concurrency 1 with
// a pause; its result wins either way.
const probeOne = async (host) => {
  const probed = await probeAll(fetch, host);
  return {
    domain: host,
    file: probed.file,
    signals: probed.signals,
    has: hasFromSignals(probed.signals, probed.native),
    usable: probed.signals.some((s) => s.ok),
  };
};
const emptyHas = () => ({ webmcp: false, skills: false, api: false, mcp: false, auth: false, ask: false });
for (let i = 0; i < hosts.length; i += 8) {
  const batch = hosts.slice(i, i + 8);
  const rows = await Promise.all(
    batch.map(async (host) => {
      try {
        return await probeOne(host);
      } catch (e) {
        console.error(`probe failed (kept, unusable): ${host} ${String(e)}`);
        return { domain: host, file: null, signals: null, has: emptyHas(), usable: false };
      }
    })
  );
  out.push(...rows);
}
for (const r of out) {
  if (r.usable || r.signals === null) continue;
  await new Promise((x) => setTimeout(x, 1500));
  try {
    const redo = await probeOne(r.domain);
    Object.assign(r, redo);
    if (redo.usable) console.error(`retry rescued: ${r.domain}`);
  } catch (e) {
    console.error(`retry failed: ${r.domain} ${String(e)}`);
  }
}
console.log(JSON.stringify({ sites: out }, null, 2));
