/// Validation for submissions and for the records the box sends back.
///
/// Submissions and callback records are validated here. DNS uses DNS over HTTPS
/// because a Worker cannot call the resolver directly. DNS is checked once at
/// submit time (resolvesPublic refuses private, loopback and metadata
/// answers); a host that rebinds to such an address afterwards is still
/// contained by the network-layer guard for the scanner user on IPv4 and
/// IPv6, which covers every fetch the browser makes no matter what DNS
/// says later.

import type { SiteRecord } from "./env";

const EXTRA_PAGES_MAX = 3;
const MAX_DESCRIPTION = 5000;
const MAX_TOOLS = 200;

/// Private, loopback, link-local, CGNAT and cloud-metadata ranges.
const BLOCKED: Array<[string, number]> = [
  ["10.0.0.0", 8],
  ["172.16.0.0", 12],
  ["192.168.0.0", 16],
  ["127.0.0.0", 8],
  ["169.254.0.0", 16],
  ["100.64.0.0", 10],
  ["0.0.0.0", 8],
  ["::1", 128],
  ["fe80::", 10],
  ["fc00::", 7],
  ["::ffff:0:0", 96],
  ["::", 128],
];

function ipToBig(ip: string): bigint | null {
  if (ip.includes(".") && !ip.includes(":")) {
    const parts = ip.split(".");
    if (parts.length !== 4) return null;
    return parts.reduce((a, b) => (a << 8n) + BigInt(Number(b) & 255), 0n);
  }
  if (!ip.includes(":")) return null;
  const [headRaw, tailRaw] = ip.split("::");
  const head = headRaw ? headRaw.split(":") : [];
  const tail = tailRaw !== undefined && tailRaw ? tailRaw.split(":") : [];
  const fill = 8 - head.length - tail.length;
  if (fill < 0) return null;
  const parts = [...head, ...Array(fill).fill("0"), ...tail];
  if (parts.length !== 8) return null;
  return parts.reduce((a, b) => (a << 16n) + BigInt(parseInt(b || "0", 16) || 0), 0n);
}

function isBlockedIp(ip: string): boolean {
  const v4 = !ip.includes(":");
  const n = ipToBig(ip);
  if (n === null) return false;
  for (const [cidr, prefix] of BLOCKED) {
    if (v4 !== !cidr.includes(":")) continue;
    const base = ipToBig(cidr);
    if (base === null) continue;
    const bits = v4 ? 32 : 128;
    const shift = BigInt(bits - prefix);
    if (base >> shift === n >> shift) return true;
  }
  return false;
}

export interface Submission {
  url: string;
  pages: string;
}

export interface Accepted {
  ok: true;
  entry: string;
  host: string;
  urls: string[];
}
export interface Rejected {
  ok: false;
  reason: string;
}

function parseHttps(raw: string): { ok: true; url: URL } | Rejected {
  let u: URL;
  try {
    u = new URL(raw.trim());
  } catch {
    return { ok: false, reason: `not a URL: ${raw.slice(0, 80)}` };
  }
  if (u.protocol !== "https:") return { ok: false, reason: "only https: URLs are checked" };
  if (u.username || u.password) return { ok: false, reason: "credentials in URLs are rejected" };
  if (u.port && u.port !== "443") return { ok: false, reason: "only the default port is checked" };
  if (!/^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$/i.test(u.hostname) || u.hostname.length > 253) {
    return { ok: false, reason: "bad hostname" };
  }
  // Sites are host names. An address literal has no DNS answer to check and
  // can point straight at a private or metadata endpoint.
  if (/^[0-9.]+$/.test(u.hostname) || u.hostname.includes(":")) {
    return { ok: false, reason: "submit a host name, not an IP address" };
  }
  return { ok: true, url: u };
}

