var __defProp = Object.defineProperty;
var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

// src/index.ts
var SCAN_VERSION = 3;
var canonicalHost = /* @__PURE__ */ __name((v) => {
  const h = v.trim().toLowerCase();
  if (h === "" || h.length > 253) return null;
  if (h !== encodeURI(h)) return null;
  if (h.startsWith("-") || h.endsWith("-") || h.includes("..")) return null;
  if (/^(localhost|localhost\.localdomain)$/.test(h)) return null;
  if (/^\d+\.\d+\.\d+\.\d+$/.test(h)) return null;
  if (!/^[a-z0-9.-]+\.[a-z]{2,}$/.test(h)) return null;
  return h;
}, "canonicalHost");
var PROBE_TIMEOUT_MS = 8e3;
var MAX_BODY = 262144;
var timedOut = /* @__PURE__ */ __name((p) => {
  let t;
  try {
    return Promise.race([
      p,
      new Promise((res) => {
        t = setTimeout(() => res(null), PROBE_TIMEOUT_MS);
      })
    ]);
  } finally {
    if (t !== void 0) clearTimeout(t);
  }
}, "timedOut");
var get = /* @__PURE__ */ __name(async (fetchImpl, host, path) => {
  const absent = { ok: false, body: null, contentType: "" };
  try {
    const r = await timedOut(fetchImpl(`https://${host}${path}`, { signal: AbortSignal.timeout(PROBE_TIMEOUT_MS) }));
    if (!r) return absent;
    const ct = r.headers.get("content-type") ?? "";
    const len = r.headers.get("content-length");
    if (len !== null && /^\d+$/.test(len.trim()) && parseInt(len.trim(), 10) > MAX_BODY) return absent;
    const body = r.ok ? await r.text() : null;
    const cut = body !== null && body.length > MAX_BODY ? body.slice(0, MAX_BODY) : body;
    return { ok: r.ok, body: cut, contentType: ct };
  } catch {
    return absent;
  }
}, "get");
var isHtml = /* @__PURE__ */ __name((body) => body !== null && /^\s*</.test(body.slice(0, 2048)), "isHtml");
var isJson = /* @__PURE__ */ __name((body) => {
  if (body === null) return false;
  try {
    JSON.parse(body);
    return true;
  } catch {
    return false;
  }
}, "isJson");
var probeAll = /* @__PURE__ */ __name(async (fetchImpl, host) => {
  const [file, llms, skillsIndex, skillMd, apiCatalog, openapi, mcpCard, oauth, agentCard, home] = await Promise.all([
    get(fetchImpl, host, "/.well-known/agent-webmcp.json"),
    get(fetchImpl, host, "/llms.txt"),
    get(fetchImpl, host, "/.well-known/agent-skills/index.json"),
    get(fetchImpl, host, "/skill.md"),
    get(fetchImpl, host, "/.well-known/api-catalog"),
    get(fetchImpl, host, "/openapi.json"),
    get(fetchImpl, host, "/.well-known/mcp/server-card.json"),
    get(fetchImpl, host, "/.well-known/oauth-protected-resource"),
    get(fetchImpl, host, "/.well-known/agent-card.json"),
    get(fetchImpl, host, "/")
  ]);
  let native = false;
  try {
    const r = await timedOut(
      fetchImpl(`https://webmcp.com/api/v1/lookup?url=https://${host}`, {
        signal: AbortSignal.timeout(PROBE_TIMEOUT_MS)
      })
    );
    if (r?.ok) {
      const j = await r.text().then((t) => JSON.parse(t));
      native = j.supported === true;
    }
  } catch {
    native = false;
  }
  const askHtml = home.ok && home.body !== null && /data-context7-ask|context7\.com\/api/i.test(home.body);
  const askLlms = llms.ok && llms.body !== null && /context7/i.test(llms.body);
  const ask = askHtml || askLlms;
  let parsed = null;
  if (file.ok && isJson(file.body)) {
    const j = JSON.parse(file.body);
    if (typeof j === "object" && j !== null) parsed = j;
  }
  const capText = /* @__PURE__ */ __name((v, n) => typeof v === "string" && v.trim() !== "" ? Array.from(v.trim()).slice(0, n).join("") : "", "capText");
  const fileValid = parsed !== null && typeof parsed.domain === "string" && parsed.domain.toLowerCase() === host;
  const fileDesc = fileValid ? capText(parsed.description, 140) : "";
  const signals = [
    { key: "file", label: "site file", blurb: "owner claim + trigger text", ok: fileValid },
    { key: "llms", label: "llms.txt", blurb: "docs index for agents", ok: llms.ok && !isHtml(llms.body) },
    { key: "skillsIndex", label: "agent skills", blurb: "skill inventory", ok: skillsIndex.ok && isJson(skillsIndex.body) },
    { key: "skillMd", label: "skill.md", blurb: "skill instructions", ok: skillMd.ok && !isHtml(skillMd.body) },
    { key: "apiCatalog", label: "api catalog", blurb: "service catalog", ok: apiCatalog.ok },
    { key: "openapi", label: "openapi.json", blurb: "HTTP API spec", ok: openapi.ok && isJson(openapi.body) },
    { key: "mcp", label: "MCP server card", blurb: "MCP endpoint", ok: mcpCard.ok && isJson(mcpCard.body) },
    { key: "oauth", label: "oauth resource", blurb: "OAuth metadata", ok: oauth.ok },
    { key: "agentCard", label: "agent card", blurb: "agent identity", ok: agentCard.ok && isJson(agentCard.body) },
    { key: "native", label: "native verbs", blurb: "callable page tools", ok: native },
    { key: "ask", label: "ask endpoint", blurb: "queryable docs", ok: ask }
  ];
  return {
    signals,
    file: fileValid ? { domain: host, description: fileDesc } : null,
    native
  };
}, "probeAll");
var hasFromSignals = /* @__PURE__ */ __name((signals, native) => {
  const ok = /* @__PURE__ */ __name((key) => signals.some((s) => s.key === key && s.ok), "ok");
  return {
    webmcp: native,
    skills: ok("skillsIndex") || ok("skillMd"),
    api: ok("apiCatalog") || ok("openapi"),
    mcp: ok("mcp"),
    auth: ok("oauth"),
    ask: ok("ask")
  };
}, "hasFromSignals");
var RESCAN_INTERVAL_MS = 24 * 60 * 60 * 1e3;
var isFresh = /* @__PURE__ */ __name((lastVerified) => {
  if (typeof lastVerified !== "string" || lastVerified === "") return false;
  const t = Date.parse(lastVerified);
  return !Number.isNaN(t) && Date.now() - t < RESCAN_INTERVAL_MS;
}, "isFresh");
var shouldServeCached = /* @__PURE__ */ __name((existing) => {
  if (!existing || !isFresh(existing.last_verified)) return false;
  const signals = parse(existing.signals_json, []);
  return Array.isArray(signals) && signals.length > 0;
}, "shouldServeCached");
var TAG_COLUMNS = {
  webmcp: "has_webmcp",
  skills: "has_skills",
  api: "has_api",
  mcp: "has_mcp",
  auth: "has_auth",
  ask: "has_ask"
};
var PAGE_DEFAULT = 50;
var PAGE_MAX = 100;
var COUNTS_SQL = "SELECT COUNT(*) AS total, COALESCE(SUM(has_webmcp), 0) AS webmcp, COALESCE(SUM(has_skills), 0) AS skills, COALESCE(SUM(has_api), 0) AS api, COALESCE(SUM(has_mcp), 0) AS mcp, COALESCE(SUM(has_auth), 0) AS auth, COALESCE(SUM(has_ask), 0) AS ask FROM sites";
var buildListQuery = /* @__PURE__ */ __name((opts) => {
  const where = [];
  const params = [];
  if (opts.cursor) {
    where.push("domain > ?");
    params.push(opts.cursor);
  }
  const tag = (opts.tag ?? "").trim().toLowerCase();
  if (tag !== "" && TAG_COLUMNS[tag] !== void 0) {
    where.push(`${TAG_COLUMNS[tag]} = 1`);
  }
  const q = (opts.q ?? "").trim().toLowerCase();
  if (q !== "") {
    const esc = q.replace(/[\\%_]/g, (c) => `\\${c}`);
    where.push("(domain LIKE ? ESCAPE '\\' OR LOWER(description) LIKE ? ESCAPE '\\')");
    params.push(`%${esc}%`, `%${esc}%`);
  }
  const n = Math.min(Math.max(opts.limit ?? PAGE_DEFAULT, 1), PAGE_MAX);
  const sql = `SELECT domain, description, description_source, has_json, has_webmcp, has_skills, has_api, has_mcp, has_auth, has_ask, state, state_reason, pinned_ref, last_verified FROM sites` + (where.length > 0 ? ` WHERE ${where.join(" AND ")}` : "") + ` ORDER BY domain LIMIT ${n + 1}`;
  return { sql, params, pageSize: n };
}, "buildListQuery");
var json = /* @__PURE__ */ __name((v, status = 200) => Response.json(v, {
  headers: { "access-control-allow-origin": "*", "cache-control": "no-store" },
  status
}), "json");
var parse = /* @__PURE__ */ __name((s, fallback) => {
  if (typeof s !== "string" || s === "") return fallback;
  try {
    return JSON.parse(s);
  } catch {
    return fallback;
  }
}, "parse");
var EMPTY_HAS = { webmcp: false, skills: false, api: false, mcp: false, auth: false, ask: false };
var num = /* @__PURE__ */ __name((v) => v === 1 || v === true, "num");
var hasOf = /* @__PURE__ */ __name((r) => {
  if (typeof r.has_webmcp === "number" || typeof r.has_webmcp === "boolean") {
    return {
      webmcp: num(r.has_webmcp),
      skills: num(r.has_skills),
      api: num(r.has_api),
      mcp: num(r.has_mcp),
      auth: num(r.has_auth),
      ask: num(r.has_ask)
    };
  }
  return parse(r.has_json, EMPTY_HAS);
}, "hasOf");
var shape = /* @__PURE__ */ __name((r) => ({
  domain: r.domain,
  description: r.description,
  source: typeof r.description_source === "string" ? r.description_source : "manual",
  has: hasOf(r),
  state: r.state,
  reason: r.state_reason ?? "",
  proof: { verified: r.last_verified ?? null, ref: r.pinned_ref ?? "" }
}), "shape");
var shapeDetail = /* @__PURE__ */ __name((r) => ({
  ...shape(r),
  signals: parse(r.signals_json, [])
}), "shapeDetail");
var DETAIL_COLS = "domain, description, description_source, has_json, has_webmcp, has_skills, has_api, has_mcp, has_auth, has_ask, signals_json, state, state_reason, pinned_ref, last_verified";
var today = /* @__PURE__ */ __name(() => (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), "today");
var RATE_WINDOW_MS = 6e4;
var RATE_MAX = 10;
var DAY_MS = 24 * 60 * 60 * 1e3;
var DAY_BUDGET = 2e3;
var budgets = /* @__PURE__ */ new Map();
var scanAllowed = /* @__PURE__ */ __name((ip, now = Date.now()) => {
  let b = budgets.get(ip);
  if (!b || now - b.dayStart >= DAY_MS) {
    b = { windowStart: now, windowCount: 0, dayStart: now, dayCount: 0 };
    budgets.set(ip, b);
  } else if (now - b.windowStart >= RATE_WINDOW_MS) {
    b.windowStart = now;
    b.windowCount = 0;
  }
  if (b.windowCount >= RATE_MAX) {
    return { ok: false, retryAfter: Math.max(1, Math.ceil((b.windowStart + RATE_WINDOW_MS - now) / 1e3)) };
  }
  if (b.dayCount >= DAY_BUDGET) {
    return { ok: false, retryAfter: Math.max(1, Math.ceil((b.dayStart + DAY_MS - now) / 1e3)) };
  }
  b.windowCount += 1;
  b.dayCount += 1;
  if (budgets.size > 5e3) {
    const k = budgets.keys().next().value;
    if (k !== void 0) budgets.delete(k);
  }
  return { ok: true, retryAfter: 0 };
}, "scanAllowed");
var READ_WINDOW_MS = 6e4;
var READ_MAX = 300;
var reads = /* @__PURE__ */ new Map();
var readAllowed = /* @__PURE__ */ __name((ip, now = Date.now()) => {
  let r = reads.get(ip);
  if (!r || now - r.start >= READ_WINDOW_MS) {
    r = { start: now, count: 0 };
    reads.set(ip, r);
  }
  if (r.count >= READ_MAX) {
    return { ok: false, retryAfter: Math.max(1, Math.ceil((r.start + READ_WINDOW_MS - now) / 1e3)) };
  }
  r.count += 1;
  if (reads.size > 5e3) {
    const k = reads.keys().next().value;
    if (k !== void 0) reads.delete(k);
  }
  return { ok: true, retryAfter: 0 };
}, "readAllowed");
var index_default = {
  async fetch(req, env) {
    const url = new URL(req.url);
    if (req.method === "POST") {
      const m2 = /^\/sites\/([^/]+)\/scan$/.exec(url.pathname);
      if (!m2) return json({ error: "unknown path" }, 404);
      const host = canonicalHost(decodeURIComponent(m2[1] ?? ""));
      if (!host) return json({ error: "bad domain" }, 400);
      const ip2 = req.headers.get("cf-connecting-ip") ?? "unknown";
      const allowance = scanAllowed(ip2);
      if (!allowance.ok) {
        const res = json({ error: "slow down" }, 429);
        res.headers.set("Retry-After", String(allowance.retryAfter));
        return res;
      }
      const existing = await env.SITES.prepare(`SELECT ${DETAIL_COLS} FROM sites WHERE domain = ?`).bind(host).first();
      if (existing && shouldServeCached(existing)) {
        return json({ site: shapeDetail(existing), version: SCAN_VERSION, cached: true });
      }
      const probed = await probeAll(fetch, host);
      const has = hasFromSignals(probed.signals, probed.native);
      const description = probed.file !== null && probed.file.description !== "" ? probed.file.description : existing?.description ?? "";
      const source = probed.file !== null && probed.file.description !== "" ? "file" : typeof existing?.description_source === "string" ? existing.description_source : "manual";
      await env.SITES.prepare(
        `INSERT INTO sites (domain, description, description_source, scan_version, has_json, has_webmcp, has_skills, has_api, has_mcp, has_auth, has_ask, signals_json, state, state_reason, pinned_ref, last_verified, updated_at, provenance) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'listed', '', '', ?, datetime('now'), 'scan') ON CONFLICT(domain) DO UPDATE SET description=excluded.description, description_source=excluded.description_source, scan_version=excluded.scan_version, has_json=excluded.has_json, has_webmcp=excluded.has_webmcp, has_skills=excluded.has_skills, has_api=excluded.has_api, has_mcp=excluded.has_mcp, has_auth=excluded.has_auth, has_ask=excluded.has_ask, signals_json=excluded.signals_json, last_verified=excluded.last_verified, updated_at=datetime('now')`
      ).bind(
        host,
        description,
        source,
        SCAN_VERSION,
        JSON.stringify(has),
        has.webmcp ? 1 : 0,
        has.skills ? 1 : 0,
        has.api ? 1 : 0,
        has.mcp ? 1 : 0,
        has.auth ? 1 : 0,
        has.ask ? 1 : 0,
        JSON.stringify(probed.signals),
        today()
      ).run();
      const row = await env.SITES.prepare(`SELECT ${DETAIL_COLS} FROM sites WHERE domain = ?`).bind(host).first();
      if (!row) return json({ error: "index failed" }, 500);
      return json({ site: shapeDetail(row), version: SCAN_VERSION, cached: false });
    }
    if (req.method !== "GET") return json({ error: "read-only" }, 405);
    const ip = req.headers.get("cf-connecting-ip") ?? "unknown";
    if (url.pathname === "/sites") {
      const tagRaw = url.searchParams.get("tag")?.trim() ?? "";
      const tag = tagRaw.toLowerCase();
      if (tag !== "" && TAG_COLUMNS[tag] === void 0) return json({ error: "bad tag" }, 400);
      const cursorRaw = url.searchParams.get("cursor")?.trim() ?? "";
      let cursor = null;
      if (cursorRaw !== "") {
        cursor = canonicalHost(cursorRaw);
        if (!cursor) return json({ error: "bad cursor" }, 400);
      }
      const limitRaw = url.searchParams.get("limit")?.trim() ?? "";
      const limit = limitRaw === "" ? PAGE_DEFAULT : Number.parseInt(limitRaw, 10);
      const q = url.searchParams.get("q")?.trim() ?? "";
      const built = buildListQuery({
        tag,
        q,
        cursor,
        limit: Number.isNaN(limit) ? PAGE_DEFAULT : limit
      });
      const read = readAllowed(ip);
      if (!read.ok) {
        const res = json({ error: "slow down" }, 429);
        res.headers.set("Retry-After", String(read.retryAfter));
        return res;
      }
      const rows = await env.SITES.prepare(built.sql).bind(...built.params).all();
      const found = (rows.results ?? []).map(shape);
      const hasNext = found.length > built.pageSize;
      const sites = hasNext ? found.slice(0, built.pageSize) : found;
      const nextCursor = hasNext ? sites[sites.length - 1]?.["domain"] ?? null : null;
      return json({ sites, nextCursor });
    }
    if (url.pathname === "/sites/counts") {
      const read = readAllowed(ip);
      if (!read.ok) {
        const res = json({ error: "slow down" }, 429);
        res.headers.set("Retry-After", String(read.retryAfter));
        return res;
      }
      const row = await env.SITES.prepare(COUNTS_SQL).first();
      const n = /* @__PURE__ */ __name((k) => typeof row?.[k] === "number" ? row[k] : 0, "n");
      return json({
        counts: {
          total: n("total"),
          webmcp: n("webmcp"),
          skills: n("skills"),
          api: n("api"),
          mcp: n("mcp"),
          auth: n("auth"),
          ask: n("ask")
        }
      });
    }
    const m = /^\/sites\/([^/]+)$/.exec(url.pathname);
    if (m) {
      const host = canonicalHost(m[1] ?? "");
      if (!host) return json({ error: "bad domain" }, 400);
      const read = readAllowed(ip);
      if (!read.ok) {
        const res = json({ error: "slow down" }, 429);
        res.headers.set("Retry-After", String(read.retryAfter));
        return res;
      }
      const row = await env.SITES.prepare(`SELECT ${DETAIL_COLS} FROM sites WHERE domain = ?`).bind(host).first();
      if (!row) return json({ error: "not listed" }, 404);
      return json({ site: shapeDetail(row), version: SCAN_VERSION });
    }
    if (url.pathname === "/health") return json({ ok: true });
    if (url.pathname === "/") {
      return json({
        name: "agent-webmcp-sites",
        description: "Agent-ready site verdicts: identity, trigger, proof. Scans persist.",
        endpoints: ["/sites", "/sites/:domain", "POST /sites/:domain/scan", "/health"]
      });
    }
    return json({ error: "unknown path" }, 404);
  }
};
export {
  COUNTS_SQL,
  DAY_BUDGET,
  MAX_BODY,
  PAGE_DEFAULT,
  PAGE_MAX,
  PROBE_TIMEOUT_MS,
  RESCAN_INTERVAL_MS,
  SCAN_VERSION,
  TAG_COLUMNS,
  buildListQuery,
  canonicalHost,
  index_default as default,
  hasFromSignals,
  isFresh,
  probeAll,
  readAllowed,
  scanAllowed,
  shouldServeCached
};
//# sourceMappingURL=index.js.map
