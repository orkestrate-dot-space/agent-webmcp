import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ensureDirs, indexPath, sitesDir } from "../paths";

/** Registry source: explicit env wins, else the public repo raw tree. */
export function registryBase(): string {
  const env = (process.env.AGENT_WEBMCP_REGISTRY ?? "").trim().replace(/\/+$/, "");
  return env === "" ? "https://raw.githubusercontent.com/orkestrate-dot-space/agent-webmcp/main/registry" : env;
}

async function getText(url: string): Promise<string> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 20000);
  try {
    const res = await fetch(url, { signal: ctrl.signal });
    if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`);
    return await res.text();
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Pull managed index.json + sites/*.js. Read-only merge: managed entries
 * refresh the local merged copy; local-index.json is never touched.
 */
export async function syncRegistry(): Promise<{ domains: number; scripts: number; source: string }> {
  ensureDirs();
  const base = registryBase();
  const rawIndex = await getText(`${base}/index.json`);
  const parsed = JSON.parse(rawIndex) as { version?: unknown; domains?: unknown };
  if (!parsed || typeof parsed !== "object" || parsed.version !== 1 || !parsed.domains || typeof parsed.domains !== "object") {
    throw new Error("remote index.json failed validation (want {version:1, domains:{}})");
  }
  writeFileSync(indexPath(), JSON.stringify(parsed));
  let scripts = 0;
  const sites = parsed.domains as Record<string, { script?: unknown }>;
  for (const [domain, entry] of Object.entries(sites)) {
    if (typeof entry.script !== "string" || entry.script === "") continue;
    const name = entry.script.split("/").pop() as string;
    if (!/^[\w.~-]+\.js$/.test(name)) continue;
    const body = await getText(`${base}/sites/${name}`);
    if (body.length > 200_000) continue;
    mkdirSync(sitesDir(), { recursive: true });
    writeFileSync(join(sitesDir(), name), body.endsWith("\n") ? body : body + "\n");
    scripts++;
  }
  return { domains: Object.keys(sites).length, scripts, source: base };
}
