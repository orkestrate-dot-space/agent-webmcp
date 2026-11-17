/// The published index: read, splice, remove, publish.
///
/// The pointer is the only mutable key. A version is written once and never
/// replaced, so a client reading during a publish sees either the old pointer
/// or the new one, and both name a complete file.

import type { Env, IndexFile, Pointer, SiteRecord } from "./env";

const POINTER_KEY = "pointer";
const DENYLIST_KEY = "denylist";
const SEEDS_KEY = "seeds";

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function readPointer(env: Env): Promise<Pointer | null> {
  const raw = await env.INDEX.get(POINTER_KEY);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as Pointer;
  } catch {
    return null;
  }
}

export async function readIndex(env: Env): Promise<IndexFile | null> {
  const pointer = await readPointer(env);
  if (!pointer) return null;
  const file = pointer.url.split("/").pop() ?? "";
  const raw = await env.INDEX.get(`version:${file}`);
  if (!raw) return null;
  try {
    const index = JSON.parse(raw) as IndexFile;
    return index.schema_version === 1 && Array.isArray(index.sites) ? index : null;
  } catch {
    return null;
  }
}

export async function readDenylist(env: Env): Promise<string[]> {
  const raw = await env.INDEX.get(DENYLIST_KEY);
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as { hosts?: unknown };
    return Array.isArray(parsed.hosts) ? parsed.hosts.filter((h): h is string => typeof h === "string") : [];
  } catch {
    return [];
  }
}

/// Write a new version, then the pointer. Returns what was published.
export async function publishIndex(env: Env, index: IndexFile): Promise<Pointer> {
  const text = `${JSON.stringify(index, null, 2)}\n`;
  const sha = await sha256Hex(text);
  const file = `index-${sha.slice(0, 12)}.json`;
  const verified = index.sites.filter((s) => s.status === "verified");
  const pointer: Pointer = {
    schema_version: index.schema_version,
    generated_at: index.generated_at,
    sha256: sha,
    url: `indexes/${file}`,
    bytes: new TextEncoder().encode(text).length,
    sites: verified.length,
    tools: verified.reduce((n, s) => n + s.tools.length, 0),
  };
  await env.INDEX.put(`version:${file}`, text);
  await env.INDEX.put(POINTER_KEY, `${JSON.stringify(pointer, null, 2)}\n`);
  return pointer;
}

export async function writeDenylist(env: Env, hosts: string[]): Promise<void> {
  const unique = [...new Set(hosts)].sort();
  await env.INDEX.put(DENYLIST_KEY, `${JSON.stringify({ hosts: unique }, null, 2)}\n`);
}

/// Replace the record for the same host and start page, or add it.
export function spliceRecord(index: IndexFile, record: SiteRecord): { replaced: number } {
  const keyOf = (s: SiteRecord) => `${s.host} ${s.pages_checked[0] ?? ""}`;
  const key = keyOf(record);
  const before = index.sites.length;
  index.sites = index.sites.filter((s) => !(s.host === record.host && keyOf(s) === key));
  index.sites.push(record);
  return { replaced: before - index.sites.length };
}

export function removeHost(index: IndexFile, host: string): number {
  const before = index.sites.length;
  index.sites = index.sites.filter((s) => s.host !== host);
  return before - index.sites.length;
}

export function touchGeneratedAt(index: IndexFile): void {
  index.generated_at = new Date().toISOString().replace(/\.\d+Z$/, "Z");
}

export async function readSeeds(env: Env): Promise<string> {
  return (await env.INDEX.get(SEEDS_KEY)) ?? "";
}

/// A listing must survive the next rebuild, which only scans the seed list.
/// Call this whenever a site is listed outside a rebuild.
export async function addSeed(env: Env, entry: string): Promise<boolean> {
  const seeds = await readSeeds(env);
  const present = seeds.split("\n").some((l) => {
    const t = l
      .replace(/(^|\s)#\s.*$/, "")
      .trim()
      .replace(/^https?:\/\//, "")
      .replace(/\/$/, "");
    return t === entry;
  });
  if (present) return false;
  const block = seeds.includes("# Submissions") ? "" : "\n# Submissions (added automatically when a site is listed)\n";
  await writeSeeds(env, `${seeds.replace(/\n?$/, "\n")}${block}${entry}\n`);
  return true;
}

export async function writeSeeds(env: Env, text: string): Promise<void> {
  await env.INDEX.put(SEEDS_KEY, text);
}

/// Why a verified record must not be listed. Empty means it may be.
export function listingBlockers(record: SiteRecord, denylist: string[]): string[] {
  const blockers: string[] = [];
  if (record.status !== "verified") blockers.push("not verified");
  if (!record.tools.length) blockers.push("no tools");
  if (denylist.includes(record.host)) blockers.push("on the takedown list");
  for (const t of record.tools) {
    if (!t.description.trim()) blockers.push(`${t.name}: no description`);
    if (t.flags?.length) blockers.push(`${t.name}: flagged ${t.flags.join(", ")}`);
  }
  return blockers;
}
