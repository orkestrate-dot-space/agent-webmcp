/// The intake flow: submit, scan, decide, list; takedown; weekly rebuild.

import type { Env, IndexFile, SiteRecord } from "./env";
import { boxes, startRebuildShard, startScan } from "./box";
import {
  addSeed,
  listingBlockers,
  publishIndex,
  readDenylist,
  readIndex,
  readSeeds,
  removeHost,
  spliceRecord,
  touchGeneratedAt,
  writeDenylist,
} from "./index-store";
import { holdBackToolLoss } from "./guard";
import { entryFor, parseHost, validateRecord, validateSubmission } from "./validate";

// Generous: a free box can pause for a cooldown mid-scan and resume with the
// job still running, so a late callback is normal, not a failure.
const SCAN_TIMEOUT_MS = 15 * 60 * 1000;
const MAX_ATTEMPTS = 2;
const REBUILD_EVERY_MS = 7 * 24 * 60 * 60 * 1000;
// A rebuild shard is a fraction of the index (the whole index is ~806 KB for
// ~260 records); 8 MB is generous and well above any honest shard.
export const REBUILD_BODY_MAX_BYTES = 8 * 1024 * 1024;

export interface Row {
  id: string;
  kind: string;
  host: string;
  entry: string;
  pages: string;
  state: string;
  outcome: string | null;
  message: string | null;
  tools: number;
  token: string;
  attempts: number;
  created_at: string;
  updated_at: string;
  checked_at: string | null;
}

