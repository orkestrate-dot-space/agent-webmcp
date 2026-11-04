import { appendFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";

// Directory usage counter.
//
// Production path: this route is a thin proxy to the Cloudflare Worker
// (workers/usage) backed by D1. The worker owns aggregation and the
// per-domain daily cap; this route never talks to the database.
//
// Local path: with no backend reachable, it falls back to the original
// JSONL file so `next dev` works offline. The file is gitignored and
// per-machine; it is not the production source of truth.
const LOG = join(process.cwd(), "data", "agent-usage.jsonl");
const BACKEND = (
  process.env.USAGE_API_URL ?? "https://agent-webmcp-usage.agent-webmcp.workers.dev"
).replace(/\/$/, "");

const isDomain = (v: unknown): v is string =>
  typeof v === "string" && /^[a-z0-9.-]+\.[a-z]{2,}$/i.test(v.trim()) && v.trim().length <= 128;

async function fromBackend(domain: string, days: number): Promise<Response | null> {
  try {
    const r = await fetch(
      `${BACKEND}/use?domain=${encodeURIComponent(domain)}&days=${days}`,
      { cache: "no-store", signal: AbortSignal.timeout(3000) }
    );
    if (!r.ok) return null;
    const body = await r.json();
    return Response.json(body, { headers: { "cache-control": "public, s-maxage=30" } });
  } catch {
    return null;
  }
}

async function fromLocal(domain: string, days: number): Promise<Response> {
  let lines: Array<string> = [];
  try {
    lines = (await readFile(LOG, "utf8")).split("\n");
  } catch {
    lines = [];
  }
  const now = new Date();
  // days=1 renders the last 24h by the hour; anything else buckets by UTC day.
  const hourly = days <= 1;
  const keys: Array<string> = [];
  const labels: Array<string> = [];
  if (hourly) {
    const top = new Date(now.getTime());
    top.setUTCMinutes(0, 0, 0);
    for (let i = 23; i >= 0; i--) {
      const h = new Date(top.getTime() - i * 3600000);
      keys.push(h.toISOString().slice(0, 13));
      labels.push(h.toISOString().slice(11, 13) + ":00");
    }
  } else {
    const today = new Date(now.getTime());
    today.setUTCHours(0, 0, 0, 0);
    for (let i = days - 1; i >= 0; i--) {
      const d = new Date(today.getTime() - i * 86400000);
      keys.push(d.toISOString().slice(0, 10));
      labels.push(d.toISOString().slice(5, 10).replace("-", "/"));
    }
  }
  const counts = new Array<number>(keys.length).fill(0);
  for (const line of lines) {
    if (line === "") continue;
    try {
      const row = JSON.parse(line) as { domain?: unknown; at?: unknown };
      if (row.domain !== domain || typeof row.at !== "string") continue;
      const at = hourly ? row.at.slice(0, 13) : row.at.slice(0, 10);
      const idx = keys.indexOf(at);
      if (idx !== -1) counts[idx] += 1;
    } catch {
      // Skip corrupt lines; the log is append-only.
    }
  }
  return Response.json({
    domain,
    total: counts.reduce((sum, n) => sum + n, 0),
    days: keys.map((_, i) => ({ day: labels[i], count: counts[i] })),
  });
}

// GET /api/use?domain=example.com&days=30
// Public per-bucket get_agent counts backing the /agents/{domain} usage
// chart. Proxies the worker; falls back to the local file offline.
export async function GET(req: Request): Promise<Response> {
  const url = new URL(req.url);
  const domain = (url.searchParams.get("domain") ?? "").toLowerCase();
  const days = Math.min(Math.max(Number(url.searchParams.get("days") ?? 30) || 30, 1), 90);
  if (!isDomain(domain)) return Response.json({ error: "bad domain" }, { status: 400 });

  const remote = await fromBackend(domain, days);
  return remote ?? fromLocal(domain, days);
}

// POST /api/use { domain } — one beacon per get_agent execution on the
// directory pages. Fire-and-forget: the client never reads the response,
// so a backend hiccup must not surface as a page error.
export async function POST(req: Request): Promise<Response> {
  let domain: unknown = "";
  try {
    domain = ((await req.json()) as { domain?: unknown }).domain;
  } catch {
    return Response.json({ error: "bad json" }, { status: 400 });
  }
  if (!isDomain(domain)) return Response.json({ error: "bad domain" }, { status: 400 });

  try {
    const r = await fetch(`${BACKEND}/use`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ domain: domain.toLowerCase() }),
      cache: "no-store",
      signal: AbortSignal.timeout(2000),
    });
    if (r.ok || r.status === 429) return new Response(null, { status: 204 });
  } catch {
    // Fall through to the local log; offline dev only.
  }

  try {
    await mkdir(dirname(LOG), { recursive: true });
    await appendFile(LOG, JSON.stringify({ domain: domain.toLowerCase(), at: new Date().toISOString() }) + "\n");
  } catch {
    // Read-only filesystem (Vercel) or a real outage: stay silent, the
    // beacon is best-effort and the chart shows what did land.
  }
  return new Response(null, { status: 204 });
}
