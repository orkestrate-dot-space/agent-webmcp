import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("./init.js", import.meta.url).pathname, "utf8");

function loadWith(native: unknown) {
  const store: Record<string, unknown> = {};
  const sandbox = {
    window: {}, document: { dispatchEvent() {}, ...(native ? { modelContext: native } : {}) },
    navigator: {}, Event: function (this: unknown, t: string) { (this as Record<string, unknown>).type = t; },
  };
  const fn = new Function("window", "document", "navigator", "Event", `
    var globalThis = this;
    ${"global.window=arguments[0];global.document=arguments[1];global.navigator=arguments[2];global.Event=arguments[3];"}
  `);
  void fn; void store;
  const run = new Function("window", "document", "navigator", "Event", src);
  run(sandbox.window, sandbox.document, sandbox.navigator, sandbox.Event);
  return sandbox as unknown as {
    window: Record<string, { list(): unknown; call(n: string, a: unknown): Promise<unknown>; define(t: unknown): string }>;
    document: Record<string, { getTools(): Array<unknown>; executeTool(n: string, a: unknown): Promise<unknown>; registerTool(t: unknown): void }>;
  };
}

describe("init.js v0", () => {
  test("defines registry where absent", async () => {
    const sb = loadWith(null);
    sb.window.webmcp.define({ name: "add", execute: async (a: { x: number; y: number }) => ({ sum: a.x + a.y }) });
    expect(await sb.window.webmcp.call("add", { x: 2, y: 3 })).toEqual({ sum: 5 });
    expect(await sb.window.webmcp.call("add", JSON.stringify({ x: 4, y: 5 }))).toEqual({ sum: 9 });
  });

  test("heals string-input callers against object-native", async () => {
    const native = {
      async executeTool(_n: string, a: unknown) {
        if (typeof a !== "object" || a === null) throw new TypeError("parameter 2 is not of type 'object'");
        return { got: a };
      },
      getTools: () => [], registerTool() {},
    };
    const sb = loadWith(native);
    expect(await sb.document.modelContext.executeTool("t", JSON.stringify({ a: 1 }))).toEqual({ got: { a: 1 } });
  });

  test("resolves names to native handles", async () => {
    const seen: Array<unknown> = [];
    const tool = { name: "add", description: "adds", execute: async (a: { x: number }) => ({ v: a.x }) };
    const handle = { name: "add", description: "adds", origin: "https://example.com" };
    const native = {
      registerTool(t: unknown) { seen.push(t); return Promise.resolve(undefined); },
      async getTools() { return [handle]; },
      async executeTool(t: unknown, a: unknown) {
        if (typeof t === "string") throw new TypeError("The provided value is not of type 'RegisteredTool'");
        if (t !== handle) throw new TypeError("Failed to read the 'origin' property from 'RegisteredTool'");
        return JSON.stringify({ got: a });
      },
    };
    const sb = loadWith(native);
    sb.document.modelContext.registerTool(tool);
    expect(seen).toEqual([tool]);
    expect(await sb.document.modelContext.executeTool("add", { x: 1 })).toEqual({ got: { x: 1 } });
    expect(await sb.document.modelContext.executeTool("add", JSON.stringify({ x: 2 }))).toEqual({ got: { x: 2 } });
    // Cached registrations are listed; pre-existing native tools are not.
    expect(await sb.document.modelContext.getTools()).toEqual([{ name: "add", description: "adds" }]);
  });
});
