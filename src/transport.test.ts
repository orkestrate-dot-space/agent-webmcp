import { describe, expect, test } from "bun:test";
import { pageApiPresent, webmcpSupported, type CdpConnection } from "./transport.ts";

const fakeConn = (send: CdpConnection["send"]): CdpConnection => ({ send, onEvent: () => {}, close: () => {} });

describe("webmcpSupported", () => {
  test("enable resolving means supported", async () => {
    expect(await webmcpSupported(fakeConn(async () => ({})), 1000)).toBe(true);
  });
  test("method-not-found means unsupported", async () => {
    const conn = fakeConn(async () => { throw new Error("CDP error: 'WebMCP.enable' wasn't found"); });
    expect(await webmcpSupported(conn, 1000)).toBe(false);
  });
  test("other errors fail open to the caller's path", async () => {
    const conn = fakeConn(async () => { throw new Error("timeout"); });
    expect(await webmcpSupported(conn, 1000)).toBe(true);
  });
});

describe("pageApiPresent", () => {
  const evalConn = (value: unknown): CdpConnection =>
    fakeConn(async (method) => {
      if (method === "Runtime.evaluate") return { result: { type: "boolean", value } };
      return {};
    });
  test("true on first clean read", async () => {
    expect(await pageApiPresent(evalConn(true), 1000)).toBe(true);
  });
  test("all-clean-false means flag-off", async () => {
    expect(await pageApiPresent(evalConn(false), 1000, undefined, 2)).toBe(false);
  }, 10000);
  test("evaluate errors fail open", async () => {
    const conn = fakeConn(async () => { throw new Error("wedged"); });
    expect(await pageApiPresent(conn, 1000, undefined, 2)).toBe(true);
  }, 10000);
});
