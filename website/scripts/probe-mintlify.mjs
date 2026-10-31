// probe-mintlify: fingerprint a Mintlify-hosted docs site and emit a
// sites.json row draft (platform "mintlify") to stdout, one JSON object
// per docs base URL. Human fills description; proof.ref records exactly
// what was verified live and when. Chat is NEVER probed here — it is
// captcha-gated and only verifiable from a non-bot browser.
// Usage: bun scripts/probe-mintlify.mjs https://claude.com/docs [...]
const day = new Date().toISOString().slice(0, 10);

const skillFor = ({ host, label, base, server, searchTool, fsTool }) => `ASK ${host} docs (platform: mintlify).
TIER 1 — headless-safe (MCP, no auth): POST ${base}/mcp JSON-RPC (Streamable HTTP, Accept: application/json, text/event-stream).
init: {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"x","version":"0"}}}
search: tools/call {"name":"${searchTool}","arguments":{"query":"..."}} -> results with page paths.
read: tools/call {"name":"${fsTool}","arguments":{"command":"head -80 /<path>.mdx"}} (append .mdx to the result path).
registerTool def to expose it: {"name":"ask_${label}","description":"Ask ${host} docs; returns cited passages.","inputSchema":{"type":"object","properties":{"question":{"type":"string"}},"required":["question"]}} with execute running TIER 1.
TIER 2 — headed browser only (bot-gated chat): open ${base}?assistant=<urlencoded question> (panel auto-opens; else click button[aria-label="Toggle assistant panel"]). Read [role="dialog"] text for the cited answer. Follow-ups: set textarea value, dispatch input, click Send — same tab keeps the thread. Needs a non-bot browser (webdriver=false alone is not enough).`;

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
    const tools = [...ttext.matchAll(/"name":"([a-z0-9_]+)"/g)]
      .map((m) => m[1])
      .filter((n, i, a) => a.indexOf(n) === i);
    const searchTool = tools.find((t) => t.startsWith("search")) || "";
    const fsTool = tools.find((t) => /filesystem|fs|read|cat/.test(t)) || "";
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
      description: "",
      platform: "mintlify",
      invoke: {
        transport: "mcp",
        endpoint: `${base}/mcp`,
        example: `tools/call ${searchTool} {"query": "..."} -> cited passages; ${fsTool} head -80 /<path>.mdx`,
      },
      skill: skillFor({ host, label, base, server, searchTool, fsTool }),
      proof: {
        verified: day,
        ref: `probe-mintlify: MCP initialize (${server || "unnamed"}) + tools/list [${tools.join(", ")}] live; llms.txt ${llms > 0 ? llms + "B" : "missing"}; siteconfig ${siteconfig ? "present" : "absent"}. Chat NOT verified (captcha-gated, needs non-bot browser).`,
      },
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
