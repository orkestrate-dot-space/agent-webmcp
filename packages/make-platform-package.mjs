#!/usr/bin/env node
// Assemble one platform package for npm: packages/out/<pkg>/{package.json,bin/<exe>}.
//   node packages/make-platform-package.mjs linux-x64 target/x86_64-unknown-linux-gnu/release/agent-webmcp
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const [pkg, bin] = process.argv.slice(2);
if (!pkg || !bin) throw new Error("usage: make-platform-package.mjs <pkg> <binary>");

const version = readFileSync(join(repo, "Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (!version) throw new Error("no version in Cargo.toml");

const [os, cpu] = pkg.split("-");
const npmOs = { linux: "linux", darwin: "darwin", win32: "win" }[os] ?? os;
const exe = pkg.startsWith("win32") ? "agent-webmcp.exe" : "agent-webmcp";

const out = join(repo, "packages", "out", pkg);
mkdirSync(join(out, "bin"), { recursive: true });
copyFileSync(join(repo, bin), join(out, "bin", exe));

writeFileSync(
  join(out, "package.json"),
  `${JSON.stringify(
    {
      name: `@orkestrate/agent-webmcp-${pkg}`,
      version,
      os: [npmOs],
      cpu: [cpu],
      files: ["bin"],
    },
    null,
    2,
  )}\n`,
);
console.log(`platform package ${pkg} ${version}`);
