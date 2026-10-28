"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";

import { editUrl } from "./page-urls";
import tocIndex from "./toc-index.json";

// Right-rail "On this page" TOC. Items come from a build-time index
// (bundled JSON — first paint includes them, zero loading state).
// Heading ids are baked by rehype-slug; the effect below only tracks
// scroll position and backfills the anchor offset class. Renders nothing
// on routes without sections.
interface TocItem {
  id: string;
  text: string;
  level: number;
}

const byRoute = tocIndex as Record<string, Array<{ anchor: string; heading: string; level: number }>>;

export function Toc() {
  const pathname = usePathname();
  const [active, setActive] = useState<string>("");

  const items: TocItem[] = (byRoute[pathname] ?? []).map((s) => ({
    id: s.anchor,
    text: s.heading,
    level: s.level,
  }));

  // Rebind on every route change: DocsLayout stays mounted across
  // client-side navigation, so a mount-only effect freezes the spy on
  // the first page.
  useEffect(() => {
    const main = document.getElementById("docs-main");
    if (!main) return;
    const heads = [...main.querySelectorAll("h2, h3")] as HTMLHeadingElement[];
    for (const h of heads) h.classList.add("scroll-mt-24");
    // Scroll-spy by position, not intersection: active is the last heading
    // clearly inside the viewport (top edge at or above 75% of its
    // height). A strict near-header line activates too late on tall
    // viewports — the visible section must win. (An IntersectionObserver
    // band froze on long sections and picked the wrong heading when
    // several shared the band — do not go back to it.)
    const onScroll = () => {
      const line = window.innerHeight * 0.75;
      let current = "";
      for (const h of heads) {
        if (h.id && h.getBoundingClientRect().top <= line) current = h.id;
      }
      // Bottom-of-page rule: short trailing content means the last heading
      // can never reach the activation line — pin it when fully scrolled.
      if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 8) {
        const last = heads[heads.length - 1];
        if (last?.id) current = last.id;
      }
      setActive(current);
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, [pathname]);

  // The column is reserved server-side by the layout, so first paint
  // already holds the space — items populate into it with no shift.
  // (Every docs page carries ≥2 h2s, so the reserved rail never sits empty.)
  if (items.length === 0) return null;

  return (
    <aside aria-label="On this page" className="sticky top-22 max-h-[calc(100vh-7rem)] overflow-y-auto overscroll-contain">
      <p className="mb-2 text-[13px] font-medium text-ink">On this page</p>
      <ul className="border-l border-line">
        {items.map((i) => (
          <li key={i.id}>
            <a
              href={`#${i.id}`}
              aria-current={active === i.id ? "true" : undefined}
              className={`block border-l-2 no-underline transition-colors ${
                i.level === 3 ? "pl-6" : "pl-3"
              } -ml-px py-1 text-[13px] ${
                active === i.id
                  ? "border-ink font-medium text-ink"
                  : "border-transparent text-ink-2 hover:text-ink"
              }`}
            >
              {i.text}
            </a>
          </li>
        ))}
      </ul>
      <div className="mt-4 space-y-1 border-t border-line pt-3">
        <button
          type="button"
          onClick={() => window.scrollTo({ top: 0, behavior: "smooth" })}
          className="block cursor-pointer py-1 text-[13px] text-ink-2 no-underline transition-colors hover:text-ink"
        >
          ↑ Scroll to top
        </button>
        <a
          href={editUrl(pathname)}
          className="block py-1 text-[13px] text-ink-2 no-underline transition-colors hover:text-ink"
        >
          Edit this page on GitHub
        </a>
      </div>
    </aside>
  );
}
