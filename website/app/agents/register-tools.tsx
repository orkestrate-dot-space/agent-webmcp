"use client";

import { useEffect } from "react";

import contractsFile from "../../sites.json";

interface ContractRow {
  origin?: unknown;
  slug?: unknown;
  description?: unknown;
  invoke?: { transport?: unknown; endpoint?: unknown; example?: unknown };
  proof?: { verified?: unknown; ref?: unknown };
}

interface ModelContext {
  registerTool(def: {
    name: string;
    title?: string;
    description: string;
    inputSchema: Record<string, unknown>;
    annotations?: Record<string, unknown>;
    execute: (input: unknown) => unknown;
  }, options?: { signal?: AbortSignal }): void;
}

function rows(): Array<{
  domain: string;
  description: string;
  transport: string;
  endpoint: string;
  example: string;
  verified: string;
}> {
  const file = contractsFile as unknown as { sites?: unknown };
  if (!Array.isArray(file.sites)) return [];
  const out: Array<{ domain: string; description: string; transport: string; endpoint: string; example: string; verified: string }> = [];
  for (const s of file.sites) {
    if (typeof s !== "object" || s === null) continue;
    const r = s as ContractRow;
    if (typeof r.slug !== "string" || r.slug === "") continue;
    const inv = (r.invoke ?? {}) as Record<string, unknown>;
    const proof = (r.proof ?? {}) as Record<string, unknown>;
    out.push({
      domain: r.slug,
      description: typeof r.description === "string" ? r.description : "",
      transport: typeof inv.transport === "string" ? inv.transport : "",
      endpoint: typeof inv.endpoint === "string" ? inv.endpoint : "",
      example: typeof inv.example === "string" ? inv.example : "",
      verified: typeof proof.verified === "string" ? proof.verified : "",
    });
  }
  return out;
}

// /agents serves its own WebMCP tools: the directory is queryable as
// verbs, not just readable as a page. Registered once per mount,
// unregistered on unmount via AbortController (the spec's teardown).
export function RegisterAgentTools() {
  useEffect(() => {
    const mc = (document as unknown as { modelContext?: ModelContext }).modelContext;
    if (!mc || typeof mc.registerTool !== "function") return;
    const ctrl = new AbortController();
    const opts = { signal: ctrl.signal };
    mc.registerTool(
      {
        name: "search_agents",
        title: "Search listed site agents",
        description:
          "Find /agents entries matching a capability or question. Returns each match with its full invoke contract: domain, description, transport, endpoint, worked example, and proof date. This is how you find and call a site's agent.",
        inputSchema: {
          type: "object",
          properties: { query: { type: "string", description: "Capability or question to match, e.g. 'docs assistant'. Empty returns all rows." } },
          required: ["query"],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: true },
        execute: (input: unknown) => {
          const q = ((input ?? {}) as { query?: unknown }).query;
          const terms = (typeof q === "string" ? q : "").toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
          const hits = rows()
            .map((r) => {
              const hay = `${r.domain} ${r.description} ${r.transport}`.toLowerCase();
              const score = terms.reduce((n, t) => n + (hay.includes(t) ? 1 : 0), 0);
              return { r, score };
            })
            .filter((h) => terms.length === 0 || h.score > 0)
            .sort((a, b) => b.score - a.score)
            .slice(0, 10)
            .map((h) => ({
              domain: h.r.domain,
              description: h.r.description,
              invoke: { transport: h.r.transport, endpoint: h.r.endpoint, example: h.r.example },
              verified: h.r.verified,
            }));
          return { agents: hits };
        },
      },
      opts
    );
    return () => ctrl.abort();
  }, []);
  return null;
}
