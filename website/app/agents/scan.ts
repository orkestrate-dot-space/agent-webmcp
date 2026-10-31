// v1 record: the minimum an agent needs to invoke a site agent.
// origin is identity + routing key. description is the one-line
// routing aid (what it answers). invoke is the contract: how to
// call it. proof is what makes "listed" mean something.
export const TRANSPORTS = ["deep-link", "mcp", "webmcp", "api", "dom"] as const;
export type Transport = (typeof TRANSPORTS)[number];

export interface InvokeContract {
  transport: Transport;
  endpoint: string;
  example: string;
}

/** One invocable verb on the site, as the site registered it. */
export interface SiteTool {
  name: string;
  /** Param signature, e.g. "query: string". */
  params: string;
}

export interface Proof {
  verified: string;
  ref: string;
}

export interface AgentRecord {
  origin: string;
  slug: string;
  description: string;
  /** Platform that ships the agent (mintlify, kapa.ai, …). "" when unknown. */
  platform: string;
  invoke: InvokeContract | null;
  /** Verbs the site registered, in registration order. Empty when unknown. */
  tools: Array<SiteTool>;
  /**
   * get_agent payload: the registerTool def + invoke recipe for this
   * row (both tiers where they exist). This is the agent-to-agent
   * contract — a calling agent registers it and drives it.
   */
  skill: string;
  proof: Proof;
}

// Cache-buster pinned to the worker's SCAN_VERSION. Every worker
// fetch carries it, so a logic bump retires all cached rows at
// every layer instead of converging by TTL.
export const SITES_API_VERSION = 3;

export const hostOf = (origin: string): string =>
  origin.replace(/^https?:\/\//, "").split("/")[0] ?? origin;

export const looksLikeDomain = (v: string): boolean =>
  /^[a-z0-9.-]+\.[a-z]{2,}(\/.*)?$/i.test(v.trim());
