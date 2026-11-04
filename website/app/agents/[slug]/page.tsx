import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { ExternalLink } from "lucide-react";

import SiteNav from "../../site-nav";
import { DomainScan } from "./domain-scan";
import { contractOf, contractSlugs } from "../list-data";
import type { InvokeContract, SiteTool } from "../scan";
import { looksLikeDomain, SITES_API_VERSION, WORKER } from "../scan";
import { OriginFavicon } from "../site-art";
import { CopyCodeButton } from "./copy-code-button";
import { UsageChart } from "./usage-chart";
import { askExamplesFor } from "./ask-examples";
import {
  mintlifyRegisterDef,
  type MintlifyDeltas,
} from "../mintlify-template";

export const dynamicParams = true;

// Detail pages render on demand, never from ISR: a loading shell
// must never be baked into the cache, or refresh() keeps serving
// it after the scan lands.
export const dynamic = "force-dynamic";

interface RecordShape {
  origin: string;
  slug: string;
  name: string;
  description: string;
  platform: string;
  skill: string;
  proof: { verified: string; ref: string } | null;
  page: string;
  searchTool: string;
  fsTool: string;
  autoSubmit: boolean;
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
    proof: contract?.proof ?? null,
    page: contract?.page || contract?.docs || "",
    searchTool: contract?.searchTool || "search",
    fsTool: contract?.fsTool || "",
    autoSubmit: contract?.autoSubmit ?? true,
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
  const deltas: MintlifyDeltas = {
    host,
    docs: record.invoke.endpoint,
    page: record.page,
    searchTool: record.searchTool,
    fsTool: record.fsTool,
    autoSubmit: record.autoSubmit,
    description: record.description,
    verified: record.proof?.verified || "unverified",
  };
  const register = mintlifyRegisterDef(deltas);
  const siblings = contractSlugs().filter(
    (s) => s !== host && (contractOf(s)?.platform || "") === record.platform && record.platform !== "",
  );
  const invoke: Array<{ method: string; path: string; desc: string }> = [
    { method: "webmcp", path: `get_agent({ domain: "${host}" })`, desc: `Full skill for this agent (${register.name}): observer + send cores, all drivers.` },
  ];
  const siteEndpoints: Array<{ method: string; path: string; desc: string }> = [
    { method: "mcp", path: `${deltas.docs.replace(/\/$/, "")}/mcp`, desc: "Site retrieval MCP (search + filesystem read, no key, no reasoning)." },
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

        <div className="mt-12 min-w-0">
          <h2 className="mt-1 text-[28px] font-medium leading-[1.15] tracking-[-0.02em] text-ink">
            Usage
          </h2>
          <div className="mt-4">
            <UsageChart domain={host} />
          </div>
          <h2 className="mt-12 text-[28px] font-medium leading-[1.15] tracking-[-0.02em] text-ink">
            Invoke this agent
          </h2>
          <div className="mt-4 overflow-hidden rounded-lg border border-line">
            {invoke.map((e) => (
              <div key={e.path} className="flex flex-col gap-1 border-b border-line px-4 py-3 last:border-b-0 sm:flex-row sm:items-baseline sm:gap-4">
                <span className="w-20 shrink-0 font-mono text-[11px] uppercase leading-5 tracking-wide text-ink-2">
                  {e.method}
                </span>
                <code className="shrink-0 font-mono text-[13px] leading-5 text-ink">{e.path}</code>
                <span className="text-sm leading-5 text-ink-2">{e.desc}</span>
              </div>
            ))}
          </div>
          <h2 className="mt-12 text-[28px] font-medium leading-[1.15] tracking-[-0.02em] text-ink">
            Site endpoints
          </h2>
          <div className="mt-4 overflow-hidden rounded-lg border border-line">
            {siteEndpoints.map((e) => (
              <div key={e.path} className="flex flex-col gap-1 border-b border-line px-4 py-3 last:border-b-0 sm:flex-row sm:items-baseline sm:gap-4">
                <span className="w-20 shrink-0 font-mono text-[11px] uppercase leading-5 tracking-wide text-ink-2">
                  {e.method}
                </span>
                <code className="shrink-0 font-mono text-[13px] leading-5 text-ink">{e.path}</code>
                <span className="text-sm leading-5 text-ink-2">{e.desc}</span>
              </div>
            ))}
          </div>

          {/* Chat panel parked: <AgentShowcase> + ./prompt-bar + ./showcase-data
              stay in the tree, unrendered, until the chat returns. */}
          {askExamplesFor(host).length > 0 ? (
            <div className="mt-12">
              <h2 className="text-[28px] font-medium leading-[1.15] tracking-[-0.02em] text-ink">
                Try it out
              </h2>
              <p className="mt-2 max-w-[70ch] text-sm leading-6 text-ink-2">
                Opens this site&apos;s assistant with the question pre-filled.
              </p>
              <div className="mt-4 border-t border-line">
                {askExamplesFor(host).map((q) => (
                  <a
                    key={q}
                    href={`${(deltas.page || deltas.docs).replace(/\/$/, "")}?assistant=${encodeURIComponent(q)}`}
                    target="_blank"
                    rel="noreferrer"
                    className="flex items-center justify-between gap-4 border-b border-line py-3 text-[15px] leading-6 text-ink no-underline transition hover:text-ink"
                  >
                    <span>{q}</span>
                    <ExternalLink
                      className="h-4 w-4 shrink-0 text-ink-2"
                      aria-label="Opens the site assistant with this question"
                    />
                  </a>
                ))}
              </div>
            </div>
          ) : null}
        </div>

        {siblings.length > 0 && (
          <div className="mt-12">
            <h2 className="text-xl font-medium leading-7 text-ink">More {platformLabel} sites</h2>
            <div className="mt-4 flex flex-wrap items-center gap-x-7 gap-y-3">
              {siblings.map((s) => (
                <Link
                  key={s}
                  href={`/agents/${s}`}
                  prefetch={false}
                  className="inline-flex items-center gap-2 text-[15px] text-ink-2 no-underline transition hover:text-ink"
                >
                  <OriginFavicon host={s} size={18} />
                  {s}
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


