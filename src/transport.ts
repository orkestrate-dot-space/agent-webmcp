// Transport: CDP over Bun's native WebSocket. Nothing above this module
// knows CDP; everything above speaks Connection.
import { readdirSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import type { Protocol } from "devtools-protocol";
import { fail } from "./result.ts";

export interface CdpConnection {
  send(method: string, params?: Record<string, unknown>, sessionId?: string): Promise<unknown>;
  onEvent(method: string, handler: (params: unknown, sessionId?: string) => void): void;
  close(): void;
}

export function connect(wsUrl: string): Promise<CdpConnection> {
  return new Promise((resolve, reject) => {
    let nextId = 1;
    let alive = false;
    const pending = new Map<number, { ok: (v: unknown) => void; bad: (e: unknown) => void }>();
    const listeners = new Map<string, Array<(params: unknown, sessionId?: string) => void>>();
    let socket: WebSocket;
    try {
      socket = new WebSocket(wsUrl);
    } catch (e) {
      reject(fail("connect", `cannot open ${wsUrl}`, "is the debugging port open? launch a browser first."));
      return;
    }
    const dead = () => fail("connect", "browser socket is closed", "the browser went away — open again.");
    socket.onopen = () => {
      alive = true;
      resolve({
        send: (method, params = {}, sessionId) =>
          new Promise((ok, bad) => {
            if (!alive) return void bad(dead());
            const id = nextId++;
            pending.set(id, { ok, bad });
            socket.send(JSON.stringify(sessionId === undefined ? { id, method, params } : { id, method, params, sessionId }));
          }),
        onEvent: (method, handler) => {
          const list = listeners.get(method) ?? [];
          list.push(handler);
          listeners.set(method, list);
        },
        close: () => {
          alive = false;
          try { socket.close(); } catch { /* already gone */ }
          for (const p of pending.values()) p.bad(dead());
          pending.clear();
        },
      });
    };
    socket.onerror = () => { if (!alive) reject(fail("connect", `cannot open ${wsUrl}`, "is the debugging port open? launch a browser first.")); };
    socket.onmessage = (event) => {
      let reply: { id?: number; method?: string; params?: unknown; sessionId?: string; result?: unknown; error?: { message?: string } };
      try { reply = JSON.parse(String(event.data)) as typeof reply; } catch { return; }
      if (reply.id !== undefined) {
        const p = pending.get(reply.id);
        if (!p) return;
        pending.delete(reply.id);
        if (reply.error !== undefined) p.bad(fail("page", `CDP error: ${String(reply.error.message ?? "unknown").slice(0, 200)}`, "the browser refused the call — old build or a dead target."));
        else p.ok(reply.result);
        return;
      }
      if (reply.method !== undefined) for (const h of listeners.get(reply.method) ?? []) h(reply.params, reply.sessionId);
    };
    socket.onclose = () => {
      alive = false;
      for (const p of pending.values()) p.bad(dead());
      pending.clear();
    };
  });
}

function withTimeout<T>(p: Promise<T>, ms: number, op: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  const gate = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(fail("timeout", `no reply in ${ms}ms`, `the page is wedged or gone — ${op}.`)), ms);
  });
  return Promise.race([p, gate]).finally(() => clearTimeout(timer!));
}

export async function evaluateJson(conn: CdpConnection, expression: string, timeoutMs: number, sessionId?: string): Promise<unknown> {
  const raw = (await withTimeout(
    conn.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true } satisfies Protocol.Runtime.EvaluateRequest, sessionId),
    timeoutMs, "Runtime.evaluate"
  )) as { result?: { type: string; value: unknown }; exceptionDetails?: unknown };
  if (!raw || typeof raw !== "object" || !("result" in raw)) {
    throw fail("decode", "CDP reply is not an evaluate result", "the browser answered outside the CDP shape.");
  }
  if (raw.exceptionDetails !== undefined) {
    throw fail("page", `page threw: ${JSON.stringify(raw.exceptionDetails).slice(0, 300)}`, "the page's own script failed.");
  }
  return raw.result?.value;
}

