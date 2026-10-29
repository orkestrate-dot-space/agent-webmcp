"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";

// Command-palette search over public/search-index.json (built prebuild,
// lazy-fetched on first open — costs the page nothing until used).
// Deterministic substring ranking: heading > page title > body, same
// ethos as the engine's own rankTools. No dependency; nine pages.

interface IndexSection {
  anchor: string;
  heading: string;
  text: string;
}

interface IndexPage {
  url: string;
  title: string;
  description: string;
  sections: IndexSection[];
}

interface Hit {
  href: string;
  page: string;
  heading: string;
  snippet: string;
  score: number;
  order: number;
}

const termsOf = (q: string): string[] =>
  q
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((t) => t !== "");

const escHtml = (s: string): string =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const markTerms = (text: string, terms: string[]): string => {
  let out = escHtml(text);
  for (const t of terms) {
    const re = new RegExp(`(${t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi");
    out = out.replace(re, "<mark>$1</mark>");
  }
  return out;
};

const snippetFor = (text: string, terms: string[]): string => {
  const low = text.toLowerCase();
  let at = -1;
  for (const t of terms) {
    const i = low.indexOf(t);
    if (i >= 0 && (at < 0 || i < at)) at = i;
  }
  if (at < 0) return text.slice(0, 140);
  const start = Math.max(0, at - 70);
  const end = Math.min(text.length, at + 90);
  return `${start > 0 ? "…" : ""}${text.slice(start, end)}${end < text.length ? "…" : ""}`;
};

const rank = (pages: IndexPage[], query: string): Hit[] => {
  const terms = termsOf(query);
  if (terms.length === 0) {
    return pages.map((p, i) => ({
      href: p.url,
      page: p.title,
      heading: "",
      snippet: p.description,
      score: 0,
      order: i,
    }));
  }
  const hits: Hit[] = [];
  pages.forEach((p, pi) => {
    const titleLow = p.title.toLowerCase();
    p.sections.forEach((s, si) => {
      const headLow = s.heading.toLowerCase();
      const textLow = s.text.toLowerCase();
      let score = 0;
      for (const t of terms) {
        if (headLow === t) score += 20;
        else if (headLow.includes(t)) score += 8;
        if (titleLow.includes(t)) score += 4;
        if (textLow.includes(t)) score += 2;
      }
      if (score > 0) {
        const bodyHit = terms.some((t) => textLow.includes(t));
        hits.push({
          href: s.anchor ? `${p.url}#${s.anchor}` : p.url,
          page: p.title,
          heading: s.heading,
          snippet: bodyHit ? snippetFor(s.text, terms) : (s.heading || p.description),
          score,
          order: pi * 100 + si,
        });
      }
    });
  });
  hits.sort((a, b) => b.score - a.score || a.order - b.order);
  return hits.slice(0, 10);
};

export function SearchPalette() {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [pages, setPages] = useState<IndexPage[] | null>(null);
  const [active, setActive] = useState(0);
  const rowRefs = useRef<Array<HTMLAnchorElement | null>>([]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen(true);
      } else if (e.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (open && pages === null) {
      fetch("/search-index.json")
        .then((r) => (r.ok ? r.json() : []))
        .then((j: IndexPage[]) => setPages(j))
        .catch(() => setPages([]));
    }
    if (open) {
      setQuery("");
      setActive(0);
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = "";
    }
  }, [open, pages]);

  const hits = useMemo(() => rank(pages ?? [], query), [pages, query]);

  useEffect(() => {
    setActive(0);
  }, [query]);

  useEffect(() => {
    rowRefs.current[active]?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const go = (href: string) => {
    setOpen(false);
    router.push(href);
  };

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-label="Search docs"
        className="inline-flex w-36 cursor-pointer items-center justify-between gap-2 rounded-xl border border-line bg-canvas px-3.5 py-2 text-[14px] text-ink-2 transition hover:border-line-strong hover:text-ink active:scale-[0.98] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus sm:w-44"
      >
        <span>Search…</span>
        <kbd className="rounded-md border border-line bg-raised px-1.5 py-0.5 font-mono text-[12px] text-ink-2">⌘K</kbd>
      </button>
      {open ? (
        <div className="fixed inset-0 z-[100]" role="dialog" aria-modal="true" aria-label="Search docs">
          <button
            type="button"
            aria-label="Close search"
            onClick={() => setOpen(false)}
            className="absolute inset-0 cursor-default bg-black/60"
          />
          <div className="relative mx-auto mt-[12vh] w-[calc(100%-2rem)] max-w-xl overflow-hidden rounded-xl border border-line bg-canvas shadow-2xl">
            <div className="flex items-center gap-3 border-b border-line px-4">
              <span aria-hidden="true" className="text-ink-2">⌕</span>
              <input
                autoFocus
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    setActive((a) => Math.min(a + 1, hits.length - 1));
                  } else if (e.key === "ArrowUp") {
                    e.preventDefault();
                    setActive((a) => Math.max(a - 1, 0));
                  } else if (e.key === "Enter" && hits[active]) {
                    go(hits[active].href);
                  }
                }}
                placeholder="Search docs…"
                aria-label="Search docs"
                aria-activedescendant={hits.length > 0 ? `search-hit-${active}` : undefined}
                className="h-12 flex-1 bg-transparent text-[15px] text-ink outline-none placeholder:text-ink-2"
              />
              <kbd className="rounded border border-line px-1.5 font-mono text-[11px] text-ink-2">ESC</kbd>
            </div>
            <div className="max-h-[50vh] overflow-y-auto p-2" role="listbox" aria-label="Results">
              {pages === null ? (
                <p className="px-3 py-6 text-center text-[13px] text-ink-2">Loading index…</p>
              ) : hits.length === 0 ? (
                <p className="px-3 py-6 text-center text-[13px] text-ink-2" role="status">
                  No matches — try a verb name, a concept, or fewer words.
                </p>
              ) : (
                hits.map((h, i) => (
                  <a
                    key={`${h.href}-${i}`}
                    id={`search-hit-${i}`}
                    ref={(el) => {
                      rowRefs.current[i] = el;
                    }}
                    href={h.href}
                    role="option"
                    aria-selected={i === active}
                    onClick={(e) => {
                      // Modified/middle clicks take the native href (new tab);
                      // only plain left clicks route client-side.
                      if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button === 1) return;
                      e.preventDefault();
                      go(h.href);
                    }}
                    onMouseMove={() => setActive(i)}
                    className={`block rounded-lg px-3 py-2.5 no-underline ${i === active ? "bg-hover" : ""}`}
                  >
                    <span className="block text-[12px] text-ink-2">
                      {h.page}
                      {h.heading ? ` › ${h.heading}` : ""}
                    </span>
                    <span
                      className="mt-0.5 block text-[13.5px] leading-6 text-ink [&_mark]:bg-transparent [&_mark]:font-medium [&_mark]:text-ink [&_mark]:underline [&_mark]:decoration-line-strong [&_mark]:underline-offset-2"
                      dangerouslySetInnerHTML={{ __html: markTerms(h.snippet, termsOf(query)) }}
                    />
                  </a>
                ))
              )}
            </div>
            <div className="border-t border-line px-4 py-2 text-[12px] text-ink-2">
              ↑↓ navigate · Enter open · ESC close
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}
