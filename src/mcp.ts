import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { installedInitPath, searchCapabilities } from "./store";
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
  { name: "tools", description: "Search local capabilities (universal + learned site toolkits). No browser touched.", inputSchema: { type: "object", properties: { query: { type: "string" } }, additionalProperties: false } },
  { name: "learn", description: "Hint for persisting a site toolkit: run `agent-webmcp learn <domain> <file.js>` in shell.", inputSchema: { type: "object", properties: { domain: { type: "string" }, file: { type: "string" } }, additionalProperties: false } },
];

export function serve(): Server {
  const server = new Server({ name: "agent-webmcp", version: VERSION }, { capabilities: { tools: {} } });
  server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: VERBS }));
  server.setRequestHandler(CallToolRequestSchema, async (req) => {
    const args = (req.params.arguments ?? {}) as Record<string, unknown>;
    if (req.params.name === "skill") return { content: [{ type: "text", text: skillText() }] };
    if (req.params.name === "init") return { content: [{ type: "text", text: installedInitPath() }] };
    if (req.params.name === "tools") {
      const q = typeof args.query === "string" ? args.query : undefined;
      return { content: [{ type: "text", text: JSON.stringify({ init: installedInitPath(), tools: searchCapabilities(q) }) }] };
    }
    if (req.params.name === "learn") {
      return { content: [{ type: "text", text: `Run in shell: agent-webmcp learn ${String(args.domain ?? "<domain>")} ${String(args.file ?? "<file.js>")}` }] };
    }
    return { content: [{ type: "text", text: `NO_SUCH_TOOL: ${req.params.name}` }], isError: true };
  });
  return server;
}

export async function serveStdio(): Promise<void> {
  await serve().connect(new StdioServerTransport());
}