export function inheritHostPort(httpEndpoint: string, wsUrl: string): string {
  let http: URL, ws: URL;
  try { http = new URL(httpEndpoint); ws = new URL(wsUrl); } catch { return wsUrl; }
  const local = ws.hostname === "localhost" || ws.hostname === "127.0.0.1" || ws.hostname === "::1";
  if (!local || ws.port !== "") return wsUrl;
  const out = new URL(wsUrl);
  out.hostname = http.hostname;
  out.port = http.port;
  return out.toString();
}

export async function discoverWs(httpEndpoint: string, timeoutMs: number): Promise<string> {
  const res = await tryFetch(`${httpEndpoint}/json/version`, timeoutMs);
  const body = (await res.json()) as { webSocketDebuggerUrl?: unknown };
  if (typeof body.webSocketDebuggerUrl !== "string") {
    throw fail("decode", "version reply names no debugger endpoint", "launch a browser first, or check the port.");
  }
  return inheritHostPort(httpEndpoint, body.webSocketDebuggerUrl);
}

async function tryFetch(url: string, timeoutMs: number): Promise<Response> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const res = await fetch(url, { signal: ctrl.signal });
    if (!res.ok) throw fail("connect", `no debugger (HTTP ${res.status})`, "launch a browser first, or check the port.");
    return res;
  } catch (e) {
    if (e instanceof Error && e.name === "AbortError") throw fail("connect", `no debugger at ${url}`, "launch a browser first, or check the port.");
    throw e instanceof Error && "reason" in e ? e : fail("connect", `no debugger: ${String(e).slice(0, 120)}`, "launch a browser first, or check the port.");
  } finally {
    clearTimeout(timer);
  }
}

export function parseActivePort(text: string): number | undefined {
  const first = text.split("\n")[0]?.trim() ?? "";
  if (!/^\d+$/.test(first)) return undefined;
  const port = Number(first);
  return Number.isInteger(port) && port >= 1024 && port <= 65535 ? port : undefined;
}

// Connect-only discovery: agent-browser daemon tmp profiles, then loopback
// 9222/9229 (an open debug port IS the opt-in). Never fails — returns
// undefined when nothing answers.
export async function discoverDaemonBrowser(options?: { tmpDir?: string; timeoutMs?: number; loopbackPorts?: number[] }): Promise<string | undefined> {
  const dir = options?.tmpDir ?? tmpdir();
  const timeoutMs = options?.timeoutMs ?? 1500;
  let entries: Array<string> = [];
  try { entries = readdirSync(dir); } catch { entries = []; }
  for (const entry of entries) {
    if (!entry.startsWith("agent-browser-chrome-")) continue;
    let port: number | undefined;
    try { port = parseActivePort(readFileSync(`${dir}/${entry}/DevToolsActivePort`, "utf8")); } catch { continue; }
    if (port === undefined) continue;
    try {
      await discoverWs(`http://127.0.0.1:${port}`, timeoutMs);
      return `http://127.0.0.1:${port}`;
    } catch { /* not ours to fail */ }
  }
  for (const port of options?.loopbackPorts ?? [9222, 9229]) {
    try {
      await discoverWs(`http://127.0.0.1:${port}`, timeoutMs);
      return `http://127.0.0.1:${port}`;
    } catch { /* keep looking */ }
  }
  return undefined;
}

export interface PageTool {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Record<string, unknown>;
  readonly frameId: string;
  readonly annotations: { readonly readOnly?: boolean; readonly untrustedContent?: boolean; readonly consequential?: boolean };
}

