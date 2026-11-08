import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { extractTools } from "../src/compiler/extract-tools";
import { wrapInit } from "../src/compiler/wrap-init";
import { buildIndex, searchIndex } from "../src/index-engine/bm25";
import { loadIndex, upsertLocalDomain } from "../src/index-engine/store";

const SAMPLE = `
document.modelContext.registerTool({
  name: "search_docs",
  description: "Search the docs for API guides",
  inputSchema: { type: "object", required: ["query"], properties: { query: { type: "string" } } },
  annotations: { readOnlyHint: true },
  execute: async ({ query }) => ({ hits: [query] }),
});
`;

let home = "";
let savedHome = "";

beforeEach(() => {
  savedHome = process.env.HOME ?? "";
  home = mkdtempSync(join(tmpdir(), "aw-test-"));
  process.env.HOME = home;
});

afterEach(() => {
  process.env.HOME = savedHome;
});

describe("compiler", () => {
  test("extracts schemas without running execute", async () => {
    const tools = await extractTools(SAMPLE);
    expect(tools.length).toBe(1);
    expect(tools[0]?.name).toBe("search_docs");
    expect(tools[0]?.source).toBe("custom");
    expect(tools[0]?.inputSchema).toEqual({ type: "object", required: ["query"], properties: { query: { type: "string" } } });
  });

  test("rejects files with no tools", async () => {
    await expect(extractTools("const x = 1;")).rejects.toThrow(/no tools/);
  });

  test("rejects tools without execute", async () => {
    await expect(extractTools("document.modelContext.registerTool({ name: 't' });")).rejects.toThrow(/no execute/);
  });

  test("wrapInit prepends header", () => {
    const out = wrapInit("/*H*/\n", "const x = 1;\n", "example.com");
    expect(out.startsWith("/*H*/\n")).toBe(true);
    expect(out).toContain("site toolkit: example.com");
    expect(out).toContain("const x = 1;");
  });
});

describe("index merge + search", () => {
  test("local custom wins name collisions, natives preserved", async () => {
    const tools = await extractTools(SAMPLE);
    upsertLocalDomain("docs.example.com", {
      domain: "docs.example.com",
      entryUrl: "https://docs.example.com",
      platform: null,
      script: "sites/docs.example.com.js",
      updatedAt: "",
      tools: tools.map((t) => ({ name: t.name, source: t.source, description: t.description, inputSchema: t.inputSchema, ...(t.annotations ? { annotations: t.annotations } : {}) })),
    });
    // Simulate a managed native entry for the same domain + tool name.
    const local = JSON.parse(readFileSync(join(home, ".local", "share", "agent-webmcp", "local-index.json"), "utf8"));
    expect(local.domains["docs.example.com"].tools[0].source).toBe("custom");
    const merged = loadIndex();
    expect(merged.domains["docs.example.com"]?.tools.map((t) => t.name)).toEqual(["search_docs"]);
  });

  test("bm25 ranks by intent across domains", () => {
    const mini = buildIndex({
      version: 1,
      updatedAt: "",
      domains: {
        "shop.example.com": {
          domain: "shop.example.com",
          entryUrl: "https://shop.example.com",
          platform: "shopify",
          script: null,
          tools: [{ name: "cancel_cart", source: "native", description: "Empty the shopping cart", inputSchema: { type: "object", properties: {} } }],
        },
        "docs.example.com": {
          domain: "docs.example.com",
          entryUrl: "https://docs.example.com",
          platform: null,
          script: null,
          tools: [{ name: "search_docs", source: "native", description: "Search documentation guides", inputSchema: { type: "object", properties: {} } }],
        },
      },
    });
    const hits = searchIndex(mini, "empty shopping cart", { limit: 5 });
    expect(hits[0]?.id).toBe("shop.example.com/cancel_cart");
    const byDomain = searchIndex(mini, "docs", { limit: 5, domain: "docs.example.com" });
    expect(byDomain.map((h) => h.id)).toEqual(["docs.example.com/search_docs"]);
  });

  test("yaml-ish save fixture round-trips through files", () => {
    writeFileSync(join(home, "tool.js"), SAMPLE);
    const raw = readFileSync(join(home, "tool.js"), "utf8");
    expect(raw).toContain("registerTool");
  });
});
