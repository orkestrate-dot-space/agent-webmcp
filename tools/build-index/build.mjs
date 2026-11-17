#!/usr/bin/env node
// Build index/index.json by loading each seed site in two browsers and keeping
// every tool either of them sees.
//
//   node tools/build-index/build.mjs [--seeds seeds/launch.txt]
//        [--out index/index.json] [--pages 4] [--workers 4]
//        [--previous index/index.json] [--only host,host]
//        [--retries 2] [--quiet 3000] [--cap 20000]
//
// Engines:
//   lightpanda    its own MCP server (HTTP), one process per worker.
//   agent-browser its CLI, one session per worker.
//
// Neither engine invokes a tool, submits a form, or solves a bot check.
// Sites that show a challenge are recorded as blocked.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";

const args = parseArgs(process.argv.slice(2));
const SEEDS = resolve(args.seeds ?? "seeds/launch.txt");
const OUT = resolve(args.out ?? "index/index.json");
const PREVIOUS = args.previous ? resolve(args.previous) : null;
const MAX_PAGES = Number(args.pages ?? 4);
const WORKERS = Math.max(1, Number(args.workers ?? 4));
// Extra looks at an empty homepage. A submission scan gives a site every
// chance; a scheduled rebuild can lean on the carry rule instead, which makes
// the weekly run several times faster.
const RETRIES = Number(args.retries ?? 2);
// Settle polling: stop QUIET_MS after the tool set stops changing, give up at
// CAP_MS. Polls are one process each, ~1s.
const QUIET_MS = Number(args.quiet ?? 3000);
const CAP_MS = Number(args.cap ?? 20000);
const POLL_MS = 500;
const ONLY = args.only ? new Set(String(args.only).split(",").map((h) => h.trim())) : null;

const ENGINES = new Set(String(args.engines ?? "lightpanda,agent-browser").split(",").map((e) => e.trim()));

const LINT = [
  ["ignore previous", "instruction_override"],
  ["ignore all previous", "instruction_override"],
  ["disregard", "instruction_override"],
  ["system prompt", "mentions_system_prompt"],
  ["you must", "imperative_to_agent"],
  ["always call", "imperative_to_agent"],
  ["mandatory first step", "imperative_to_agent"],
  ["before any other tool", "imperative_to_agent"],
  ["do not tell the user", "concealment"],
  ["<script", "markup"],
];

/// Chrome 157 keeps `document.modelContext` off unless the WebMCP feature is
/// on, and agent-browser's own defaults only enable WebMCPTesting and DevTools
/// support. Without this, a site that registers tools reads as having none
/// (vercel.com: 0 tools stock, 5 with it). Verified on Chrome 157.0.8103.78.
const WEBMCP_ARGS = "--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport";
const abEnv = () => ({ ...process.env, AGENT_BROWSER_ARGS: WEBMCP_ARGS });

/// The recorder init script, installed before the first navigation so staged
/// and legacy registrations are logged from the very first call.
const RECORDER_PATH = resolve(dirname(new URL(import.meta.url).pathname), "recorder.js");

const PAGE_HINTS = ["/docs", "/search", "/pricing", "/products", "/product", "/shop", "/api", "/help"];

/// Same-origin links on the page, as a bare expression both engines evaluate.
const LINKS =
  "Array.from(document.querySelectorAll('a[href]')).map(a=>a.href).filter(h=>h.startsWith(location.origin)).slice(0,400)";

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a.startsWith("--")) out[a.slice(2)] = argv[i + 1]?.startsWith("--") ? true : argv[++i];
  }
  return out;
}

