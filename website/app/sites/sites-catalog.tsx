"use client";

import Link from "next/link";
import { useCallback, useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";

import SiteNav from "../site-nav";
import { hostOf, looksLikeDomain, SITES_API_VERSION, TAGS, tagsOf, type Tag } from "./scan";
import { OriginFavicon } from "./site-art";

export interface SiteEntry {
  origin: string;
  slug: string;
  description: string;
  category?: string;
  state: string;
  reason: string;
  proof: { verified: string; ref: string };
  repo: string;
  path: string;
  has: { webmcp: boolean; skills: boolean; api: boolean; mcp: boolean; auth: boolean; ask: boolean };
}

export type { Tag };

export function Pills({ tags }: { tags: Array<Tag> }) {
  if (tags.length === 0) return <span className="shrink-0 text-[13px] leading-5 text-ink-2">—</span>;
  return (
    <span className="flex shrink-0 flex-wrap gap-1.5 max-sm:basis-full">
      {tags.map((t) => (
        <span
          key={t}
          className="rounded-md border border-line bg-raised px-2 py-0.5 text-xs leading-4 text-ink-2"
        >
          {t}
        </span>
      ))}
    </span>
  );
}

function Row({ site }: { site: SiteEntry }) {
  const host = hostOf(site.origin);
  return (
    <Link
      href={`/sites/${host}`}
      prefetch={false}
      className="flex flex-wrap items-center gap-4 border-b border-line py-4 no-underline transition-colors hover:bg-hover/40"
    >
      <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[10px] border border-line bg-raised">
        <OriginFavicon host={host} size={22} />
      </span>
      <span className="w-36 shrink-0 truncate text-[15px] font-medium leading-5 text-ink sm:w-52">{host}</span>
      <span className="min-w-0 flex-1 truncate text-sm leading-5 text-ink-2">{site.description}</span>
      <Pills tags={tagsOf(site)} />
    </Link>
  );
}

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

interface WorkerRow {
  domain?: unknown;
  description?: unknown;
  has?: unknown;
  state?: unknown;
  reason?: unknown;
  proof?: { verified?: unknown; ref?: unknown };
}

const asEntry = (r: WorkerRow): SiteEntry | null => {
  if (typeof r.domain !== "string" || r.domain === "") return null;
  const has = (r.has ?? {}) as Record<string, unknown>;
  const bool = (k: string): boolean => has[k] === true;
  return {
    origin: `https://${r.domain}`,
    slug: r.domain,
    description: typeof r.description === "string" ? r.description : "",
    state: r.state === "quarantined" ? "quarantined" : "listed",
    reason: typeof r.reason === "string" ? r.reason : "",
    proof: {
      verified: typeof r.proof?.verified === "string" ? r.proof.verified : "",
      ref: typeof r.proof?.ref === "string" ? r.proof.ref : "",
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
  };
};

const fetchPage = async (
  params: Record<string, string>
): Promise<{ sites: Array<SiteEntry>; nextCursor: string | null }> => {
  const empty = { sites: [], nextCursor: null };
  try {
    const qs = new URLSearchParams({ limit: "50", v: String(SITES_API_VERSION), ...params });
    const r = await fetch(`${WORKER}/sites?${qs.toString()}`);
    if (!r.ok) return empty;
    const j = (await r.json()) as { sites?: unknown; nextCursor?: unknown };
    if (!Array.isArray(j.sites)) return empty;
    const sites = j.sites.map(asEntry).filter((s): s is SiteEntry => s !== null);
    return {
      sites,
      nextCursor: typeof j.nextCursor === "string" && j.nextCursor !== "" ? j.nextCursor : null,
    };
  } catch {
    return empty;
  }
};

import type { Counts } from "./list-data";

export function SitesCatalog({
  initialSites,
  initialCursor,
  initialCounts,
}: {
  initialSites: Array<SiteEntry>;
  initialCursor: string | null;
  initialCounts: Counts;
}) {
  const router = useRouter();
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [tag, setTag] = useState<"All" | Tag>("All");
  const [items, setItems] = useState<Array<SiteEntry>>(initialSites);
  const [cursor, setCursor] = useState<string | null>(initialCursor);
  const [loadingMore, setLoadingMore] = useState(false);
  const [filtering, setFiltering] = useState(false);
  const [counts] = useState<Counts>(initialCounts);

  const filterActive = query.trim() !== "" || tag !== "All";

  // Server fallback: if page 1 came from sites.json, adopt the
  // worker page once when it answers.
  useEffect(() => {
    if (initialCursor !== null || initialSites.length === 0) return;
    let on = true;
    fetchPage({})
      .then((p) => {
        if (!on || p.sites.length === 0) return;
        setItems(p.sites);
        setCursor(p.nextCursor);
      })
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [initialCursor, initialSites.length]);

  // Chips come server-rendered; no client walk, no flash.

  // Filter mode: debounced worker query on q/tag. Domain-looking
  // and single-char input never queries — the scan→ row covers the
  // former, and a 1-char LIKE scans everything for nothing.
  useEffect(() => {
    if (!filterActive) return;
    const q = query.trim();
    if (tag === "All" && q !== "" && (looksLikeDomain(q) || q.length < 2)) {
      setFiltering(false);
      return;
    }
    setFiltering(true);
    const t = setTimeout(() => {
      const params: Record<string, string> = {};
      if (tag !== "All") params["tag"] = tag.toLowerCase();
      if (q !== "") params["q"] = q;
      fetchPage(params)
        .then((p) => {
          setItems(p.sites);
          setCursor(p.nextCursor);
        })
        .catch(() => {})
        .finally(() => setFiltering(false));
    }, 300);
    return () => clearTimeout(t);
  }, [query, tag, filterActive]);

  // Leaving filter mode restores the browsed pages.
  const leaveFilter = useCallback(() => {
    fetchPage({})
      .then((p) => {
        setItems(p.sites);
        setCursor(p.nextCursor);
      })
      .catch(() => {});
  }, []);

  const onTag = (t: "All" | Tag) => {
    const was = tag !== "All" || query.trim() !== "";
    setTag(t);
    if (was && t === "All" && query.trim() === "") leaveFilter();
  };

  const onQuery = (v: string) => {
    const was = filterActive;
    setQuery(v);
    if (was && v.trim() === "" && tag === "All") leaveFilter();
  };

  const onMore = () => {
    if (!cursor || loadingMore) return;
    setLoadingMore(true);
    const params: Record<string, string> = { cursor };
    if (filterActive) {
      if (tag !== "All") params["tag"] = tag.toLowerCase();
      if (query.trim() !== "") params["q"] = query.trim();
    }
    fetchPage(params)
      .then((p) => {
        if (p.sites.length > 0) setItems((prev) => [...prev, ...p.sites]);
        setCursor(p.nextCursor);
      })
      .catch(() => {})
      .finally(() => setLoadingMore(false));
  };

  const domainQuery =
    looksLikeDomain(query.trim()) &&
    !items.some((s) => hostOf(s.origin).toLowerCase() === hostOf(query.trim()).toLowerCase())
      ? hostOf(query.trim())
      : null;

  const onEnter = () => {
    const v = query.trim();
    if (v === "" || !looksLikeDomain(v)) return;
    router.push(`/sites/${hostOf(v).toLowerCase()}`);
  };

  const onAdd = () => {
    const v = query.trim();
    if (v !== "" && looksLikeDomain(v)) {
      router.push(`/sites/${hostOf(v).toLowerCase()}`);
      return;
    }
    inputRef.current?.focus();
  };

  const chips: Array<"All" | Tag> = ["All", ...TAGS.filter((t) => counts[t] > 0)];

  return (
    <>
      <SiteNav />
      <div className="mx-auto flex w-full flex-col px-6 pt-2 md:px-10 lg:px-16">
        <main id="main" suppressHydrationWarning className="flex min-h-0 flex-1 flex-col">
          <div className="flex w-full flex-col gap-6 pb-8 pt-10 lg:flex-row lg:items-end lg:justify-between">
            <h1 className="text-[36px] font-medium leading-[1.05] tracking-[-0.04em] text-ink sm:text-[56px]">Browse Sites</h1>
            <div className="max-w-[48ch]">
              <p className="text-[15px] leading-6 text-ink-2">
                Websites your agent can work with.
              </p>
              <p className="mt-4 flex flex-wrap gap-3">
                <button
                  type="button"
                  onClick={onAdd}
                  className="inline-flex h-10 cursor-pointer items-center justify-center rounded-full bg-ink px-5 text-sm font-medium leading-5 text-canvas transition hover:opacity-90 active:scale-[0.97]"
                >
                  Add website
                </button>
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
                onChange={(e) => onQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") onEnter();
                }}
                placeholder="Search sites or paste a domain…"
                autoComplete="off"
                spellCheck={false}
                aria-label="Search sites or open a domain"
                className="h-12 w-full rounded-lg border border-line bg-canvas pl-11 pr-4 text-[15px] text-ink transition-colors focus:border-line-strong focus:outline-none lg:h-14"
              />
            </span>
          </div>

          <div className="mt-6 flex flex-wrap items-baseline gap-x-5 gap-y-2" role="tablist" aria-label="Surfaces">
            {chips.map((t) => {
              const n = t === "All" ? counts.total : counts[t];
              const active = tag === t;
              return (
                <button
                  key={t}
                  role="tab"
                  aria-selected={active}
                  type="button"
                  onClick={() => onTag(t)}
                  className={
                    active
                      ? "cursor-pointer font-mono text-sm leading-5 text-ink underline underline-offset-8"
                      : "cursor-pointer font-mono text-sm leading-5 text-ink-2 transition hover:text-ink"
                  }
                >
                  {t} <span className="tabular-nums">{n}</span>
                </button>
              );
            })}
          </div>

          <div className="mt-4 border-t border-line">
            {domainQuery !== null ? (
              <Link
                href={`/sites/${domainQuery}`}
                prefetch={false}
                className="flex flex-wrap items-center gap-4 border-b border-line py-4 no-underline transition-colors hover:bg-hover/40"
              >
                <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[10px] border border-line bg-raised">
                  <OriginFavicon host={domainQuery} size={22} />
                </span>
                <span className="w-36 shrink-0 truncate text-[15px] font-medium leading-5 text-ink sm:w-52">{domainQuery}</span>
                <span className="min-w-0 flex-1 truncate text-sm leading-5 text-ink-2" />
                <span className="shrink-0 text-sm leading-5 text-ink-2">scan →</span>
              </Link>
            ) : null}
            {filtering && items.length === 0 ? (
              <p className="py-16 text-center text-[15px] leading-6 text-ink-2">Searching…</p>
            ) : items.length === 0 && domainQuery === null ? (
              <p className="py-16 text-center text-[15px] leading-6 text-ink-2">
                {items.length === 0 ? "No sites indexed yet. Paste a domain above to scan the first one in." : "No sites match."}
              </p>
            ) : (
              items.map((s) => <Row key={s.slug} site={s} />)
            )}
          </div>
          {cursor !== null ? (
            <p className="py-6 text-center">
              <button
                type="button"
                onClick={onMore}
                disabled={loadingMore}
                className="inline-flex h-10 cursor-pointer items-center justify-center rounded-full border border-line px-5 text-sm font-medium leading-5 text-ink no-underline transition hover:bg-hover active:scale-[0.97] disabled:cursor-default disabled:opacity-60"
              >
                {loadingMore ? "Loading…" : "Show more"}
              </button>
            </p>
          ) : null}
          <div className="pb-10" />
        </main>
      </div>
    </>
  );
}
