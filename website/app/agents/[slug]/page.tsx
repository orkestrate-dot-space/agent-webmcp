import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { ExternalLink, Info, MessageSquare, Search } from "lucide-react";

import SiteNav from "../../site-nav";
import { DomainScan } from "./domain-scan";
import { contractOf, contractSlugs } from "../list-data";
import type { InvokeContract, SiteTool } from "../scan";
import { looksLikeDomain, SITES_API_VERSION } from "../scan";
import { OriginFavicon } from "../site-art";
import { CopyCodeButton } from "./copy-code-button";
import { IntegrateTabs } from "./integrate-tabs";

export const dynamicParams = true;

// Detail pages render on demand, never from ISR: a loading shell
// must never be baked into the cache, or refresh() keeps serving
// it after the scan lands.
export const dynamic = "force-dynamic";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

interface RecordShape {
  origin: string;
  slug: string;
  name: string;
  description: string;
  platform: string;
  skill: string;
  tools: Array<SiteTool>;
  invoke: InvokeContract | null;
}

function toRecord(j: unknown): { domain: string; description: string } | null {
  if (typeof j !== "object" || j === null) return null;
  const s = (j as { site?: unknown }).site;
  if (typeof s !== "object" || s === null) return null;
  const r = s as Record<string, unknown>;
  if (typeof r["domain"] !== "string" || r["domain"] === "") return null;
  return {
    domain: r["domain"],
    description: typeof r["description"] === "string" ? r["description"] : "",
  };
}

async function readRecord(host: string): Promise<RecordShape | null> {
  let worker: { domain: string; description: string } | null = null;
  try {
    const r = await fetch(`${WORKER}/sites/${encodeURIComponent(host)}?v=${SITES_API_VERSION}`, {
      next: { revalidate: 3600, tags: ["site", host] },
    });
    if (r.ok) worker = toRecord(await r.json());
  } catch {
    // Fall through to the contract below.
  }
  const contract = contractOf(host);
  if (!worker && !contract) return null;
  return {
    origin: `https://${worker?.domain ?? host}`,
    slug: host,
    name: contract?.name || host,
    description: contract?.description || worker?.description || "",
    platform: contract?.platform || "",
    skill: contract?.skill || "",
    tools: contract?.tools || [],
    invoke: contract?.invoke ?? null,
  };
}

