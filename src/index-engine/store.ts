import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { indexPath, localIndexPath, registryDir, seedManaged } from "../paths";
import { EMPTY_INDEX, type ToolEntry, type WebMcpIndex } from "../types";

function readIndexFile(path: string): WebMcpIndex | undefined {
  try {
    const raw = JSON.parse(readFileSync(path, "utf8")) as WebMcpIndex;
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return undefined;
    if (!raw.domains || typeof raw.domains !== "object") return undefined;
    return raw;
  } catch {
    return undefined;
  }
}

/** Managed index: local merged copy wins, repo registry/ is the fallback seed. */
function managedIndex(): WebMcpIndex {
  return (
    readIndexFile(indexPath()) ??
    readIndexFile(join(registryDir(), "index.json")) ??
    EMPTY_INDEX
  );
}

function localIndex(): WebMcpIndex {
  return readIndexFile(localIndexPath()) ?? { ...EMPTY_INDEX, domains: {} };
}

/**
 * Tool-level merge. Precedence: managed native < managed custom < local custom.
 * Local entries win name collisions; sync-managed updates never delete locals.
 */
export function loadIndex(): WebMcpIndex {
  seedManaged();
  const managed = managedIndex();
  const local = localIndex();
  const domains: WebMcpIndex["domains"] = {};
  for (const [domain, entry] of Object.entries(managed.domains)) {
    domains[domain] = { ...entry, tools: [...entry.tools] };
  }
  for (const [domain, entry] of Object.entries(local.domains)) {
    const base = domains[domain];
    if (!base) {
      domains[domain] = { ...entry, tools: [...entry.tools] };
      continue;
    }
    const tools: ToolEntry[] = [...base.tools];
    for (const t of entry.tools) {
      const i = tools.findIndex((x) => x.name === t.name);
      if (i >= 0) tools[i] = t;
      else tools.push(t);
    }
    domains[domain] = {
      ...base,
      ...entry,
      entryUrl: entry.entryUrl || base.entryUrl,
      platform: entry.platform ?? base.platform,
      script: entry.script ?? base.script,
      tools,
    };
  }
  return { version: 1, updatedAt: managed.updatedAt || local.updatedAt || "", domains };
}

export function writeLocalIndex(index: WebMcpIndex): void {
  seedManaged();
  writeFileSync(
    localIndexPath(),
    JSON.stringify({ version: 1 as const, updatedAt: new Date().toISOString(), domains: index.domains })
  );
}

/** Persist one domain's custom entry into the local index (save path). */
export function upsertLocalDomain(domain: string, entry: WebMcpIndex["domains"][string]): void {
  const local = localIndex();
  local.domains[domain] = entry;
  writeLocalIndex(local);
}
