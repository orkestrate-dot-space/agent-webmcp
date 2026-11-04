"use client";

import { useEffect } from "react";

import contractsFile from "../../sites.json";
import {
  DRIVERS,
  driverNote,
  mintlifyRegisterDef,
  mintlifySkill,
  OBSERVER_CORE,
  SEND_CORE,
  type MintlifyDeltas,
} from "./mintlify-template";

interface ContractRow {
  origin?: unknown;
  slug?: unknown;
  name?: unknown;
  description?: unknown;
  platform?: unknown;
  skill?: unknown;
  page?: unknown;
  searchTool?: unknown;
  fsTool?: unknown;
  autoSubmit?: unknown;
  tools?: unknown;
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
  }, options?: { signal?: AbortSignal }): unknown;
}

type ToolDefinition = Parameters<ModelContext["registerTool"]>[0];

// Native WebMCP returns a promise that rejects with AbortError when the
// registration signal aborts (React StrictMode remounts, route teardown).
// That is expected lifecycle, not a page error — swallow it.
function registerSafely(
  mc: ModelContext,
  def: ToolDefinition,
  options: { signal?: AbortSignal }
): void {
  try {
    const pending = mc.registerTool(def, options) as { catch?: (fn: () => void) => void } | null;
    if (pending && typeof pending.catch === "function") pending.catch(() => {});
  } catch {
    // Registration is best-effort; the page must render regardless.
  }
}

function rows(): Array<{
  domain: string;
  name: string;
  description: string;
  platform: string;
  skill: string;
  page: string;
  searchTool: string;
  fsTool: string;
  autoSubmit: boolean;
  tools: Array<{ name: string; params: string }>;
  transport: string;
  endpoint: string;
  example: string;
  verified: string;
}> {
  const file = contractsFile as unknown as { sites?: unknown };
  if (!Array.isArray(file.sites)) return [];
  const out: Array<{ domain: string; name: string; description: string; platform: string; skill: string; page: string; searchTool: string; fsTool: string; autoSubmit: boolean; tools: Array<{ name: string; params: string }>; transport: string; endpoint: string; example: string; verified: string }> = [];
  for (const s of file.sites) {
    if (typeof s !== "object" || s === null) continue;
    const r = s as ContractRow;
    if (typeof r.slug !== "string" || r.slug === "") continue;
    const inv = (r.invoke ?? {}) as Record<string, unknown>;
    const proof = (r.proof ?? {}) as Record<string, unknown>;
    const tools = Array.isArray(r.tools)
      ? (r.tools as Array<Record<string, unknown>>)
          .filter((t) => typeof t.name === "string" && t.name !== "")
          .map((t) => ({ name: t.name as string, params: typeof t.params === "string" ? t.params : "" }))
      : [];
    out.push({
      domain: r.slug,
      name: typeof r.name === "string" && r.name !== "" ? r.name : r.slug,
      description: typeof r.description === "string" ? r.description : "",
      platform: typeof r.platform === "string" ? r.platform : "",
      skill: typeof r.skill === "string" ? r.skill : "",
      page: typeof r.page === "string" ? r.page : "",
      searchTool: typeof r.searchTool === "string" ? r.searchTool : "search",
      fsTool: typeof r.fsTool === "string" ? r.fsTool : "",
      autoSubmit: r.autoSubmit === false ? false : true,
      tools,
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
    registerSafely(mc,
      {
        name: "search_agents",
        title: "Search listed site agents",
        description:
          "Find /agents entries matching a capability or question. Returns each match with its full invoke contract: domain, description, platform, transport, endpoint, worked example, and proof date. This is how you find and call a site's agent.",
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
              const hay = `${r.domain} ${r.name} ${r.description} ${r.platform} ${r.transport}`.toLowerCase();
              const score = terms.reduce((n, t) => n + (hay.includes(t) ? 1 : 0), 0);
              return { r, score };
            })
            .filter((h) => terms.length === 0 || h.score > 0)
            .sort((a, b) => b.score - a.score)
            .slice(0, 10)
            .map((h) => ({
              domain: h.r.domain,
              description: h.r.description,
              platform: h.r.platform,
              invoke: { transport: h.r.transport, endpoint: h.r.endpoint, example: h.r.example },
              verified: h.r.verified,
            }));
          return { agents: hits };
        },
      },
      opts
    );
    registerSafely(mc,
      {
        name: "get_agent",
        title: "Get a site agent's skill",
        description:
          "Return the full skill for one listed site agent: how to register it as a custom WebMCP tool (registerTool def) and how to invoke it through agent-webmcp, headless tier and headed tier. Call search_agents first to find the domain.",
        inputSchema: {
          type: "object",
          properties: { domain: { type: "string", description: "Exact listed domain, e.g. 'claude.com'." } },
          required: ["domain"],
          additionalProperties: false,
        },
        annotations: { readOnlyHint: true },
        execute: (input: unknown) => {
          const d = (((input ?? {}) as { domain?: unknown }).domain ?? "").toString().toLowerCase().trim();
          const hit = rows().find((r) => r.domain === d || r.domain === d.replace(/^www\./, ""));
          if (!hit) return { error: `no listed agent for '${d}' — try search_agents to find one` };
          try {
            fetch("/api/use", {
              method: "POST",
              keepalive: true,
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ domain: hit.domain }),
            }).catch(() => {});
          } catch {
            // Telemetry is best-effort; never break the skill return.
          }
          const deltas: MintlifyDeltas = {
            host: hit.domain,
            docs: hit.endpoint,
            page: hit.page || hit.endpoint,
            searchTool: hit.searchTool,
            fsTool: hit.fsTool,
            autoSubmit: hit.autoSubmit,
            description: hit.description,
            verified: hit.verified,
          };
          const drivers = DRIVERS.map((driver) => ({ driver, how: driverNote(driver) }));
          return {
            domain: hit.domain,
            name: hit.name,
            description: hit.description,
            platform: hit.platform,
            tools: hit.tools,
            invoke: { transport: hit.transport, endpoint: hit.endpoint, example: hit.example },
            register: mintlifyRegisterDef(deltas),
            skill: mintlifySkill(deltas),
            drivers,
            observerCore: OBSERVER_CORE,
            sendCore: SEND_CORE,
            verified: hit.verified,
          };
        },
      },
      opts
    );
    return () => ctrl.abort();
  }, []);
  return null;
}
