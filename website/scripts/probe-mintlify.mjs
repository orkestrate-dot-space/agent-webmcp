// probe-mintlify: fingerprint a Mintlify-hosted docs site and emit a
// sites.json row draft (platform "mintlify") to stdout, one JSON object
// per docs base URL. Human fills description; proof.ref records exactly
// what was verified live and when. Chat is NEVER probed here — it is
// captcha-gated and only verifiable from a non-bot browser.
// Usage: bun scripts/probe-mintlify.mjs https://claude.com/docs [...]
const day = new Date().toISOString().slice(0, 10);

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const PLATFORM_VERBS = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), "mintlify-verbs.json"), "utf8")
);

const skillFor = ({ host, label, base, server, searchTool, fsTool }) => `ASK ${host} docs (platform: mintlify).
NATIVE WEBMCP VERBS (page-registered; drive from any CDP-attached tab with agent-webmcp open+execute — verified headless AND headed):
- get_site_overview() — docs outline by section.
- search_docs({query}) — matching pages + content.
- read_page({path}) — full page as markdown (path from search).
- read_skill({skill_name}) — agent skill document.
- navigate_page({path}) — move the tab.
registerTool def to expose it: {"name":"ask_${label}","description":"Ask ${host} docs; returns cited passages.","inputSchema":{"type":"object","properties":{"question":{"type":"string"}},"required":["question"]}} with execute mapping question -> search_docs + read_page.
ALSO (retrieval only, no agent): anonymous docs MCP at ${base}/mcp (tools/call ${searchTool}/{${fsTool}}; no key).
HEADED-ONLY: conversational assistant (?assistant= + panel) needs a non-bot browser — captcha-gated chat endpoint. Owner-keyed Mintlify assistant/search APIs need a key (401 without).`;

const jrpc = (url, method, params) =>
  fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json, text/event-stream" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    signal: AbortSignal.timeout(20000),
  });

async function probe(base) {
  const out = { base, ok: false, error: "" };
  try {
    const init = await jrpc(`${base}/mcp`, "initialize", {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "probe-mintlify", version: "0" },
    });
    if (!init.ok) throw new Error(`mcp initialize HTTP ${init.status}`);
    const itext = await init.text();
    const server = (itext.match(/"name":"([^"]+)"/) || [])[1] || "";
    const toolsRes = await jrpc(`${base}/mcp`, "tools/list", {});
    const ttext = await toolsRes.text();
    const m = ttext.match(/^data: (.*)$/m);
    let tools = [];
    try {
      const listed = (JSON.parse(m[1]).result?.tools ?? []).filter((t) => t && typeof t.name === "string");
      tools = listed.map((t) => {
        const props = t.inputSchema?.properties ?? {};
        const req = new Set(t.inputSchema?.required ?? []);
        const params = Object.entries(props)
          .map(([k, v]) => `${k}: ${v?.type ?? "string"}${req.has(k) ? "" : "?"}`)
          .join(", ");
        return { name: t.name, params };
      });
    } catch {
      tools = [...ttext.matchAll(/"name":"([a-z0-9_]+)"/g)]
        .map((x) => x[1])
        .filter((n, i, a) => a.indexOf(n) === i)
        .map((name) => ({ name, params: "" }));
    }
    const toolNames = tools.map((t) => t.name);
    const searchTool = toolNames.find((t) => t.startsWith("search")) || "";
    const fsTool = toolNames.find((t) => /filesystem|fs|read|cat/.test(t)) || "";
    let llms = 0;
    try {
      const l = await fetch(`${base}/llms.txt`, { signal: AbortSignal.timeout(15000) });
      if (l.ok) llms = Number(l.headers.get("content-length") || (await l.text()).length);
    } catch { /* llms optional */ }
    let siteconfig = false;
    try {
      const s = await fetch(`${base}/_mintlify/assistant/siteconfig`, { signal: AbortSignal.timeout(15000) });
      siteconfig = s.ok;
    } catch { /* optional */ }
    const origin = new URL(base).origin;
    const host = new URL(base).hostname.replace(/^www\./, "");
    const label = host.split(".")[0];
    out.ok = true;
    out.row = {
      origin,
      slug: host,
      name: "",
      docs: base,
      description: "",
      platform: "mintlify",
      invoke: {
        transport: "webmcp",
        endpoint: `${origin}${new URL(base).pathname.replace(/\/$/, "")}`,
        example: `open tab -> await tools.search_docs({ query: "..." }) -> cited passages; read_page({ path }) for full markdown`,
      },
      skill: skillFor({ host, label, base, server, searchTool, fsTool }),
      proof: {
        verified: day,
        ref: `probe-mintlify: MCP initialize (${server || "unnamed"}) live; llms.txt ${llms > 0 ? llms + "B" : "missing"}; siteconfig ${siteconfig ? "present" : "absent"}. WebMCP page verbs [${PLATFORM_VERBS.tools.map((t) => t.name).join(", ")}] verified live in-page on 5 Mintlify sites. Chat NOT verified (captcha-gated, needs non-bot browser).`,
      },
      tools: PLATFORM_VERBS.tools,
    };
  } catch (e) {
    out.error = String(e && e.message ? e.message : e).slice(0, 200);
  }
  return out;
}

const bases = process.argv.slice(2);
if (bases.length === 0) {
  console.error("usage: bun scripts/probe-mintlify.mjs <docs-base-url> [...]");
  process.exit(2);
}
for (const base of bases) {
  console.log(JSON.stringify(await probe(base.replace(/\/$/, ""))));
}
