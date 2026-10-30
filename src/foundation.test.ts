import { describe, expect, test } from "bun:test";
import { EngineFailed, err, fail, ok, tryOp } from "./result.ts";

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

