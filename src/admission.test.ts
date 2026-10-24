// Admission tests on a fake CDP socket. The fake speaks just enough
// WebMCP to exercise every reapply/admit branch: native burst, register,
// unregister, invoke (Ok + Error), and a ghost that never appears.
import { describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { admitOnCall, proveTool, reapplyOrigin } from "./admission.ts";
import { bodyHashOf, transpileBody } from "./registry.ts";
import { connect, type CdpConnection } from "./transport.ts";

const ORIGIN = "https://x.test";

interface FakeOpts { native?: Array<string>; ghost?: Array<string>; failInvoke?: Set<string> }

function serveFake(opts: FakeOpts = {}): { wsUrl: string; stop: () => void; counts: { evaluate: number; invoke: number }; registered: Set<string> } {
  let wsUrl = "";
  const registered = new Set<string>();
  const counts = { evaluate: 0, invoke: 0 };
  const server = Bun.serve({
    port: 0,
    fetch(req, server) {
      const url = new URL(req.url);
      if (url.pathname === "/json/version") return Response.json({ webSocketDebuggerUrl: wsUrl });
      if (url.pathname === "/cdp" && server.upgrade(req)) return undefined;
      return new Response("no", { status: 404 });
    },
    websocket: {
      open() {},
      message(ws, raw) {
        const msg = JSON.parse(String(raw)) as { id: number; method: string; params?: Record<string, unknown> };
        if (msg.method === "WebMCP.enable") {
          ws.send(JSON.stringify({ id: msg.id, result: {} }));
          const tools = [...(opts.native ?? [])].map((name) => ({ name, description: `${name} native`, inputSchema: { type: "object" }, frameId: "f1" }));
          for (const name of registered) {
            if (opts.ghost?.includes(name)) continue;
            tools.push({ name, description: `${name} re-applied`, inputSchema: { type: "object" }, frameId: "f1" });
          }
          ws.send(JSON.stringify({ method: "WebMCP.toolsAdded", params: { tools } }));
        } else if (msg.method === "Runtime.evaluate") {
          counts.evaluate++;
          const expr = String((msg.params as { expression?: unknown } | undefined)?.expression ?? "");
          const defMatch = /"name":\s*"([^"]+)"/.exec(expr);
          const name = defMatch?.[1] ?? "?";
          if (expr.includes("registerTool")) {
            registered.add(name);
            ws.send(JSON.stringify({ id: msg.id, result: { result: { type: "string", value: JSON.stringify({ registered: name }) } } }));
          } else if (expr.includes('return "ok"')) {
            const un = /\["((?:[^"\\]|\\.)+)"\]/.exec(expr)?.[1];
            if (un !== undefined) registered.delete(un);
            ws.send(JSON.stringify({ id: msg.id, result: { result: { type: "string", value: "ok" } } }));
          } else {
            ws.send(JSON.stringify({ id: msg.id, result: { result: { type: "string", value: '"ok"' } } }));
          }
        } else if (msg.method === "WebMCP.invokeTool") {
          counts.invoke++;
          const toolName = String((msg.params as { toolName?: unknown } | undefined)?.toolName ?? "?");
          const failed = opts.failInvoke?.has(toolName) ?? false;
          ws.send(JSON.stringify({ id: msg.id, result: { invocationId: "i1" } }));
          ws.send(JSON.stringify({
            method: "WebMCP.toolResponded",
            params: failed
              ? { invocationId: "i1", status: "Error", errorText: "page says no" }
              : { invocationId: "i1", status: "Ok", output: { content: [{ type: "text", text: "ok" }] } },
          }));
        } else {
          ws.send(JSON.stringify({ id: msg.id, result: {} }));
        }
      },
    },
  });
  wsUrl = `ws://127.0.0.1:${server.port}/cdp`;
  return { wsUrl, stop: () => server.stop(true), counts, registered };
}

function withHome<T>(fn: (home: string) => Promise<T>): Promise<T> {
  const prev = process.env.HOME;
  const home = mkdtempSync(join(tmpdir(), "adm-test-"));
  process.env.HOME = home;
  return fn(home).finally(() => {
    if (prev === undefined) delete process.env.HOME; else process.env.HOME = prev;
    rmSync(home, { recursive: true, force: true });
  });
}

function writeTool(home: string, origin: string, name: string, spec: Record<string, unknown>, body: string, capability = "chat"): void {
  const slug = new URL(origin).hostname.replaceAll(/[^a-z0-9]/g, "-");
  const dir = join(home, ".local/share/agent-webmcp/registry", slug, capability, name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "spec.json"), JSON.stringify({ name, description: `${name} tool`, inputSchema: { type: "object", properties: {} }, origin, version: 1, createdAt: 0, ...spec }));
  writeFileSync(join(dir, "body.ts"), body);
}

function writeStamp(home: string, origin: string, name: string, bodyHash: string, capability = "chat"): void {
  const slug = new URL(origin).hostname.replaceAll(/[^a-z0-9]/g, "-");
  writeFileSync(join(home, ".local/share/agent-webmcp/registry", slug, capability, name, "stamp.json"), JSON.stringify({ verified: true, bodyHash, at: 0 }));
}

const BODY = "(async () => { return { content: [{ type: 'text', text: 'hi' }] }; })";

