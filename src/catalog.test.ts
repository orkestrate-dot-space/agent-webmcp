import { describe, expect, test } from "bun:test";
import { buildSignature, catalogDetail, searchCatalog, type CatalogEntry } from "./catalog.ts";

const entries: Array<CatalogEntry> = [
  { path: "add_book", description: "Add a book", admitted: true },
  { path: "p1.count_books", description: "Count books", admitted: true },
  { path: "mark_read", description: "Mark read", admitted: false },
];

describe("searchCatalog", () => {
  test("empty query lists all, capped", () => {
    expect(searchCatalog(entries, {}).map((e) => e.path)).toEqual(["add_book", "p1.count_books", "mark_read"]);
    expect(searchCatalog(entries, { limit: 1 }).length).toBe(1);
  });
  test("query filters path + description; namespace scopes", () => {
    expect(searchCatalog(entries, { query: "count" }).map((e) => e.path)).toEqual(["p1.count_books"]);
    expect(searchCatalog(entries, { namespace: "p1" }).map((e) => e.path)).toEqual(["p1.count_books"]);
    expect(searchCatalog(entries, { namespace: "p1." }).map((e) => e.path)).toEqual(["p1.count_books"]);
  });
  test("unadmitted rows say how to admit", () => {
    expect(searchCatalog(entries, { query: "mark" })[0]?.description).toMatch(/first call admits/);
  });
});

describe("buildSignature", () => {
  test("renders required vs optional, degrades gracefully", () => {
    expect(buildSignature("f", { type: "object", properties: { a: { type: "string" }, b: { type: "number" } }, required: ["a"] })).toBe("f({ a: string, b?: number })");
    expect(buildSignature("f", null)).toBe("f()");
    expect(buildSignature("f", "junk")).toBe("f()");
  });
});

describe("catalogDetail", () => {
  test("single and full forms; unknown names name the fix", () => {
    const one = catalogDetail(entries, "add_book") as { path: string };
    expect(one.path).toBe("add_book");
    expect((catalogDetail(entries, "nope") as { error: string }).error).toMatch(/search\(\)/);
    expect((catalogDetail(entries) as Array<unknown>).length).toBe(3);
  });
});
