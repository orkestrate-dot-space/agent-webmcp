import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";

import SiteNav from "../../site-nav";
import { DomainScan } from "./domain-scan";
import { contractOf, contractSlugs } from "../list-data";
import type { InvokeContract, SiteTool } from "../scan";
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

const isReadOnly = (name: string): boolean => !/feedback|submit|navigate|open|create|update/i.test(name);

function Pill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center rounded-full border border-line px-2 py-0.5 font-mono text-[11px] leading-4 text-ink-2">
      {children}
    </span>
  );
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

  const short = host.split(".")[0];
  const platformLabel = record.platform === "" ? "site" : record.platform;
  const prompts = examplePrompts(host, short);
  const siblings = contractSlugs()
    .filter((s) => s !== host && (contractOf(s)?.platform || "") === record.platform && record.platform !== "")
    .map((s) => ({ slug: s, tools: contractOf(s)?.tools.length ?? 0 }));
  // Measured runs are mintlify.com's — never show another host's numbers.
  const runs = host === "mintlify.com" ? measuredRuns() : [];
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
  const facts: Array<{ label: string; value: string }> = [
    { label: "Tools", value: String(record.tools.length) },
    { label: "Transport", value: record.invoke.transport },
    { label: "Platform", value: platformLabel },
    { label: "Verified", value: host === "mintlify.com" || host === "claude.com" ? "2026-11-01" : "MCP handshake" },
  ];

  return (
    <>
      <SiteNav />
      <div className="mx-auto w-full max-w-[1120px] px-6 py-10">
        <p className="font-mono text-xs uppercase tracking-wide text-ink-2">
          <Link href="/agents" className="no-underline hover:text-ink">
            Agents directory
          </Link>
          {" / "}
          {platformLabel}
          {" / "}
          {host}
        </p>

        <div className="mt-6 flex min-w-0 items-start gap-4">
          <div className="mt-1 shrink-0">
            <OriginFavicon host={host} size={56} />
          </div>
          <div className="w-max min-w-0 max-w-full">
            <p className="font-mono text-xs uppercase tracking-wide text-ink-2">
              {platformLabel} · {host}
            </p>
            <h1 className="mt-1 break-all text-[clamp(2rem,1.5rem+3.5vw,3.5rem)] font-medium leading-[1.05] tracking-[-0.04em] text-ink">
              {host}
            </h1>
          </div>
        </div>

        {record.description !== "" ? (
          <p className="mt-4 max-w-[70ch] text-[15px] leading-6 text-ink-2">{record.description}</p>
        ) : null}

        <div className="mt-5 flex flex-wrap gap-3">
          <a
            href={record.origin}
            target="_blank"
            rel="noreferrer"
            className="inline-flex h-9 cursor-pointer items-center rounded-full bg-ink px-4 text-sm font-medium leading-5 text-canvas no-underline transition hover:opacity-85"
          >
            Open {host} <ExternalLink className="ml-1.5 h-3.5 w-3.5" aria-hidden="true" />
          </a>
          <CopyCodeButton code={COPY_PROMPT(host, record.description)} label="Copy prompt" />
        </div>

        <div className="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_320px]">
          <div className="min-w-0">
            <h2 className="text-xl font-medium leading-7 text-ink">Ask an agent to</h2>
            <ul className="mt-2 list-none p-0">
              {prompts.map((p) => (
                <li key={p} className="border-b border-line py-3 text-[15px] leading-6 text-ink">
                  {p}
                </li>
              ))}
            </ul>

            <h2 className="mt-8 text-xl font-medium leading-7 text-ink">
              {record.tools.length} tool{record.tools.length === 1 ? "" : "s"}
            </h2>
            {record.tools.length === 0 ? (
              <p className="mt-2 text-[15px] leading-6 text-ink-2">
                Verbs observed live; full list pending a rescan.
              </p>
            ) : (
              <ul className="mt-2 list-none p-0">
                {record.tools.map((t) => (
                  <li key={t.name} className="border-b border-line py-3">
                    <p className="flex flex-wrap items-center gap-2 font-mono text-[13px] leading-5 text-ink">
                      {t.name}
                      {isReadOnly(t.name) ? <Pill>read-only</Pill> : null}
                      <Pill>untrusted content</Pill>
                    </p>
                    {t.params !== "" ? (
                      <p className="mt-1 font-mono text-xs leading-5 text-ink-2">{t.params}</p>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}

            {(host === "mintlify.com" || host === "claude.com") && (
              <div className="mt-8 rounded-lg border border-line bg-raised p-5">
                <p className="font-mono text-xs uppercase tracking-wide text-ink-2">
                  Tried live — {host === "claude.com" ? "multi-turn, headed browser" : "one real exchange"}
                </p>
                {host === "mintlify.com" ? (
                  <>
                    <p className="mt-3 text-[15px] leading-6 text-ink">
                      <span className="text-ink-2">Q: </span>
                      {TRIED_Q}
                    </p>
                    <p className="mt-1 text-[15px] leading-6 text-ink">
                      <span className="text-ink-2">A: </span>
                      {TRIED_A}
                    </p>
                  </>
                ) : (
                  <>
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
                  </>
                )}
              </div>
            )}

            {runs.length > 0 && (
              <div className="mt-8">
                <h2 className="text-xl font-medium leading-7 text-ink">Measured, not modeled</h2>
                <p className="mt-1 max-w-[70ch] text-sm leading-6 text-ink-2">
                  Every run below is a real ask through the engine — open tab, deep link, wait for the settled
                  answer, read it. No sampling, no estimates.
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
            )}

            {record.skill !== "" && (
              <div className="mt-8">
                <h2 className="text-xl font-medium leading-7 text-ink">Use it programmatically — get_agent</h2>
                <p className="mt-1 max-w-[70ch] text-sm leading-6 text-ink-2">
                  One call returns the full skill: the registerTool def plus the headless and headed invoke
                  recipes. Register it in your own page, drive it through agent-webmcp.
                </p>
                <div className="mt-3 rounded-lg border border-line bg-raised p-5">
                  <div className="flex items-center justify-end">
                    <CopyCodeButton code={`await tools.get_agent({ domain: "${host}" })`} />
                  </div>
                  <pre className="mt-1 overflow-x-auto whitespace-pre-wrap font-mono text-[13px] leading-6 text-ink">
                    {record.skill}
                  </pre>
                </div>
              </div>
            )}
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
              Tool names are what {host} registered via its docs MCP ({record.invoke?.endpoint}). The
              description and example prompts are suggestions — except the tried-live transcript, which is
              verbatim from a verified run.
            </p>
          </aside>
        </div>

        {siblings.length > 0 && (
          <div className="mt-12">
            <h2 className="text-xl font-medium leading-7 text-ink">More {platformLabel} sites</h2>
            <div className="mt-4 grid gap-px overflow-hidden rounded-lg border border-line bg-line sm:grid-cols-2 lg:grid-cols-3">
              {siblings.map((s) => (
                <Link
                  key={s.slug}
                  href={`/agents/${s.slug}`}
                  prefetch={false}
                  className="flex items-center gap-3 bg-canvas p-4 no-underline transition hover:bg-hover/40"
                >
                  <OriginFavicon host={s.slug} size={28} />
                  <span className="min-w-0">
                    <span className="block truncate text-[15px] font-medium leading-5 text-ink">{s.slug}</span>
                    <span className="block font-mono text-xs leading-4 text-ink-2">
                      {s.tools} tool{s.tools === 1 ? "" : "s"} · {s.slug}
                    </span>
                  </span>
                </Link>
              ))}
            </div>
            <p className="mt-3 text-[15px] leading-6 text-ink-2">
              <Link href="/agents" className="text-link no-underline hover:opacity-80">
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
