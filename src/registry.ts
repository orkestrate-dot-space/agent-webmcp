// Registry: authored tools as files in ONE fixed directory. The engine
// READS (load, validate, inject, stamp); the agent WRITES with its own
// file tools, taught by the skill. No flags, no overlays, no markers —
// the origin lives inside spec.json and is verified on load.
// Layout: <root>/<origin-slug>/{SKILL.md, <name>/{spec.json, body.ts, stamp.json}}
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { normalize, relative, sep } from "node:path";
import { Transpiler } from "bun";
import { fail } from "./result.ts";

export function registryRoot(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? tmpdir();
  if (process.platform === "darwin") return `${home}/Library/Application Support/agent-webmcp/registry`;
  if (process.platform === "win32") return `${process.env.LOCALAPPDATA ?? home}/agent-webmcp/registry`;
  return `${home}/.local/share/agent-webmcp/registry`;
}

export function slugOf(origin: string): string {
  let host: string;
  try {
    const u = new URL(origin);
    host = u.host.toLowerCase(); // host INCLUDES a non-default port — :8099 and :8100 are different origins
  } catch { host = origin.toLowerCase(); }
  return host.replaceAll(/[^a-z0-9]/g, "-");
}

export interface ToolSpec {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Record<string, unknown>;
  readonly origin: string;
  readonly annotations?: Record<string, unknown>;
  readonly fixtureInput?: unknown;
  readonly consequential?: boolean;
  // Declared output contract: surfaced to agents via list/catalog so the
  // return shape is visible before the first call. v1 is surface-only
  // (validation lands with a schema-validator dep, not hand-rolled).
  readonly outputSchema?: Record<string, unknown>;
}

export interface ToolStamp { readonly verified: boolean; readonly bodyHash: string; readonly at: number }

