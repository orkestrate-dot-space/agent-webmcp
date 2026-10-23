// Runner: agent JS in a Bun.Worker, host bridge over a queue. Budgets
// enforced with plain counters — timeout, max calls, max chars.
// No hand timers: the run ends at the gate or the timeout below.
export interface RunBudgets { readonly timeoutMs: number; readonly maxToolCalls: number; readonly maxChars: number; readonly maxResultChars: number }

export type CallFn = (input: unknown) => Promise<unknown>;

export type RunResult =
  | { readonly ok: true; readonly value: unknown; readonly spilled: boolean; readonly toolCalls: number; readonly calls: ReadonlyArray<string>; readonly logs: ReadonlyArray<string> }
  | { readonly ok: false; readonly error: { readonly kind: "ParseError" | "TimeoutExceeded" | "ToolCallLimitExceeded" | "ToolFailure"; readonly message: string }; readonly toolCalls: number; readonly calls: ReadonlyArray<string>; readonly logs: ReadonlyArray<string> };

type WorkerMsg =
  | { readonly type: "call"; readonly id: number; readonly path: string; readonly input: unknown }
  | { readonly type: "done"; readonly value: unknown; readonly logs: ReadonlyArray<string> }
  | { readonly type: "fail"; readonly kind: "ParseError" | "ToolFailure"; readonly message: string; readonly logs: ReadonlyArray<string> };

type HostMsg = { readonly type: "result"; readonly id: number; readonly ok: boolean; readonly value?: unknown; readonly message?: string };

// Worker source: `tools` is a proxy tree — every property access builds a
// deeper path, every call posts it to the host. Unknown paths reach the
// host too (reply: unknown tool path) instead of dying as TypeErrors
// in-page. `then`/symbols resolve undefined so `await` never traps.
const workerSource = (code: string): string => `
const __pending = new Map();
let __seq = 0;
const __call = (path, input) => new Promise((resolve, reject) => {
  const id = __seq++;
  __pending.set(id, { resolve, reject });
  postMessage({ type: "call", id, path, input });
});
const __at = (path) => new Proxy(function () {}, {
  get: (_, prop) => {
    if (typeof prop === "symbol" || prop === "then") return undefined;
    // Reserved dialect builtins: search() and catalog() resolve against
    // the host catalog mid-run (discovery without leaving the code).
    if (path === "" && (prop === "search" || prop === "catalog")) {
      return (...__a) => __call(prop, __a.length > 0 ? __a[0] : null);
    }
    const p = path === "" ? String(prop) : path + "." + String(prop);
    return __at(p);
  },
  apply: (_, __, a) => __call(path, a.length > 0 ? a[0] : null),
});
const tools = __at("");
// Console capture: agent print-debugging returns as data (capped), so a
// run's reasoning trace survives even when the return doesn't show it.
const __logs = [];
for (const __k of ["log", "warn", "error"]) {
  console[__k] = (...__a) => {
    try {
      __logs.push(("[" + __k + "] " + __a.map((__x) => typeof __x === "string" ? __x : JSON.stringify(__x)).join(" ")).slice(0, 500));
      if (__logs.length > 50) __logs.shift();
    } catch {}
  };
}
onmessage = (e) => {
  const m = e.data;
  if (m && m.type === "result") {
    const p = __pending.get(m.id);
    __pending.delete(m.id);
    if (p) { m.ok ? p.resolve(m.value) : p.reject(new Error(m.message)); }
  }
};
// Reserved dialect builtins: bare search()/catalog() resolve against the
// host catalog mid-run (discovery without leaving the code). Passed as
// fn params because new Function bodies see only params + globals.
const __search = (...__a) => __call("search", __a.length > 0 ? __a[0] : null);
const __catalog = (...__a) => __call("catalog", __a.length > 0 ? __a[0] : null);
function __normalize(v, depth) {
  if (depth > 32) return "[truncated: depth]";
  if (v === undefined) return null;
  if (v === null) return null;
  if (typeof v === "function") return "[function]";
  if (typeof v === "bigint") return String(v);
  if (typeof v === "symbol") return String(v);
  if (typeof v !== "object") return v;
  if (Array.isArray(v)) return v.map((x) => __normalize(x, depth + 1));
  const o = {};
  for (const k of Object.keys(v).slice(0, 1000)) o[k] = __normalize(v[k], depth + 1);
  return o;
}
(async () => {
  let fn;
  try {
    fn = new Function("tools", "search", "catalog", '"use strict"; return (async () => {\\n' + ${JSON.stringify(code)} + '\\n})()');
  } catch (err) {
    postMessage({ type: "fail", kind: "ParseError", message: String(err && err.message || err).slice(0, 500), logs: __logs.slice(-20) });
    return;
  }
  try {
    const value = await fn(tools, __search, __catalog);
    postMessage({ type: "done", value: __normalize(value, 0), logs: __logs.slice(-20) });
  } catch (err) {
    postMessage({ type: "fail", kind: "ToolFailure", message: String(err && err.message || err).slice(0, 500), logs: __logs.slice(-20) });
  }
})();
`;

