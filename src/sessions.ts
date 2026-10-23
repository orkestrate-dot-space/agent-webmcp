// Sessions: open/close/list over disk handles against a browser we don't
// own. Tabs are created fresh per open in a foreign browser; close drops the
// record and our tab, never the browser. Record is {handle, targetId,
// origin} — authored tool lists live on disk (by origin), not here.
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { connect, discoverDaemonBrowser, discoverWs, type CdpConnection } from "./transport.ts";
import { fail } from "./result.ts";

export interface SessionRecord { readonly handle: string; readonly url: string; readonly origin: string; readonly targetId: string; readonly createdAt: number; readonly cdpEndpoint?: string }

const sessionRoot = (): string => `${tmpdir()}/agent-webmcp-sessions`;
const recordPath = (handle: string): string => `${sessionRoot()}/${handle}.json`;

function ensureRoot(): void {
  try { mkdirSync(sessionRoot(), { recursive: true }); } catch { throw fail("io", "session store unwritable", "disk or permissions."); }
}

export function loadRecord(handle: string): SessionRecord {
  ensureRoot();
  try {
    const rec = JSON.parse(readFileSync(recordPath(handle), "utf8")) as SessionRecord;
    if (typeof rec.targetId !== "string" || typeof rec.origin !== "string") throw 0;
    return rec;
  } catch {
    throw fail("bad-handle", `unknown session '${handle.slice(0, 32)}'`, "open again for a fresh handle.");
  }
}

export function saveRecord(rec: SessionRecord): void {
  ensureRoot();
  writeFileSync(recordPath(rec.handle), JSON.stringify(rec));
}

export function dropRecord(handle: string): void {
  try { rmSync(recordPath(handle), { force: true }); } catch { /* already gone */ }
}

export function listRecords(): Array<SessionRecord> {
  ensureRoot();
  let files: Array<string> = [];
  try { files = readdirSync(sessionRoot()).filter((f) => f.endsWith(".json")); } catch { return []; }
  const out: Array<SessionRecord> = [];
  for (const f of files) {
    try {
      const rec = JSON.parse(readFileSync(`${sessionRoot()}/${f}`, "utf8")) as SessionRecord;
      if (typeof rec.targetId === "string" && typeof rec.origin === "string") out.push(rec);
    } catch { /* corrupt record — skip, close prunes */ }
  }
  return out;
}

// Per-tab mutex (cross-process): at most one execute runs against a tab at
// a time. Locks are directories (mkdir is atomic); the owner file names the
// holder. A lock whose owner pid is dead is stale — steal it.
const lockDir = (targetId: string): string => `${tmpdir()}/agent-webmcp-locks/${targetId.replace(/[^A-Za-z0-9_-]/g, "_")}`;

function readOwner(targetId: string): { pid: number; handle: string } | undefined {
  try {
    const parsed = JSON.parse(readFileSync(`${lockDir(targetId)}/owner`, "utf8")) as { pid?: unknown; handle?: unknown };
    if (typeof parsed.pid !== "number" || typeof parsed.handle !== "string") return undefined;
    return { pid: parsed.pid, handle: parsed.handle };
  } catch { return undefined; }
}

function ownerAlive(pid: number): boolean {
  try { process.kill(pid, 0); return true; }
  catch (e) { return (e as { code?: string } | null)?.code !== "ESRCH"; }
}

function clearLock(targetId: string): void {
  try { rmSync(lockDir(targetId), { recursive: true, force: true }); } catch { /* lost a race */ }
}

// Returns the holder's handle when contended, undefined once acquired.
export function acquireTabLock(targetId: string, handle: string): string | undefined {
  try { mkdirSync(`${tmpdir()}/agent-webmcp-locks`, { recursive: true }); } catch { return handle; }
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      mkdirSync(lockDir(targetId));
      try { writeFileSync(`${lockDir(targetId)}/owner`, JSON.stringify({ pid: process.pid, handle })); }
      catch { clearLock(targetId); return handle; }
      return undefined;
    } catch {
      const owner = readOwner(targetId);
      if (owner === undefined) continue;
      if (!ownerAlive(owner.pid)) { clearLock(targetId); continue; }
      return owner.handle;
    }
  }
  const owner = readOwner(targetId);
  return owner?.handle;
}