// checkSpec: the gate every file passes before it may reach a page. Pure:
// same bytes → same verdict. Reasons name the fix for the author.
export function checkSpec(spec: unknown, code: string): string | null {
  const s = spec as { name?: unknown; description?: unknown; inputSchema?: unknown; origin?: unknown; outputSchema?: unknown };
  if (typeof s !== "object" || s === null) return "spec.json is not an object";
  if (typeof s.name !== "string" || !/^[A-Za-z][A-Za-z0-9._-]{0,63}$/.test(s.name)) return `bad tool name '${String(s.name)}' (letters, digits, ._-; lead with a letter)`;
  if (typeof s.description !== "string" || s.description.trim().length === 0) return "description is empty";
  if (typeof s.inputSchema !== "object" || s.inputSchema === null || Array.isArray(s.inputSchema)) return "inputSchema must be a JSON Schema object";
  if (s.outputSchema !== undefined && (typeof s.outputSchema !== "object" || s.outputSchema === null || Array.isArray(s.outputSchema))) return "outputSchema must be a JSON Schema object";
  if (typeof s.origin !== "string" || s.origin.length === 0) return "origin is missing (the page origin this tool belongs to)";
  if (code.trim().length === 0) return "body is empty";
  if (code.length > 64000) return `body is ${code.length} chars (max 64000)`;
  const masked = maskCode(code);
  if (/\beval\s*\(/.test(masked)) return "body uses eval() — second-order eval, never legitimate in a page body";
  if (/\bnew Function\s*\(/.test(masked)) return "body uses new Function() — second-order eval, never legitimate in a page body";
  return null;
}

// Mask strings/comments before pattern matching: quoted code must never
// trip the gates above.
export function maskCode(code: string): string {
  return code
    .replace(/`(?:\\.|[^`\\])*`/g, "``")
    .replace(/'(?:\\.|[^'\\])*'/g, "''")
    .replace(/"(?:\\.|[^"\\])*"/g, '""')
    .replace(/\/\/[^\n]*/g, "")
    .replace(/\/\*[\s\S]*?\*\//g, "");
}

// Driver-idiom sniff (warn-only): function calls from the wrong layer
// pasted as a page body. Advisory — validation proceeds.
export function bodyWarnings(code: string): Array<string> {
  const masked = maskCode(code);
  const found: Array<string> = [];
  if (/\bpage\.\w+\s*\(/.test(masked)) found.push("looks like driver code (page.* calls) pasted as a page body — page bodies run in the page, drivers act from outside");
  if (/\$x\s*\(/.test(masked)) found.push("looks like driver code ($x() helper) pasted as a page body — page bodies run in the page, drivers act from outside");
  return found;
}

const ts = new Transpiler({ loader: "ts" });

// TypeScript bodies, plain JS included. Bun drops bare expression
// statements, so bodies ride inside a synthetic `export default` that is
// stripped back off — the snippet still receives a bare function
// expression. A body that does not transpile is corrupt, named at load.
export function transpileBody(source: string): string {
  let out: string;
  try { out = ts.transformSync(`export default (${source});`); }
  catch (e) { throw fail("corrupt", `body does not transpile: ${String(e).slice(0, 160)}`, "fix the file or re-author the tool."); }
  const body = out.replace(/^export default /, "").replace(/;\s*$/, "");
  if (body.trim().length === 0) throw fail("corrupt", "body transpiled to nothing", "fix the file or re-author the tool.");
  return body;
}

function insideRoot(root: string, path: string): void {
  const rel = relative(normalize(root), normalize(path));
  if (rel === "" || rel.startsWith("..") || rel.startsWith(sep)) {
    throw fail("outside-root", `refusing path outside registry: ${path}`, "keep tool files under the registry dir.");
  }
}

export interface LoadedTool { readonly spec: ToolSpec; readonly body: string; readonly stamp: ToolStamp | null }

export function loadTool(origin: string, name: string): LoadedTool {
  const root = registryRoot();
  const dir = `${root}/${slugOf(origin)}/${name}`;
  insideRoot(root, dir);
  let raw: string, source: string;
  try {
    raw = readFileSync(`${dir}/spec.json`, "utf8");
    try { source = readFileSync(`${dir}/body.ts`, "utf8"); }
    catch { source = readFileSync(`${dir}/body.js`, "utf8"); }
  } catch {
    throw fail("missing", `no saved tool '${name}' for ${origin}`, "write spec.json plus body.ts first.");
  }
  let spec: ToolSpec;
  try { spec = JSON.parse(raw) as ToolSpec; }
  catch { throw fail("corrupt", `spec.json unparseable for '${name}'`, "fix the file or re-author the tool."); }
  if (spec.name !== name) throw fail("corrupt", `spec name mismatch for '${name}'`, "fix the file or re-author the tool.");
  if (spec.origin !== origin) throw fail("origin-mismatch", `'${name}' belongs to ${spec.origin}, not ${origin}`, "open the tool's own origin, or fix spec.origin.");
  const shapeError = checkSpec(spec, source);
  if (shapeError !== null) throw fail("bad-shape", `bad shape (${shapeError})`, "fix the file; nothing was injected.");
  let stamp: ToolStamp | null = null;
  try {
    const parsed = JSON.parse(readFileSync(`${dir}/stamp.json`, "utf8")) as ToolStamp;
    if (parsed && typeof parsed.bodyHash === "string") stamp = parsed;
  } catch { /* unstamped — first admission will stamp */ }
  return { spec, body: transpileBody(source), stamp };
}

export function stampTool(origin: string, name: string, bodyHash: string): void {
  const root = registryRoot();
  const dir = `${root}/${slugOf(origin)}/${name}`;
  insideRoot(root, dir);
  mkdirSync(dir, { recursive: true });
  writeFileSync(`${dir}/stamp.json`, JSON.stringify({ verified: true, bodyHash, at: Date.now() }));
}

export function bodyHashOf(body: string): string {
  return Bun.hash(body).toString(16);
}

export function listOriginTools(origin: string): Array<string> {
  const dir = `${registryRoot()}/${slugOf(origin)}`;
  try {
    return readdirSync(dir, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name).sort();
  } catch { return []; }
}

export function originSkill(origin: string): string | null {
  try {
    const text = readFileSync(`${registryRoot()}/${slugOf(origin)}/SKILL.md`, "utf8");
    return text.trim().length === 0 ? null : text.slice(0, 4096);
  } catch { return null; }
}

export function registryExists(): boolean {
  try { return existsSync(registryRoot()); } catch { return false; }
}
