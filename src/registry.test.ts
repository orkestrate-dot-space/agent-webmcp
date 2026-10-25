import { describe, expect, test } from "bun:test";
import { bodyWarnings, checkSpec, maskCode, slugOf, transpileBody } from "./registry.ts";

const goodSpec = (extra?: Record<string, unknown>): Record<string, unknown> => ({
  name: "ping", description: "Ping", inputSchema: { type: "object", properties: {} }, origin: "https://x.test", outputSchema: { type: "string" }, ...(extra ?? {}),
});

describe("checkSpec", () => {
  test("good shape passes", () => {
    expect(checkSpec(goodSpec(), "(async () => 1)")).toBeNull();
  });
  test("bad shapes name the fix", () => {
    expect(checkSpec({ ...goodSpec(), name: "0bad" }, "1")).toMatch(/bad tool name/);
    expect(checkSpec({ ...goodSpec(), description: "  " }, "1")).toMatch(/description/);
    expect(checkSpec({ ...goodSpec(), inputSchema: [] }, "1")).toMatch(/inputSchema/);
    expect(checkSpec({ ...goodSpec(), origin: undefined }, "1")).toMatch(/origin/);
    expect(checkSpec({ ...goodSpec(), outputSchema: [] }, "1")).toMatch(/outputSchema/);
    expect(checkSpec({ ...goodSpec(), outputSchema: { type: "object" } }, "1")).toMatch(/properties/);
    expect(checkSpec({ ...goodSpec(), outputSchema: { type: "object", properties: {}, additionalProperties: false } }, "1")).toBeNull();
    expect(checkSpec({ ...goodSpec(), outputSchema: undefined }, "1")).toMatch(/outputSchema is required/);
    expect(checkSpec(goodSpec(), "")).toMatch(/body is empty/);
    expect(checkSpec(goodSpec(), "(async () => eval('1'))")).toMatch(/eval/);
    expect(checkSpec(goodSpec(), "(async () => new Function('1'))")).toMatch(/new Function/);
    // Quoted eval is masked, never trips.
    expect(checkSpec(goodSpec(), "(async () => { const s = \"eval('x')\"; return s; })")).toBeNull();
  });
  test("driver idioms warn, never reject", () => {
    expect(checkSpec(goodSpec(), "(async () => { await page.click('x'); })")).toBeNull();
    expect(bodyWarnings("(async () => { await page.click('x'); })").length).toBe(1);
    expect(bodyWarnings("(async () => 1)").length).toBe(0);
  });
  test("maskCode strips strings and comments", () => {
    expect(maskCode(`"eval(" // eval(`)).not.toContain("eval(");
  });
});

describe("transpileBody", () => {
  test("typescript strips, plain JS passes through", () => {
    const out = transpileBody("(async (input: { q: string }): Promise<unknown> => input)");
    expect(out).not.toContain(": string");
    expect(out).toContain("input");
    expect(transpileBody("(async () => 1)")).toBe("async () => 1");
  });
  test("garbage fails loud", () => {
    expect(() => transpileBody("(async (() => ")).toThrow(/transpile/);
  });
});

describe("slugOf", () => {
  test("host lowercased, non-alphanumerics dashed, ports kept", () => {
    expect(slugOf("https://WWW.Example.COM/x")).toBe("www-example-com");
    expect(slugOf("https://a_b.test/")).toBe("a-b-test");
    expect(slugOf("http://127.0.0.1:8099/index.html")).toBe("127-0-0-1-8099");
    expect(slugOf("http://127.0.0.1:8100/")).not.toBe(slugOf("http://127.0.0.1:8099/"));
  });
});
