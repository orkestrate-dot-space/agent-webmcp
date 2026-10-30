// Sessions: open/close over disk handles against a browser we
// don't own. open mints a fresh tab ({url}) or borrows a live one
// ({targetId}); close reaps ours, never theirs. Records live here,
// nothing else does — no registry, no stamps.
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { connect, discoverDaemonBrowser, discoverWs, type CdpConnection } from "./transport.ts";
import { fail } from "./result.ts";

export interface SessionRecord { readonly handle: string; readonly url: string; readonly origin: string; readonly targetId: string; readonly createdAt: number; readonly cdpEndpoint?: string; readonly owned: boolean }

// Records written before owned existed are ours (open always minted).
function normalize(rec: SessionRecord): SessionRecord {
  return { ...rec, owned: rec.owned !== false };
}

const sessionRoot = (): string => `${tmpdir()}/agent-webmcp-sessions`;
const HANDLE_RE = /^s_[0-9a-f]{12}$/;

const recordPath = (handle: string): string => {
  // Handles are engine-minted (open/attach); anything else shaped is a
  // traversal attempt, not a lookup miss — refuse before touching the fs.
  if (!HANDLE_RE.test(handle)) throw fail("bad-handle", `bad session handle '${handle.slice(0, 32)}'`, "handles come from open — pass one back unchanged.");
  return `${sessionRoot()}/${handle}.json`;
};

function ensureRoot(): void {
  try { mkdirSync(sessionRoot(), { recursive: true }); } catch { throw fail("io", "session store unwritable", "disk or permissions."); }
}

export function loadRecord(handle: string): SessionRecord {
  ensureRoot();
  try {
    const rec = JSON.parse(readFileSync(recordPath(handle), "utf8")) as SessionRecord;
    if (typeof rec.targetId !== "string" || typeof rec.origin !== "string") throw 0;
    return normalize(rec);
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
      if (typeof rec.targetId === "string" && typeof rec.origin === "string") out.push(normalize(rec));
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
  readonly owned: boolean;
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
      handle, url, origin: parsed.origin, targetId: created.targetId, createdAt: Date.now(), owned: true,
      ...(endpoint !== undefined ? { cdpEndpoint: endpoint } : {}),
    };
    saveRecord(rec);
    return { conn, sessionId: attached.sessionId, opened: { handle, url, origin: parsed.origin, targetId: created.targetId, cdpUrl: httpEndpoint, createdAt: rec.createdAt, owned: true } };
  } catch (e) {
    await conn.close();
    throw e;
  }
}

export interface AttachSelector { readonly targetId?: string; readonly url?: string }

interface TargetInfo { readonly targetId: string; readonly type: string; readonly url: string }

// Borrow a live tab the browser already owns. Some debuggers (Electron
// app windows) refuse Target.createTarget, so open can never mint there —
// attach meets the tab where it lives. The record is owned:false: close
// detaches, never closes the tab.
export async function attachSession(sel: AttachSelector, endpoint?: string): Promise<{ conn: CdpConnection; sessionId: string; opened: Opened }> {
  const { targetId, url } = sel;
  if (targetId === undefined && url === undefined) throw fail("bad-args", "attach needs {targetId} or {url}.", "point at a live tab the browser already owns.");
  if (url !== undefined) {
    try { void new URL(url); } catch { throw fail("bad-url", `not a URL: ${url.slice(0, 120)}`, "pass an absolute http(s) URL."); }
  }
  const httpEndpoint = await browserEndpoint(endpoint);
  const wsUrl = await discoverWs(httpEndpoint, 5000);
  const conn = await connect(wsUrl);
  try {
    const targets = (await conn.send("Target.getTargets", {})) as { targetInfos?: Array<TargetInfo> };
    const infos = targets.targetInfos ?? [];
    const match = targetId !== undefined
      ? infos.find((t) => t.targetId === targetId)
      : infos.find((t) => t.type === "page" && t.url === url);
    if (match === undefined) {
      const known = infos.filter((t) => t.type === "page").map((t) => `${t.targetId.slice(0, 8)} ${(t.url || "").slice(0, 80)}`);
      throw fail("page",
        targetId !== undefined ? `no such target '${targetId.slice(0, 32)}'` : `no page tab for '${String(url).slice(0, 120)}'`,
        known.length > 0 ? `live pages: ${known.join(" | ").slice(0, 300)}.` : "the browser has no page tabs — open one first.");
    }
    const attached = (await conn.send("Target.attachToTarget", { targetId: match.targetId, flatten: true })) as { sessionId: string };
    let parsed: URL;
    try { parsed = new URL(match.url); } catch { throw fail("page", `attached tab has no usable URL: '${match.url.slice(0, 120)}'`, "attach a tab on an http(s) page."); }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") throw fail("page", `attached tab is not a web page: '${match.url.slice(0, 120)}'`, "attach a tab on an http(s) page.");
    const handle = `s_${crypto.randomUUID().replaceAll("-", "").slice(0, 12)}`;
    const rec: SessionRecord = {
      handle, url: match.url, origin: parsed.origin, targetId: match.targetId, createdAt: Date.now(), owned: false,
      ...(endpoint !== undefined ? { cdpEndpoint: endpoint } : {}),
    };
    saveRecord(rec);
    return { conn, sessionId: attached.sessionId, opened: { handle, url: match.url, origin: parsed.origin, targetId: match.targetId, cdpUrl: httpEndpoint, createdAt: rec.createdAt, owned: false } };
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
    const again = record.owned ? "open again." : "attach again.";
    throw fail("page", `tab closed since ${record.owned ? "open" : "attach"} (record ${record.handle} dropped) — ${again}`, "open the url again for a fresh tab, or attach a live one.");
  }
}

export async function closeSession(handle: string): Promise<void> {
  const record = loadRecord(handle);
  // Borrowed tabs are never ours to kill: drop the record, detach only.
  if (record.owned) {
    const endpoint = record.cdpEndpoint ?? await discoverDaemonBrowser();
    if (endpoint !== undefined) {
      try {
        const wsUrl = await discoverWs(endpoint, 5000);
        const conn = await connect(wsUrl);
        try { await conn.send("Target.closeTarget", { targetId: record.targetId }); } finally { conn.close(); }
      } catch { /* tab or browser already gone — success either way */ }
    }
  }
  dropRecord(handle);
}
