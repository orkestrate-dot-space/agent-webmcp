import data from "../../sites.json";
import type { AgentRecord, InvokeContract, Proof, SiteTool } from "./scan";
import { SITES_API_VERSION } from "./scan";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

export interface SiteEntry {
  origin: string;
  slug: string;
  description: string;
  platform: string;
  skill: string;
  tools: Array<SiteTool>;
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
    platform?: unknown;
    skill?: unknown;
    tools?: unknown;
    invoke?: { transport?: unknown; endpoint?: unknown; example?: unknown };
    proof?: { verified?: unknown; ref?: unknown };
  }>;
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

function contracts(): Map<string, { description: string; platform: string; skill: string; tools: Array<SiteTool>; invoke: InvokeContract | null; proof: Proof }> {
  const out = new Map<string, { description: string; platform: string; skill: string; tools: Array<SiteTool>; invoke: InvokeContract | null; proof: Proof }>();
  const file = data as unknown as ContractFile;
  if (!Array.isArray(file.sites)) return out;
  for (const s of file.sites) {
    if (typeof s !== "object" || s === null || typeof s.slug !== "string") continue;
    const inv = s.invoke;
    out.set(s.slug, {
      description: str(s.description),
      platform: str(s.platform),
      skill: str(s.skill),
      tools: Array.isArray(s.tools)
        ? s.tools.flatMap((t) =>
            typeof t === "object" && t !== null && typeof (t as SiteTool).name === "string"
              ? [{ name: (t as SiteTool).name, params: str((t as SiteTool).params) }]
              : []
          )
        : [],
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
// Union: every contracted row lists (contract wins on conflicts);
// worker-only rows append as discovered-but-uncontracted entries.
export async function readAgents(): Promise<Array<SiteEntry>> {
  const known = contracts();
  const toEntry = (slug: string, c: { description: string; platform: string; skill: string; tools: Array<SiteTool>; invoke: InvokeContract | null; proof: Proof }): SiteEntry => ({
    origin: `https://${slug}`,
    slug,
    description: c.description,
    platform: c.platform,
    skill: c.skill,
    tools: c.tools,
    invoke: c.invoke,
    proof: c.proof,
  });
  const fallback = (): Array<SiteEntry> => [...known].map(([slug, c]) => toEntry(slug, c));
  const rows = new Map<string, SiteEntry>();
  for (const [slug, c] of known) rows.set(slug, toEntry(slug, c));
  try {
    const r = await fetch(`${WORKER}/sites?limit=100&v=${SITES_API_VERSION}`, {
      next: { revalidate: 300, tags: ["sites-list"] },
    });
    if (r.ok) {
      const j = (await r.json()) as { sites?: unknown };
      if (Array.isArray(j.sites)) {
        for (const w of j.sites) {
          if (typeof w !== "object" || w === null) continue;
          const row = w as WorkerRow;
          if (typeof row.domain !== "string" || row.domain === "") continue;
          if (rows.has(row.domain)) continue;
          rows.set(row.domain, {
            origin: `https://${row.domain}`,
            slug: row.domain,
            description: str(row.description),
            platform: "",
            skill: "",
            tools: [],
            invoke: null,
            proof: {
              verified: str((row.proof as { verified?: unknown } | undefined)?.verified),
              ref: "",
            },
          });
        }
      }
    }
  } catch {
    // Worker down: contracts still list.
  }
  return [...rows.values()];
}

// Records for static params + metadata: contracts only (worker is live data, not build input).
export function contractSlugs(): Array<string> {
  return [...contracts()].map(([slug]) => slug);
}

export function contractOf(slug: string): Omit<AgentRecord, "origin" | "slug"> | null {
  const c = contracts().get(slug);
  return c ?? null;
}
