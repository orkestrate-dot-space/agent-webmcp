import { existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

export function dataDir(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? "/tmp";
  return join(home, ".local", "share", "agent-webmcp");
}

export function toolkitsDir(): string {
  return join(dataDir(), "toolkits");
}

export function ensureDirs(): void {
  mkdirSync(toolkitsDir(), { recursive: true });
}

export interface ToolkitInfo {
  readonly domain: string;
  readonly file: string;
}

export function listToolkits(): Array<ToolkitInfo> {
  ensureDirs();
  let files: Array<string> = [];
  try { files = readdirSync(toolkitsDir()); } catch { return []; }
  return files
    .filter((f) => f.endsWith(".js"))
    .map((f) => ({ domain: f.replace(/\.js$/, ""), file: join(toolkitsDir(), f) }))
    .sort((a, b) => a.domain.localeCompare(b.domain));
}

export interface Builtin {
  readonly name: string;
  readonly description: string;
  readonly match: string;
}

export const BUILTINS: Array<Builtin> = [
  { name: "ask_docs_expert", description: "Ask the site's embedded docs assistant (Mintlify/Inkeep/Kapa/ReadMe auto-detected at load).", match: "universal docs-chat fingerprint" },
  { name: "submit_search_form", description: "Fill and submit the page's search/filter form.", match: "universal declarative + semantic forms" },
  { name: "run_command", description: "Run the page's command palette entry.", match: "universal cmdk/dialog" },
  { name: "extract_state", description: "Read page hydration/entity state (__NEXT_DATA__/JSON-LD/tables) as typed JSON.", match: "universal state introspector" },
  { name: "read_network", description: "Replay an observed same-origin read the app already performs.", match: "universal passive network harvest" },
];

export function searchCapabilities(query?: string): Array<{ name: string; via: string; description: string }> {
  const q = (query ?? "").trim().toLowerCase();
  const out: Array<{ name: string; via: string; description: string }> = [];
  for (const b of BUILTINS) {
    if (q && !(b.name.toLowerCase().includes(q) || b.description.toLowerCase().includes(q))) continue;
    out.push({ name: b.name, via: "universal", description: b.description });
  }
  for (const t of listToolkits()) {
    if (q && !t.domain.toLowerCase().includes(q)) continue;
    let names: Array<string> = [];
    try {
      const src = readFileSync(t.file, "utf8");
      const re = /registerTool\(\s*{[^}]*name\s*:\s*["']([^"']+)["']/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(src)) !== null) names.push(m[1] as string);
    } catch { /* unreadable toolkit: list domain only */ }
    if (names.length === 0) out.push({ name: `${t.domain}/*`, via: "site", description: `Site toolkit for ${t.domain}.` });
    else for (const n of names) {
      if (q && !n.toLowerCase().includes(q)) continue;
      out.push({ name: n, via: "site", description: `Site toolkit for ${t.domain}.` });
    }
  }
  return out.slice(0, 50);
}

export function baseInitPath(): string {
  // Dev: repo root. Installed: beside binary, or share dir.
  const home = process.env.HOME ?? process.env.USERPROFILE ?? "/tmp";
  const candidates = [
    join(dirname(new URL(import.meta.url).pathname), "..", "init.js"),
    join(dirname(process.execPath), "init.js"),
    join(home, ".local", "share", "agent-webmcp", "init.js"),
  ];
  for (const c of candidates) if (existsSync(c)) return c;
  return candidates[0] as string;
}

export function installedInitPath(): string {
  return join(dataDir(), "init.js");
}
