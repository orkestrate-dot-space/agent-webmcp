#!/usr/bin/env node
// Validation for site submissions. Two jobs share this module:
//
//   validateSubmission({ url, pages })
//     Parse the issue-form fields. Every URL must be public https. DNS is
//     resolved and any private/loopback/link-local/cloud-metadata address
//     rejects the request before a browser ever loads it.
//
//   validateResult(record, { entry })
//     Strictly check a scan-result artifact before it can touch the index.
//     The artifact comes from an untrusted run, so this treats every field
//     as hostile: types, sizes, host allowlist, no extra keys.

import { lookup } from "node:dns/promises";
import { isIP } from "node:net";

const EXTRA_PAGES_MAX = 3;
const MAX_DESCRIPTION = 5000;
const MAX_TOOLS = 200;
const MAX_RECORD_BYTES = 500_000;

// Ranges a scanned host must never resolve to.
const BLOCKED = [
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

function ipToBig(ip) {
  const v = isIP(ip);
  if (v === 4) return { v, n: ip.split(".").reduce((a, b) => a * 256n + BigInt(b), 0n) };
  const expanded = ip.split("::");
  let head = expanded[0] ? expanded[0].split(":") : [];
  let tail = expanded[1] ? expanded[1].split(":") : [];
  while (head.length + tail.length < 8) head.push("0");
  const parts = [...head, ...tail].slice(0, 8);
  return { v, n: parts.reduce((a, b) => (a << 16n) + BigInt(parseInt(b || "0", 16)), 0n) };
}

function cidrContains(cidr, prefix, ip) {
  const a = ipToBig(cidr);
  const b = ipToBig(ip);
  if (a.v !== b.v) return false;
  const bits = a.v === 4 ? 32 : 128;
  const shift = BigInt(bits - prefix);
  return (a.n >> shift) === (b.n >> shift);
}

function isBlockedIp(ip) {
  return BLOCKED.some(([cidr, prefix]) => cidrContains(cidr, prefix, ip));
}

function parseHttps(raw) {
  let u;
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
  return { ok: true, url: u };
}

async function hostResolvesPublic(hostname) {
  let addrs;
  try {
    addrs = await lookup(hostname, { all: true });
  } catch {
    return { ok: false, reason: `DNS does not resolve: ${hostname}` };
  }
  const bad = addrs.map((a) => a.address).filter(isBlockedIp);
  if (bad.length) return { ok: false, reason: `${hostname} resolves to a non-public address` };
  return { ok: true };
}

/// The seed-list entry for a URL: host, or host/path when the site lives
/// under one. Comment lines (# ...) are stripped so fragments survive.
export function entryFor(raw) {
  const cleaned = raw
    .replace(/(^|\s)#\s.*$/, "")
    .trim()
    .replace(/^https?:\/\//, "")
    .split("#")[0]
    .replace(/\/$/, "");
  return cleaned;
}

export async function validateSubmission({ url, pages }) {
  const main = parseHttps(url ?? "");
  if (!main.ok) return main;
  const extra = String(pages ?? "")
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
    const dns = await hostResolvesPublic(u.hostname);
    if (!dns.ok) return dns;
  }
  return { ok: true, entry: entryFor(url), urls: urls.map((u) => u.href) };
}

const RECORD_KEYS = new Set([
  "host",
  "status",
  "checked_at",
  "pages_checked",
  "platform",
  "tools",
  "note",
  "missed",
  "engines",
]);
const TOOL_KEYS = new Set(["name", "description", "input_schema", "page", "origin", "annotations", "flags", "seen_by"]);

export function validateResult(record, { entry }) {
  const fail = (reason) => ({ ok: false, reason });
  if (!record || typeof record !== "object" || Array.isArray(record)) return fail("not an object");
  if (JSON.stringify(record).length > MAX_RECORD_BYTES) return fail("record too large");
  for (const k of Object.keys(record)) if (!RECORD_KEYS.has(k)) return fail(`unexpected key ${k}`);
  const wantHost = String(entry).split("/")[0].toLowerCase().replace(/^www\./, "");
  if (record.host !== wantHost) return fail(`host ${record.host} does not match submission ${wantHost}`);
  if (!["verified", "no_tools", "blocked", "error"].includes(record.status)) return fail("bad status");
  if (typeof record.checked_at !== "string" || Number.isNaN(Date.parse(record.checked_at))) return fail("bad checked_at");
  if (!Array.isArray(record.pages_checked) || record.pages_checked.length > 30) return fail("bad pages_checked");
  for (const p of record.pages_checked) {
    let u;
    try {
      u = new URL(p);
    } catch {
      return fail("pages_checked has a non-URL");
    }
    if (u.protocol !== "https:" || u.hostname.toLowerCase().replace(/^www\./, "") !== wantHost) {
      return fail("pages_checked leaves the submitted host");
    }
  }
  if (!Array.isArray(record.tools) || record.tools.length > MAX_TOOLS) return fail("bad tools");
  for (const t of record.tools) {
    if (!t || typeof t !== "object") return fail("bad tool");
    for (const k of Object.keys(t)) if (!TOOL_KEYS.has(k)) return fail(`unexpected tool key ${k}`);
    if (typeof t.name !== "string" || !t.name || t.name.length > 200) return fail("bad tool name");
    if (typeof t.description !== "string" || t.description.length > MAX_DESCRIPTION) return fail("bad tool description");
    if (typeof t.input_schema !== "object" || t.input_schema === null) return fail("bad input_schema");
    if (typeof t.page !== "string") return fail("bad tool page");
    const a = t.annotations ?? {};
    for (const k of ["read_only", "consequential", "untrusted_content"]) {
      if (k in a && typeof a[k] !== "boolean") return fail("bad annotation");
    }
  }
  return { ok: true };
}
