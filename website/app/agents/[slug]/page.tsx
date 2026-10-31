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

// One runnable recipe per transport. v1 only serves mcp rows; a row
// whose transport has no recipe renders its endpoint + example raw.
function recipeFor(invoke: InvokeContract): string | null {
  if (invoke.transport !== "deep-link") return null;
  return `// Ask the site's agent. Needs tabs + JS (any DOM-capable harness).
const QUESTION = "<question>";
const tab = await tools.browser.tabs.open({url: "${invoke.endpoint}".replace("<page>", "quickstart").replace("<question>", encodeURIComponent(QUESTION))});
const wait = "(async () => { const t0 = Date.now(); let prev = ''; while (Date.now() - t0 < 30000) { let best = ''; document.querySelectorAll('[role=dialog],[role=complementary],aside').forEach(function (el) { const tx = (el.innerText || '').trim(); if (tx.indexOf(QUESTION.slice(0, 20)) !== -1 && tx.length > best.length) best = tx; }); if (best.length > 200 && best === prev) return best.slice(0, 2500); if (best) prev = best; await new Promise(function (r) { setTimeout(r, 1000); }); } return 'TIMEOUT >>> ' + prev.slice(0, 800); })()";
const answer = await tools.browser.evaluate({tabID: tab.id, script: wait});
await tools.browser.tabs.close({tabID: tab.id});
return answer; // follow-ups: repeat in the same tab — the thread persists`;
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

export default async function SitePage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  if (!slug.includes(".")) notFound();
  const host = slug.toLowerCase();
  if (!looksLikeDomain(host)) notFound();

  const record = await readRecord(host);
  if (!record) {
    return (
      <Shell host={host}>
        <DomainScan host={host} />
      </Shell>
    );
  }

  const recipe = record.invoke ? recipeFor(record.invoke) : null;
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
          <h1 className="break-all text-[clamp(1.75rem,1.25rem+3.5vw,3rem)] font-medium leading-[1.05] tracking-[-0.04em] text-ink">
            {host}{" "}
            <a
              href={`https://${host}`}
              target="_blank"
              rel="noreferrer"
              aria-label={`Open ${host} in a new tab`}
              className="text-ink-2 no-underline transition hover:text-ink"
            >
              <ExternalLink size={26} aria-hidden="true" className="inline-block" />
            </a>
          </h1>
        </div>

        {record.invoke && recipe ? (
          <div className="mt-8 rounded-lg border border-line bg-raised p-5">
            <div className="flex items-center justify-between gap-4">
              <p className="font-mono text-xs uppercase tracking-wide text-ink-2">Run in a DOM-capable harness</p>
              <CopyCodeButton code={recipe} />
            </div>
            <pre className="mt-3 overflow-x-auto font-mono text-[13px] leading-6 text-ink">{recipe}</pre>
          </div>
        ) : record.invoke ? (
          <div className="mt-8 rounded-lg border border-line bg-raised p-5">
            <p className="font-mono text-xs uppercase tracking-wide text-ink-2">Invoke · {record.invoke.transport}</p>
            <p className="mt-2 break-all font-mono text-sm leading-6 text-ink">{record.invoke.endpoint}</p>
            <p className="mt-3 max-w-[70ch] text-sm leading-6 text-ink-2">{record.invoke.example}</p>
          </div>
        ) : (
          <p className="mt-8 max-w-[60ch] text-[15px] leading-6 text-ink-2">
            No invoke contract yet — the row exists but nobody has verified a calling path.
          </p>
        )}

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
