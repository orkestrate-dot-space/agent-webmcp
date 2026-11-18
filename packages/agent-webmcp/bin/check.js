#!/usr/bin/env node
// Postinstall: fail loudly when no platform binary landed, so `agent-webmcp`
// never silently does nothing.
import { createRequire } from "node:module";

const pkgs = {
  "linux-x64": "@orkestrate/agent-webmcp-linux-x64",
  "linux-arm64": "@orkestrate/agent-webmcp-linux-arm64",
  "darwin-arm64": "@orkestrate/agent-webmcp-darwin-arm64",
  "darwin-x64": "@orkestrate/agent-webmcp-darwin-x64",
  "win32-x64": "@orkestrate/agent-webmcp-win32-x64",
};
const name = pkgs[`${process.platform}-${process.arch}`];
if (!name) {
  console.warn(`agent-webmcp: no binary for ${process.platform}-${process.arch}; this platform is untested`);
  process.exit(0);
}
try {
  createRequire(import.meta.url).resolve(`${name}/package.json`);
} catch {
  console.error(`agent-webmcp: ${name} did not install. Check your npm optional-dependency settings and reinstall.`);
  process.exit(1);
}
