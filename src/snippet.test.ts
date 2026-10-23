import { describe, expect, test } from "bun:test";
import { parseRegisterReply, parseUnregisterReply, registerSnippet, unregisterSnippet } from "./snippet.ts";

describe("snippets", () => {
  test("register carries def+body and aborts previous", () => {
    const s = registerSnippet("ping", `{"name":"ping"}`, "(async () => 1)");
    expect(s).toContain("document.modelContext.registerTool");
    expect(s).toContain("__agentWebmcp");
    expect(s).toContain("(async () => 1)");
  });
  test("unregister aborts the stored controller", () => {
    expect(unregisterSnippet("ping")).toContain("abort()");
  });
  test("register reply matrix", () => {
    expect(parseRegisterReply(JSON.stringify({ registered: "ping" }), "ping")).toBeNull();
    expect(parseRegisterReply(JSON.stringify({ error: "dup" }), "ping")).toMatch(/page refused: dup/);
    expect(parseRegisterReply("not json", "ping")).toMatch(/non-JSON/);
    // Absent domain names the fix, not the browser.
    expect(parseRegisterReply(undefined, "ping")).toMatch(/WebMCP-capable browser/);
    expect(parseRegisterReply(JSON.stringify({ error: "document.modelContext is undefined" }), "ping")).toMatch(/WebMCP-capable browser/);
  });
  test("unregister reply matrix", () => {
    expect(parseUnregisterReply("ok")).toBeNull();
    expect(parseUnregisterReply(JSON.stringify({ error: "gone" }))).toMatch(/page refused: gone/);
    expect(parseUnregisterReply(42)).toMatch(/non-JSON/);
  });
});
