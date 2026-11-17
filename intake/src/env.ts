/// Types shared by the intake Worker's modules.

export interface Env {
  DB: D1Database;
  INDEX: KVNamespace;
  /// instabox machines, comma separated: the first scans submissions, all of
  /// them share the weekly rebuild. None holds credentials.
  BOX_IDS: string;
  /// Where the box posts results back, e.g. https://agent-webmcp-intake.<acct>.workers.dev
  WORKER_URL: string;
  /// Public pointer clients read; the box fetches the previous index from it.
  POINTER_URL: string;
  /// instabox API key (machines:read,machines:write). Secret.
  INSTABOX_KEY: string;
  /// Bearer token for /admin/* and rebuild callbacks. Secret.
  ADMIN_TOKEN: string;
  /// Turnstile secret; empty disables verification (local dev only).
  TURNSTILE_SECRET: string;
  /// Salt for hashing submitter IPs before storing them. Secret.
  IP_SALT: string;
}

export type Status = "verified" | "no_tools" | "blocked" | "error";

export interface Tool {
  name: string;
  description: string;
  input_schema: unknown;
  page: string;
  origin?: string;
  annotations?: { read_only?: boolean; consequential?: boolean; untrusted_content?: boolean };
  flags?: string[];
  seen_by?: string[];
}

export interface SiteRecord {
  host: string;
  status: Status;
  checked_at: string;
  pages_checked: string[];
  platform?: string;
  tools: Tool[];
  note?: string;
  missed?: number;
  engines?: string[];
}

export interface IndexFile {
  schema_version: number;
  generated_at: string;
  sites: SiteRecord[];
}

export interface Pointer {
  schema_version: number;
  generated_at: string;
  sha256: string;
  url: string;
  bytes: number;
  sites: number;
  tools: number;
}
