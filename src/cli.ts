import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { buildIndex, searchIndex } from "./index-engine/bm25";
import { loadIndex, upsertLocalDomain } from "./index-engine/store";
import { syncRegistry } from "./index-engine/sync";
import { extractTools } from "./compiler/extract-tools";
import { wrapInit } from "./compiler/wrap-init";
import { polyfillHeader } from "./runtime/polyfill-header";
import { cacheDir, ensureDirs, ensureInstalledInit, indexPath, installedInitPath, localIndexPath, sitesDir } from "./paths";
import { VERSION } from "./version";
import type { DomainEntry } from "./types";

function fail(msg: string): never {
  console.error(`agent-webmcp: ${msg}`);
  process.exit(1);
}

interface Flags {
  positional: string[];
  bools: Set<string>;
  values: Record<string, string>;
}

function parseArgs(args: string[]): Flags {
  const positional: string[] = [];
  const bools = new Set<string>();
  const values: Record<string, string> = {};
  for (let i = 0; i < args.length; i++) {
    const a = args[i] as string;
    if (a.startsWith("--")) {
      const eq = a.indexOf("=");
      if (eq >= 0) {
        values[a.slice(2, eq)] = a.slice(eq + 1);
      } else if (i + 1 < args.length && !(args[i + 1] as string).startsWith("--")) {
        values[a.slice(2)] = args[++i] as string;
      } else {
        bools.add(a.slice(2));
      }
    } else {
      positional.push(a);
    }
  }
  return { positional, bools, values };
}

function cmdInit(args: string[]): void {
  const { bools } = parseArgs(args);
  const installed = ensureInstalledInit();
  if (bools.has("stdout")) {
    process.stdout.write(readFileSync(installed, "utf8"));
    return;
  }
  console.log(installed);
}

function toYamlLines(value: unknown, indent: string): string[] {
  if (value === null || value === undefined) return [`${indent}null`];
  if (typeof value === "string") return [`${indent}${value}`];
  if (typeof value === "number" || typeof value === "boolean") return [`${indent}${String(value)}`];
  if (Array.isArray(value)) {
    if (value.length === 0) return [`${indent}[]`];
    const out: string[] = [];
    for (const item of value) {
      const sub = toYamlLines(item, `${indent}  `);
      out.push(`${indent}- ${sub[0]?.trimStart() ?? ""}`);
      for (const line of sub.slice(1)) out.push(line);
    }
    return out;
  }
  const out: string[] = [];
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (v !== null && typeof v === "object") {
      out.push(`${indent}${k}:`);
      for (const line of toYamlLines(v, `${indent}  `)) out.push(line);
    } else {
      out.push(`${indent}${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`);
    }
  }
  return out;
}

function cmdSearch(args: string[]): void {
  const { positional, bools, values } = parseArgs(args);
  const query = positional.join(" ");
  if (!query) fail("usage: agent-webmcp search <query> [--limit N] [--domain D] [--source native|custom] [--json]");
  const limit = values["limit"] !== undefined ? Number(values["limit"]) : 10;
  if (!Number.isInteger(limit) || limit < 1) fail("--limit wants a positive integer");
  const source = values["source"];
  if (source !== undefined && source !== "native" && source !== "custom") fail('--source wants "native" or "custom"');
  const mini = buildIndex(loadIndex());
  const hits = searchIndex(mini, query, {
    limit,
    ...(values["domain"] ? { domain: values["domain"] } : {}),
    ...(source ? { source: source as "native" | "custom" } : {}),
  });
  if (bools.has("json")) {
    console.log(JSON.stringify({ query, matches: hits }));
    return;
  }
  const lines = [`query: "${query}"`, "matches:"];
  for (const h of hits) {
    for (const l of toYamlLines({ id: h.id, domain: h.domain, source: h.source, entry_url: h.entryUrl, description: h.description, score: Math.round(h.score * 100) / 100 }, "  ")) lines.push(l);
    lines.push("  ---");
  }
  if (hits.length > 0) lines.push("next_step: Run `agent-webmcp get <domain>` for the init script path and driver commands.");
  console.log(lines.join("\n"));
}

function normalizeDomain(target: string): { domain: string; tool?: string } {
  const t = target.trim();
  if (!t.includes("://") && t.includes("/")) {
    const slash = t.indexOf("/");
    return { domain: t.slice(0, slash).toLowerCase(), tool: t.slice(slash + 1) || undefined };
  }
  try {
    if (t.includes("://")) return { domain: new URL(t).hostname.toLowerCase() };
  } catch { /* not a URL: treat as domain */ }
  return { domain: t.toLowerCase() };
}

/** (Re)build the targeted bundle for a domain. Returns the bundle path. */
export function ensureBundle(domain: string, entry: DomainEntry | undefined): string {
  ensureDirs();
  mkdirSync(cacheDir(), { recursive: true });
  const header = polyfillHeader();
  if (!entry || !entry.script) {
    const out = join(cacheDir(), "polyfill.init.js");
    writeFileSync(out, header.endsWith("\n") ? header : header + "\n");
    return out;
  }
  const rawName = basename(entry.script);
  const candidates = [join(sitesDir(), rawName), join(sitesDir(), `${domain}.js`)];
  let body: string | undefined;
  for (const c of candidates) {
    try {
      body = readFileSync(c, "utf8");
      break;
    } catch { /* next */ }
  }
  if (body === undefined) fail(`site script missing for ${domain} (want one of ${candidates.join(", ")}) — run \`agent-webmcp save ${domain} <file.js>\``);
  const out = join(cacheDir(), `${domain}.init.js`);
  writeFileSync(out, wrapInit(header, body as string, domain));
  return out;
}

