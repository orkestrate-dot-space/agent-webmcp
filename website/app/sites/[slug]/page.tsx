import type { Metadata } from "next";
import Link from "next/link";
import { notFound, redirect } from "next/navigation";

import sitesFile from "../../../sites.json";
import SiteNav from "../../site-nav";
import { DomainScan } from "./domain-scan";
import { hostOf, looksLikeDomain, SITES_API_VERSION, tagsOf, type SignalItem } from "../scan";
import { Pills, type SiteEntry } from "../sites-catalog";
import { OriginFavicon } from "../site-art";
import { RescanButton } from "./rescan-button";
import { CopyPromptButton } from "./copy-prompt-button";
import { SignalsMatrix } from "./signals-matrix";
import { ExternalLink, FileText } from "lucide-react";

export const dynamicParams = true;

// Detail pages render on demand, never from ISR: a loading shell
// must never be baked into the cache, or refresh() keeps serving
// it after the scan lands. At this traffic the per-view render
// costs nothing; the data still comes from the tagged worker
// fetch (300s), and rescans invalidate on demand.
export const dynamic = "force-dynamic";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

interface RecordShape extends SiteEntry {
  signals: Array<SignalItem>;
}

function allSites(): Array<SiteEntry> {
  const raw = sitesFile as unknown as { sites?: unknown };
  if (!Array.isArray(raw.sites)) return [];
  return raw.sites.filter(
    (o): o is SiteEntry => typeof o === "object" && o !== null && typeof (o as Record<string, unknown>).slug === "string"
  );
}

function toRecord(j: unknown, fallback: string): RecordShape | null {
  if (typeof j !== "object" || j === null) return null;
  const s = (j as { site?: unknown }).site;
  if (typeof s !== "object" || s === null) return null;
  const r = s as Record<string, unknown>;
  if (typeof r["domain"] !== "string" || r["domain"] === "") return null;
  const has = (r["has"] ?? {}) as Record<string, unknown>;
  const bool = (k: string): boolean => has[k] === true;
  const signals = Array.isArray(r["signals"])
    ? (r["signals"] as Array<unknown>).filter(
        (x): x is SignalItem =>
          typeof x === "object" &&
          x !== null &&
          typeof (x as SignalItem).key === "string" &&
          typeof (x as SignalItem).label === "string"
      )
    : [];
  const proof = (r["proof"] ?? {}) as Record<string, unknown>;
  return {
    origin: `https://${r["domain"]}`,
    slug: r["domain"] as string,
    description: typeof r["description"] === "string" ? r["description"] : fallback,
    state: r["state"] === "quarantined" ? "quarantined" : "listed",
    reason: typeof r["reason"] === "string" ? r["reason"] : "",
    proof: {
      verified: typeof proof["verified"] === "string" ? (proof["verified"] as string) : "",
      ref: typeof proof["ref"] === "string" ? (proof["ref"] as string) : "",
    },
    repo: "",
    path: "",
    has: {
      webmcp: bool("webmcp"),
      skills: bool("skills"),
      api: bool("api"),
      mcp: bool("mcp"),
      auth: bool("auth"),
      ask: bool("ask"),
    },
    signals,
  };
}

async function readRecord(host: string): Promise<RecordShape | null> {
  try {
    const r = await fetch(`${WORKER}/sites/${encodeURIComponent(host)}?v=${SITES_API_VERSION}`, {
      next: { revalidate: 3600, tags: ["site", host] },
    });
    if (!r.ok) return null;
    return toRecord(await r.json(), "");
  } catch {
    return null;
  }
}

const STALE_MS = 7 * 24 * 60 * 60 * 1000;

const isStale = (verified: string): boolean => {
  const t = Date.parse(verified);
  if (Number.isNaN(t)) return true;
  return Date.now() - t > STALE_MS;
};