function toPageTool(t: {
  name: unknown; description: unknown; inputSchema?: unknown; frameId: unknown; annotations?: unknown;
}): PageTool {
  if (typeof t.name !== "string" || typeof t.frameId !== "string") throw fail("decode", "tool burst shape broke", "the page's tool catalog is malformed.");
  const ann = (t.annotations ?? {}) as { readOnly?: unknown; readOnlyHint?: unknown; untrustedContent?: unknown; consequential?: unknown };
  return {
    name: t.name,
    description: typeof t.description === "string" ? t.description : "",
    inputSchema: typeof t.inputSchema === "object" && t.inputSchema !== null && !Array.isArray(t.inputSchema) ? t.inputSchema as Record<string, unknown> : { type: "object" },
    frameId: t.frameId,
    annotations: {
      ...(typeof ann.readOnly === "boolean" ? { readOnly: ann.readOnly } : typeof ann.readOnlyHint === "boolean" ? { readOnly: ann.readOnlyHint } : {}),
      ...(typeof ann.untrustedContent === "boolean" ? { untrustedContent: ann.untrustedContent } : {}),
      ...(typeof ann.consequential === "boolean" ? { consequential: ann.consequential } : {}),
    },
  };
}

// Native catalog read. Subscribes BEFORE enable (the burst fires
// immediately). Empty pages resolve [] — open must succeed with toolCount 0.
// A missing domain throws; callers that tolerate plain Chromium catch it.
export async function listPageTools(conn: CdpConnection, timeoutMs: number, sessionId?: string): Promise<Array<PageTool>> {
  const burst = new Promise<Array<PageTool>>((resolve, reject) => {
    conn.onEvent("WebMCP.toolsAdded", (params, eventSession) => {
      if (sessionId !== undefined && eventSession !== undefined && eventSession !== sessionId) return;
      try {
        const tools = (params as { tools?: unknown }).tools;
        if (!Array.isArray(tools)) throw 0;
        resolve(tools.map((t) => toPageTool(t as Parameters<typeof toPageTool>[0])));
      } catch {
        reject(fail("decode", "tool burst shape broke", "the page's tool catalog is malformed."));
      }
    });
  });
  await withTimeout(conn.send("WebMCP.enable", {}, sessionId), timeoutMs, "WebMCP.enable");
  const grace = new Promise<Array<PageTool>>((resolve) => setTimeout(() => resolve([]), Math.min(timeoutMs, 3000)));
  return Promise.race([burst, grace]);
}

export interface InvocationResult { readonly status: string; readonly output?: unknown; readonly errorText?: string; }

export async function invokePageTool(
  conn: CdpConnection, sessionId: string,
  input: { frameId: string; toolName: string; args: Record<string, unknown> }, timeoutMs: number
): Promise<InvocationResult> {
  let wanted: string | null = null;
  const terminal = new Promise<InvocationResult>((resolve, reject) => {
    conn.onEvent("WebMCP.toolResponded", (params, eventSession) => {
      if (eventSession !== undefined && eventSession !== sessionId) return;
      const p = params as { invocationId?: unknown; status?: unknown; output?: unknown; errorText?: unknown };
      if (wanted !== null && p.invocationId !== wanted) return;
      if (typeof p.invocationId !== "string" || (p.status !== "Ok" && p.status !== "Completed" && p.status !== "Error")) {
        reject(fail("decode", "response shape broke", "the page answered outside the contract."));
        return;
      }
      resolve({ status: p.status, ...(p.output !== undefined ? { output: p.output } : {}), ...(typeof p.errorText === "string" ? { errorText: p.errorText } : {}) });
    });
  });
  const reply = (await withTimeout(
    conn.send("WebMCP.invokeTool", { frameId: input.frameId, toolName: input.toolName, input: input.args }, sessionId),
    timeoutMs, `WebMCP.invokeTool ${input.toolName}`
  )) as { invocationId: string };
  wanted = reply.invocationId;
  try {
    return await withTimeout(terminal, timeoutMs, `WebMCP.invokeTool ${input.toolName}`);
  } catch (e) {
    await conn.send("WebMCP.cancelInvocation", { invocationId: reply.invocationId }, sessionId).catch(() => {});
    throw e;
  }
}
