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
  return { title: `${host} · agent-webmcp`, description: `Invoke ${host} as a subagent through agent-webmcp.` };
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

function Step({ n, title, code }: { n: string; title: string; code: string }) {
  return (
    <div className="mt-8">
      <p className="text-sm font-medium leading-6 text-ink">
        <span className="mr-2 font-mono text-ink-2">{n}</span>
        {title}
      </p>
      <div className="mt-3 rounded-lg border border-line bg-raised p-5">
        <div className="flex items-center justify-end">
          <CopyCodeButton code={code} />
        </div>
        <pre className="mt-1 overflow-x-auto font-mono text-[13px] leading-6 text-ink">{code}</pre>
      </div>
    </div>
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

  // Tool name matches the engine convention: ask_<first host label>.
  const tool = `ask_${host.split(".")[0]}`;
  const askSnippet = `// via agent-webmcp execute, on a ${host} session\nconst answer = await tools.${tool}({ question: "<question>" });`;
  const findSnippet = `// via agent-webmcp execute, on any /agents session\nconst found = await tools.search_agents({ query: "<what you need>" });\n// → { domain: "${host}", invoke: { transport, endpoint, example }, verified }`;

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
            <p className="font-mono text-xs uppercase tracking-wide text-ink-2">{record.invoke.transport}</p>
            <h1 className="mt-1 break-all text-[clamp(1.75rem,1.25rem+3.5vw,3rem)] font-medium leading-[1.05] tracking-[-0.04em] text-ink">
              {host}
            </h1>
            {record.description !== "" ? (
              <p className="mt-3 max-w-[60ch] text-[15px] leading-6 text-ink-2">{record.description}</p>
            ) : null}
            <p className="mt-4 flex flex-wrap gap-3">
              <CopyCodeButton code={`await tools.${tool}({ question: "<question>" })`} />
              <a
                href={`https://${host}`}
                target="_blank"
                rel="noreferrer"
                className="inline-flex h-9 cursor-pointer items-center rounded-full border border-line px-4 text-sm font-medium leading-5 text-ink no-underline transition hover:bg-hover"
              >
                Open site ↗
              </a>
            </p>
          </div>
        </div>

        <Step n="01" title={`Ask it — worked answer, with citations`} code={askSnippet} />
        <Step n="02" title="Find it programmatically — from any agent" code={findSnippet} />

        <div className="mt-10">
          <h2 className="text-xl font-medium leading-7 text-ink">Why ask instead of crawling</h2>
          <ul className="mt-3 max-w-[70ch] list-none space-y-3 p-0 text-[15px] leading-6 text-ink-2">
            <li>
              <span className="text-ink">Cited answers.</span> The agent searches its own docs and links the pages it used — verify instead of reading everything.
            </li>
            <li>
              <span className="text-ink">It keeps the thread.</span> Follow-ups resolve in context; no re-asking, no re-feeding context.
            </li>
            <li>
              <span className="text-ink">Honest no.</span> Out of scope comes back as a refusal with a redirect, not junk snippets.
            </li>
          </ul>
        </div>

        <div className="mt-10">
          <h2 className="text-xl font-medium leading-7 text-ink">More agents</h2>
          <p className="mt-3 text-[15px] leading-6 text-ink-2">
            <Link href="/agents" className="no-underline hover:text-ink">
              Browse all agents
            </Link>
          </p>
        </div>

        <p className="mt-8 border-t border-line pt-4 text-sm leading-5 text-ink-2">
          Something wrong?{" "}
          <a href="https://github.com/orkestrate-dot-space/agent-webmcp/issues" target="_blank" rel="noreferrer">
            Open an issue
          </a>{" "}
          to delist it here.
        </p>
      </div>
    </>
  );
}
