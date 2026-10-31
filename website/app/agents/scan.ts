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

export interface Proof {
  verified: string;
  ref: string;
}

export interface AgentRecord {
  origin: string;
  slug: string;
  description: string;
  invoke: InvokeContract | null;
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
