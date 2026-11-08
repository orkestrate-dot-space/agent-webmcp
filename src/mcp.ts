import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { buildIndex, searchIndex } from "./index-engine/bm25";
import { loadIndex } from "./index-engine/store";
import { ensureBundle } from "./cli";
import { installedInitPath } from "./paths";
import { VERSION } from "./version";

function skillText(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? "/tmp";
  const fromSource = new URL(import.meta.url).pathname;
  for (const base of [join(home, ".local", "share", "agent-webmcp", "skills"), join(dirname(process.execPath), "..", "skills"), join(dirname(fromSource), "..", "skills")]) {
    try { return readFileSync(join(base, "agent-webmcp", "SKILL.md"), "utf8"); }
    catch { /* next */ }
  }
  return "# agent-webmcp\nMount init.js once, goto -> webmcp.list() before snapshot().";
}

const VERBS = [
  { name: "skill", description: "The compose loop: call with no args before any other tool.", inputSchema: { type: "object", properties: {}, additionalProperties: false } },
  { name: "init", description: "Installed init.js path (mount once via your driver). No browser touched.", inputSchema: { type: "object", properties: {}, additionalProperties: false } },
  {
    name: "search",
    description: "Search indexed capabilities (native + custom) by intent. No browser touched.",
    inputSchema: { type: "object", properties: { query: { type: "string" }, limit: { type: "number" }, domain: { type: "string" }, source: { type: "string" } }, required: ["query"], additionalProperties: false },
  },
  {
    name: "get",
    description: "Unified toolset for a domain: schemas, entry URL, init script path.",
    inputSchema: { type: "object", properties: { target: { type: "string" } }, required: ["target"], additionalProperties: false },
  },
  {
    name: "save",
    description: "Hint for persisting a site toolkit: run `agent-webmcp save <domain> <file.js>` in shell (64KB max, MCP takes content).",
    inputSchema: { type: "object", properties: { domain: { type: "string" }, content: { type: "string" } }, required: ["domain"], additionalProperties: false },
  },
];

export function serve(): Server {
  const server = new Server({ name: "agent-webmcp", version: VERSION }, { capabilities: { tools: {} } });
  server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: VERBS }));
  server.setRequestHandler(CallToolRequestSchema, async (req) => {
    const args = (req.params.arguments ?? {}) as Record<string, unknown>;
    if (req.params.name === "skill") return { content: [{ type: "text", text: skillText() }] };
    if (req.params.name === "init") return { content: [{ type: "text", text: installedInitPath() }] };
    if (req.params.name === "search") {
      const q = typeof args.query === "string" ? args.query : "";
      if (!q) return { content: [{ type: "text", text: "search needs {query}." }], isError: true };
      const limit = typeof args.limit === "number" ? args.limit : 10;
      const domain = typeof args.domain === "string" ? args.domain : undefined;
      const source = args.source === "native" || args.source === "custom" ? args.source : undefined;
      const hits = searchIndex(buildIndex(loadIndex()), q, { limit, ...(domain ? { domain } : {}), ...(source ? { source } : {}) });
      return { content: [{ type: "text", text: JSON.stringify({ init: installedInitPath(), matches: hits }) }] };
    }
    if (req.params.name === "get") {
      const target = typeof args.target === "string" ? args.target : "";
      if (!target) return { content: [{ type: "text", text: "get needs {target} (domain, URL, or domain/tool)." }], isError: true };
      const merged = loadIndex();
      const key = target.trim().toLowerCase();
      const domain = key.includes("://") ? (() => { try { return new URL(key).hostname; } catch { return key; } })() : key.split("/")[0];
      const entry = merged.domains[domain];
      const script = ensureBundle(domain, entry);
      return {
        content: [{
          type: "text",
          text: JSON.stringify({
            ...(entry ? { domain, entry_url: entry.entryUrl } : { domain, status: "unindexed_domain" }),
            init_script: script,
            tools: (entry?.tools ?? []).map((t) => ({ name: t.name, source: t.source, inputSchema: t.inputSchema })),
          }),
        }],
      };
    }
    if (req.params.name === "save") {
      const domain = typeof args.domain === "string" ? args.domain : "<domain>";
      const hasContent = typeof args.content === "string";
      return {
        content: [{
          type: "text",
          text: hasContent
            ? `Write the content to a temp file (max 64KB), then run: agent-webmcp save ${domain} <file.js>`
            : `Run in shell: agent-webmcp save ${domain} <file.js>`,
        }],
      };
    }
    return { content: [{ type: "text", text: `NO_SUCH_TOOL: ${req.params.name}` }], isError: true };
  });
  return server;
}

export async function serveStdio(): Promise<void> {
  await serve().connect(new StdioServerTransport());
}
