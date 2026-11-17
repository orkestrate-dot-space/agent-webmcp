#!/usr/bin/env node
// Build index/index.json by loading each seed site in two browsers and keeping
// every tool either of them sees.
//
//   node tools/build-index/build.mjs [--seeds seeds/launch.txt]
//        [--out index/index.json] [--pages 4] [--workers 4]
//        [--previous index/index.json] [--only host,host]
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
  const child = spawn("lightpanda", ["mcp", "--port", String(port), "--host", "127.0.0.1"], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", () => {});
  child.stderr.on("data", () => {});
  const base = `http://127.0.0.1:${port}/mcp`;
  for (let i = 0; i < 100; i += 1) {
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
  child.kill();
  throw new Error(`lightpanda mcp did not start on port ${port}`);
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

async function agentBrowserPage(session, url) {
  const out = { url, final_url: url, tools: [], blocked: false, error: null, links: [] };
  // One process per page. `agent-browser` costs about a second to start, so a
  // call per command made a full run take an hour. Commands go in as JSON on
  // stdin: the argv form strips quotes out of an eval expression.
  // Pages must finish loading before we read tools: wait for the load event
  // and then for the network to go quiet (analytics and chat widgets keep it
  // busy on some sites, so this can time out and that is fine). Tools can
  // also register in stages after load (shop.dev shows 4 then 22), so list,
  // wait, and list again, keeping whichever saw more.
  const commands = [
    ["open", url],
    ["wait", "--load", "load"],
    ["wait", "--load", "networkidle"],
    ["webmcp", "list"],
    ["wait", "2500"],
    ["webmcp", "list"],
    ["eval", LINKS],
    ["get", "url"],
  ];
  const res = spawnSync("agent-browser", ["--session", session, "batch", "--json"], {
    input: JSON.stringify(commands),
    encoding: "utf8",
    timeout: 120_000,
  });
  let batch;
  try {
    batch = JSON.parse((res.stdout || "").trim());
  } catch {
    out.error = (res.stderr || res.stdout || "batch failed").trim().slice(0, 200);
    return out;
  }
  if (!Array.isArray(batch)) {
    out.error = "unexpected batch output";
    return out;
  }
  const result = (i) => batch[i]?.result;
  const opened = result(0);
  if (batch[0]?.error && !opened?.url) {
    out.error = String(batch[0].error).slice(0, 200);
    return out;
  }
  if (opened?.url) out.final_url = opened.url;

  const first = result(3)?.tools ?? [];
  const second = result(5)?.tools ?? [];
  const fromOpen = opened?.webmcp?.tools ?? [];
  const tools = [fromOpen, first, second].reduce((best, list) => (list.length > best.length ? list : best), []);
  out.tools = tools.map((t) => ({
    name: t.name,
    description: t.description ?? "",
    input_schema: t.inputSchema ?? {},
    origin: t.origin ?? "",
    annotations: normAnnotations(t.annotations),
  }));

  const links = result(6)?.result;
  out.links = Array.isArray(links) ? links : [];
  const now = result(7);
  const finalUrl = typeof now === "string" ? now : now?.url ?? "";
  if (/^https?:\/\//.test(finalUrl)) out.final_url = finalUrl;
  out.blocked = /just a moment|attention required|access denied|verify you are human|are you a robot|cf-challenge/i.test(
    `${out.final_url} ${res.stdout.slice(0, 4000)}`,
  );
  const failed = batch.find((b) => b?.error);
  if (failed) out.note = String(failed.error).slice(0, 200);
  return out;
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

/// Check one site with both engines and return its index record.
export async function checkSite(entry, { pages = 4, engines = ["lightpanda", "agent-browser"] } = {}) {
  const start = readSeeds(SEEDS).find((s) => s.entry === entry);
  const host = (start?.host ?? entry.split("/")[0]).replace(/^www\./, "");
  let lpPages;
  let abPages;

  if (engines.includes("lightpanda")) {
    const lp = await startLightpanda(8300 + (process.pid % 200));
    try {
      await lightpandaInit(lp);
      const page = (url) => lightpandaPage(lp, url);
      page.engine = "lightpanda";
      lpPages = await collectPages(page, entry, pages);
    } finally {
      lp.child.kill();
    }
  }
  if (engines.includes("agent-browser")) {
    const session = `awm-check-${process.pid}`;
    try {
      const page = (url) => agentBrowserPage(session, url);
      page.engine = "agent-browser";
      abPages = await collectPages(page, entry, pages);
    } finally {
      spawnSync("agent-browser", ["--session", session, "close"], { encoding: "utf8", timeout: 30_000 });
    }
  }
  const site = {
    entry,
    host: host.toLowerCase(),
    url: `https://${entry.replace(/\/$/, "")}/`,
  };
  return mergeSite(site, lpPages ? lpPages.map((p) => ({ ...p, engine: "lightpanda" })) : undefined, abPages ? abPages.map((p) => ({ ...p, engine: "agent-browser" })) : undefined);
}

/// A homepage plus the likely pages it links to, for one engine.
async function collectPages(pageFn, entry, maxPages) {
  const url = `https://${entry.replace(/\/$/, "")}/`;
  const pages = [];
  let first = await pageFn(url);
  if (!first.tools.length && !first.error && !first.blocked) {
    const again = await pageFn(url);
    if (again.tools.length) first = again;
  }
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
          let first = await lightpandaPage(lp, site.url);
          if (!first.tools.length && !first.error && !first.blocked) {
            const again = await lightpandaPage(lp, site.url);
            if (again.tools.length) first = again;
          }
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
  const sessions = Array.from({ length: WORKERS }, (_, i) => `awm-build-${process.pid}-${i}`);
  const abQueue = [...sites];
  const abResults = new Map();
  let abDone = 0;
  await Promise.all(
    sessions.map(async (session) => {
      while (abQueue.length) {
        const site = abQueue.shift();
        if (!site) break;
        const pages = [];
        let first = await agentBrowserPage(session, site.url);
        if (!first.tools.length && !first.error && !first.blocked) {
          const again = await agentBrowserPage(session, site.url);
          if (again.tools.length) first = again;
        }
        pages.push(first);
        if (!first.error && !first.blocked) {
          for (const url of pickPages(first.final_url, first.links, MAX_PAGES)) pages.push(await agentBrowserPage(session, url));
        }
        for (const p of pages) p.engine = "agent-browser";
        abResults.set(site.entry, pages);
        abDone += 1;
        process.stderr.write(`agent-browser: ${abDone}/${sites.length} ${site.entry} ${pages.reduce((n, p) => n + p.tools.length, 0)} tools\n`);
      }
      spawnSync("agent-browser", ["--session", session, "close"], { encoding: "utf8", timeout: 30000 });
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
