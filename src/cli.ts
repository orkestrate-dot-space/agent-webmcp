import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { baseInitPath, dataDir, ensureDirs, installedInitPath, listToolkits, searchCapabilities, toolkitsDir } from "./store";
import { VERSION } from "./version";

function fail(msg: string): never {
  console.error(`agent-webmcp: ${msg}`);
  process.exit(1);
}

function cmdInit(args: Array<string>): void {
  const stdout = args.includes("--stdout");
  ensureDirs();
  const installed = installedInitPath();
  if (!existsSync(installed)) {
    const base = baseInitPath();
    if (!existsSync(base)) fail(`base init.js not found (${base})`);
    copyFileSync(base, installed);
  }
  if (stdout) {
    process.stdout.write(readFileSync(installed, "utf8"));
    return;
  }
  console.log(installed);
}

function cmdTools(args: Array<string>): void {
  const json = args.includes("--json");
  const q = args.find((a) => !a.startsWith("--"));
  const rows = searchCapabilities(q);
  if (json) console.log(JSON.stringify({ init: installedInitPath(), tools: rows }));
  else {
    console.log(`init: ${installedInitPath()}`);
    for (const r of rows) console.log(`- ${r.name} [${r.via}] ${r.description}`);
  }
}

function cmdLearn(args: Array<string>): void {
  const [domain, file] = args.filter((a) => !a.startsWith("--"));
  if (!domain || !file) fail("usage: agent-webmcp learn <domain> <file.js>");
  const src = ((): string => {
    try { return readFileSync(file as string, "utf8"); }
    catch { fail(`cannot read ${file}`); throw 0; }
  })();
  if (!/registerTool\s*\(|webmcp\s*\.\s*define\s*\(/.test(src)) {
    fail("file defines no tools (want registerTool({name,...}) or webmcp.define(...))");
  }
  if (src.length > 200_000) fail("toolkit too large (max 200KB)");
  ensureDirs();
  const dest = join(toolkitsDir(), `${domain as string}.js`);
  writeFileSync(dest, src.endsWith("\n") ? src : src + "\n");
  recompile();
  console.log(`learned ${domain} -> ${dest}`);
}

function recompile(): void {
  ensureDirs();
  const base = baseInitPath();
  if (!existsSync(base)) fail(`base init.js not found (${base})`);
  let out = readFileSync(base, "utf8");
  if (!out.endsWith("\n")) out += "\n";
  for (const t of listToolkits()) {
    try {
      const src = readFileSync(t.file, "utf8");
      out += `\n/* toolkit: ${t.domain} (${basename(t.file)}) */\n;(function(){\n${src}\n})();\n`;
    } catch { /* skip unreadable */ }
  }
  mkdirSync(dataDir(), { recursive: true });
  writeFileSync(installedInitPath(), out);
}

function cmdMcp(args: Array<string>): void {
  if (args[0] !== "serve") fail("usage: agent-webmcp mcp serve");
  import("./mcp").then((m) => m.serveStdio()).catch((e) => fail(String(e)));
}

const [cmd, ...rest] = process.argv.slice(2);
if (cmd === "init") cmdInit(rest);
else if (cmd === "tools") cmdTools(rest);
else if (cmd === "learn") cmdLearn(rest);
else if (cmd === "mcp") cmdMcp(rest);
else if (cmd === "--version" || cmd === "-V") console.log(VERSION);
else {
  console.log(`agent-webmcp ${VERSION}\nusage: agent-webmcp <init|tools|learn|mcp>\n  init [--stdout]            print installed init.js path (or source)\n  tools [query] [--json]     search local capabilities (no browser)\n  learn <domain> <f>         save a site toolkit, recompile init.js\n  mcp serve                  stdio MCP server (skill/init/tools/learn)`);
  process.exit(cmd === "--help" || cmd === "-h" || cmd === undefined ? 0 : 1);
}