export async function runCode(options: { code: string; calls: Record<string, CallFn>; budgets: RunBudgets }): Promise<RunResult> {
  return runWithWorker(options.code, options.calls, options.budgets);
}

async function runWithWorker(code: string, calls: Record<string, CallFn>, budgets: RunBudgets): Promise<RunResult> {
  const { tmpdir } = await import("node:os");
  const { unlinkSync } = await import("node:fs");
  const file = `${tmpdir()}/agent-webmcp-run-${crypto.randomUUID()}.mjs`;
  await Bun.write(file, workerSource(code));
  const worker = new Worker(file);
  try {
    return await new Promise<RunResult>((resolve) => {
      let calls_n = 0;
      let spilled = false;
      const paths: Array<string> = [];
      let done = false;
      const finish = (r: RunResult): void => { if (!done) { done = true; resolve(r); } };
      const timer = setTimeout(() => finish({ ok: false, error: { kind: "TimeoutExceeded", message: `past timeoutMs (${budgets.timeoutMs})` }, toolCalls: calls_n, calls: paths, logs: [] }), budgets.timeoutMs);
      const tell = (msg: HostMsg): void => { try { worker.postMessage(msg); } catch { /* worker gone */ } };
      worker.onmessage = (event) => {
        const msg = event.data as WorkerMsg;
        if (msg.type === "done" || msg.type === "fail") {
          clearTimeout(timer);
          if (msg.type === "done") {
            const text = JSON.stringify(msg.value) ?? "null";
            if (text.length > budgets.maxResultChars) {
              finish({ ok: true, value: `${text.slice(0, budgets.maxResultChars)}…[truncated]`, spilled: true, toolCalls: calls_n, calls: paths, logs: msg.logs });
            } else {
              finish({ ok: true, value: msg.value === undefined ? null : msg.value, spilled, toolCalls: calls_n, calls: paths, logs: msg.logs });
            }
          } else {
            finish({ ok: false, error: { kind: msg.kind, message: msg.message }, toolCalls: calls_n, calls: paths, logs: msg.logs });
          }
          return;
        }
        const call = calls[msg.path];
        if (call === undefined) { tell({ type: "result", id: msg.id, ok: false, message: `unknown tool path: ${msg.path}` }); return; }
        const inputText = JSON.stringify(msg.input) ?? "null";
        if (inputText.length > budgets.maxChars) { tell({ type: "result", id: msg.id, ok: false, message: `input exceeds maxChars (${inputText.length} > ${budgets.maxChars})` }); return; }
        if (calls_n >= budgets.maxToolCalls) {
          finish({ ok: false, error: { kind: "ToolCallLimitExceeded", message: `past maxToolCalls (${budgets.maxToolCalls})` }, toolCalls: calls_n, calls: paths, logs: [] });
          tell({ type: "result", id: msg.id, ok: false, message: `past maxToolCalls (${budgets.maxToolCalls})` });
          return;
        }
        calls_n++;
        paths.push(msg.path);
        call(msg.input).then(
          (v) => {
            const outText = JSON.stringify(v) ?? "null";
            if (outText.length > budgets.maxChars) spilled = true;
            tell({ type: "result", id: msg.id, ok: true, value: v });
          },
          (e) => tell({ type: "result", id: msg.id, ok: false, message: String(e instanceof Error ? e.message : e).slice(0, 500) })
        );
      };
      worker.onerror = (e) => {
        clearTimeout(timer);
        finish({ ok: false, error: { kind: "ToolFailure", message: `worker defect: ${String(e.message ?? e).slice(0, 200)}` }, toolCalls: calls_n, calls: paths, logs: [] });
      };
    });
  } finally {
    try { worker.terminate(); } catch { /* already gone */ }
    try { unlinkSync(file); } catch { /* /tmp cleans itself */ }
  }
}