function readSeeds(path) {
  // Missing seeds are fine: check-site scans an entry that need not be listed.
  if (!existsSync(path)) return [];
  return readFileSync(path, "utf8")
    .split("\n")
    // A comment starts with "# " or sits at the start of a line, so URLs with
    // fragments survive.
    .map((l) => l.replace(/(^|\s)#\s.*$/, "").trim())
    .map((l) => (l.startsWith("#") ? "" : l))
    .filter(Boolean)
    .map((entry) => {
      const host = entry.split("/")[0].toLowerCase();
      const url = /^https?:\/\//.test(entry) ? entry : `https://${entry.replace(/\/$/, "")}/`;
      return { entry, host, url };
    });
}

function bareHost(host) {
  return host.replace(/^www\./, "");
}

// ---------------------------------------------------------------- lightpanda

async function startLightpanda(port) {
  let child;
  try {
    child = spawn("lightpanda", ["mcp", "--port", String(port), "--host", "127.0.0.1"], {
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (e) {
    throw new Error(`lightpanda not available: ${String(e).slice(0, 120)}`);
  }
  // If the binary is missing the child emits 'error' (ENOENT) instead of
  // throwing. Swallow it here so the fetch loop below times out cleanly and
  // the caller gets a normal error, not an unhandled 'error' crash.
  let spawnError = null;
  child.on("error", (e) => {
    spawnError = e;
  });
  child.stdout.on("data", () => {});
  child.stderr.on("data", () => {});
  const base = `http://127.0.0.1:${port}/mcp`;
  for (let i = 0; i < 100; i += 1) {
    if (spawnError) break;
    try {
      await fetch(base, {
        method: "POST",
        headers: { "content-type": "application/json", accept: "application/json, text/event-stream" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: initParams() }),
      });
      return { child, base, session: null };
    } catch {
      await sleep(200);
    }
  }
  try {
    child.kill();
  } catch {}
  throw new Error(
    spawnError ? `lightpanda not available: ${String(spawnError).slice(0, 120)}` : `lightpanda mcp did not start on port ${port}`,
  );
}

function initParams() {
  return {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: { name: "agent-webmcp-index", version: "1" },
  };
}

// One JSON-RPC call over the streamable HTTP transport. Returns the result.
async function rpc(lp, method, params, id = Math.floor(Math.random() * 1e9)) {
  const headers = {
    "content-type": "application/json",
    accept: "application/json, text/event-stream",
    "mcp-protocol-version": "2025-06-18",
  };
  if (lp.session) headers["mcp-session-id"] = lp.session;
  const res = await fetch(lp.base, {
    method: "POST",
    headers,
    body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
  });
  const sid = res.headers.get("mcp-session-id");
  if (sid) lp.session = sid;
  if (method === "notifications/initialized") return null;
  const text = await res.text();
  const payload = parseSseOrJson(text);
  if (payload?.error) throw new Error(`${method}: ${payload.error.message}`);
  return payload?.result ?? null;
}

function parseSseOrJson(text) {
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith("{")) return JSON.parse(trimmed);
  const line = trimmed
    .split("\n")
    .filter((l) => l.startsWith("data:"))
    .map((l) => l.slice(5).trim())
    .join("");
  return line ? JSON.parse(line) : null;
}

async function lightpandaInit(lp) {
  await rpc(lp, "initialize", initParams());
  await rpc(lp, "notifications/initialized", undefined);
}

async function lightpandaPage(lp, url) {
  const out = { url, final_url: url, tools: [], blocked: false, error: null, links: [] };
  try {
    const nav = await rpc(lp, "tools/call", { name: "goto", arguments: { url } });
    out.final_url = nav?.structuredContent?.url ?? url;
    const status = nav?.structuredContent?.httpStatus ?? 0;
    // Let hydration and late registration finish, but never wait long for a
    // network that never goes idle: analytics and chat widgets keep fetching.
    await sleep(800);
    await rpc(lp, "tools/call", {
      name: "waitForState",
      arguments: { state: "networkalmostidle", timeout: 4000 },
    }).catch(() => {});
    // Tools can register in stages; take early ones, then look again.
    let tools = await rpc(lp, "tools/call", { name: "pageTools", arguments: {} });
    for (let i = 0; i < 3 && extractTools(tools).length === 0; i += 1) {
      await sleep(700);
      tools = await rpc(lp, "tools/call", { name: "pageTools", arguments: {} });
    }
    out.tools = extractTools(tools);
    const linkRes = await rpc(lp, "tools/call", {
      name: "evaluate",
      arguments: {
        expression: LINKS,
      },
    }).catch(() => null);
    out.links = linkList(linkRes);
    const apiRes = await rpc(lp, "tools/call", {
      name: "evaluate",
      arguments: { expression: "typeof document.modelContext" },
    }).catch(() => null);
    // The value comes back as text or structured content; read only an exact
    // "object"/"undefined" value so envelope words cannot fake a present API.
    const apiCandidates = [...linkList(apiRes)];
    if (typeof apiRes?.structuredContent === "string") apiCandidates.push(apiRes.structuredContent);
    for (const block of apiRes?.content ?? []) {
      if (block.type === "text" && typeof block.text === "string") {
        const t = block.text.trim().replace(/^"|"$/g, "");
        if (t === "object" || t === "undefined") apiCandidates.push(t);
      }
    }
    if (apiCandidates.includes("object")) out.api = true;
    else if (apiCandidates.includes("undefined")) out.api = false;
    out.blocked = isBlocked(out, nav);
    if (status >= 400) out.error = `HTTP ${status}`;
  } catch (e) {
    out.error = String(e).slice(0, 200);
  }
  return out;
}

function extractTools(result) {
  const value = result?.structuredContent ?? result;
  const text = JSON.stringify(value);
  // pageTools returns the tools array (usually as JSON text or structured content).
  const candidates = [];
  const push = (v) => {
    if (Array.isArray(v)) candidates.push(...v);
    else if (v && Array.isArray(v.tools)) candidates.push(...v.tools);
  };
  push(result?.structuredContent);
  for (const block of result?.content ?? []) {
    if (block.type !== "text") continue;
    try {
      push(JSON.parse(block.text));
    } catch {
      // not JSON
    }
  }
  if (candidates.length === 0 && text.includes('"name"')) {
    try {
      push(JSON.parse(text));
    } catch {}
  }
  return candidates
    .filter((t) => t && typeof t.name === "string")
    .map((t) => ({
      name: t.name,
      description: typeof t.description === "string" ? t.description : "",
      input_schema: t.inputSchema ?? {},
      origin: typeof t.origin === "string" ? t.origin : "",
      annotations: normAnnotations(t.annotations),
    }));
}

function normAnnotations(a = {}) {
  return {
    read_only: Boolean(a.readOnly ?? a.readOnlyHint),
    consequential: Boolean(a.consequential ?? a.consequentialHint),
    untrusted_content: Boolean(a.untrustedContent ?? a.untrustedContentHint),
  };
}

function linkList(result) {
  const out = [];
  const collect = (v) => {
    if (Array.isArray(v)) {
      for (const x of v) if (typeof x === "string") out.push(x);
    }
  };
  collect(result?.structuredContent);
  for (const block of result?.content ?? []) {
    if (block.type !== "text") continue;
    try {
      collect(JSON.parse(block.text));
    } catch {
      // not JSON
    }
  }
  return out;
}

function isBlocked(out, nav) {
  const text = JSON.stringify(nav ?? {});
  return /bot challenge|Blocked by a .* bot challenge|are you a robot|just a moment|verify you are human|captcha/i.test(
    text,
  );
}

// ------------------------------------------------------------- agent-browser

/// What the settle poll reads each round: the current tool names, whether the
/// page API exists, and how many registrations the recorder has seen. Any
/// change restarts the quiet window.
const STATE_SRC = `(async()=>{
  let names=[], api="missing", logLen=0;
  try { api = (typeof document.modelContext); } catch(e) { api = "error"; }
  try { if (document.modelContext && document.modelContext.getTools) names = (await document.modelContext.getTools()).map(t=>t && t.name).filter(Boolean); } catch(e) {}
  try { logLen = (window.__wmcLog||[]).length; } catch(e) {}
  return {names, api, logLen};
})()`;

/// Full tool records for the final union. inputSchema can arrive as an object
/// (webmcp list) or a JSON string (page getTools); both are normalized.
const FULL_SRC = `(async()=>{
  try {
    if (document.modelContext && document.modelContext.getTools) {
      const t = await document.modelContext.getTools();
      return t.map(x=>({name:x.name,description:x.description||"",inputSchema:(typeof x.inputSchema==="string"?(function(){try{return JSON.parse(x.inputSchema)}catch(e){return {}}})():x.inputSchema)??x.input_schema??{},origin:x.origin||"",annotations:x.annotations||{}}));
    }
    return [];
  } catch(e) { return []; }
})()`;

const RECORDER_SRC = `(()=>{try{return (window.__wmcLog||[]).map(e=>e.name).filter(Boolean);}catch(e){return [];}})()`;

function spawnAgentBrowser(argv, { input, timeout = 60_000 } = {}) {
  return spawnSync("agent-browser", argv, { input, encoding: "utf8", timeout, env: abEnv() });
}

function parseBatch(stdout) {
  try {
    const batch = JSON.parse((stdout || "").trim());
    return Array.isArray(batch) ? batch : null;
  } catch {
    return null;
  }
}

function evalState(session, src) {
  const res = spawnAgentBrowser(["--session", session, "eval", "--stdin", "--json"], { input: src });
  try {
    const j = JSON.parse((res.stdout || "").trim());
    const data = j && typeof j === "object" && "data" in j ? j.data : j;
    return data?.result ?? null;
  } catch {
    return null;
  }
}

function normRecord(t) {
  let schema = t.inputSchema ?? t.input_schema ?? {};
  if (typeof schema === "string") {
    try {
      schema = JSON.parse(schema);
    } catch {
      schema = {};
    }
  }
  return {
    name: t.name,
    description: t.description ?? "",
    input_schema: typeof schema === "object" && schema !== null ? schema : {},
    origin: t.origin ?? "",
    annotations: normAnnotations(t.annotations),
  };
}

/// Merge full records by tool name, keeping the richer one.
function unionRecords(lists) {
  const byName = new Map();
  for (const list of lists) {
    for (const raw of list) {
      if (!raw || typeof raw.name !== "string") continue;
      const next = normRecord(raw);
      const prev = byName.get(next.name);
      if (!prev) {
        byName.set(next.name, next);
        continue;
      }
      if (!prev.description && next.description) prev.description = next.description;
      if (!prev.origin && next.origin) prev.origin = next.origin;
      if (!prev.input_schema || Object.keys(prev.input_schema).length === 0) prev.input_schema = next.input_schema;
      prev.annotations = {
        read_only: prev.annotations.read_only || next.annotations.read_only,
        consequential: prev.annotations.consequential || next.annotations.consequential,
        untrusted_content: prev.annotations.untrusted_content || next.annotations.untrusted_content,
      };
    }
  }
  return [...byName.values()];
}

function settleLoop(session) {
  // Poll until the tool set stops changing for QUIET_MS, or CAP_MS passes.
  // Staged registration (shop.dev shows 4 then 22) arrives in batches seconds
  // apart, so a fixed sleep either misses stages or wastes minutes.
  const t0 = Date.now();
  let prevKey = null;
  let quietSince = t0;
  let polls = 0;
  let state = null;
  for (;;) {
    state = evalState(session, STATE_SRC);
    polls += 1;
    const key = state
      ? `${[...state.names].sort().join(",")}#${state.logLen}`
      : `null#${polls}`;
    const now = Date.now();
    if (key !== prevKey) {
      prevKey = key;
      quietSince = now;
    }
    if (now - quietSince >= QUIET_MS) return { settled: true, ms: now - t0, polls, state };
    if (now - t0 >= CAP_MS) return { settled: false, ms: now - t0, polls, state };
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, POLL_MS);
  }
}

async function agentBrowserPage(session, url) {
  const out = { url, final_url: url, tools: [], blocked: false, error: null, links: [] };
  // One process per call; `agent-browser` costs about a second to start. The
  // recorder init script is installed before the first navigation so staged
  // and legacy registrations are logged from the very first call.
  const openRes = spawnAgentBrowser(
    ["--init-script", RECORDER_PATH, "--session", session, "batch", "--json"],
    {
      input: JSON.stringify([
        ["open", url],
        ["wait", "--load", "load", "--timeout", "15000"],
      ]),
      timeout: 90_000,
    },
  );
  const openBatch = parseBatch(openRes.stdout);
  if (!openBatch) {
    out.error = (openRes.stderr || openRes.stdout || "open failed").trim().slice(0, 200);
    return out;
  }
  const opened = openBatch[0]?.result;
  if (openBatch[0]?.error && !opened?.url) {
    out.error = String(openBatch[0].error).slice(0, 200);
    return out;
  }
  if (opened?.url) out.final_url = opened.url;
  const openTools = opened?.webmcp?.tools ?? [];

  const p1 = settleLoop(session);

  const finalRes = spawnAgentBrowser(["--session", session, "batch", "--json"], {
    input: JSON.stringify([
      ["webmcp", "list"],
      ["eval", FULL_SRC],
      ["eval", RECORDER_SRC],
      ["eval", LINKS],
      ["get", "url"],
    ]),
    timeout: 90_000,
  });
  const finalBatch = parseBatch(finalRes.stdout);
  const result = (i) => finalBatch?.[i]?.result;
  const unwrap = (v) => (v && typeof v === "object" && "result" in v ? v.result : v);
  const listTools = result(0)?.tools ?? [];
  const fullTools = unwrap(result(1)) ?? [];
  void unwrap(result(2)); // recorder names: activity signal only, resolved via getTools
  const links = unwrap(result(3));
  out.links = Array.isArray(links) ? links : [];
  const finalUrl = unwrap(result(4));
  const finalUrlStr = typeof finalUrl === "string" ? finalUrl : finalUrl?.url ?? "";
  if (/^https?:\/\//.test(finalUrlStr)) out.final_url = finalUrlStr;

  // The API exists whenever the scan browser is right, even on a site with no
  // tools. Absent everywhere means the scanner is wrong, not the site.
  if (p1.state?.api === "object") out.api = true;
  else if (typeof p1.state?.api === "string") out.api = false;

  // Records only: a name the recorder saw but no source can describe is not
  // listable. The recorder still did its job by keeping the set open.
  const getToolsRecords = Array.isArray(fullTools) ? fullTools : [];
  out.tools = unionRecords([openTools, listTools, getToolsRecords]);

  // One more look: registration can miss a busy first load.
  if (!out.tools.length && !out.error && !isBlockedPage(out, finalRes.stdout)) {
    const reloadRes = spawnAgentBrowser(["--session", session, "batch", "--json"], {
      input: JSON.stringify([
        ["reload"],
        ["wait", "--load", "load", "--timeout", "15000"],
      ]),
      timeout: 90_000,
    });
    if (parseBatch(reloadRes.stdout)) {
      const p2 = settleLoop(session);
      if (p2.state?.api === "object") out.api = true;
      const list2 = spawnAgentBrowser(["--session", session, "webmcp", "list", "--json"], { timeout: 60_000 });
      let second = [];
      try {
        const j = JSON.parse((list2.stdout || "").trim());
        second = (j && typeof j === "object" && "data" in j ? j.data : j)?.tools ?? [];
      } catch {}
      const full2 = evalState(session, FULL_SRC);
      out.tools = unionRecords([out.tools, second, Array.isArray(full2) ? full2 : []]);
    }
  }

  out.blocked = isBlockedPage(out, (openRes.stdout || "") + (finalRes.stdout || ""));
  const failed = [...(openBatch ?? []), ...(finalBatch ?? [])].find((b) => b?.error);
  if (failed && !out.tools.length) out.note = String(failed.error).slice(0, 200);
  return out;

  function isBlockedPage(o, text) {
    return (
      /just a moment|attention required|access denied|verify you are human|are you a robot|cf-challenge/i.test(
        `${o.final_url} ${String(text).slice(0, 4000)}`,
      ) && o.tools.length === 0
    );
  }
}

// ------------------------------------------------------------------ planning

function pickPages(start, links, max) {
  if (!Array.isArray(links)) return [];
  const picked = [];
  let origin;
  try {
    origin = new URL(start).origin;
  } catch {
    return [];
  }
  for (const hint of PAGE_HINTS) {
    if (picked.length + 1 >= max) break;
    const match = links.find((l) => {
      if (typeof l !== "string" || l.includes("#")) return false;
      let u;
      try {
        u = new URL(l);
      } catch {
        return false;
      }
      return u.origin === origin && u.pathname.startsWith(hint) && u.href !== start && !picked.includes(u.href);
    });
    if (match) picked.push(match.replace(/#.*$/, ""));
  }
  return picked;
}

// ---------------------------------------------------------------- one engine

async function runEngine(name, sites, workerFn) {
  const queue = [...sites];
  const results = new Map();
  let done = 0;
  const workers = Array.from({ length: WORKERS }, async () => {
    while (queue.length) {
      const site = queue.shift();
      if (!site) break;
      const pages = [];
      try {
        let first = await workerFn(site.url);
        if (!first.tools.length && !first.error && !first.blocked) {
          // One more look: registration can miss a busy first load.
          const again = await workerFn(site.url);
          if (again.tools.length) first = again;
        }
        pages.push(first);
        if (!first.error && !first.blocked) {
          for (const url of pickPages(first.final_url, first.links, MAX_PAGES)) {
            pages.push(await workerFn(url));
          }
        }
      } catch (e) {
        pages.push({ url: site.url, final_url: site.url, tools: [], error: String(e).slice(0, 200), links: [], blocked: false });
      }
      results.set(site.entry, pages);
      done += 1;
      const found = pages.reduce((n, p) => n + p.tools.length, 0);
      process.stderr.write(`${name}: ${done}/${sites.length} ${site.entry} ${found} tools\n`);
    }
  });
  await Promise.all(workers);
  return results;
}

// -------------------------------------------------------------------- merging

function lintFlags(description) {
  const lower = description.toLowerCase();
  const flags = [];
  for (const [needle, flag] of LINT) if (lower.includes(needle) && !flags.includes(flag)) flags.push(flag);
  if (!description.trim()) flags.push("no_description");
  return flags;
}

function detectPlatform(names) {
  const has = (n) => names.includes(n);
  if (["search_docs", "read_page", "navigate_page", "get_site_overview"].every(has)) return "mintlify";
  return undefined;
}

function mergeSite(site, lightpandaPages, abPages) {
  const all = [...(lightpandaPages ?? []), ...(abPages ?? [])];
  const tools = new Map();
  const pagesChecked = [];
  const engines = new Set();
  if (lightpandaPages) engines.add("lightpanda");
  if (abPages) engines.add("agent-browser");

  for (const page of all) {
    if (page.final_url && !pagesChecked.includes(page.final_url)) pagesChecked.push(page.final_url);
    for (const t of page.tools) {
      const prev = tools.get(t.name);
      if (!prev) {
        tools.set(t.name, {
          name: t.name,
          description: t.description,
          input_schema: t.input_schema,
          page: page.final_url,
          origin: t.origin,
          annotations: t.annotations,
          flags: lintFlags(t.description),
          seen_by: [page.engine ?? "unknown"],
        });
      } else {
        // Prefer the richer record, keep the first page.
        if (!prev.description && t.description) prev.description = t.description;
        if (!prev.origin && t.origin) prev.origin = t.origin;
        if (!prev.input_schema || Object.keys(prev.input_schema).length === 0) prev.input_schema = t.input_schema;
        prev.annotations = {
          read_only: prev.annotations.read_only || t.annotations.read_only,
          consequential: prev.annotations.consequential || t.annotations.consequential,
          untrusted_content: prev.annotations.untrusted_content || t.annotations.untrusted_content,
        };
        prev.seen_by = [...new Set([...prev.seen_by, page.engine ?? "unknown"])];
      }
    }
  }

  const list = [...tools.values()];
  const first = all[0];
  // The page API exists whenever the scan browser is right, even on a site
  // with no tools. Absent everywhere (and seen nowhere) means the scanner is
  // wrong, not the site: route to error so a maintainer looks, and so the
  // carry rule never mistakes it for an empty site.
  const apiSeen = all.some((p) => p.api === true);
  const apiAbsent = !apiSeen && all.some((p) => p.api === false);
  let status;
  let note;
  if (list.length) {
    status = "verified";
  } else if (all.some((p) => p.blocked)) {
    status = "blocked";
    note = "a bot check blocked the page; it was not solved";
  } else if (all.length && all.every((p) => p.error)) {
    status = "error";
    note = all[0].error;
  } else if (apiAbsent) {
    status = "error";
    note = "the WebMCP page API was absent in the scan browser; the run may lack the feature flag";
  } else {
    status = "no_tools";
  }

  return {
    host: bareHost(site.host),
    status,
    checked_at: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
    pages_checked: pagesChecked.slice(0, 30),
    platform: detectPlatform(list.map((t) => t.name)),
    tools: list,
    note,
    missed: 0,
    engines: [...engines],
  };
}

// A site that had tools and shows none this run keeps its last tools until it
// misses two runs in a row. One slow load should not delist a site.
function carryForward(sites, previous) {
  let kept = 0;
  for (const site of sites) {
    if (site.status === "verified") continue;
    const prev = previous.find((p) => p.host === site.host && p.status === "verified");
    if (!prev) continue;
    const missed = (prev.missed ?? 0) + 1;
    if (missed >= 2) continue;
    site.status = "verified";
    site.tools = prev.tools.map((t) => ({ ...t, seen_by: t.seen_by ?? [] }));
    site.platform = prev.platform;
    site.note = `${site.note ?? "no tools seen at this check"}; showing tools from ${prev.checked_at}`;
    site.missed = missed;
    kept += 1;
  }
  return kept;
}

// ------------------------------------------------------------------ one site

/// Check one site with the requested engines and return its index record.
/// Defaults to agent-browser only: it is the launch scanner (Chrome 157 needs
/// the WebMCP flag, set in-process). Lightpanda stays available via --engines
/// for cross-checks. A missing engine binary yields an `error` record, never
/// a crash, so CI runners without that binary still produce a valid result.
export async function checkSite(entry, { pages = 4, engines = ["agent-browser"], extras = [] } = {}) {
  const start = readSeeds(SEEDS).find((s) => s.entry === entry);
  const host = (start?.host ?? entry.split("/")[0]).replace(/^www\./, "");
  let lpPages;
  let abPages;

  if (engines.includes("lightpanda")) {
    let lp = null;
    try {
      lp = await startLightpanda(8300 + (process.pid % 200));
      await lightpandaInit(lp);
      const page = (url) => lightpandaPage(lp, url);
      page.engine = "lightpanda";
      lpPages = await collectPages(page, entry, pages);
    } catch (e) {
      const url = `https://${entry.replace(/\/$/, "")}/`;
      lpPages = [{ url, final_url: url, tools: [], error: String(e).slice(0, 200), links: [], blocked: false, engine: "lightpanda" }];
    } finally {
      try {
        lp?.child?.kill();
      } catch {}
    }
  }
  if (engines.includes("agent-browser")) {
    const session = process.env.AWM_SESSION || `awm-check-${process.pid}`;
    try {
      const page = (url) => agentBrowserPage(session, url);
      page.engine = "agent-browser";
      abPages = await collectPages(page, entry, pages);
      // Pages the owner named are checked as given, in order.
      for (const extra of extras.slice(0, 3)) {
        if (!/^https:\/\//.test(extra)) continue;
        const p = await page(extra);
        p.engine = "agent-browser";
        abPages.push(p);
      }
    } finally {
      spawnSync("agent-browser", ["--session", session, "close"], { encoding: "utf8", timeout: 30_000, env: abEnv() });
    }
  }
  const site = {
    entry,
    host: host.toLowerCase(),
    url: `https://${entry.replace(/\/$/, "")}/`,
  };
  return mergeSite(site, lpPages ? lpPages.map((p) => ({ ...p, engine: "lightpanda" })) : undefined, abPages ? abPages.map((p) => ({ ...p, engine: "agent-browser" })) : undefined);
}

/// The homepage, reloaded when it shows nothing. Registration can miss a
/// busy first load (a 51-second GitHub scan of cookiebot.com saw zero tools;
/// a rescan minutes later saw three), so an empty, healthy page gets two
/// more looks spaced a few seconds apart before it counts as empty.
async function loadHomepage(pageFn, url) {
  let first = await pageFn(url);
  for (let i = 0; i < RETRIES && !first.tools.length && !first.error && !first.blocked; i += 1) {
    await sleep(3000);
    const again = await pageFn(url);
    if (again.tools.length || again.error || again.blocked) return again;
    first = again;
  }
  return first;
}

/// A homepage plus the likely pages it links to, for one engine.
async function collectPages(pageFn, entry, maxPages) {
  const url = `https://${entry.replace(/\/$/, "")}/`;
  const pages = [];
  const first = await loadHomepage(pageFn, url);
  pages.push(first);
  if (!first.error && !first.blocked) {
    for (const next of pickPages(first.final_url, first.links, maxPages)) pages.push(await pageFn(next));
  }
  return pages;
}

// ---------------------------------------------------------------------- main

async function main() {
  const sites = readSeeds(SEEDS).filter((s) => !ONLY || ONLY.has(s.entry) || ONLY.has(s.host));
  if (!sites.length) throw new Error(`no sites in ${SEEDS}`);
  const previous = PREVIOUS && existsSync(PREVIOUS) ? JSON.parse(readFileSync(PREVIOUS, "utf8")).sites : [];

  const started = Date.now();
  process.stderr.write(`checking ${sites.length} entries with ${WORKERS} workers per engine\n`);

  // Lightpanda: one MCP server (HTTP) per worker, so browsing sessions are isolated.
  const jump = spawnSync("which", ["lightpanda"], { encoding: "utf8" });
  const haveLightpanda = jump.status === 0 && ENGINES.has("lightpanda");
  const lpResults = new Map();
  if (haveLightpanda) {
    const servers = await Promise.all(
      Array.from({ length: WORKERS }, (_, i) => startLightpanda(8100 + i)),
    );
    let cursor = 0;
    const queue = [...sites];
    const pools = servers.map((lp) => ({ lp }));
    await Promise.all(
      pools.map(async ({ lp }) => {
        await lightpandaInit(lp);
        while (queue.length) {
          const site = queue.shift();
          if (!site) break;
          const pages = [];
          const first = await loadHomepage((url) => lightpandaPage(lp, url), site.url);
          pages.push(first);
          if (!first.error && !first.blocked) {
            for (const url of pickPages(first.final_url, first.links, MAX_PAGES)) {
              const p = await lightpandaPage(lp, url);
              p.engine = "lightpanda";
              pages.push(p);
            }
          }
          for (const p of pages) p.engine = "lightpanda";
          lpResults.set(site.entry, pages);
          cursor += 1;
          process.stderr.write(`lightpanda: ${cursor}/${sites.length} ${site.entry} ${pages.reduce((n, p) => n + p.tools.length, 0)} tools\n`);
        }
      }),
    );
    for (const s of servers) s.child.kill();
  } else {
    process.stderr.write("lightpanda not found; building from agent-browser only\n");
  }

  // agent-browser: one session per worker.
  if (!ENGINES.has("agent-browser")) {
    return finish(sites, lpResults, new Map(), previous, started);
  }
  const sessionBase = process.env.AWM_SESSION || `awm-build-${process.pid}`;
  const sessions = Array.from({ length: WORKERS }, (_, i) => `${sessionBase}-${i}`);
  const abQueue = [...sites];
  const abResults = new Map();
  let abDone = 0;
  await Promise.all(
    sessions.map(async (session) => {
      while (abQueue.length) {
        const site = abQueue.shift();
        if (!site) break;
        const pages = [];
        const first = await loadHomepage((url) => agentBrowserPage(session, url), site.url);
        pages.push(first);
        if (!first.error && !first.blocked) {
          for (const url of pickPages(first.final_url, first.links, MAX_PAGES)) pages.push(await agentBrowserPage(session, url));
        }
        for (const p of pages) p.engine = "agent-browser";
        abResults.set(site.entry, pages);
        abDone += 1;
        process.stderr.write(`agent-browser: ${abDone}/${sites.length} ${site.entry} ${pages.reduce((n, p) => n + p.tools.length, 0)} tools\n`);
      }
      spawnSync("agent-browser", ["--session", session, "close"], { encoding: "utf8", timeout: 30000, env: abEnv() });
    }),
  );

  finish(sites, lpResults, abResults, previous, started);
}

/// Merge what each engine saw, keep sites through one missed check, write.
function finish(sites, lpResults, abResults, previous, started) {
  const merged = sites.map((site) => mergeSite(site, lpResults.get(site.entry), abResults.get(site.entry)));
  const kept = carryForward(merged, previous);
  const totals = merged.reduce((m, s) => ({ ...m, [s.status]: (m[s.status] ?? 0) + 1 }), {});
  const tools = merged.reduce((n, s) => n + s.tools.length, 0);
  const minutes = ((Date.now() - started) / 60000).toFixed(1);

  const index = {
    schema_version: 1,
    generated_at: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
    sites: merged,
  };
  mkdirSync(dirname(OUT), { recursive: true });
  writeFileSync(OUT, `${JSON.stringify(index, null, 2)}\n`);

  process.stderr.write(
    `\nchecked ${sites.length} entries in ${minutes} min: ${JSON.stringify(totals)}, ${tools} tools` +
      `${kept ? `, kept ${kept} from the previous index` : ""}\nwrote ${OUT}\n`,
  );
}

// Importable: `checkSite` scans one site for tools/check-site.mjs. Only a
// direct run builds the index.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((e) => {
    console.error(e);
    process.exit(1);
  });
}
