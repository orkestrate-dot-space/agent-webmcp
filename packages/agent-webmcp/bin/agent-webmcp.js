#!/usr/bin/env node
// Run the platform binary installed as an optional dependency. npm installs
// only the optional dependency matching this machine, so exactly one exists.
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const pkgs = {
  "linux-x64": "@orkestrate/agent-webmcp-linux-x64",
  "linux-arm64": "@orkestrate/agent-webmcp-linux-arm64",
  "darwin-arm64": "@orkestrate/agent-webmcp-darwin-arm64",
  "darwin-x64": "@orkestrate/agent-webmcp-darwin-x64",
  "win32-x64": "@orkestrate/agent-webmcp-win32-x64",
};
const key = `${process.platform}-${process.arch}`;
const name = pkgs[key];
if (!name) {
  console.error(`agent-webmcp: no binary for ${key}`);
  process.exit(1);
}
const subpath = `${name}/bin/agent-webmcp${process.platform === "win32" ? ".exe" : ""}`;

function tryResolve(base) {
  try {
    return createRequire(base).resolve(subpath);
  } catch {
    return null;
  }
}

// A folder install (`npm i ./packages/...`) symlinks the wrapper, and Node
// resolves symlinks before looking for siblings. Fall back to the project
// the command runs in, which lists the same optional dependency.
const bin =
  tryResolve(import.meta.url) ??
  tryResolve(pathToFileURL(join(process.cwd(), "package.json")).href);
if (!bin) {
  console.error(`agent-webmcp: optional dependency ${name} is missing; reinstall with npm i -g @orkestrate/agent-webmcp`);
  process.exit(1);
}
const r = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
process.exit(r.status ?? 1);
