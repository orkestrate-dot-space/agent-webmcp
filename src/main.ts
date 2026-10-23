// main: argument parse, one dispatch. piped=JSON, writes need --yes.
// Exits 0 ok / 2 usage / 1 defect.
import { serveStdio } from "./server.ts";
import { parsePolicy } from "./policy.ts";
import { VERSION } from "./version.ts";

function usage(): string {
  return [
    "agent-webmcp <command> [options]",
    "",
    "  mcp serve [--consequential allow|ask|deny]   serve tools over stdio",
    "  doctor [--cdp URL]                           reachability + release report",
    "  --version                                    print version",
  ].join("\n");
}

const args = process.argv.slice(2);

function flag(name: string): string | undefined {
  const i = args.findIndex((a) => a === name || a.startsWith(`${name}=`));
  if (i === -1) return undefined;
  const a = args[i] as string;
  if (a.includes("=")) return a.slice(name.length + 1);
  return args[i + 1] !== undefined && !args[i + 1]?.startsWith("--") ? args[i + 1] : undefined;
}

async function main(): Promise<void> {
  const [cmd, sub] = args;
  if (cmd === "--version" || cmd === "-V") { console.log(VERSION); return; }
  if (cmd === "doctor") {
    const { discoverDaemonBrowser } = await import("./transport.ts");
    const cdp = flag("--cdp");
    let endpoint: string | undefined;
    try {
      const { discoverWs } = await import("./transport.ts");
      if (cdp !== undefined) { await discoverWs(cdp, 5000); endpoint = cdp; }
      else endpoint = await discoverDaemonBrowser();
    } catch { endpoint = undefined; }
    console.log(JSON.stringify({
      release: "beta", version: VERSION,
      browser: endpoint !== undefined ? { reachable: true, endpoint } : { reachable: false, endpoint: null },
    }));
    return;
  }
  if (cmd === "mcp" && sub === "serve") {
    let policy;
    try { policy = parsePolicy(flag("--consequential")); }
    catch (e) { console.error(e instanceof Error ? e.message : String(e)); process.exit(2); }
    const source = flag("--consequential") === undefined ? "default" as const : "flag" as const;
    await serveStdio({ policy: policy as ReturnType<typeof parsePolicy>, policySource: source });
    return;
  }
  console.error(usage());
  process.exit(2);
}

await main().catch((e) => {
  console.error(e instanceof Error ? e.message : String(e));
  process.exit(1);
});
