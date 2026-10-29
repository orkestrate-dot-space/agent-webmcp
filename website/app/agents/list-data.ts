import data from "../../sites.json";
import type { SiteEntry } from "./sites-catalog";
import { SITES_API_VERSION } from "./scan";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

interface WorkerList {
  sites?: unknown;
  nextCursor?: unknown;
}

interface WorkerCounts {
  counts?: Record<string, unknown>;
}

export interface Counts {
  total: number;
  WebMCP: number;
  Skills: number;
  API: number;
  MCP: number;
  Auth: number;
  Ask: number;
}

const EMPTY_COUNTS: Counts = { total: 0, WebMCP: 0, Skills: 0, API: 0, MCP: 0, Auth: 0, Ask: 0 };

const bool = (v: unknown): boolean => v === true;

// Page 1 + chip counts from the worker (both cached 300s
// there), sites.json fallback. Counts ride the server render so
// chips never flash uncounted on mount or tab-back.
export async function readFirstPage(): Promise<{ sites: Array<SiteEntry>; cursor: string | null; counts: Counts }> {
  const fallback = (): Array<SiteEntry> => {
    const file = data as unknown as { sites?: Array<SiteEntry> };
    return Array.isArray(file.sites) ? file.sites : [];
  };
  const num = (v: unknown): number => (typeof v === "number" && Number.isFinite(v) ? v : 0);
  const readCounts = async (): Promise<Counts> => {
    try {
      const r = await fetch(`${WORKER}/sites/counts?v=${SITES_API_VERSION}`, {
        next: { revalidate: 300, tags: ["sites-counts"] },
      });
      if (!r.ok) return EMPTY_COUNTS;
      const j = (await r.json()) as WorkerCounts;
      const c = j.counts ?? {};
      return {
        total: num(c["total"]),
        WebMCP: num(c["webmcp"]),
        Skills: num(c["skills"]),
        API: num(c["api"]),
        MCP: num(c["mcp"]),
        Auth: num(c["auth"]),
        Ask: num(c["ask"]),
      };
    } catch {
      return EMPTY_COUNTS;
    }
  };
  try {
    const [r, counts] = await Promise.all([
      fetch(`${WORKER}/sites?limit=50&v=${SITES_API_VERSION}`, {
        next: { revalidate: 300, tags: ["sites-list"] },
      }),
      readCounts(),
    ]);
    if (!r.ok) return { sites: fallback(), cursor: null, counts };
    const j = (await r.json()) as WorkerList;
    if (!Array.isArray(j.sites)) return { sites: fallback(), cursor: null, counts };
    const sites: Array<SiteEntry> = [];
    for (const s of j.sites) {
      if (typeof s !== "object" || s === null) continue;
      const row = s as Record<string, unknown>;
      if (typeof row["domain"] !== "string" || row["domain"] === "") continue;
      const has = (row["has"] ?? {}) as Record<string, unknown>;
      const proof = (row["proof"] ?? {}) as Record<string, unknown>;
      sites.push({
        origin: `https://${row["domain"]}`,
        slug: row["domain"] as string,
        description: typeof row["description"] === "string" ? (row["description"] as string) : "",
        state: row["state"] === "quarantined" ? "quarantined" : "listed",
        reason: typeof row["reason"] === "string" ? (row["reason"] as string) : "",
        proof: {
          verified: typeof proof["verified"] === "string" ? (proof["verified"] as string) : "",
          ref: typeof proof["ref"] === "string" ? (proof["ref"] as string) : "",
        },
        repo: "",
        path: "",
        has: {
          webmcp: bool(has["webmcp"]),
          skills: bool(has["skills"]),
          api: bool(has["api"]),
          mcp: bool(has["mcp"]),
          auth: bool(has["auth"]),
          ask: bool(has["ask"]),
        },
      });
    }
    return {
      sites,
      cursor: typeof j.nextCursor === "string" && j.nextCursor !== "" ? j.nextCursor : null,
      counts,
    };
  } catch {
    return { sites: fallback(), cursor: null, counts: EMPTY_COUNTS };
  }
}