export function generateStaticParams(): Array<{ slug: string }> {
  // Top 50 prerender at build; the rest generate on demand.
  return allSites()
    .slice(0, 50)
    .map((o) => ({ slug: hostOf(o.origin) }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  const host = slug.includes(".") ? slug.toLowerCase() : null;
  const o = allSites().find((x) => x.slug === slug || (host !== null && hostOf(x.origin) === host));
  if (host !== null) {
    const title = `${host} · agent-webmcp sites`;
    return { title, description: o?.description ?? `Agent-ready verdict for ${host}.` };
  }
  if (!o) return { title: "Not found · agent-webmcp" };
  const title = `${o.origin} · agent-webmcp sites`;
  return { title, description: o.description };
}

function Shell({ host, children }: { host: string; children: React.ReactNode }) {
  return (
    <>
      <SiteNav />
      <div className="mx-auto w-full max-w-[1120px] px-6 py-10">
        <p className="text-sm leading-5 text-ink-2">
          <Link href="/sites" className="no-underline hover:text-ink">
            ← All sites
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
  if (!slug.includes(".")) {
    // Legacy dash slug — the canonical URL is the raw domain.
    const staticEntry = allSites().find((x) => x.slug === slug);
    if (staticEntry) redirect(`/sites/${hostOf(staticEntry.origin)}`);
    notFound();
  }
  const host = slug.toLowerCase();
  if (!looksLikeDomain(host)) notFound();

  const record = await readRecord(host);
  // Empty signals means a seed without probes — scan, don't render hollow.
  if (!record || isStale(record.proof.verified) || record.signals.length === 0) {
    return (
      <Shell host={host}>
        <DomainScan host={host} />
      </Shell>
    );
  }

  const tags = tagsOf(record);
  const usable = record.signals.some((s) => s.ok);
  const fileServed = record.signals.some((s) => s.key === "file" && s.ok);
  const llmsServed = record.signals.some((s) => s.key === "llms" && s.ok);
  const sourceHref = fileServed
    ? `https://${host}/.well-known/agent-webmcp.json`
    : `https://${host}/llms.txt`;
  const verified = record.proof.verified
    ? new Date(`${record.proof.verified}T00:00:00Z`).toLocaleDateString("en-US", {
        month: "short",
        day: "numeric",
        year: "numeric",
        timeZone: "UTC",
      })
    : "";
  return (
    <>
      <SiteNav />
      <div className="mx-auto w-full max-w-[1120px] px-6 py-10">
        <p className="text-sm leading-5 text-ink-2">
          <Link href="/sites" className="no-underline hover:text-ink">
            ← All sites
          </Link>
        </p>
        <div className="mt-8 flex flex-col gap-6 lg:flex-row lg:items-start lg:justify-between">
          <div className="flex min-w-0 items-start gap-5">
            <div className="mt-1.5 shrink-0">
              <OriginFavicon host={host} size={64} />
            </div>
            <div className="w-max min-w-0 max-w-full">
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
              {record.description !== "" ? (
                <p className="mt-3 max-w-[60ch] text-[15px] leading-6 text-ink-2">{record.description}</p>
              ) : usable ? null : (
                <p className="mt-3 max-w-[60ch] text-[15px] leading-6 text-ink-2">
                  Nothing here agents can use yet.
                </p>
              )}
              {tags.length > 0 ? (
                <p className="mt-3">
                  <Pills tags={tags} />
                </p>
              ) : null}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-4 text-sm leading-5">
            <p className="text-ink-2">
              {record.state === "listed"
                ? `Indexed${verified ? ` · ${verified}` : ""}`
                : `Quarantined · ${record.reason}`}
            </p>
            <span aria-hidden="true" className="h-4 w-px bg-line" />
            <RescanButton host={host} />
          </div>
        </div>

        <div className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-2 border-b border-line pb-5 text-[15px] leading-6">
          <a
            href={`https://${host}`}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-2 text-ink no-underline transition hover:opacity-80"
          >
            <ExternalLink size={15} aria-hidden="true" />
            Open site <span aria-hidden="true">↗</span>
          </a>
          <span aria-hidden="true" className="hidden h-4 w-px bg-line sm:block" />
          {fileServed || llmsServed ? (
            <>
              <a
                href={sourceHref}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-2 text-ink-2 no-underline transition hover:text-ink"
              >
                <FileText size={15} aria-hidden="true" />
                View source <span aria-hidden="true">→</span>
              </a>
              <span aria-hidden="true" className="hidden h-4 w-px bg-line sm:block" />
            </>
          ) : null}
          <CopyPromptButton host={host} signals={record.signals} />
          <span aria-hidden="true" className="hidden h-4 w-px bg-line sm:block" />
          <a
            href="https://github.com/orkestrate-dot-space/agent-webmcp/issues"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-2 text-ink-2 no-underline transition hover:text-ink"
          >
            Report an issue <span aria-hidden="true">→</span>
          </a>
        </div>

        <SignalsMatrix host={host} signals={record.signals} verified={verified} />

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