export function generateStaticParams(): Array<{ slug: string }> {
  // Contracted rows prerender at build; the rest generate on demand.
  return contractSlugs().map((slug) => ({ slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  const host = slug.includes(".") ? slug.toLowerCase() : null;
  if (host === null) return { title: "Not found · agent-webmcp" };
  return { title: `${host} · agent-webmcp`, description: `Ask ${host}'s AI assistant through agent-webmcp.` };
}

function Shell({ host, children }: { host: string; children: React.ReactNode }) {
  return (
    <>
      <SiteNav />
      <div className="mx-auto w-full max-w-[1120px] px-6 py-10">
        <p className="text-sm leading-5 text-ink-2">
          <Link href="/agents" className="no-underline hover:text-ink">
            ← All agents
          </Link>
        </p>
        <h1 className="mt-6 text-[36px] font-medium leading-[1.05] tracking-[-0.04em] text-ink sm:text-[56px]">
          {host}
        </h1>
        {children}
      </div>
    </>
  );
}

const COPY_PROMPT = (host: string, description: string) =>
  `You can ask ${host}'s docs assistant anything about what's in their docs. ${description} It answers with citations to the pages it used, keeps context across follow-up questions, and says no to out-of-scope questions instead of guessing. When I ask about ${host}, consult it first and cite what it says.`;

// Example prompts: verified transcripts first, then suggestions.
function examplePrompts(host: string, short: string): Array<string> {
  if (host === "mintlify.com")
    return ["How do I get started?", "What is the assistant widget?", "How does billing work for the assistant?"];
  if (host === "claude.com")
    return [
      "What is a connector?",
      "How is that different from an MCP server?",
      "How do directory connectors differ from custom ones?",
    ];
  return [`Search ${short} docs for "…"`, `Read a ${short} docs page as Markdown`];
}

const PROMPT_ICONS = [Search, MessageSquare, Info];

const isReadOnly = (name: string): boolean => !/feedback|submit|navigate|open|create|update/i.test(name);

function Pill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center rounded-full border border-line px-2 py-0.5 font-mono text-[11px] leading-4 text-ink-2">
      {children}
    </span>
  );
}

interface Snippets {
  cli: string;
  ts: string;
  py: string;
}

function integrateSnippets(endpoint: string, searchTool: string): Snippets {
  const q = "How do I get started?";
  const ts =
    `const res = await fetch("${endpoint}", {\n` +
    `  method: "POST",\n` +
    `  headers: { "Content-Type": "application/json", Accept: "application/json, text/event-stream" },\n` +
    `  body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call",\n` +
    `    params: { name: "${searchTool}", arguments: { query: "${q}" } } }),\n` +
    `});\n` +
    `// -> cited passages with source links (no key, no browser)`;
  const cli =
    `curl -X POST "${endpoint}" \\\n` +
    `  -H "Content-Type: application/json" \\\n` +
    `  -H "Accept: application/json, text/event-stream" \\\n` +
    `  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call",` +
    `"params":{"name":"${searchTool}","arguments":{"query":"${q}"}}}'`;
  const py =
    `import json, urllib.request\n\n` +
    `req = urllib.request.Request("${endpoint}",\n` +
    `    data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",\n` +
    `        "params": {"name": "${searchTool}",\n` +
    `                   "arguments": {"query": "${q}"}}}).encode(),\n` +
    `    headers={"Content-Type": "application/json",\n` +
    `             "Accept": "application/json, text/event-stream"})\n` +
    `print(urllib.request.urlopen(req).read().decode()[:2000])`;
  return { cli, ts, py };
}

export default async function SitePage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  if (!slug.includes(".")) notFound();
  const host = slug.toLowerCase();
  if (!looksLikeDomain(host)) notFound();

  const record = await readRecord(host);
  if (!record || !record.invoke) {
    return (
      <Shell host={host}>
        <DomainScan host={host} />
      </Shell>
    );
  }

  const platformLabel = record.platform === "" ? "site" : record.platform;
  const prompts = examplePrompts(host, host.split(".")[0]);
  const searchTool = record.tools.find((t) => t.name.startsWith("search"))?.name ?? "search";
  const siblings = contractSlugs()
    .filter((s) => s !== host && (contractOf(s)?.platform || "") === record.platform && record.platform !== "")
    .map((s) => ({ slug: s, tools: contractOf(s)?.tools.length ?? 0 }));
  const facts: Array<{ label: string; value: string }> = [
    { label: "Tools", value: String(record.tools.length) },
    { label: "Transport", value: record.invoke.transport },
    { label: "Platform", value: platformLabel },
    { label: "Verified", value: host === "mintlify.com" || host === "claude.com" ? "2026-11-01" : "MCP handshake" },
    { label: "get_agent calls", value: String(await usageCount(host)) },
  ];

  return (
    <>
      <SiteNav />
      <div className="mx-auto w-full max-w-[1120px] px-6 py-10">
        <p className="text-sm leading-5 text-ink-2">
          <Link href="/agents" className="no-underline hover:text-ink">
            ← All agents
          </Link>
        </p>

        <div className="mt-6 flex min-w-0 items-center gap-3">
          <div className="shrink-0">
            <OriginFavicon host={host} size={32} />
          </div>
          <p className="font-mono text-xs uppercase leading-5 tracking-wide text-ink-2">{host}</p>
        </div>
        <h1 className="mt-2 break-all text-[clamp(2.5rem,1.75rem+4vw,3.5rem)] font-medium leading-[1.05] tracking-[-0.04em] text-ink">
          {record.name}
        </h1>

        {record.description !== "" ? (
          <p className="mt-3 max-w-[70ch] text-[15px] leading-6 text-ink-2">{record.description}</p>
        ) : null}

        <div className="mt-5 flex flex-wrap gap-3">
          <a
            href={record.origin}
            target="_blank"
            rel="noreferrer"
            className="inline-flex h-10 cursor-pointer items-center rounded-full bg-ink px-5 text-sm font-medium leading-5 text-canvas no-underline transition hover:opacity-85"
          >
            Open {host} <ExternalLink className="ml-1.5 h-3.5 w-3.5" aria-hidden="true" />
          </a>
          <CopyCodeButton code={COPY_PROMPT(host, record.description)} label="Copy prompt" secondary />
        </div>

        <div className="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_300px]">
          <div className="min-w-0">
            <h2 className="text-xl font-medium leading-7 text-ink">Ask an agent to</h2>
            <div className="mt-3 space-y-2.5">
              {prompts.map((p, i) => {
                const Icon = PROMPT_ICONS[i % PROMPT_ICONS.length];
                return (
                  <div
                    key={p}
                    className="flex items-center gap-3 rounded-lg border border-line px-4 py-3 text-[15px] leading-6 text-ink"
                  >
                    <Icon className="h-4 w-4 shrink-0 text-ink-2" aria-hidden="true" />
                    {p}
                  </div>
                );
              })}
            </div>

            <h2 className="mt-9 text-xl font-medium leading-7 text-ink">
              {record.tools.length} tool{record.tools.length === 1 ? "" : "s"}
            </h2>
            {record.tools.length === 0 ? (
              <p className="mt-2 text-[15px] leading-6 text-ink-2">
                Verbs observed live; full list pending a rescan.
              </p>
            ) : (
              <div className="mt-3 rounded-lg border border-line px-5">
                {record.tools.map((t) => (
                  <div key={t.name} className="border-b border-line py-3.5 last:border-b-0">
                    <p className="flex flex-wrap items-center gap-2 font-mono text-[13px] leading-5 text-ink">
                      {t.name}
                      {isReadOnly(t.name) ? <Pill>read-only</Pill> : null}
                      <Pill>untrusted content</Pill>
                    </p>
                    {t.params !== "" ? (
                      <p className="mt-1 font-mono text-xs leading-5 text-ink-2">{t.params}</p>
                    ) : null}
                  </div>
                ))}
              </div>
            )}

            {host === "claude.com" && (
              <div className="mt-4 rounded-lg border border-line bg-raised p-5">
                <p className="font-mono text-xs uppercase tracking-wide text-ink-2">
                  Tried live — multi-turn, headed browser
                </p>
                <p className="mt-3 text-[15px] leading-6 text-ink">
                  <span className="text-ink-2">Q: </span>
                  What is a connector?
                </p>
                <p className="mt-1 text-[15px] leading-6 text-ink">
                  <span className="text-ink-2">A: </span>
                  Read 1 file. A connector links Claude to an outside app or service… with links to Get started
                  with connectors and the connectors directory.
                </p>
                <p className="mt-3 text-[15px] leading-6 text-ink">
                  <span className="text-ink-2">Q: </span>
                  How is that different from an MCP server?
                </p>
                <p className="mt-1 text-[15px] leading-6 text-ink">
                  <span className="text-ink-2">A: </span>
                  Resolved “that” in context: a connector is the user-facing presentation, the MCP server is what
                  it connects to — directory vs custom, same infrastructure.
                </p>
              </div>
            )}

            <p className="mt-11 font-mono text-xs uppercase tracking-wide text-ink-2">Integrate</p>
            <h2 className="mt-1 text-[28px] font-medium leading-[1.15] tracking-[-0.02em] text-ink">
              Use from your agent
            </h2>
            <p className="mt-2 max-w-[70ch] text-sm leading-6 text-ink-2">
              Call {short(host)} as a tool in any agent with agent-webmcp.
            </p>
            <ul className="mt-3 list-none space-y-2 p-0 text-[15px] leading-6 text-ink">
              <li>
                <span className="mr-2 text-emerald-500" aria-hidden="true">
                  ✓
                </span>
                Single call to register and use tools
              </li>
              <li>
                <span className="mr-2 text-emerald-500" aria-hidden="true">
                  ✓
                </span>
                Typed tool definitions
              </li>
              <li>
                <span className="mr-2 text-emerald-500" aria-hidden="true">
                  ✓
                </span>
                Works in any agent runtime
              </li>
            </ul>
            <IntegrateTabs snippets={integrateSnippets(record.invoke.endpoint, searchTool)} />
            <p className="mt-3 text-sm leading-6 text-ink-2">
              Full skill (registerTool def + headed-browser tier):{" "}
              <code className="font-mono text-[13px] text-ink">await tools.get_agent({"{"} domain: &quot;{host}&quot; {"}"})</code>
            </p>
          </div>

          <aside className="min-w-0">
            <dl className="overflow-hidden rounded-lg border border-line">
              {facts.map((f) => (
                <div key={f.label} className="flex items-center justify-between gap-4 border-b border-line px-4 py-2.5 last:border-b-0">
                  <dt className="text-sm leading-5 text-ink-2">{f.label}</dt>
                  <dd className="truncate font-mono text-[13px] leading-5 text-ink">{f.value}</dd>
                </div>
              ))}
            </dl>
            <p className="mt-4 text-sm leading-6 text-ink-2">
              Tool names are what {host} registered via its docs MCP. Example prompts are suggestions — except the
              tried-live transcript, which is verbatim from a verified run. Usage counts direct get_agent calls
              from this page (local counter, resets on redeploy).
            </p>
          </aside>
        </div>

        {siblings.length > 0 && (
          <div className="mt-12">
            <h2 className="text-xl font-medium leading-7 text-ink">More {platformLabel} sites</h2>
            <div className="mt-4 flex flex-wrap items-center gap-x-7 gap-y-3">
              {siblings.map((s) => (
                <Link
                  key={s.slug}
                  href={`/agents/${s.slug}`}
                  prefetch={false}
                  className="inline-flex items-center gap-2 text-[15px] text-ink-2 no-underline transition hover:text-ink"
                >
                  <OriginFavicon host={s.slug} size={18} />
                  {s.slug}
                </Link>
              ))}
            </div>
            <p className="mt-4 text-[15px] leading-6">
              <Link href="/agents" className="text-ink underline decoration-line underline-offset-4 hover:text-ink-2">
                Browse all agents
              </Link>
            </p>
          </div>
        )}

        <p className="mt-8 border-t border-line pt-4 text-sm leading-5 text-ink-2">
          Something wrong?{" "}
          <a href="https://github.com/orkestrate-dot-space/agent-webmcp/issues" target="_blank" rel="noreferrer" className="text-link no-underline hover:opacity-80">
            Open an issue
          </a>{" "}
          to delist it here.
        </p>
      </div>
    </>
  );
}

import { readFile } from "node:fs/promises";
import { join } from "node:path";

async function usageCount(host: string): Promise<number> {
  try {
    const text = await readFile(join(process.cwd(), "data", "agent-usage.jsonl"), "utf8");
    let n = 0;
    for (const line of text.split("\n")) {
      if (line === "") continue;
      try {
        if ((JSON.parse(line) as { domain?: unknown }).domain === host) n += 1;
      } catch {
        // Skip corrupt lines; the log is append-only.
      }
    }
    return n;
  } catch {
    return 0;
  }
}

function short(host: string): string {
  return host.split(".")[0].charAt(0).toUpperCase() + host.split(".")[0].slice(1);
}
