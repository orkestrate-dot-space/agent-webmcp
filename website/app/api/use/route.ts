import { appendFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";

const LOG = join(process.cwd(), "data", "agent-usage.jsonl");

const isDomain = (v: unknown): v is string =>
  typeof v === "string" && /^[a-z0-9.-]+\.[a-z]{2,}$/i.test(v.trim()) && v.trim().length <= 128;

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