/// The entry a record is keyed by: host, or host/path when the site lives
/// under one. Comments are stripped so fragments survive.
export function entryFor(raw: string): string {
  return raw
    .replace(/(^|\s)#\s.*$/, "")
    .trim()
    .replace(/^https?:\/\//, "")
    .split("#")[0]
    .replace(/\/$/, "");
}

async function resolvesPublic(host: string): Promise<{ ok: boolean; reason?: string }> {
  let answered = false;
  for (const type of ["A", "AAAA"]) {
    let data: { Status?: number; Answer?: Array<{ data?: string }> };
    try {
      const res = await fetch(
        `https://cloudflare-dns.com/dns-query?name=${encodeURIComponent(host)}&type=${type}`,
        { headers: { accept: "application/dns-json" } },
      );
      if (!res.ok) return { ok: false, reason: `DNS lookup failed for ${host}` };
      data = (await res.json()) as typeof data;
    } catch {
      return { ok: false, reason: `DNS lookup failed for ${host}` };
    }
    if (data.Status !== 0) return { ok: false, reason: `${host} does not resolve` };
    for (const a of data.Answer ?? []) {
      const ip = typeof a.data === "string" ? a.data.trim() : "";
      // A CNAME answer is a name, not an address: skip it rather than feeding
      // a name to the IP parser.
      if (!/^[0-9.]+$/.test(ip) && !ip.includes(":")) continue;
      answered = true;
      if (isBlockedIp(ip)) {
        return { ok: false, reason: `${host} resolves to a non-public address` };
      }
    }
  }
  // No A or AAAA answer means there is nothing public to load.
  if (!answered) return { ok: false, reason: `${host} does not resolve to a public address` };
  return { ok: true };
}

export async function validateSubmission(input: Submission): Promise<Accepted | Rejected> {
  const main = parseHttps(input.url ?? "");
  if (!main.ok) return main;
  const extra = String(input.pages ?? "")
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .slice(0, EXTRA_PAGES_MAX);
  const urls = [main.url];
  for (const raw of extra) {
    const p = parseHttps(raw);
    if (!p.ok) return { ok: false, reason: `extra page rejected: ${p.reason}` };
    urls.push(p.url);
  }
  for (const u of urls) {
    const dns = await resolvesPublic(u.hostname);
    if (!dns.ok) return { ok: false, reason: dns.reason ?? "DNS rejected" };
  }
  const entry = entryFor(input.url);
  return { ok: true, entry, host: entry.split("/")[0].toLowerCase().replace(/^www\./, ""), urls: urls.map((u) => u.href) };
}

const RECORD_KEYS = new Set(["host", "status", "checked_at", "pages_checked", "platform", "tools", "note", "missed", "engines"]);
const TOOL_KEYS = new Set(["name", "description", "input_schema", "page", "origin", "annotations", "flags", "seen_by"]);

/// Hostile input: the record comes from a box that loaded a stranger's page.
export function validateRecord(record: unknown, entry: string): { ok: true; record: SiteRecord } | Rejected {
  const fail = (reason: string) => ({ ok: false as const, reason });
  if (!record || typeof record !== "object" || Array.isArray(record)) return fail("not an object");
  const rec = record as Record<string, unknown>;
  if (JSON.stringify(record).length > 500_000) return fail("record too large");
  for (const k of Object.keys(rec)) if (!RECORD_KEYS.has(k)) return fail(`unexpected key ${k}`);
  const wantHost = entry.split("/")[0].toLowerCase().replace(/^www\./, "");
  if (rec.host !== wantHost) return fail(`host ${String(rec.host)} does not match submission ${wantHost}`);
  if (!["verified", "no_tools", "blocked", "error"].includes(String(rec.status))) return fail("bad status");
  if (typeof rec.checked_at !== "string" || Number.isNaN(Date.parse(rec.checked_at))) return fail("bad checked_at");
  if (!Array.isArray(rec.pages_checked) || rec.pages_checked.length > 30) return fail("bad pages_checked");
  for (const p of rec.pages_checked) {
    let u: URL;
    try {
      u = new URL(String(p));
    } catch {
      return fail("pages_checked has a non-URL");
    }
    if (u.protocol !== "https:" || u.hostname.toLowerCase().replace(/^www\./, "") !== wantHost) {
      return fail("pages_checked leaves the submitted host");
    }
  }
  if (!Array.isArray(rec.tools) || rec.tools.length > MAX_TOOLS) return fail("bad tools");
  for (const raw of rec.tools) {
    if (!raw || typeof raw !== "object") return fail("bad tool");
    const t = raw as Record<string, unknown>;
    for (const k of Object.keys(t)) if (!TOOL_KEYS.has(k)) return fail(`unexpected tool key ${k}`);
    if (typeof t.name !== "string" || !t.name || t.name.length > 200) return fail("bad tool name");
    if (typeof t.description !== "string" || t.description.length > MAX_DESCRIPTION) return fail("bad tool description");
    if (typeof t.input_schema !== "object" || t.input_schema === null) return fail("bad input_schema");
    if (typeof t.page !== "string") return fail("bad tool page");
    // A tool page must be an https URL on the submitted host: it is rendered
    // as a link and used by agents to open the page.
    let toolPage: URL;
    try {
      toolPage = new URL(t.page);
    } catch {
      return fail("tool page is not a URL");
    }
    if (toolPage.protocol !== "https:" || toolPage.hostname.toLowerCase().replace(/^www\./, "") !== wantHost) {
      return fail("tool page leaves the submitted host");
    }
    if ("flags" in t) {
      if (!Array.isArray(t.flags) || t.flags.length > 20 || t.flags.some((f) => typeof f !== "string" || f.length > 64)) {
        return fail("bad flags");
      }
    }
    if ("annotations" in t && t.annotations !== undefined) {
      if (!t.annotations || typeof t.annotations !== "object" || Array.isArray(t.annotations)) {
        return fail("bad annotations");
      }
      const a = t.annotations as Record<string, unknown>;
      for (const k of ["read_only", "consequential", "untrusted_content"]) {
        if (k in a && typeof a[k] !== "boolean") return fail("bad annotation");
      }
    }
    if ("seen_by" in t) {
      if (!Array.isArray(t.seen_by) || t.seen_by.some((e) => typeof e !== "string")) return fail("bad seen_by");
    }
  }
  return { ok: true, record: rec as unknown as SiteRecord };
}

/// A host name as accepted by the takedown form: bare, lowercase, no path.
export function parseHost(raw: string): { ok: true; host: string } | Rejected {
  const host = String(raw ?? "")
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, "")
    .split("/")[0]
    .replace(/^www\./, "");
  if (!/^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$/.test(host) || host.length > 253 || !host.includes(".")) {
    return { ok: false, reason: "not a host name" };
  }
  return { ok: true, host };
}
