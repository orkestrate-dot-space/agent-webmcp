import MiniSearch from "minisearch";
import type { SearchDocument, WebMcpIndex } from "../types";

export interface SearchHit extends SearchDocument {
  score: number;
}

const SEARCH_FIELDS: Array<keyof SearchDocument> = ["domain", "toolName", "description", "platform", "paramNames"];

export function toDocuments(index: WebMcpIndex): SearchDocument[] {
  const docs: SearchDocument[] = [];
  for (const [domain, entry] of Object.entries(index.domains)) {
    for (const t of entry.tools) {
      const paramNames = Object.keys(t.inputSchema?.properties ?? {}).join(" ");
      docs.push({
        id: `${domain}/${t.name}`,
        domain,
        entryUrl: entry.entryUrl,
        platform: (entry.platforms ?? []).join(" "),
        toolName: t.name,
        source: t.source,
        description: t.description,
        paramNames,
        readOnly: t.annotations?.readOnlyHint === true,
      });
    }
  }
  return docs;
}

export function buildIndex(index: WebMcpIndex): MiniSearch<SearchDocument> {
  const mini = new MiniSearch<SearchDocument>({
    fields: SEARCH_FIELDS as string[],
    storeFields: ["id", "domain", "entryUrl", "platform", "toolName", "source", "description", "paramNames", "readOnly"] as Array<keyof SearchDocument> as string[],
    searchOptions: { fuzzy: 0.2, prefix: true, boost: { domain: 3, toolName: 2.5, description: 1.5, platform: 1.2, paramNames: 1 } },
  });
  mini.addAll(toDocuments(index));
  return mini;
}

export function searchIndex(
  mini: MiniSearch<SearchDocument>,
  query: string,
  options?: { limit?: number; domain?: string; source?: "native" | "custom"; platform?: string }
): SearchHit[] {
  const limit = options?.limit && Number.isInteger(options.limit) && options.limit > 0 ? Math.min(options.limit, 100) : 10;
  const raw = query.trim() === "" ? [] : (mini.search(query, { fuzzy: 0.2, prefix: true }) as unknown as Array<SearchDocument & { score: number }>);
  const out: SearchHit[] = [];
  const wantPlatform = options?.platform?.toLowerCase();
  for (const r of raw) {
    if (options?.domain && r.domain !== options.domain) continue;
    if (options?.source && r.source !== options.source) continue;
    if (wantPlatform && !r.platform.toLowerCase().split(/\s+/).includes(wantPlatform)) continue;
    out.push({ ...r });
    if (out.length >= limit) break;
  }
  return out;
}
