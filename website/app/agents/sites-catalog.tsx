"use client";

import Link from "next/link";
import { useRef, useState } from "react";
import { useRouter } from "next/navigation";

import SiteNav from "../site-nav";
import { hostOf, looksLikeDomain } from "./scan";
import type { SiteEntry } from "./list-data";
import { OriginFavicon } from "./site-art";

function Row({ site }: { site: SiteEntry }) {
  const host = hostOf(site.origin);
  return (
    <Link
      href={`/agents/${host}`}
      prefetch={false}
      className="flex items-center gap-4 border-b border-line py-4 no-underline transition-colors hover:bg-hover/40"
    >
      <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[10px] border border-line bg-raised">
        <OriginFavicon host={host} size={22} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-baseline gap-x-3">
          <span className="text-[15px] font-medium leading-5 text-ink">{host}</span>
          <span className="font-mono text-[13px] leading-5 text-ink-2">{site.origin}</span>
        </span>
        <span className="mt-0.5 block truncate text-sm leading-5 text-ink-2">{site.description}</span>
      </span>
      {site.platform !== "" ? (
        <span className="hidden shrink-0 rounded-full border border-line px-2.5 py-1 font-mono text-xs leading-4 text-ink-2 sm:inline-block">
          {site.platform}
        </span>
      ) : null}
    </Link>
  );
}

export function SitesCatalog({ initialSites }: { initialSites: Array<SiteEntry> }) {
  const router = useRouter();
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");

  const onEnter = () => {
    const v = query.trim();
    if (v === "" || !looksLikeDomain(v)) return;
    router.push(`/agents/${hostOf(v).toLowerCase()}`);
  };

  return (
    <>
      <SiteNav />
      <div className="mx-auto flex w-full flex-col px-6 pt-2 md:px-10 lg:px-16">
        <main id="main" suppressHydrationWarning className="flex min-h-0 flex-1 flex-col">
          <div className="flex w-full flex-col gap-6 pb-8 pt-10 lg:flex-row lg:items-end lg:justify-between">
            <h1 className="text-[36px] font-medium leading-[1.05] tracking-[-0.04em] text-ink sm:text-[56px]">Browse Agents</h1>
            <div className="max-w-[48ch]">
              <p className="text-[15px] leading-6 text-ink-2">
                Subagents for the web — ask a question, get back a worked answer.
              </p>
              <p className="mt-4 flex flex-wrap gap-3">
                <a
                  href="/docs"
                  className="inline-flex h-10 cursor-pointer items-center justify-center rounded-full border border-line px-5 text-sm font-medium leading-5 text-ink no-underline transition hover:bg-hover active:scale-[0.97]"
                >
                  Read the docs
                </a>
              </p>
            </div>
          </div>

          <div className="flex w-full">
            <span className="relative min-w-0 flex-1">
              <svg
                aria-hidden="true"
                width={18}
                height={18}
                viewBox="0 0 16 16"
                fill="none"
                className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-ink-2"
              >
                <circle cx="7" cy="7" r="4.5" stroke="currentColor" strokeWidth="1.5" />
                <path d="m10.5 10.5 3 3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              </svg>
              <input
                ref={inputRef}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") onEnter();
                }}
                placeholder="Paste a domain to inspect it…"
                autoComplete="off"
                spellCheck={false}
                aria-label="Open a domain"
                className="h-12 w-full rounded-lg border border-line bg-canvas pl-11 pr-4 text-[15px] text-ink transition-colors focus:border-line-strong focus:outline-none lg:h-14"
              />
            </span>
          </div>

          <div className="mt-4 border-t border-line">
            {initialSites.length === 0 ? (
              <p className="py-16 text-center text-[15px] leading-6 text-ink-2">
                No agents listed yet. Paste a domain above to inspect the first one.
              </p>
            ) : (
              initialSites.map((s) => <Row key={s.slug} site={s} />)
            )}
          </div>
          <div className="pb-10" />
        </main>
      </div>
    </>
  );
}
