import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";

import SiteNav from "../../site-nav";
import { DomainScan } from "./domain-scan";
import { contractOf, contractSlugs } from "../list-data";
import type { InvokeContract } from "../scan";
import { looksLikeDomain, SITES_API_VERSION } from "../scan";
import { OriginFavicon } from "../site-art";
import { CopyCodeButton } from "./copy-code-button";
import { LatencyChart } from "./latency-chart";
import runsFile from "../../../data/mintlify-runs.json";
import { ExternalLink } from "lucide-react";

export const dynamicParams = true;

// Detail pages render on demand, never from ISR: a loading shell
// must never be baked into the cache, or refresh() keeps serving
// it after the scan lands.
export const dynamic = "force-dynamic";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

interface RecordShape {
  origin: string;
  slug: string;
  description: string;
  platform: string;
  skill: string;
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
    description: contract?.description || worker?.description || "",
    platform: contract?.platform || "",
    skill: contract?.skill || "",
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

const ASK_SNIPPET = (tool: string) =>
  `const answer = await tools.${tool}({\n  question: "How do I get started?",\n});\n// → worked answer with citations`;

const TRIED_Q = "How do I get started?";
const TRIED_A =
  "Read 1 file and ran 1 command. Follow the Quickstart: deploy via mintlify.com/start, then make changes in the web editor or with the CLI… with links to the pages it used.";

const COPY_PROMPT = (host: string, description: string) =>
  `You can ask ${host}'s docs assistant anything about what's in their docs. ${description} It answers with citations to the pages it used, keeps context across follow-up questions, and says no to out-of-scope questions instead of guessing. When I ask about ${host}, consult it first and cite what it says.`;

interface RunRow {
  label?: unknown;
  question?: unknown;
  ok?: unknown;
  ms?: unknown;
  cited?: unknown;
}

function measuredRuns(): Array<{ label: string; question: string; ok: boolean; ms: number; cited: boolean }> {
  const file = runsFile as unknown as { runs?: unknown };
  if (!Array.isArray(file.runs)) return [];
  return file.runs.flatMap((r) => {
    if (typeof r !== "object" || r === null) return [];
    const row = r as Record<string, unknown>;
    if (typeof row.label !== "string" || typeof row.question !== "string" || typeof row.ms !== "number") return [];
    return [{ label: row.label, question: row.question, ok: row.ok === true, ms: row.ms, cited: row.cited === true }];
  });
}

const fmtSecs = (ms: number): string => `${(Math.round((ms / 1000) * 10) / 10).toFixed(1)}s`;

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

  const tool = `ask_${host.split(".")[0]}`;
  const askSnippet = ASK_SNIPPET(tool);
  const copyPrompt = COPY_PROMPT(host, record.description);
  const tryHref = `https://www.${host}/docs/quickstart?assistant=${encodeURIComponent("How do I get started?")}`;
  const isMintlify = host === "mintlify.com";
  const isMintlifyPlatform = record.platform === "mintlify" && !isMintlify;
  const mcpTool = (record.invoke?.example.match(/tools\/call (\S+)/) ?? [])[1] ?? "search";
  const mcpSnippet = `const res = await fetch("${record.invoke?.endpoint ?? ""}", {\n  method: "POST",\n  headers: { "Content-Type": "application/json", Accept: "application/json, text/event-stream" },\n  body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call",\n    params: { name: "${mcpTool}", arguments: { query: "How do I get started?" } } }),\n});\n// -> cited passages with source links (no key, no browser)`;
// Measured runs are mintlify.com's — never show another host's numbers.
  const runs = isMintlify ? measuredRuns() : [];
  const okRuns = runs.filter((r) => r.ok);
  const sortedMs = [...runs.map((r) => r.ms)].sort((a, b) => a - b);
  const medianMs = sortedMs.length > 0 ? sortedMs[Math.floor(sortedMs.length / 2)] : 0;
  const slowestMs = sortedMs.length > 0 ? sortedMs[sortedMs.length - 1] : 0;
  const citedCount = runs.filter((r) => r.ok && r.cited).length;
  const stats: Array<{ label: string; value: string }> =
    runs.length > 0
      ? [
          { label: "Success rate", value: `${okRuns.length}/${runs.length}` },
          { label: "Median response", value: fmtSecs(medianMs) },
          { label: "Slowest verified", value: fmtSecs(slowestMs) },
          { label: "Cited answers", value: `${citedCount}/${okRuns.length}` },
        ]
      : [];

