import { describe, expect, test } from "bun:test";
import { EngineFailed, err, fail, ok, tryOp } from "./result.ts";
import { consequentialGate, parsePolicy } from "./policy.ts";

describe("result kit", () => {
  test("ok/err wrap", () => {
    expect(ok(1)).toEqual({ ok: true, v: 1 });
    expect(err("x")).toEqual({ ok: false, e: "x" });
  });
  test("fail carries reason + fix", () => {
    const e = fail("NO_SUCH_TOOL", "gone", "write it");
    expect(e).toBeInstanceOf(EngineFailed);
    expect(e.reason).toBe("NO_SUCH_TOOL");
    expect(e.fix).toBe("write it");
  });
  test("tryOp wraps foreign throws, passes EngineFailed through", async () => {
    const kept = fail("K", "k", "f");
    await expect(tryOp("op", "fix", async () => { throw kept; })).rejects.toBe(kept);
    await expect(tryOp("op", "fix", async () => { throw new Error("boom"); })).rejects.toMatchObject({ reason: "io" });
    expect(await tryOp("op", "fix", async () => 42)).toBe(42);
  });
});

describe("policy", () => {
  test("parse matrix", () => {
    expect(parsePolicy(undefined)).toBe("ask");
    expect(parsePolicy("")).toBe("ask");
    expect(parsePolicy("allow")).toBe("allow");
    expect(parsePolicy("deny")).toBe("deny");
    expect(() => parsePolicy("sometimes")).toThrow(/consequential/);
  });
  test("gate matrix", () => {
    expect(consequentialGate("allow", false, "w", true)).toBeNull();
    expect(consequentialGate("ask", false, "w", false)).toBeNull();
    expect(consequentialGate("ask", true, "w", true)).toBeNull();
    expect(consequentialGate("deny", true, "w", true)).toMatch(/deny/);
    expect(consequentialGate("ask", false, "w", true)).toMatch(/confirm:true/);
  });
});
