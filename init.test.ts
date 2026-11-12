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
    document: Record<string, { getTools(): Array<unknown>; executeTool(n: string, a: unknown): Promise<unknown>; registerTool(t: unknown): void; unregisterTool(n: string): void }>;
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
    const preexisting = { name: "search_docs", description: "search", origin: "https://example.com" };
    const native = {
      registerTool(t: unknown) { seen.push(t); return Promise.resolve(undefined); },
      async getTools() { return [handle, preexisting]; },
      async executeTool(t: unknown, a: unknown) {
        if (typeof t === "string") throw new TypeError("The provided value is not of type 'RegisteredTool'");
        if (t !== handle && t !== preexisting) throw new TypeError("Failed to read the 'origin' property from 'RegisteredTool'");
        return JSON.stringify({ got: a });
      },
    };
    const sb = loadWith(native);
    sb.document.modelContext.registerTool(tool);
    expect(seen).toEqual([tool]);
    expect(await sb.document.modelContext.executeTool("add", { x: 1 })).toEqual({ got: { x: 1 } });
    expect(await sb.document.modelContext.executeTool("add", JSON.stringify({ x: 2 }))).toEqual({ got: { x: 2 } });
    // Merged listing: local registration first, pre-existing natives kept.
    expect(await sb.document.modelContext.getTools()).toEqual([
      { name: "add", description: "adds" },
      { name: "search_docs", description: "search" },
    ]);
  });

  test("local registration wins getTools collisions, upstream survivors listed", async () => {
    const tool = { name: "ask_docs", description: "local asker", execute: async () => ({}) };
    const native = {
      registerTool() { return Promise.resolve(undefined); },
      async getTools() {
        return [
          { name: "ask_docs", description: "stale upstream copy", origin: "https://example.com" },
          { name: "read_page", description: "read", origin: "https://example.com" },
        ];
      },
      async executeTool() { return {}; },
    };
    const sb = loadWith(native);
    expect(await sb.document.modelContext.getTools()).toEqual([
      { name: "ask_docs", description: "stale upstream copy" },
      { name: "read_page", description: "read" },
    ]);
    sb.document.modelContext.registerTool(tool);
    expect(await sb.document.modelContext.getTools()).toEqual([
      { name: "ask_docs", description: "local asker" },
      { name: "read_page", description: "read" },
    ]);
  });

  test("binds window.webmcp.<tool> shorthands on define, drops on unregister", async () => {
    const sb = loadWith(null);
    sb.window.webmcp.define({ name: "add", execute: async (a: { x: number; y: number }) => ({ sum: a.x + a.y }) });
    const wm = sb.window.webmcp as unknown as Record<string, (a: unknown) => Promise<unknown>>;
    expect(typeof wm["add"]).toBe("function");
    expect(await wm["add"]({ x: 1, y: 2 })).toEqual({ sum: 3 });
    // Reserved names are never shadowed.
    sb.window.webmcp.define({ name: "list", execute: async () => ({}) });
    expect(typeof sb.window.webmcp.list).toBe("function");
    sb.document.modelContext.unregisterTool("add");
    expect("add" in sb.window.webmcp).toBe(false);
  });

  test("late native modelContext merges into list, call falls through", async () => {
    const sb = loadWith(null);
    sb.window.webmcp.define({ name: "ask_docs", description: "local", execute: async () => ({ a: 1 }) });
    expect(await sb.window.webmcp.list()).toEqual([
      expect.objectContaining({ name: "ask_docs" }),
    ]);
    // Page hydrates AFTER mount, replacing document.modelContext.
    const late = {
      getTools: () => [{ name: "search_docs", description: "native search" }],
      executeTool: async (n: string) => ({ ran: n }),
    };
    (sb.document as unknown as Record<string, unknown>).modelContext = late;
    expect(await sb.window.webmcp.list()).toEqual([
      expect.objectContaining({ name: "ask_docs" }),
      { name: "search_docs", description: "native search" },
    ]);
    expect(await sb.window.webmcp.call("ask_docs", {})).toEqual({ a: 1 });
    expect(await sb.window.webmcp.call("search_docs", { q: "x" })).toEqual({ ran: "search_docs" });
  });
});