const now = () => new Date().toISOString().replace(/\.\d+Z$/, "Z");
const randomHex = (bytes: number) =>
  [...crypto.getRandomValues(new Uint8Array(bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");

export async function sha256(text: string): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function turnstileOk(env: Env, token: string | undefined, ip: string | null): Promise<boolean> {
  if (!env.TURNSTILE_SECRET) return true; // local dev
  if (!token) return false;
  const body = new FormData();
  body.set("secret", env.TURNSTILE_SECRET);
  body.set("response", token);
  if (ip) body.set("remoteip", ip);
  const res = await fetch("https://challenges.cloudflare.com/turnstile/v0/siteverify", { method: "POST", body });
  if (!res.ok) return false;
  const out = (await res.json()) as { success?: boolean };
  return out.success === true;
}

export async function submit(
  env: Env,
  input: { url?: string; pages?: string; turnstile?: string },
  ip: string | null,
  admin = false,
): Promise<{ status: number; body: unknown }> {
  // The admin token skips the bot check and the per-address limit: that is how
  // a maintainer re-checks a site, or rehearses the flow, without waiting.
  if (!admin && !(await turnstileOk(env, input.turnstile, ip))) {
    return { status: 403, body: { error: "verification failed; reload the page and try again" } };
  }
  const v = await validateSubmission({ url: input.url ?? "", pages: input.pages ?? "" });
  if (!v.ok) return { status: 400, body: { error: v.reason } };

  if ((await readDenylist(env)).includes(v.host)) {
    return { status: 403, body: { error: `${v.host} asked not to be listed` } };
  }

  // One submitter cannot flood the queue: five requests an hour per address.
  if (ip && !admin) {
    const ipHash = await sha256(`${env.IP_SALT}:${ip}`);
    const recent = await env.DB.prepare(
      "SELECT COUNT(*) AS n FROM requests WHERE ip_hash = ? AND created_at > ?",
    )
      .bind(ipHash, new Date(Date.now() - 60 * 60 * 1000).toISOString())
      .first<{ n: number }>();
    if ((recent?.n ?? 0) >= 5) {
      return { status: 429, body: { error: "too many submissions from this network; try again later" } };
    }
    return enqueue(env, v, ipHash);
  }
  return enqueue(env, v, null);
}

async function enqueue(
  env: Env,
  v: { entry: string; host: string; urls: string[] },
  ipHash: string | null,
): Promise<{ status: number; body: unknown }> {
  // The same host within 10 minutes returns the request already in flight.
  const active = await env.DB.prepare(
    "SELECT * FROM requests WHERE host = ? AND kind = 'site' AND state IN ('queued','scanning') AND created_at > ? ORDER BY created_at DESC LIMIT 1",
  )
    .bind(v.host, new Date(Date.now() - 10 * 60 * 1000).toISOString())
    .first<Row>();
  if (active) return { status: 200, body: { id: active.id, state: active.state, existing: true } };

  const id = randomHex(8);
  const token = randomHex(24);
  const at = now();
  await env.DB.prepare(
    `INSERT INTO requests (id, kind, host, entry, pages, state, token, ip_hash, created_at, updated_at)
     VALUES (?, 'site', ?, ?, ?, 'queued', ?, ?, ?, ?)`,
  )
    .bind(id, v.host, v.entry, JSON.stringify(v.urls.slice(1)), token, ipHash, at, at)
    .run();
  return { status: 202, body: { id, state: "queued" } };
}

export interface StatusView {
  id: string;
  host: string;
  state: string;
  outcome: string | null;
  message: string | null;
  tools: number;
  checked_at: string | null;
}

export async function status(env: Env, id: string): Promise<StatusView | null> {
  const row = await env.DB.prepare("SELECT * FROM requests WHERE id = ?").bind(id).first<Row>();
  if (!row) return null;
  return {
    id: row.id,
    host: row.host,
    state: row.state,
    outcome: row.outcome,
    message: row.message,
    tools: row.tools,
    checked_at: row.checked_at,
  };
}

/// Start the oldest queued scan. One at a time: the box is one browser.
export async function pumpQueue(env: Env): Promise<void> {
  // A scan that never reported back is not left forever.
  const stalled = await env.DB.prepare(
    "SELECT * FROM requests WHERE state = 'scanning' AND kind = 'site' AND updated_at < ?",
  )
    .bind(new Date(Date.now() - SCAN_TIMEOUT_MS).toISOString())
    .all<Row>();
  for (const row of stalled.results ?? []) {
    if (row.attempts + 1 < MAX_ATTEMPTS) {
      await env.DB.prepare("UPDATE requests SET state='queued', attempts=attempts+1, updated_at=? WHERE id=?")
        .bind(now(), row.id)
        .run();
    } else {
      await env.DB.prepare(
        "UPDATE requests SET state='done', outcome='error', message=?, updated_at=? WHERE id=?",
      )
        .bind("The scan did not report back in time. A maintainer will look.", now(), row.id)
        .run();
    }
  }

  // One scan at a time: the box has one browser and the scripts share a
  // session. A live scan means wait for the next minute.
  const live = await env.DB.prepare(
    "SELECT 1 AS n FROM requests WHERE state = 'scanning' AND kind = 'site' AND updated_at > ? LIMIT 1",
  )
    .bind(new Date(Date.now() - SCAN_TIMEOUT_MS).toISOString())
    .first<{ n: number }>();
  if (live) return;

  const next = await env.DB.prepare(
    "SELECT * FROM requests WHERE state = 'queued' AND kind = 'site' ORDER BY created_at LIMIT 1",
  ).first<Row>();
  if (!next) return;

  await env.DB.prepare("UPDATE requests SET state='scanning', updated_at=? WHERE id=?").bind(now(), next.id).run();
  try {
    let extras: string[] = [];
    try {
      const parsed = JSON.parse(next.pages) as unknown;
      if (Array.isArray(parsed)) extras = parsed.filter((u): u is string => typeof u === "string");
    } catch {}
    await startScan(env, {
      id: next.id,
      entry: next.entry,
      extras,
      callback: `${env.WORKER_URL}/callback`,
      token: next.token,
    });
  } catch (e) {
    if (next.attempts + 1 >= MAX_ATTEMPTS) {
      await env.DB.prepare(
        "UPDATE requests SET state='done', outcome='error', message=?, attempts=attempts+1, updated_at=? WHERE id=?",
      )
        .bind("The scan box could not be reached. A maintainer will look.", now(), next.id)
        .run();
    } else {
      await env.DB.prepare("UPDATE requests SET state='queued', attempts=attempts+1, updated_at=? WHERE id=?")
        .bind(now(), next.id)
        .run();
    }
    throw e;
  }
}

/// The box posts a site record here, with the token it was started with.
export async function receiveResult(
  env: Env,
  query: { id?: string; token?: string },
  record: unknown,
): Promise<{ status: number; body: unknown }> {
  const id = query.id ?? "";
  const token = query.token ?? "";
  const row = await env.DB.prepare("SELECT * FROM requests WHERE id = ?").bind(id).first<Row>();
  if (!row || row.token !== token) return { status: 403, body: { error: "unknown request" } };
  if (row.state !== "scanning") return { status: 409, body: { error: `request is ${row.state}` } };

  const checked = validateRecord(record, row.entry);
  if (!checked.ok) {
    await env.DB.prepare(
      "UPDATE requests SET state='done', outcome='error', message=?, updated_at=? WHERE id=?",
    )
      .bind(`The scan result did not validate: ${checked.reason}`, now(), id)
      .run();
    return { status: 200, body: { ok: true, listed: false } };
  }

  const rec = checked.record;
  const denylist = await readDenylist(env);
  const blockers = listingBlockers(rec, denylist);
  let listed = false;
  let message = "";

  if (!blockers.length) {
    const index = await readIndex(env);
    if (!index) {
      message = "The index is unavailable; a maintainer will list this by hand.";
    } else {
      spliceRecord(index, rec);
      touchGeneratedAt(index);
      await publishIndex(env, index);
      // The rebuild only scans the seed list: without this the listing would
      // vanish on the next rebuild (lost cookiebot.com and cal.com that way).
      await addSeed(env, entryFor(rec.pages_checked[0] ?? rec.host));
      listed = true;
      message = `Listed with ${rec.tools.length} tool${rec.tools.length === 1 ? "" : "s"}.`;
    }
  } else if (rec.status === "blocked") {
    message = "A bot check stopped the browser, so the pages could not be seen. The check was not solved and never is.";
  } else if (rec.status === "error") {
    message = `The pages could not be read (${(rec.note ?? "see the log").slice(0, 200)}).`;
  } else if (rec.status === "verified") {
    message = `Tools were found but not listed: ${blockers.slice(0, 5).join("; ")}.`;
  } else if (blockers.includes("on the takedown list")) {
    message = "This host asked not to be listed.";
  } else {
    message = "Its pages loaded and none registered a WebMCP tool. Tools that appear only after logging in or interacting are not indexed yet.";
  }

  const outcome = listed ? "listed" : rec.status === "verified" ? "not_listed" : rec.status;
  await env.DB.prepare(
    "UPDATE requests SET state='done', outcome=?, message=?, tools=?, checked_at=?, updated_at=? WHERE id=?",
  )
    .bind(outcome, message, rec.tools.length, rec.checked_at, now(), id)
    .run();
  return { status: 200, body: { ok: true, listed } };
}

/// A takedown is applied immediately: it only removes, so there is nothing to
/// get wrong by acting on it.
export async function takedown(
  env: Env,
  input: { host?: string; reason?: string; turnstile?: string },
  ip: string | null,
  admin = false,
): Promise<{ status: number; body: unknown }> {
  if (!admin && !(await turnstileOk(env, input.turnstile, ip))) {
    return { status: 403, body: { error: "verification failed; reload the page and try again" } };
  }
  const parsed = parseHost(input.host ?? "");
  if (!parsed.ok) return { status: 400, body: { error: parsed.reason } };
  // A takedown only removes, but it should not be a mass-delisting tool.
  if (ip && !admin) {
    const ipHash = await sha256(`${env.IP_SALT}:${ip}`);
    const recent = await env.DB.prepare(
      "SELECT COUNT(*) AS n FROM requests WHERE ip_hash = ? AND kind = 'takedown' AND created_at > ?",
    )
      .bind(ipHash, new Date(Date.now() - 60 * 60 * 1000).toISOString())
      .first<{ n: number }>();
    if ((recent?.n ?? 0) >= 5) {
      return { status: 429, body: { error: "too many takedowns from this network; try again later" } };
    }
    return applyTakedown(env, parsed.host);
  }
  return applyTakedown(env, parsed.host);
}

async function applyTakedown(env: Env, host: string): Promise<{ status: number; body: unknown }> {

  const denylist = await readDenylist(env);
  if (!denylist.includes(host)) {
    await writeDenylist(env, [...denylist, host]);
  }
  let removed = 0;
  const index = await readIndex(env);
  if (index && (removed = removeHost(index, host)) > 0) {
    touchGeneratedAt(index);
    await publishIndex(env, index);
  }
  await env.DB.prepare("UPDATE requests SET state='done', updated_at=? WHERE host=? AND state IN ('queued','scanning')")
    .bind(now(), host)
    .run();

  const id = randomHex(8);
  const at = now();
  await env.DB.prepare(
    `INSERT INTO requests (id, kind, host, entry, pages, state, outcome, message, token, created_at, updated_at)
     VALUES (?, 'takedown', ?, ?, '[]', 'done', 'taken_down', ?, ?, ?, ?)`,
  )
    .bind(
      id,
      host,
      host,
      removed ? "The host is on the takedown list; its listing was removed." : "The host is on the takedown list and is not scanned.",
      randomHex(24),
      at,
      at,
    )
    .run();
  return { status: 200, body: { ok: true, host, removed } };
}

/// Weekly: scan the whole seed list across the boxes and publish the result.
/// Each box scans one shard and posts a partial index; the last one to report
/// triggers the merge.
export async function maybeStartRebuild(env: Env, force = false): Promise<boolean> {
  const last = await env.DB.prepare("SELECT * FROM requests WHERE kind = 'rebuild' ORDER BY created_at DESC LIMIT 1").first<Row>();
  if (!force && last && Date.now() - Date.parse(last.created_at) < REBUILD_EVERY_MS - 60 * 60 * 1000) return false;
  if (last && last.state === "scanning" && Date.now() - Date.parse(last.updated_at) < 120 * 60 * 1000) return false;
  const seeds = await readSeeds(env);
  if (!seeds.trim()) {
    // Invisible would be wrong: Monday would come and go with no rebuild.
    console.error("rebuild: no seed list published; nothing to scan");
    await env.DB.prepare(
      `INSERT INTO requests (id, kind, host, entry, pages, state, outcome, message, token, created_at, updated_at)
       VALUES (?, 'rebuild', 'index', 'index', '[]', 'done', 'error', ?, ?, ?, ?)`,
    )
      .bind(randomHex(8), "No seed list is published; the rebuild did not run.", randomHex(24), now(), now())
      .run();
    return false;
  }

  // The first box scans submissions; the rebuild uses the rest so the two
  // never share a browser. With a single box, it shares.
  const all = boxes(env);
  const list = all.length > 1 ? all.slice(1) : all;
  const id = randomHex(8);
  const token = randomHex(24);
  const at = now();
  await env.DB.prepare(
    `INSERT INTO requests (id, kind, host, entry, pages, state, token, created_at, updated_at)
     VALUES (?, 'rebuild', 'index', 'index', ?, 'scanning', ?, ?, ?)`,
  )
    .bind(id, JSON.stringify({ shards: list.length }), token, at, at)
    .run();

  // A box that fails to start is reported as a missing shard, and the merge
  // refuses rather than publishing a partial index.
  for (let shard = 0; shard < list.length; shard += 1) {
    try {
      await startRebuildShard(env, {
        id,
        callback: `${env.WORKER_URL}/callback/rebuild`,
        token,
        boxId: list[shard],
        shard,
        count: list.length,
      });
    } catch (e) {
      console.error(`rebuild shard ${shard} did not start`, e);
    }
  }
  return true;
}

interface MergeShard {
  sites: SiteRecord[];
}

/// The same rule as tools/merge-index.mjs: one record per start page, the
/// richer record wins, pages are unioned.
function mergeShards(shards: MergeShard[]): IndexFile {
  const sites: SiteRecord[] = [];
  const byKey = new Map<string, SiteRecord>();
  let generatedAt = "";
  for (const shard of shards) {
    const at = (shard as { generated_at?: string }).generated_at ?? "";
    if (at > generatedAt) generatedAt = at;
    for (const site of shard.sites) {
      const key = site.pages_checked[0] ?? site.host;
      const seen = byKey.get(key);
      if (!seen) {
        byKey.set(key, site);
        sites.push(site);
        continue;
      }
      if (site.tools.length > seen.tools.length) {
        seen.tools = site.tools;
        seen.status = site.status;
        seen.checked_at = site.checked_at;
        seen.note = site.note;
        seen.missed = site.missed;
        seen.engines = [...new Set([...(seen.engines ?? []), ...(site.engines ?? [])])];
      }
      for (const page of site.pages_checked) {
        if (!seen.pages_checked.includes(page)) seen.pages_checked.push(page);
      }
      seen.platform ??= site.platform;
    }
  }
  return { schema_version: 1, generated_at: generatedAt || now(), sites };
}

/// One box posts its shard here. When every shard is in, merge and publish.
export async function receiveRebuild(
  env: Env,
  query: { token?: string; shard?: string },
  body: unknown,
): Promise<{ status: number; body: unknown }> {
  const running = await env.DB.prepare(
    "SELECT * FROM requests WHERE kind = 'rebuild' AND state = 'scanning' ORDER BY created_at DESC LIMIT 1",
  ).first<Row>();
  if (!running || running.token !== (query.token ?? "")) {
    return { status: 403, body: { error: "unknown rebuild" } };
  }
  const shard = Number(query.shard ?? "-1");
  const expected = (() => {
    try {
      return (JSON.parse(running.pages) as { shards?: number }).shards ?? 0;
    } catch {
      return 0;
    }
  })();
  if (!Number.isInteger(shard) || shard < 0 || shard >= expected) {
    return { status: 400, body: { error: "bad shard" } };
  }

  const index = body as IndexFile;
  const valid =
    index &&
    typeof index === "object" &&
    index.schema_version === 1 &&
    Array.isArray(index.sites) &&
    index.sites.every(
      (s) =>
        s &&
        typeof s.host === "string" &&
        ["verified", "no_tools", "blocked", "error"].includes(s.status) &&
        Array.isArray(s.tools) &&
        Array.isArray(s.pages_checked),
    );
  if (!valid) {
    await env.DB.prepare("UPDATE requests SET state='done', outcome='error', message=?, updated_at=? WHERE id=?")
      .bind(`shard ${shard} did not validate; keeping the old index.`, now(), running.id)
      .run();
    return { status: 200, body: { ok: false } };
  }

  await env.INDEX.put(`rebuild:${running.id}:${shard}`, JSON.stringify(index));

  const shards: MergeShard[] = [];
  let present = 0;
  for (let i = 0; i < expected; i += 1) {
    const raw = await env.INDEX.get(`rebuild:${running.id}:${i}`);
    if (!raw) continue;
    present += 1;
    shards.push(JSON.parse(raw) as MergeShard);
  }
  if (present < expected) return { status: 200, body: { ok: true, waiting: expected - present } };

  const merged = mergeShards(shards);
  for (let i = 0; i < expected; i += 1) await env.INDEX.delete(`rebuild:${running.id}:${i}`);

  const verifiedNow = merged.sites.filter((s) => s.status === "verified").length;
  const current = await readIndex(env);
  const verifiedBefore = current?.sites.filter((s) => s.status === "verified").length ?? 0;
  if (verifiedBefore >= 20 && verifiedNow < Math.max(20, Math.floor(verifiedBefore * 0.9))) {
    await env.DB.prepare("UPDATE requests SET state='done', outcome='error', message=?, updated_at=? WHERE id=?")
      .bind(`Rebuild found ${verifiedNow} verified sites, down from ${verifiedBefore}; kept the old index.`, now(), running.id)
      .run();
    return { status: 200, body: { ok: false } };
  }

  const guarded = holdBackToolLoss(merged.sites, current?.sites ?? []);
  merged.sites = guarded.sites;
  const guardNote = guarded.kept.length
    ? ` Kept the previous record for ${guarded.kept.length} site(s) that lost over a third of their tools: ${guarded.kept.slice(0, 10).join(", ")}.`
    : "";
  if (guarded.kept.length) console.error(`rebuild: kept previous record for ${guarded.kept.join(", ")}`);

  const denylist = await readDenylist(env);
  // A rebuild is not a bypass: a record that could not be listed through a
  // submission (bad host, flagged description) must not enter through here.
  const kept: SiteRecord[] = [];
  const quarantined: string[] = [];
  for (const site of merged.sites) {
    if (denylist.includes(site.host)) continue;
    const entry = site.pages_checked[0]
      ? site.pages_checked[0].replace(/^https:\/\//, "").replace(/\/$/, "")
      : site.host;
    const checked = validateRecord(site, entry);
    if (!checked.ok) {
      quarantined.push(`${site.host} (invalid: ${checked.reason})`);
      continue;
    }
    const blockers = site.status === "verified" ? listingBlockers(site, denylist) : [];
    if (blockers.length) {
      quarantined.push(`${site.host} (${blockers.slice(0, 3).join("; ")})`);
      continue;
    }
    kept.push(site);
  }
  merged.sites = kept;
  const quarantineNote = quarantined.length
    ? ` Quarantined ${quarantined.length}: ${quarantined.slice(0, 10).join(", ")}${quarantined.length > 10 ? ", …" : ""}.`
    : "";
  if (quarantined.length) console.error(`rebuild: quarantined ${quarantined.join(", ")}`);
  await publishIndex(env, merged);
  const verifiedKept = merged.sites.filter((s) => s.status === "verified").length;
  await env.DB.prepare("UPDATE requests SET state='done', outcome='rebuilt', message=?, updated_at=? WHERE id=?")
    .bind(
      `Rebuilt: ${merged.sites.length} sites, ${verifiedKept} verified, ${merged.sites.reduce((n, s) => n + s.tools.length, 0)} tools.` +
        guardNote +
        quarantineNote,
      now(),
      running.id,
    )
    .run();
  return { status: 200, body: { ok: true } };
}
