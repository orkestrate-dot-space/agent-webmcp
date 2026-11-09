import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { ToolEntry } from "../types";

/** The polyfill header: the repo init.js in dev, the installed copy in the binary. */
export function polyfillHeader(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? "/tmp";
  const candidates = [
    join(dirname(new URL(import.meta.url).pathname), "..", "..", "init.js"),
    join(home, ".local", "share", "agent-webmcp", "init.js"),
  ];
  for (const c of candidates) {
    try {
      return readFileSync(c, "utf8");
    } catch { /* next */ }
  }
  throw new Error("polyfill header not found (init.js missing)");
}

export interface ExtractedTool extends ToolEntry {
  executeSource: string;
}

/**
 * Run a site script in a vm sandbox with a stub document.modelContext that
 * captures registerTool calls. execute() bodies are captured as source and
 * NEVER invoked.
 */
export async function extractTools(source: string): Promise<ExtractedTool[]> {
  const vm = await import("node:vm");
  const captured: ExtractedTool[] = [];
  const collector = {
    registerTool(tool: Record<string, unknown>, _options?: unknown) {
      if (!tool || typeof tool.name !== "string") throw new Error("registerTool needs {name}");
      if (typeof tool.execute !== "function") throw new Error(`tool '${tool.name}' has no execute function`);
      const inputSchema =
        tool.inputSchema !== null && typeof tool.inputSchema === "object" && !Array.isArray(tool.inputSchema)
          ? (tool.inputSchema as ToolEntry["inputSchema"])
          : { type: "object" as const, properties: {} };
      captured.push({
        name: tool.name,
        source: "custom",
        description: typeof tool.description === "string" ? tool.description : "",
        inputSchema,
        ...(tool.annotations !== null && typeof tool.annotations === "object" && !Array.isArray(tool.annotations)
          ? { annotations: tool.annotations as ToolEntry["annotations"] }
          : {}),
        executeSource: Function.prototype.toString.call(tool.execute),
      });
      return tool.name;
    },
    unregisterTool() { return false; },
    getTools() { return []; },
    async executeTool() { throw new Error("executeTool unavailable at extract time"); },
  };
  const sandbox = {
    document: { modelContext: collector },
    navigator: {},
    window: {},
    location: { hostname: "" },
    console: { log() {}, warn() {}, error() {} },
  };
  vm.createContext(sandbox);
  vm.runInContext(source, sandbox, { timeout: 5000, filename: "site-tool.js" });
  if (captured.length === 0) throw new Error("file defines no tools (want document.modelContext.registerTool({...}))");
  return captured;
}
