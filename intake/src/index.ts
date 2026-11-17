/// agent-webmcp intake: submissions, takedowns, and the weekly rebuild.
///
/// Public endpoints (CORS for the website):
///   POST /submit         {url, pages?, turnstile?}  -> {id, state}
///   GET  /status?id=     -> status view
///   POST /takedown       {host, reason?, turnstile?} -> {ok, removed}
///   GET  /seeds.txt      the seed list the weekly rebuild scans
/// Box endpoints (one-time token from the start command):
///   POST /callback?id=&token=          a site record
///   POST /callback/rebuild?token=      a whole rebuilt index
/// Admin (bearer ADMIN_TOKEN):
///   GET  /admin/queue    queued, scanning and recently failed requests
///   POST /admin/retry?id=  requeue one
/// Cron every minute: pump the queue. Cron Monday 06:00: start the rebuild.

import type { Env } from "./env";
import {
  maybeStartRebuild,
  pumpQueue,
  receiveRebuild,
  receiveResult,
  status,
  submit,
  takedown,
} from "./flow";

const ALLOWED_ORIGINS = [
  "https://web.orkestrate.space",
  "https://orkestrate.space",
  "https://www.orkestrate.space",
  "http://localhost:3000",
  "http://127.0.0.1:3000",
];

function cors(origin: string | null): Record<string, string> {
  const allow = origin && ALLOWED_ORIGINS.includes(origin) ? origin : ALLOWED_ORIGINS[0];
  return {
    "access-control-allow-origin": allow,
    "access-control-allow-methods": "GET, POST, OPTIONS",
    "access-control-allow-headers": "content-type",
    "access-control-max-age": "86400",
    vary: "origin",
  };
}

function json(body: unknown, status: number, origin: string | null): Response {
  return new Response(`${JSON.stringify(body)}\n`, {
    status,
    headers: { "content-type": "application/json; charset=utf-8", ...cors(origin) },
  });
}

async function bodyOf<T>(request: Request): Promise<T | null> {
  try {
    return (await request.json()) as T;
  } catch {
    return null;
  }
}

function bearer(request: Request): string {
  const h = request.headers.get("authorization") ?? "";
  return h.startsWith("Bearer ") ? h.slice("Bearer ".length) : "";
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const origin = request.headers.get("origin");

    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors(origin) });

    if (url.pathname === "/submit" && request.method === "POST") {
      const body = await bodyOf<{ url?: string; pages?: string; turnstile?: string }>(request);
      if (!body) return json({ error: "expected a JSON body" }, 400, origin);
      const out = await submit(
        env,
        body,
        request.headers.get("cf-connecting-ip"),
        bearer(request) === env.ADMIN_TOKEN,
      );
      return json(out.body, out.status, origin);
    }

    if (url.pathname === "/status" && request.method === "GET") {
      const view = await status(env, url.searchParams.get("id") ?? "");
      if (!view) return json({ error: "unknown request" }, 404, origin);
      return json(view, 200, origin);
    }

    if (url.pathname === "/takedown" && request.method === "POST") {
      const body = await bodyOf<{ host?: string; reason?: string; turnstile?: string }>(request);
      if (!body) return json({ error: "expected a JSON body" }, 400, origin);
      const out = await takedown(
        env,
        body,
        request.headers.get("cf-connecting-ip"),
        bearer(request) === env.ADMIN_TOKEN,
      );
      return json(out.body, out.status, origin);
    }

    if (url.pathname === "/seeds.txt" && request.method === "GET") {
      const seeds = (await env.INDEX.get("seeds")) ?? "";
      // ?shard=N&count=M returns that slice, so each box gets one share.
      const shardRaw = url.searchParams.get("shard");
      let text = seeds;
      if (shardRaw !== null) {
        const shard = Number(shardRaw);
        const count = Math.max(1, Number(url.searchParams.get("count") ?? "1"));
        const entries = seeds
          .split("\n")
          .map((l) => l.replace(/(^|\s)#\s.*$/, "").trim())
          .filter((l) => l && !l.startsWith("#"));
        const size = Math.ceil(entries.length / count);
        text = `${entries.slice(shard * size, (shard + 1) * size).join("\n")}\n`;
      }
      return new Response(text, { headers: { "content-type": "text/plain; charset=utf-8", ...cors(origin) } });
    }

    if (url.pathname === "/callback" && request.method === "POST") {
      const body = await bodyOf<unknown>(request);
      const out = await receiveResult(
        env,
        { id: url.searchParams.get("id") ?? "", token: url.searchParams.get("token") ?? "" },
        body,
      );
      return json(out.body, out.status, null);
    }

    if (url.pathname === "/callback/rebuild" && request.method === "POST") {
      const body = await bodyOf<unknown>(request);
      const out = await receiveRebuild(
        env,
        { token: url.searchParams.get("token") ?? "", shard: url.searchParams.get("shard") ?? undefined },
        body,
      );
      return json(out.body, out.status, null);
    }

    if (url.pathname.startsWith("/admin/")) {
      if (bearer(request) !== env.ADMIN_TOKEN) return json({ error: "unauthorized" }, 401, origin);
      if (url.pathname === "/admin/queue" && request.method === "GET") {
        const rows = await env.DB.prepare(
          "SELECT id, kind, host, entry, state, outcome, message, tools, attempts, created_at, updated_at FROM requests WHERE state IN ('queued','scanning') OR (state='done' AND updated_at > ?) ORDER BY created_at DESC LIMIT 100",
        )
          .bind(new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString())
          .all();
        return json({ rows: rows.results ?? [] }, 200, origin);
      }
      if (url.pathname === "/admin/retry" && request.method === "POST") {
        const id = url.searchParams.get("id") ?? "";
        await env.DB.prepare("UPDATE requests SET state='queued', attempts=0, updated_at=? WHERE id=? AND state='done'")
          .bind(new Date().toISOString().replace(/\.\d+Z$/, "Z"), id)
          .run();
        return json({ ok: true }, 200, origin);
      }
      // Start the weekly rebuild now, ignoring the schedule.
      if (url.pathname === "/admin/rebuild" && request.method === "POST") {
        try {
          const started = await maybeStartRebuild(env, true);
          return json({ ok: true, started }, 200, origin);
        } catch (e) {
          return json({ ok: false, error: String(e).slice(0, 500) }, 500, origin);
        }
      }
      // Run the queue pump now (the cron does this every minute). Handy when a
      // scan failed and the box is free again.
      if (url.pathname === "/admin/pump" && request.method === "POST") {
        try {
          await pumpQueue(env);
          return json({ ok: true }, 200, origin);
        } catch (e) {
          return json({ ok: false, error: String(e).slice(0, 500) }, 500, origin);
        }
      }
    }

    if (url.pathname === "/" || url.pathname === "/health") {
      return json({ ok: true, service: "agent-webmcp-intake" }, 200, origin);
    }

    return json({ error: "not found" }, 404, origin);
  },

  async scheduled(controller: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    if (controller.cron === "0 6 * * 1") {
      ctx.waitUntil(maybeStartRebuild(env).catch((e) => console.error("rebuild start failed", e)));
      return;
    }
    ctx.waitUntil(pumpQueue(env).catch((e) => console.error("queue pump failed", e)));
  },
} satisfies ExportedHandler<Env>;
