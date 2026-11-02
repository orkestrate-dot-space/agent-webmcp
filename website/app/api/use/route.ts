import { appendFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const LOG = join(process.cwd(), "data", "agent-usage.jsonl");

const isDomain = (v: unknown): v is string =>
  typeof v === "string" && /^[a-z0-9.-]+\.[a-z]{2,}$/i.test(v.trim()) && v.trim().length <= 128;

// GET /api/use?domain=example.com&days=30
// Public per-day get_agent invocation counts backing the /agents/{domain}
// usage chart. Local-first like the POST beacon: resets on redeploy.
export async function GET(req: Request): Promise<Response> {
  const url = new URL(req.url);
  const domain = (url.searchParams.get("domain") ?? "").toLowerCase();
  const days = Math.min(Math.max(Number(url.searchParams.get("days") ?? 30) || 30, 1), 90);
  if (!isDomain(domain)) return Response.json({ error: "bad domain" }, { status: 400 });
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
    top.setMinutes(0, 0, 0);
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
  let total = 0;
  for (const line of lines) {
    if (line === "") continue;
    try {
      const row = JSON.parse(line) as { domain?: unknown; at?: unknown };
      if (row.domain !== domain || typeof row.at !== "string") continue;
      total += 1;
      const at = hourly ? row.at.slice(0, 13) : row.at.slice(0, 10);
      const idx = keys.indexOf(at);
      if (idx !== -1) counts[idx] += 1;
    } catch {
      // Skip corrupt lines; the log is append-only.
    }
  }
  return Response.json({
    domain,
    total,
    days: keys.map((_, i) => ({ day: labels[i], count: counts[i] })),
  });
}
// Honest local-first counter: every get_agent execution on our own
// /agents pages fires one beacon here (fire-and-forget, keepalive).
// No identity, no query, no PII — just {domain, at}. Counts reset on
// fresh deploys; the page says so next to the number.
export async function POST(req: Request): Promise<Response> {
  let domain: unknown = "";
  try {
    domain = ((await req.json()) as { domain?: unknown }).domain;
  } catch {
    return Response.json({ error: "bad json" }, { status: 400 });
  }
  if (!isDomain(domain)) return Response.json({ error: "bad domain" }, { status: 400 });
  try {
    await mkdir(dirname(LOG), { recursive: true });
    await appendFile(LOG, JSON.stringify({ domain: domain.toLowerCase(), at: new Date().toISOString() }) + "\n");
  } catch {
    return Response.json({ error: "unavailable" }, { status: 503 });
  }
  return new Response(null, { status: 204 });
}
