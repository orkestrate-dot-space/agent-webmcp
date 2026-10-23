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
  test("unknown tool path fails the run, not the worker", async () => {
    const r = await runCode({ code: "return await tools.nope({});", calls: {}, budgets });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.message).toMatch(/unknown tool path/);
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
      code: "await tools.hang({}); return 1;",
      calls: { hang: async () => new Promise(() => {}) },
      budgets: { ...budgets, timeoutMs: 300 },
    });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("TimeoutExceeded");
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
});