  return (
    <>
      <SiteNav />
      <div className="mx-auto w-full max-w-[1120px] px-6 py-10">
        <p className="text-sm leading-5 text-ink-2">
          <Link href="/agents" className="no-underline hover:text-ink">
            ← All agents
          </Link>
        </p>
        <div className="mt-8 flex min-w-0 items-start gap-5">
          <div className="mt-1.5 shrink-0">
            <OriginFavicon host={host} size={64} />
          </div>
          <div className="w-max min-w-0 max-w-full">
            <h1 className="break-all text-[clamp(1.75rem,1.25rem+3.5vw,3rem)] font-medium leading-[1.05] tracking-[-0.04em] text-ink">
              {host}{" "}
              <span className="inline-flex items-center gap-1.5 align-middle font-sans text-sm font-normal leading-5 text-ink-2">
                <span aria-hidden="true" className="inline-block h-2 w-2 rounded-full bg-emerald-500" />
                Verified
              </span>{" "}
              {record.platform !== "" ? (
                <span className="inline-flex items-center rounded-full border border-line px-2.5 py-1 align-middle font-mono text-xs font-normal leading-4 text-ink-2">
                  {record.platform}
                </span>
              ) : null}
            </h1>
            {record.description !== "" ? (
              <p className="mt-3 max-w-[60ch] text-[15px] leading-6 text-ink-2">{record.description}</p>
            ) : null}
            <div className="mt-4 flex flex-wrap gap-3">
              <CopyCodeButton code={copyPrompt} label="Copy prompt" primary />
              <a
                href={tryHref}
                target="_blank"
                rel="noreferrer"
                className="inline-flex h-9 cursor-pointer items-center rounded-full border border-line px-4 text-sm font-medium leading-5 text-ink no-underline transition hover:bg-hover"
              >
                Try it ↗
              </a>
            </div>
          </div>
        </div>

        {isMintlify ? (
        <div className="mt-10 rounded-lg border border-line bg-raised p-5">
          <p className="font-mono text-xs uppercase tracking-wide text-ink-2">Tried live — one real exchange</p>
          <p className="mt-3 text-[15px] leading-6 text-ink">
            <span className="text-ink-2">Q: </span>
            {TRIED_Q}
          </p>
          <p className="mt-1 text-[15px] leading-6 text-ink">
            <span className="text-ink-2">A: </span>
            {TRIED_A}
          </p>
        </div>
        ) : null}
        {host === "claude.com" ? (
        <div className="mt-10 rounded-lg border border-line bg-raised p-5">
          <p className="font-mono text-xs uppercase tracking-wide text-ink-2">Tried live — multi-turn, headed browser</p>
          <p className="mt-3 text-[15px] leading-6 text-ink">
            <span className="text-ink-2">Q: </span>
            What is a connector?
          </p>
          <p className="mt-1 text-[15px] leading-6 text-ink">
            <span className="text-ink-2">A: </span>
            Read 1 file. A connector links Claude to an outside app or service… with links to Get started with connectors and the connectors directory.
          </p>
          <p className="mt-3 text-[15px] leading-6 text-ink">
            <span className="text-ink-2">Q: </span>
            How is that different from an MCP server?
          </p>
          <p className="mt-1 text-[15px] leading-6 text-ink">
            <span className="text-ink-2">A: </span>
            Resolved “that” in context: a connector is the user-facing presentation, the MCP server is what it connects to — directory vs custom, same infrastructure.
          </p>
        </div>
        ) : null}

        {runs.length > 0 ? (
          <div className="mt-10">
            <h2 className="text-xl font-medium leading-7 text-ink">Measured, not modeled</h2>
            <p className="mt-1 max-w-[70ch] text-sm leading-6 text-ink-2">
              Every run below is a real ask through the engine — open tab, deep link, wait for the settled answer,
              read it. No sampling, no estimates.
            </p>
            <div className="mt-4 grid grid-cols-2 gap-4 lg:grid-cols-4">
              {stats.map((s) => (
                <div key={s.label} className="rounded-lg border border-line bg-raised p-5">
                  <p className="text-sm leading-5 text-ink-2">{s.label}</p>
                  <p className="mt-1 font-mono text-2xl leading-8 text-ink">{s.value}</p>
                </div>
              ))}
            </div>
            <div className="mt-4 rounded-lg border border-line bg-raised p-5">
              <p className="font-mono text-xs uppercase tracking-wide text-ink-2">Response time per verified run</p>
              <div className="mt-2 h-56">
                <LatencyChart runs={runs.map((r) => ({ label: r.label, ms: r.ms }))} />
              </div>
            </div>
          </div>
        ) : null}

        <div className="mt-10">
          <h2 className="text-xl font-medium leading-7 text-ink">
            <span className="mr-3 font-mono text-ink-2">01</span>Ask it — one call
          </h2>
          {isMintlifyPlatform ? (
            <>
              <p className="mt-1 max-w-[70ch] text-sm leading-6 text-ink-2">
                Headless-safe: this docs MCP answers without a key or a browser.
              </p>
              <div className="mt-3 rounded-lg border border-line bg-raised p-5">
                <div className="flex items-center justify-end">
                  <CopyCodeButton code={mcpSnippet} />
                </div>
                <pre className="mt-1 overflow-x-auto font-mono text-[13px] leading-6 text-ink">{mcpSnippet}</pre>
              </div>
            </>
          ) : (
            <>
              <p className="mt-1 max-w-[70ch] text-sm leading-6 text-ink-2">
                No install — built into agent-webmcp. Ask from any {host} session:
              </p>
              <div className="mt-3 rounded-lg border border-line bg-raised p-5">
                <div className="flex items-center justify-end">
                  <CopyCodeButton code={askSnippet} />
                </div>
                <pre className="mt-1 overflow-x-auto font-mono text-[13px] leading-6 text-ink">{askSnippet}</pre>
              </div>
            </>
          )}
          <p className="mt-3 max-w-[70ch] text-sm leading-6 text-ink-2">
            Find it programmatically:{" "}
            <code className="font-mono text-[13px] text-ink">await tools.search_agents({"{"} query: "…"
            {"}"})</code>{" "}
            on any /agents page.
          </p>
        </div>

        {record.skill !== "" ? (
          <div className="mt-10">
            <h2 className="text-xl font-medium leading-7 text-ink">
              <span className="mr-3 font-mono text-ink-2">02</span>Use it programmatically — get_agent
            </h2>
            <p className="mt-1 max-w-[70ch] text-sm leading-6 text-ink-2">
              One call returns the full skill: the registerTool def plus the headless and headed invoke
              recipes. Register it in your own page, drive it through agent-webmcp.
            </p>
            <div className="mt-3 rounded-lg border border-line bg-raised p-5">
              <div className="flex items-center justify-end">
                <CopyCodeButton code={`await tools.get_agent({ domain: "${host}" })`} />
              </div>
              <pre className="mt-1 overflow-x-auto whitespace-pre-wrap font-mono text-[13px] leading-6 text-ink">{record.skill}</pre>
            </div>
          </div>
        ) : null}

        <div className="mt-10 max-w-[70ch]">
          <h2 className="text-xl font-medium leading-7 text-ink">Why ask instead of crawling</h2>
          <ul className="mt-3 list-none space-y-3 p-0 text-[15px] leading-6 text-ink-2">
            <li>
              <span className="text-ink">Cited answers.</span> The agent searches its own docs and links the pages it
              used — verify instead of reading everything.
            </li>
            <li>
              <span className="text-ink">It keeps the thread.</span> Follow-ups resolve in context: no re-asking, no
              re-feeding context.
            </li>
            <li>
              <span className="text-ink">Honest no.</span> Out of scope comes back as a refusal with a redirect, not
              junk snippets.
            </li>
          </ul>
        </div>

        <div className="mt-10">
          <h2 className="text-xl font-medium leading-7 text-ink">More agents</h2>
          <p className="mt-3 text-[15px] leading-6 text-ink-2">
            <Link href="/agents" className="text-link no-underline hover:opacity-80">
              Browse all agents
            </Link>
          </p>
        </div>

        {isMintlify ? (
        <>
                <div className="mt-10">
                  <h2 className="text-xl font-medium leading-7 text-ink">Links</h2>
                  <ul className="mt-3 list-none space-y-2 p-0 text-[15px] leading-6">
                    <li>
                      <a
                        href="https://www.mintlify.com/docs"
                        target="_blank"
                        rel="noreferrer"
                        className="text-link no-underline hover:opacity-80"
                      >
                        Official documentation ↗
                      </a>
                    </li>
                    <li>
                      <a
                        href="https://www.mintlify.com/docs/assistant"
                        target="_blank"
                        rel="noreferrer"
                        className="text-link no-underline hover:opacity-80"
                      >
                        Assistant docs ↗
                      </a>
                    </li>
                  </ul>
                </div>
        </>
) : null}

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
