import data from "../../sites.json";
import type { AgentRecord, InvokeContract, Proof } from "./scan";
import { SITES_API_VERSION } from "./scan";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

export interface SiteEntry {
  origin: string;
  slug: string;
  description: string;
  invoke: InvokeContract | null;
  proof: Proof;
}

interface WorkerRow {
  domain?: unknown;
  description?: unknown;
  proof?: { verified?: unknown };
}

interface ContractFile {
  sites?: Array<{
    origin?: unknown;
    slug?: unknown;
    description?: unknown;
    invoke?: { transport?: unknown; endpoint?: unknown; example?: unknown };
    proof?: { verified?: unknown; ref?: unknown };
  }>;
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

function contracts(): Map<string, { description: string; invoke: InvokeContract | null; proof: Proof }> {
  const out = new Map<string, { description: string; invoke: InvokeContract | null; proof: Proof }>();
  const file = data as unknown as ContractFile;
  if (!Array.isArray(file.sites)) return out;
  for (const s of file.sites) {
    if (typeof s !== "object" || s === null || typeof s.slug !== "string") continue;
    const inv = s.invoke;
    out.set(s.slug, {
      description: str(s.description),
      invoke:
        typeof inv === "object" && inv !== null
          ? { transport: inv.transport as InvokeContract["transport"], endpoint: str(inv.endpoint), example: str(inv.example) }
          : null,
      proof: { verified: str(s.proof?.verified), ref: str(s.proof?.ref) },
    });
  }
  return out;
}

// v1: worker is presence, sites.json is the contract. One merged page,
// no cursor — the list is short enough to serve whole.
export async function readAgents(): Promise<Array<SiteEntry>> {
  const known = contracts();
  const fallback = (): Array<SiteEntry> =>
    [...known].map(([slug, c]) => ({
      origin: `https://${slug}`,
      slug,
      description: c.description,
      invoke: c.invoke,
      proof: c.proof,
    }));
  try {
    const r = await fetch(`${WORKER}/sites?limit=100&v=${SITES_API_VERSION}`, {
      next: { revalidate: 300, tags: ["sites-list"] },
    });
    if (!r.ok) return fallback();
    const j = (await r.json()) as { sites?: unknown };
    if (!Array.isArray(j.sites)) return fallback();
    const rows: Array<SiteEntry> = [];
    for (const w of j.sites) {
      if (typeof w !== "object" || w === null) continue;
      const row = w as WorkerRow;
      if (typeof row.domain !== "string" || row.domain === "") continue;
      const c = known.get(row.domain);
      rows.push({
        origin: `https://${row.domain}`,
        slug: row.domain,
        description: c?.description || str(row.description),
        invoke: c?.invoke ?? null,
        proof: {
          verified: str((row.proof as { verified?: unknown } | undefined)?.verified) || c?.proof.verified || "",
          ref: c?.proof.ref || "",
        },
      });
    }
    return rows;
  } catch {
    return fallback();
  }
}

// Records for static params + metadata: contracts only (worker is live data, not build input).
export function contractSlugs(): Array<string> {
  return [...contracts()].map(([slug]) => slug);
}

export function contractOf(slug: string): Omit<AgentRecord, "origin" | "slug"> | null {
  const c = contracts().get(slug);
  return c ?? null;
}
