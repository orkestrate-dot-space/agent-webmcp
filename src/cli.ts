#!/usr/bin/env node
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

/** True when no index is available (fresh machine, nothing synced or saved yet). */
export function indexIsEmpty(): boolean {
  return Object.keys(loadIndex().domains).length === 0;
}

function cmdSearch(args: string[]): void {  const { positional, bools, values } = parseArgs(args);
  let query = positional.join(" ");
  const platform = values["platform"];
  if (!query && !platform) fail("usage: agent-webmcp search <query> [--limit N] [--domain D] [--source native|custom] [--platform P] [--json]");
  if (!query && platform) query = platform;
  const limit = values["limit"] !== undefined ? Number(values["limit"]) : 10;
  if (!Number.isInteger(limit) || limit < 1) fail("--limit wants a positive integer");
  const source = values["source"];
  if (source !== undefined && source !== "native" && source !== "custom") fail('--source wants "native" or "custom"');
  const mini = buildIndex(loadIndex());
  const hits = searchIndex(mini, query, {
    limit,
    ...(values["domain"] ? { domain: values["domain"] } : {}),
    ...(source ? { source: source as "native" | "custom" } : {}),
    ...(platform ? { platform } : {}),
  });
  if (bools.has("json")) {
    console.log(JSON.stringify({ query, matches: hits }));
    return;
  }
  const lines = [`query: "${query}"`, "matches:"];
  for (const h of hits) {
    for (const l of toYamlLines({ id: h.id, domain: h.domain, source: h.source, platform: h.platform === "" ? "(none)" : h.platform, entry_url: h.entryUrl, description: h.description, score: Math.round(h.score * 100) / 100 }, "  ")) lines.push(l);
    lines.push("  ---");
  }
  if (hits.length > 0) lines.push("next_step: Run `agent-webmcp get <domain>` for the init script path and driver commands.");
  else if (indexIsEmpty()) lines.push("hint: index is empty (fresh machine?) — run `agent-webmcp sync` to pull the managed index, then search again.");
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

export interface GetResult {
  domain: string;
  status?: string;
  entry_url?: string;
  platforms?: string[];
  init_script: string;
  tool?: string;
  tools: Array<{ name: string; source: string; description?: string; read_only: boolean; inputSchema: unknown; timeout_ms: number }>;
  how_to_run: Record<string, string>;
  hint?: string;
}

/** Suggested driver timeout per tool. Askers settle on the page's schedule. */
export function timeoutFor(toolName: string): number {
  return toolName.startsWith("ask_") ? 180_000 : 60_000;
}

export function getSite(target: string): GetResult {
  const { domain, tool } = normalizeDomain(target);
  if (!domain) throw new Error("usage: agent-webmcp get <domain|url|domain/tool> [--json]");
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
  const url = entry?.entryUrl ?? `https://${domain}`;
  const callName = tool ?? tools[0]?.name ?? "<TOOL_NAME>";
  const exampleArgs = `{ /* ${callName} arguments */ }`;
  // Static template, interpolated — not per-driver logic. The Browse block
  // uses evaluate-inline because script-tag/init-script injection is proven
  // dead in the execute realm; agent-browser uses --init-script (live-proven).
  // list()/call() are async: every snippet awaits inside the evaluation, and
  // call(name, args) is the robust form (pre-existing native tools have no
  // window.webmcp.<tool> shorthand — shorthands bind only at register time).
  const staged = `${domain}.init.js`;
  const how_to_run = {
    opencode_browse:
      `# terminal first: cp "${script}" "<files.dir>/${staged}"\n` +
      `#   (files.dir comes from one execute: \`return { dir: files.dir }\` —\n` +
      `#   shared /tmp paths are rejected, use that private dir)\n` +
      `# then a single execute block:\n` +
      `const src = await files.read(files.path("${staged}"), "utf8");\n` +
      `await page.goto("${url}", { waitUntil: 'domcontentloaded' });\n` +
      `try { await page.waitForFunction(() => typeof document.modelContext !== "undefined" && typeof document.modelContext.getTools === "function", null, { timeout: 15000 }); } catch (e) { /* custom-only surface: mount anyway */ }\n` +
      `await page.waitForTimeout(2000); // settle: natives register during hydration\n` +
      `await page.evaluate(src); // mount (evaluate-inline; re-mount after every nav)\n` +
      `const tools = await page.evaluate(() => window.webmcp.list()); // await: list() is async\n` +
      `return await page.evaluate((args) => window.webmcp.call("${callName}", args), ${exampleArgs});`,
    agent_browser:
      `agent-browser --init-script "${script}" open "${url}"\n` +
      `agent-browser webmcp invoke ${callName} --params '${exampleArgs}'`,
    playwright_mcp:
      `browser_navigate -> "${url}"\n` +
      `wait until document.modelContext exists (poll via browser_evaluate; skip on timeout: custom-only surface)\n` +
      `browser_evaluate -> load "${script}" bytes, then in the same evaluation:\n` +
      `  await window.webmcp.call("${callName}", ${exampleArgs}) // call() is the robust form; always await`,
  };
  return {
    ...(entry
      ? { domain, entry_url: entry.entryUrl, ...(entry.platforms?.length ? { platforms: entry.platforms } : {}) }
      : { domain, status: "unindexed_domain" }),
    init_script: script,
    ...(tool ? { tool } : {}),
    tools: tools.map((t) => ({
      name: t.name,
      source: t.source,
      ...(typeof t.description === "string" && t.description !== "" ? { description: t.description } : {}),
      read_only: t.annotations?.readOnlyHint === true,
      inputSchema: t.inputSchema,
      timeout_ms: timeoutFor(t.name),
    })),
    how_to_run,
    ...(entry
      ? {}
      : { hint: `No indexed tools for ${domain}. Fresh machine? Run \`agent-webmcp sync\` first. To persist your own custom tools, run \`agent-webmcp save ${domain} <file.js>\`.` }),
  };
}

function cmdGet(args: string[]): void {
  const { positional, bools } = parseArgs(args);
  const target = positional[0];
  if (!target) fail("usage: agent-webmcp get <domain|url|domain/tool> [--json]");
  let payload: GetResult;
  try {
    payload = getSite(target);
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
    throw new Error("unreachable");
  }
  if (bools.has("json")) {
    console.log(JSON.stringify(payload));
    return;
  }
  console.log(toYamlLines(payload, "").join("\n"));
}

export interface SavedSite {
  domain: string;
  tools: string[];
  bundle: string;
}

/** Shared save pipeline (CLI + MCP): validate, compile, merge. Never runs execute(). */
export async function saveSite(domainRaw: string, src: string, entryUrl?: string): Promise<SavedSite> {
  const domain = domainRaw.toLowerCase();
  if (!/^[a-z0-9.-]+(?::\d+)?$/.test(domain)) throw new Error(`bad domain '${domain}'`);
  if (src.length > 200_000) throw new Error("toolkit too large (max 200KB)");
  const found = await extractTools(src);
  if (found.length === 0) throw new Error("file defines no tools (want document.modelContext.registerTool({...}))");
  ensureDirs();
  mkdirSync(sitesDir(), { recursive: true });
  writeFileSync(join(sitesDir(), `${domain}.js`), src.endsWith("\n") ? src : src + "\n");
  const merged = loadIndex();
  const prev = merged.domains[domain];
  const entry: DomainEntry = {
    domain,
    entryUrl: entryUrl || prev?.entryUrl || `https://${domain}`,
    platforms: prev?.platforms ?? [],
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
  return { domain, tools: found.map((t) => t.name), bundle };
}

async function cmdSave(args: string[]): Promise<void> {
  const { positional, values } = parseArgs(args);
  const [domainRaw, file] = positional;
  if (!domainRaw || !file) fail("usage: agent-webmcp save <domain> <file.js> [--entry-url <url>]");
  let src: string;
  try {
    src = readFileSync(file as string, "utf8");
  } catch {
    fail(`cannot read ${file}`);
    throw new Error("unreachable");
  }
  try {
    const saved = await saveSite(domainRaw as string, src, values["entry-url"] || values["entryUrl"]);
    console.log(`saved ${saved.domain} (${saved.tools.length} tool${saved.tools.length === 1 ? "" : "s"}) -> ${saved.bundle}`);
    console.log(`local index: ${localIndexPath()} | merged index: ${indexPath()}`);
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
    throw new Error("unreachable");
  }
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
      `  search <q> [--limit N] [--domain D] [--source native|custom] [--platform P] [--json]\n` +
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