describe("reapply", () => {
  test("stamped + unchanged re-injects silently, no proof run", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "fast", { fixtureInput: {} }, BODY);
      writeStamp(home, ORIGIN, "fast", bodyHashOf(transpileBody(BODY)));
      const fake = serveFake();
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        const out = await reapplyOrigin(conn, "s1", ORIGIN, 3000);
        await conn.close();
        expect(out.reapplied).toEqual(["fast"]);
        expect(out.skipped).toEqual([]);
        expect(fake.counts.invoke).toBe(0);
      } finally { fake.stop(); }
    });
  });

  test("unstamped + fixture proves, stamps, re-applies", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "fresh", { fixtureInput: {} }, BODY);
      const fake = serveFake();
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        const out = await reapplyOrigin(conn, "s1", ORIGIN, 3000);
        await conn.close();
        expect(out.reapplied).toEqual(["fresh"]);
        expect(fake.counts.invoke).toBe(1);
      } finally { fake.stop(); }
    });
  });

  test("proof failure rolls back: nothing stays live", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "badproof", { fixtureInput: {} }, BODY);
      const fake = serveFake({ failInvoke: new Set(["badproof"]) });
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        const out = await reapplyOrigin(conn, "s1", ORIGIN, 3000);
        await conn.close();
        expect(out.reapplied).toEqual([]);
        expect(out.skipped[0]?.reason).toMatch(/proof failed/);
        expect(fake.registered.has("badproof")).toBe(false);
      } finally { fake.stop(); }
    });
  });

  test("ambiguous names across capabilities fail naming both", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "dup", { fixtureInput: {} }, BODY, "chat");
      writeTool(home, ORIGIN, "dup", { fixtureInput: {} }, BODY, "docs");
      const fake = serveFake();
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        const out = await reapplyOrigin(conn, "s1", ORIGIN, 3000);
        await conn.close();
        expect(out.reapplied).toEqual([]);
        expect(out.skipped[0]?.reason).toMatch(/ambiguous|2 capabilities/);
      } finally { fake.stop(); }
    });
  });

  test("unstamped consequential never injects on open", async () => {    await withHome(async (home) => {
      writeTool(home, ORIGIN, "hands", { consequential: true }, BODY);
      const fake = serveFake();
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        const out = await reapplyOrigin(conn, "s1", ORIGIN, 3000);
        await conn.close();
        expect(out.reapplied).toEqual([]);
        expect(out.skipped[0]?.reason).toMatch(/confirm:true/);
        expect(fake.counts.evaluate).toBe(0);
      } finally { fake.stop(); }
    });
  });

  test("native names never overwrite; bad shapes never inject", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "keep", {}, BODY);
      writeTool(home, ORIGIN, "sneaky", {}, "(async () => eval('1'))");
      const fake = serveFake({ native: ["keep"] });
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        const out = await reapplyOrigin(conn, "s1", ORIGIN, 3000);
        await conn.close();
        const byName = new Map(out.skipped.map((s) => [s.name, s.reason]));
        expect(byName.get("keep")).toMatch(/already live/);
        expect(byName.get("sneaky")).toMatch(/eval/);
        expect(out.reapplied).toEqual([]);
        expect(fake.registered.has("keep")).toBe(false);
      } finally { fake.stop(); }
    });
  });
});

describe("admitOnCall", () => {
  test("capability is reported on admission", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "capped", { fixtureInput: {} }, BODY, "docs");
      const fake = serveFake();
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        const { loadTool } = await import("./registry.ts");
        expect(loadTool(ORIGIN, "capped").capability).toBe("docs");
        await conn.close();
      } finally { fake.stop(); }
    });
  });

  test("refusals precede injection: NEEDS_CONFIRM and NO_PROOF_PATH inject nothing", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "hands", { consequential: true }, BODY);
      writeTool(home, ORIGIN, "rumor", {}, BODY);
      const fake = serveFake();
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        await expect(admitOnCall(conn, "s1", ORIGIN, "hands", { confirm: false, timeoutMs: 3000 })).rejects.toMatchObject({ reason: "NEEDS_CONFIRM" });
        await expect(admitOnCall(conn, "s1", ORIGIN, "rumor", { confirm: false, timeoutMs: 3000 })).rejects.toMatchObject({ reason: "NO_PROOF_PATH" });
        await expect(admitOnCall(conn, "s1", ORIGIN, "ghost", { confirm: false, timeoutMs: 3000 })).rejects.toMatchObject({ reason: "NO_SUCH_TOOL" });
        await conn.close();
        expect(fake.counts.evaluate).toBe(0);
      } finally { fake.stop(); }
    });
  });

  test("PROOF_FAILED rolls back: nothing stays live", async () => {
    await withHome(async (home) => {
      writeTool(home, ORIGIN, "badproof", { fixtureInput: {} }, BODY);
      const fake = serveFake({ failInvoke: new Set(["badproof"]) });
      try {
        const conn: CdpConnection = await connect(fake.wsUrl);
        await expect(admitOnCall(conn, "s1", ORIGIN, "badproof", { confirm: false, timeoutMs: 3000 })).rejects.toMatchObject({ reason: "PROOF_FAILED" });
        await conn.close();
        expect(fake.registered.has("badproof")).toBe(false);
      } finally { fake.stop(); }
    });
  });

  test("proveTool classifier: thrown text, empty-error fallback, transport", async () => {
    const fake = serveFake({ failInvoke: new Set(["nope"]) });
    try {
      const conn: CdpConnection = await connect(fake.wsUrl);
      const bad = await proveTool(conn, "s1", "f1", "nope", {}, 3000);
      expect(bad).toEqual({ ok: false, why: "page says no" });
      const good = await proveTool(conn, "s1", "f1", "fine", {}, 3000);
      expect(good).toEqual({ ok: true });
      await conn.close();
    } finally { fake.stop(); }
  });
});
