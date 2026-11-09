export interface ToolAnnotations {
  readOnlyHint?: boolean;
  consequentialHint?: boolean;
  untrustedContentHint?: boolean;
}

export interface ToolInputSchema {
  type: "object";
  required?: string[];
  properties: Record<string, unknown>;
}

export interface ToolEntry {
  name: string;
  source: "native" | "custom";
  description: string;
  inputSchema: ToolInputSchema;
  annotations?: ToolAnnotations;
}

export interface DomainEntry {
  domain: string;
  entryUrl: string;
  platform?: string | null;
  script: string | null;
  updatedAt?: string;
  tools: ToolEntry[];
}

export interface WebMcpIndex {
  version: 1;
  updatedAt: string;
  domains: Record<string, DomainEntry>;
}

export interface SearchDocument {
  id: string;
  domain: string;
  entryUrl: string;
  platform: string;
  toolName: string;
  source: "native" | "custom";
  description: string;
  paramNames: string;
  readOnly: boolean;
}

export const EMPTY_INDEX: WebMcpIndex = { version: 1, updatedAt: "", domains: {} };
