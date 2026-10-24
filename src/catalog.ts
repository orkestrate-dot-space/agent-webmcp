// Catalog: in-dialect discovery. search() filters, catalog() details.
// Pure: same entries + query → same rows. The worker calls these as
// reserved paths; the host serves them from the live + on-disk verbs.
export interface CatalogEntry {
  readonly path: string;
  readonly description: string;
  readonly inputSchema?: Record<string, unknown>;
  readonly outputSchema?: Record<string, unknown>;
  readonly capability?: string;
  readonly admitted: boolean;
}

export interface SearchQuery { readonly query?: string; readonly namespace?: string; readonly limit?: number }

export function searchCatalog(entries: ReadonlyArray<CatalogEntry>, q: SearchQuery): Array<{ path: string; description: string; capability?: string }> {
  const query = (q.query ?? "").trim().toLowerCase();
  const namespace = (q.namespace ?? "").trim().toLowerCase().replace(/\.+$/, "");
  const limit = Number.isInteger(q.limit) && (q.limit as number) > 0 ? Math.min(q.limit as number, 100) : 25;
  const out: Array<{ path: string; description: string; capability?: string }> = [];
  for (const e of entries) {
    if (namespace !== "" && !(e.path.toLowerCase() === namespace || e.path.toLowerCase().startsWith(namespace + "."))) continue;
    if (query !== "" && !(e.path.toLowerCase().includes(query) || e.description.toLowerCase().includes(query) || (e.capability ?? "").toLowerCase().includes(query))) continue;
    out.push({ path: e.path, description: e.description + (e.admitted ? "" : " [on disk — first call admits it]"), ...(e.capability !== undefined ? { capability: e.capability } : {}) });
    if (out.length >= limit) break;
  }
  return out;
}

// Render a TS-ish signature from a JSON Schema object. Best-effort:
// unknown shapes degrade to `args: object`, never throw.
export function buildSignature(name: string, schema: unknown): string {
  try {
    const s = schema as { properties?: Record<string, { type?: unknown; description?: unknown }>; required?: unknown };
    const props = s !== null && typeof s === "object" && s.properties !== null && typeof s.properties === "object" && !Array.isArray(s.properties)
      ? s.properties as Record<string, { type?: unknown }>
      : {};
    const required = new Set(Array.isArray(s?.required) ? (s.required as Array<unknown>).filter((r): r is string => typeof r === "string") : []);
    const keys = Object.keys(props);
    if (keys.length === 0) return `${name}()`;
    const inner = keys.slice(0, 20).map((k) => `${k}${required.has(k) ? "" : "?"}: ${typeof props[k]?.type === "string" ? props[k]?.type : "unknown"}`).join(", ");
    return `${name}({ ${inner} })`;
  } catch {
    return `${name}(args: object)`;
  }
}

export function catalogDetail(entries: ReadonlyArray<CatalogEntry>, path?: string): unknown {
  if (path === undefined) {
    return entries.slice(0, 30).map((e) => ({ path: e.path, signature: buildSignature(e.path.split(".").pop() ?? e.path, e.inputSchema), description: e.description, ...(e.capability !== undefined ? { capability: e.capability } : {}) }));
  }
  const found = entries.find((e) => e.path === path);
  if (!found) return { error: `no verb '${path}' — search() to discover verbs.` };
  return { path: found.path, signature: buildSignature(found.path.split(".").pop() ?? found.path, found.inputSchema), description: found.description, inputSchema: found.inputSchema ?? { type: "object" }, ...(found.outputSchema !== undefined ? { outputSchema: found.outputSchema } : {}), ...(found.capability !== undefined ? { capability: found.capability } : {}) };
}