function cmdGet(args: string[]): void {
  const { positional, bools } = parseArgs(args);
  const target = positional[0];
  if (!target) fail("usage: agent-webmcp get <domain|url|domain/tool> [--json]");
  const { domain, tool } = normalizeDomain(target);
  if (!domain) fail("usage: agent-webmcp get <domain|url|domain/tool> [--json]");
  const merged = loadIndex();
  let entry = merged.domains[domain];
  if (!entry) {
    const parent = domain
      .split(".")
      .slice(1)
      .map((_, i, parts) => parts.slice(i).join("."))
      .find((d) => merged.domains[d]);
    if (parent) entry = merged.domains[parent];
  }
  const tools = (entry?.tools ?? []).filter((t) => !tool || t.name === tool);
  const script = ensureBundle(domain, entry);
  const payload = {
    ...(entry
      ? { domain, entry_url: entry.entryUrl, ...(entry.platform ? { platform: entry.platform } : {}) }
      : { domain, status: "unindexed_domain" }),
    init_script: script,
    ...(tool ? { tool } : {}),
    tools: tools.map((t) => ({
      name: t.name,
      source: t.source,
      ...(typeof t.description === "string" && t.description !== "" ? { description: t.description } : {}),
      read_only: t.annotations?.readOnlyHint === true,
      inputSchema: t.inputSchema,
    })),
    ...(entry
      ? {}
      : { hint: `No indexed tools for ${domain}. The polyfill still exposes live native tools; to persist custom tools run \`agent-webmcp save ${domain} <file.js>\`.` }),
  };
  if (bools.has("json")) {
    console.log(JSON.stringify(payload));
    return;
  }
  console.log(toYamlLines(payload, "").join("\n"));
}

async function cmdSave(args: string[]): Promise<void> {
  const { positional, values } = parseArgs(args);
  const [domainRaw, file] = positional;
  if (!domainRaw || !file) fail("usage: agent-webmcp save <domain> <file.js> [--entry-url <url>]");
  const domain = (domainRaw as string).toLowerCase();
  if (!/^[a-z0-9.-]+(?::\d+)?$/.test(domain)) fail(`bad domain '${domain}'`);
  let src: string;
  try {
    src = readFileSync(file as string, "utf8");
  } catch {
    fail(`cannot read ${file}`);
    throw new Error("unreachable");
  }
  if (src.length > 200_000) fail("toolkit too large (max 200KB)");
  let found;
  try {
    found = await extractTools(src);
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
    throw new Error("unreachable");
  }
  if (found.length === 0) fail("file defines no tools (want document.modelContext.registerTool({...}))");
  ensureDirs();
  mkdirSync(sitesDir(), { recursive: true });
  writeFileSync(join(sitesDir(), `${domain}.js`), src.endsWith("\n") ? src : src + "\n");
  const merged = loadIndex();
  const prev = merged.domains[domain];
  const entry: DomainEntry = {
    domain,
    entryUrl: values["entry-url"] || values["entryUrl"] || prev?.entryUrl || `https://${domain}`,
    platform: prev?.platform ?? null,
    script: `sites/${domain}.js`,
    updatedAt: new Date().toISOString(),
    tools: found.map((t) => ({
      name: t.name,
      source: "custom" as const,
      description: t.description,
      inputSchema: t.inputSchema,
      ...(t.annotations ? { annotations: t.annotations } : {}),
    })),
  };
  upsertLocalDomain(domain, entry);
  const bundle = ensureBundle(domain, loadIndex().domains[domain]);
  console.log(`saved ${domain} (${found.length} tool${found.length === 1 ? "" : "s"}) -> ${bundle}`);
  console.log(`local index: ${localIndexPath()} | merged index: ${indexPath()}`);
}

async function cmdSync(): Promise<void> {
  try {
    const r = await syncRegistry();
    console.log(`synced ${r.domains} domains, ${r.scripts} scripts from ${r.source}`);
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
  }
}

function cmdMcp(args: string[]): void {
  if (args[0] !== "serve") fail("usage: agent-webmcp mcp serve");
  import("./mcp").then((m) => m.serveStdio()).catch((e) => fail(String(e)));
}

function usage(): void {
  console.log(
    `agent-webmcp ${VERSION}\nusage: agent-webmcp <init|search|get|save|sync|mcp>\n` +
      `  init [--stdout]              print installed init.js path (or source)\n` +
      `  search <q> [--limit N] [--domain D] [--source native|custom] [--json]\n` +
      `  get <domain|url|domain/tool> [--json]\n` +
      `  save <domain> <f> [--entry-url <u>]\n` +
      `  sync                         pull managed index (only network verb)\n` +
      `  mcp serve                    stdio MCP server`
  );
}

async function main(): Promise<void> {
  const [cmd, ...rest] = process.argv.slice(2);
  if (cmd === "init") return cmdInit(rest);
  if (cmd === "search" || cmd === "tools") return cmdSearch(rest);
  if (cmd === "get") return cmdGet(rest);
  if (cmd === "save" || cmd === "learn") return await cmdSave(rest);
  if (cmd === "sync") return await cmdSync();
  if (cmd === "mcp") return cmdMcp(rest);
  if (cmd === "--version" || cmd === "-V") {
    console.log(VERSION);
    return;
  }
  usage();
  process.exit(cmd === "--help" || cmd === "-h" || cmd === undefined ? 0 : 1);
}

if (import.meta.main) {
  void main().catch((e) => fail(e instanceof Error ? e.message : String(e)));
}