export function releaseTabLock(targetId: string, handle: string): void {
  const owner = readOwner(targetId);
  if (owner !== undefined && owner.handle === handle) clearLock(targetId);
}

async function browserEndpoint(explicit?: string): Promise<string> {
  if (explicit !== undefined) return explicit;
  const found = await discoverDaemonBrowser();
  if (found === undefined) {
    throw fail("connect", "no browser reachable: start an agent-browser daemon or pass --cdp URL (your driver skill knows it).", "agent-browser open about:blank (daemon), or point --cdp at any CDP endpoint.");
  }
  return found;
}

export interface Opened {
  readonly handle: string; readonly url: string; readonly origin: string;
  readonly targetId: string; readonly cdpUrl: string; readonly createdAt: number;
}

export async function openSession(url: string, endpoint?: string): Promise<{ conn: CdpConnection; sessionId: string; opened: Opened }> {
  let parsed: URL;
  try { parsed = new URL(url); } catch { throw fail("bad-url", `not a URL: ${url.slice(0, 120)}`, "pass an absolute http(s) URL."); }
  const httpEndpoint = await browserEndpoint(endpoint);
  const wsUrl = await discoverWs(httpEndpoint, 5000);
  const conn = await connect(wsUrl);
  try {
    const created = (await conn.send("Target.createTarget", { url })) as { targetId: string };
    const attached = (await conn.send("Target.attachToTarget", { targetId: created.targetId, flatten: true })) as { sessionId: string };
    const handle = `s_${crypto.randomUUID().replaceAll("-", "").slice(0, 12)}`;
    const rec: SessionRecord = {
      handle, url, origin: parsed.origin, targetId: created.targetId, createdAt: Date.now(),
      ...(endpoint !== undefined ? { cdpEndpoint: endpoint } : {}),
    };
    saveRecord(rec);
    return { conn, sessionId: attached.sessionId, opened: { handle, url, origin: parsed.origin, targetId: created.targetId, cdpUrl: httpEndpoint, createdAt: rec.createdAt } };
  } catch (e) {
    await conn.close();
    throw e;
  }
}

// Reattach: dial the recorded session's tab. A dead TAB drops the record
// (tabs never come back); a dead BROWSER fails loud and keeps it.
export async function reattach(record: SessionRecord): Promise<{ conn: CdpConnection; sessionId: string }> {
  const httpEndpoint = record.cdpEndpoint ?? await browserEndpoint();
  const wsUrl = await discoverWs(httpEndpoint, 5000);
  const conn = await connect(wsUrl);
  try {
    const attached = (await conn.send("Target.attachToTarget", { targetId: record.targetId, flatten: true })) as { sessionId: string };
    return { conn, sessionId: attached.sessionId };
  } catch {
    await conn.close();
    const alive = await discoverWs(httpEndpoint, 3000).then(() => true, () => false);
    if (!alive) throw fail("connect", `browser unreachable (record ${record.handle} kept) — start it again.`, "start an agent-browser daemon, or point --cdp at the browser.");
    dropRecord(record.handle);
    throw fail("page", `tab closed since open (record ${record.handle} dropped) — open again.`, "open the url again for a fresh tab.");
  }
}

export async function closeSession(handle: string): Promise<void> {
  const record = loadRecord(handle);
  const endpoint = record.cdpEndpoint ?? await discoverDaemonBrowser();
  if (endpoint !== undefined) {
    try {
      const wsUrl = await discoverWs(endpoint, 5000);
      const conn = await connect(wsUrl);
      try { await conn.send("Target.closeTarget", { targetId: record.targetId }); } finally { conn.close(); }
    } catch { /* tab or browser already gone — success either way */ }
  }
  dropRecord(handle);
}
