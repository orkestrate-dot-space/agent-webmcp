"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { docsNavigation } from "./navigation";

const currentName = (pathname: string): string => {
  for (const s of docsNavigation) {
    for (const i of s.items) {
      if (i.href === pathname) return i.name;
    }
  }
  return "Docs";
};

export function DocsSidebarNav() {
  const pathname = usePathname();
  return (
    <div className="flex flex-col gap-8">
      {docsNavigation.map((section, i) => (
        <div key={section.title ?? `group-${i}`}>
          {section.title ? (
            <p className="mb-2 whitespace-nowrap text-[13.5px] text-ink-2">
              {section.title}
            </p>
          ) : null}
          <ul className="flex flex-col gap-0.5">
            {section.items.map((item) => {
              const active = pathname === item.href;
              return (
                <li key={item.href}>
                  <Link
                    href={item.href}
                    aria-current={active ? "page" : undefined}
                    className={
                      active
                        ? "block whitespace-nowrap rounded-lg bg-black/[0.06] px-3 py-2 text-[15px] font-medium text-ink no-underline dark:bg-white/10"
                        : "block whitespace-nowrap rounded-lg px-3 py-2 text-[15px] text-ink-2 no-underline transition-colors hover:bg-black/[0.04] hover:text-ink dark:hover:bg-white/[0.06]"
                    }
                  >
                    {item.name}
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </div>
  );
}

// Mobile: the rail leaves document flow. A sticky contents bar under the
// header opens the same nav as an overlay drawer — the pattern from the
// reference sites. Desktop keeps the sticky rail (rendered by the layout).
export function MobileDocsNav() {
  const pathname = usePathname();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    setOpen(false);
  }, [pathname]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    document.body.style.overflow = open ? "hidden" : "";
    return () => {
      document.body.style.overflow = "";
    };
  }, [open ]);

  return (
    <div className="lg:hidden">
      <div className="sticky top-16 z-40 -mx-5 border-b border-line bg-canvas/95 px-5 py-2.5 backdrop-blur sm:-mx-8 sm:px-8">
        <button
          type="button"
          onClick={() => setOpen(true)}
          aria-expanded={open}
          aria-label="Open docs navigation"
          className="inline-flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-[14px] text-ink-2 transition-colors hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        >
          <span aria-hidden="true">☰</span>
          <span>{currentName(pathname)}</span>
          <span aria-hidden="true" className="text-[11px]">▾</span>
        </button>
      </div>
      {open ? (
        <div className="fixed inset-0 z-[90] bg-canvas" role="dialog" aria-modal="true" aria-label="Docs navigation">
          <div className="flex h-full flex-col">
            <div className="flex items-center justify-between border-b border-line px-5 py-4">
              <span className="text-[16px] font-medium text-ink">Browse</span>
              <button
                type="button"
                onClick={() => setOpen(false)}
                aria-label="Close navigation"
                className="cursor-pointer rounded-md px-2 py-1 text-[18px] text-ink-2 hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
              >
                ✕
              </button>
            </div>
            <div className="flex-1 overflow-y-auto px-3 py-4" onClick={(e) => {
              if ((e.target as HTMLElement).closest("a")) setOpen(false);
            }}>
              <DocsSidebarNav />
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
