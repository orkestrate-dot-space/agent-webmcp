// agent-webmcp usage counter.
//
// POST /use  { domain }  -> 204, increments the current UTC hour bucket
// GET  /use?domain=&days= -> { domain, total, days: [{ day, count }] }
// GET  /health            -> { ok: true }
//
// The public contract is intentionally tiny: no identity, no query text,
// no IP, no cookies. One domain + one hour + one count. The directory's
// beacon is fire-and-forget; this worker never blocks a page render.

interface Env {
  DB: D1Database;
}

const HOUR_MS = 3_600_000;
const DAY_MS = 86_400_000;
const DAY_CAP = 50_000;
const MAX_BODY = 1024;

// Same shape the site validates, so a domain that reaches the page can
// always reach the counter.
function isDomain(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 128 &&
    /^[a-z0-9.-]+\.[a-z]{2,}$/i.test(value.trim())
  );
}

function hourBucket(at: Date): string {
  return at.toISOString().slice(0, 13);
}

function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "x-content-type-options": "nosniff",
      ...headers,
    },
  });
}

function noContent(): Response {
  return new Response(null, {
    status: 204,
    headers: {
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    },
  });
}

function parseDays(raw: string | null): number {
  const n = Number(raw ?? 30);
  if (!Number.isFinite(n)) return 30;
  return Math.min(Math.max(Math.trunc(n), 1), 90);
}

async function increment(env: Env, domain: string, now: Date): Promise<Response> {
  const bucket = hourBucket(now);
  const dayStart = `${bucket.slice(0, 10)}T00`;

  // Abuse ceiling: the chart is a directional public metric, not an
  // audited ledger. One domain cannot consume the free tier alone.
  const today = await env.DB.prepare(
    "SELECT COALESCE(SUM(count), 0) AS n FROM usage WHERE domain = ?1 AND bucket >= ?2"
  )
    .bind(domain, dayStart)
    .first<{ n: number }>();

  if ((today?.n ?? 0) >= DAY_CAP) {
    const reset = new Date(now);
    reset.setUTCHours(24, 0, 0, 0);
    const retry = Math.max(1, Math.ceil((reset.getTime() - now.getTime()) / 1000));
    return json({ error: "daily cap reached" }, 429, { "retry-after": String(retry) });
  }

  await env.DB.prepare(
    "INSERT INTO usage (domain, bucket, count) VALUES (?1, ?2, 1) " +
      "ON CONFLICT(domain, bucket) DO UPDATE SET count = count + 1"
  )
    .bind(domain, bucket)
    .run();

  return noContent();
}

async function read(env: Env, domain: string, days: number, now: Date): Promise<Response> {
  const keys: Array<string> = [];
  const labels: Array<string> = [];

  if (days === 1) {
    const top = new Date(now.getTime());
    top.setUTCMinutes(0, 0, 0);
    for (let i = 23; i >= 0; i--) {
      const d = new Date(top.getTime() - i * HOUR_MS);
      keys.push(d.toISOString().slice(0, 13));
      labels.push(`${d.toISOString().slice(11, 13)}:00`);
    }
  } else {
    const today = new Date(now.getTime());
    today.setUTCHours(0, 0, 0, 0);
    for (let i = days - 1; i >= 0; i--) {
      const d = new Date(today.getTime() - i * DAY_MS);
      keys.push(d.toISOString().slice(0, 10));
      labels.push(d.toISOString().slice(5, 10).replace("-", "/"));
    }
  }

  const since = keys[0] as string;
  const rows = await env.DB.prepare(
    "SELECT bucket, count FROM usage WHERE domain = ?1 AND bucket >= ?2 ORDER BY bucket"
  )
    .bind(domain, since)
    .all<{ bucket: string; count: number }>();

  const counts = new Map<string, number>();
  for (const row of rows.results ?? []) {
    const key = days === 1 ? row.bucket : row.bucket.slice(0, 10);
    counts.set(key, (counts.get(key) ?? 0) + row.count);
  }

  const buckets = keys.map((key, i) => ({ day: labels[i] as string, count: counts.get(key) ?? 0 }));
  const total = buckets.reduce((sum, b) => sum + b.count, 0);

  return json(
    { domain, total, days: buckets },
    200,
    { "cache-control": "public, max-age=0, s-maxage=30" }
  );
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/health") {
      return json({ ok: true, now: new Date().toISOString() });
    }

    if (url.pathname !== "/use") {
      return json({ error: "not found" }, 404);
    }

    if (request.method === "POST") {
      if (Number(request.headers.get("content-length") ?? 0) > MAX_BODY) {
        return json({ error: "body too large" }, 413);
      }

      let domain: unknown = "";
      try {
        const body = (await request.json()) as { domain?: unknown } | null;
        domain = body?.domain;
      } catch {
        return json({ error: "bad json" }, 400);
      }

      if (!isDomain(domain)) {
        return json({ error: "bad domain" }, 400);
      }

      return increment(env, domain.toLowerCase().trim(), new Date());
    }

    if (request.method === "GET") {
      const domain = (url.searchParams.get("domain") ?? "").toLowerCase().trim();
      if (!isDomain(domain)) {
        return json({ error: "bad domain" }, 400);
      }
      return read(env, domain, parseDays(url.searchParams.get("days")), new Date());
    }

    return json({ error: "method not allowed" }, 405, { allow: "GET, POST" });
  },
};
