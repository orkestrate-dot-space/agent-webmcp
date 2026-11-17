// The public index server.
//
// Two kinds of file, both served with the same URLs as before KV:
//   /index.json              a small pointer: current version's sha256 and url
//   /indexes/index-<sha>.json an immutable, hash-addressed index version
//   /denylist.json           hosts that asked not to be listed
//
// The pointer is the only thing that changes. Versions are written once and
// never replaced, so a client that reads a half-written update is impossible:
// it either sees the old pointer or the new one, and both name a complete file.
//
// KV holds them under `pointer`, `denylist`, and `version:<file>`. Static
// assets are the fallback, so a first deploy (or a KV outage) still serves the
// last file published in the Worker.

interface Env {
  INDEX: KVNamespace;
  ASSETS: Fetcher;
}

const POINTER_HEADERS: Record<string, string> = {
  // no-cache: every read revalidates, so a publish is visible as soon as KV
  // propagates. The ETag keeps a poll cheap (304, no body).
  "cache-control": "no-cache",
  "access-control-allow-origin": "*",
  "x-content-type-options": "nosniff",
};

const VERSION_HEADERS: Record<string, string> = {
  "cache-control": "public, max-age=31536000, immutable",
  "access-control-allow-origin": "*",
  "x-content-type-options": "nosniff",
};

const FILE = /^\/indexes\/index-[0-9a-f]{12}\.json$/;

function json(body: string, headers: Record<string, string>, etag?: string | null): Response {
  const h = new Headers(headers);
  h.set("content-type", "application/json; charset=utf-8");
  if (etag) h.set("etag", etag);
  return new Response(body, { headers: h });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;

    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("method not allowed", { status: 405, headers: { allow: "GET, HEAD" } });
    }

    if (path === "/index.json") {
      const pointer = await env.INDEX.get("pointer");
      if (pointer !== null) {
        // The ETag is the index version, not the pointer's bytes: they change
        // together, and this makes a 304 mean "you already have this index".
        let etag: string | null = null;
        try {
          const sha = (JSON.parse(pointer) as { sha256?: unknown }).sha256;
          if (typeof sha === "string") etag = `"${sha}"`;
        } catch {
          // A malformed pointer is still served; the client will reject it.
        }
        const inm = request.headers.get("if-none-match");
        if (etag && inm && inm.split(",").map((s) => s.trim()).includes(etag)) {
          return new Response(null, { status: 304, headers: { ...POINTER_HEADERS, etag } });
        }
        return json(pointer, POINTER_HEADERS, etag);
      }
      return env.ASSETS.fetch(request);
    }

    if (path === "/denylist.json") {
      const list = await env.INDEX.get("denylist");
      if (list !== null) return json(list, POINTER_HEADERS);
      return env.ASSETS.fetch(request);
    }

    if (FILE.test(path)) {
      const file = path.slice("/indexes/".length); // index-<sha>.json
      const body = await env.INDEX.get(`version:${file}`);
      if (body !== null) {
        const etag = `"${file.slice("index-".length, -".json".length)}"`;
        const inm = request.headers.get("if-none-match");
        if (inm && inm.split(",").map((s) => s.trim()).includes(etag)) {
          return new Response(null, { status: 304, headers: { ...VERSION_HEADERS, etag } });
        }
        return json(body, VERSION_HEADERS, etag);
      }
      return env.ASSETS.fetch(request);
    }

    return env.ASSETS.fetch(request);
  },
} satisfies ExportedHandler<Env>;
