// One vocabulary for chips and rows: what a record proves the
// entry serves. Each tag answers one question: how does an
// agent get in? WebMCP is callable page tools, Skills is
// published instruction, API/MCP are callable endpoints,
// Auth means acting needs login, Ask is queryable docs.
export const TAGS = ["WebMCP", "Skills", "API", "MCP", "Auth", "Ask"] as const;
export type Tag = (typeof TAGS)[number];

// Cache-buster pinned to the worker's SCAN_VERSION. Every worker
// fetch carries it, so a logic bump retires all cached rows at
// every layer (CDN, Next data cache) instead of converging by TTL.
export const SITES_API_VERSION = 3;

export interface HasFlags {
  has: { webmcp: boolean; skills: boolean; api: boolean; mcp: boolean; auth: boolean; ask: boolean };
}

export interface SignalItem {
  key: string;
  label: string;
  blurb: string;
  ok: boolean;
}

export const tagsOf = (s: HasFlags): Array<Tag> => {
  const out: Array<Tag> = [];
  if (s.has.webmcp) out.push("WebMCP");
  if (s.has.skills) out.push("Skills");
  if (s.has.api) out.push("API");
  if (s.has.mcp) out.push("MCP");
  if (s.has.auth) out.push("Auth");
  if (s.has.ask) out.push("Ask");
  return out;
};

export const hostOf = (origin: string): string =>
  origin.replace(/^https?:\/\//, "").split("/")[0] ?? origin;

export const looksLikeDomain = (v: string): boolean =>
  /^[a-z0-9.-]+\.[a-z]{2,}(\/.*)?$/i.test(v.trim());
