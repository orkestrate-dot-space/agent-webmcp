import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export function dataDir(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? "/tmp";
  return join(home, ".local", "share", "agent-webmcp");
}

export function sitesDir(): string {
  return join(dataDir(), "sites");
}

export function cacheDir(): string {
  return join(dataDir(), "cache");
}

export function indexPath(): string {
  return join(dataDir(), "index.json");
}

export function localIndexPath(): string {
  return join(dataDir(), "local-index.json");
}

export function installedInitPath(): string {
  return join(dataDir(), "init.js");
}

/** Repo root when running from source (dev); undefined in the compiled binary. */
function repoRoot(): string | undefined {
  const root = join(dirname(new URL(import.meta.url).pathname), "..");
  if (existsSync(join(root, "package.json")) && existsSync(join(root, "init.js"))) return root;
  return undefined;
}

export function baseInitPath(): string {
  const repo = repoRoot();
  if (repo) return join(repo, "init.js");
  const home = process.env.HOME ?? process.env.USERPROFILE ?? "/tmp";
  for (const c of [join(home, ".local", "share", "agent-webmcp", "init.js"), join(dirname(process.execPath), "init.js")]) {
    if (existsSync(c)) return c;
  }
  return join(home, ".local", "share", "agent-webmcp", "init.js");
}

/** Managed registry dir (repo `registry/` in dev; seeded copy under share when installed). */
export function registryDir(): string {
  const repo = repoRoot();
  if (repo) {
    const d = join(repo, "registry");
    if (existsSync(d)) return d;
  }
  return join(dataDir(), "registry");
}

export function ensureDirs(): void {
  for (const d of [dataDir(), sitesDir(), cacheDir()]) mkdirSync(d, { recursive: true });
}

/** First-run seed: copy managed registry into the local data dir if present. */
export function seedManaged(): void {
  ensureDirs();
  const reg = registryDir();
  const idx = join(reg, "index.json");
  if (existsSync(idx) && !existsSync(indexPath())) {
    try {
      const raw = readFileSync(idx, "utf8");
      JSON.parse(raw);
      writeFileSync(indexPath(), raw);
    } catch { /* corrupt seed: start empty, sync repairs */ }
  }
  if (!existsSync(localIndexPath())) {
    writeFileSync(localIndexPath(), JSON.stringify({ version: 1, updatedAt: "", domains: {} }));
  }
}
