import { describe, expect, test } from "bun:test";
import { runCode } from "./runner.ts";

const budgets = { timeoutMs: 5000, maxToolCalls: 25, maxChars: 16000, maxResultChars: 64000 };

describe("runner", () => {
  test("return value flows back with call paths", async () => {
    const r = await runCode({
      code: "const a = await tools.add({x: 1}); return { got: a };",
      calls: { add: async (input) => ({ echo: (input as { x: number }).x }) },
      budgets,
    });
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.value).toEqual({ got: { echo: 1 } });
      expect(r.calls).toEqual(["add"]);
      expect(r.toolCalls).toBe(1);
    }
  });
  test("unknown tool path fails the run and names the fix", async () => {
    const r = await runCode({ code: "return await tools.nope({});", calls: {}, budgets });
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.error.message).toMatch(/unknown tool path/);
      expect(r.error.message).toMatch(/unknown tool path: nope/);
    }
  });
  test("call budget ends the run", async () => {
    const r = await runCode({
      code: "while (true) { await tools.ping({}); }",
      calls: { ping: async () => 1 },
      budgets: { ...budgets, maxToolCalls: 3 },
    });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("ToolCallLimitExceeded");
  });
  test("timeout ends the run", async () => {
    const r = await runCode({
      code: "await tools.fast({}); await tools.hang({}); return 1;",
      calls: { fast: async () => ({ n: 1 }), hang: async () => new Promise(() => {}) },
      budgets: { ...budgets, timeoutMs: 300 },
    });
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.error.kind).toBe("TimeoutExceeded");
      expect(r.partial).toEqual([{ path: "fast", head: "{\"n\":1}" }]);
    }
  });
  test("mid-run throw keeps the completed-call ledger", async () => {
    const r = await runCode({
      code: "await tools.a({}); await tools.b({}); return 1;",
      calls: { a: async () => ({ n: 1 }), b: async () => { throw new Error("boom"); } },
      budgets,
    });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.partial).toEqual([{ path: "a", head: "{\"n\":1}" }]);
  });
  test("result cap truncates with spilled flag", async () => {
    const r = await runCode({
      code: "return await tools.big({});",
      calls: { big: async () => "x".repeat(1000) },
      budgets: { ...budgets, maxResultChars: 100 },
    });
    expect(r.ok).toBe(true);
    if (r.ok) { expect(r.spilled).toBe(true); expect(String(r.value).length).toBeLessThanOrEqual(120); }
  });

  test("console output returns as logs", async () => {
    const r = await runCode({
      code: "console.log('trace', { n: 1 }); console.warn('careful'); return 1;",
      calls: {},
      budgets,
    });
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.logs).toEqual(["[log] trace {\"n\":1}", "[warn] careful"]);
      expect(r.value).toBe(1);
    }
  });

  test("reserved search/catalog paths reach the host", async () => {
    const r = await runCode({
      code: "const hits = await search({ query: 'book' }); const det = await catalog('add_book'); return { hits, det };",
      calls: {
        search: async (input) => [{ path: "add_book", description: `hit for ${JSON.stringify(input)}` }],
        catalog: async (input) => ({ detail: String(input) }),
      },
      budgets,
    });
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.value).toEqual({ hits: [{ path: "add_book", description: 'hit for {"query":"book"}' }], det: { detail: "add_book" } });
      expect(r.calls).toEqual(["search", "catalog"]);
    }
  });
});
