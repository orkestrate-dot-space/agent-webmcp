"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname } from "next/navigation";

import { editUrl, rawUrl } from "./page-urls";

const rowIcon = "flex w-4 shrink-0 items-center justify-center text-ink-2";

function CopyIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true" className="shrink-0">
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" stroke="currentColor" strokeWidth="1.5" />
      <path d="M10.5 5.5v-2a1.5 1.5 0 0 0-1.5-1.5H4A1.5 1.5 0 0 0 2.5 3.5v5A1.5 1.5 0 0 0 4 10h1.5" stroke="currentColor" strokeWidth="1.5" />
    </svg>
  );
}

// GitHub mark vendored from Simple Icons (CC0).
function GitHubIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" className="shrink-0">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  );
}

function MdBadge() {
  return (
    <span aria-hidden="true" className="flex h-4 w-6 shrink-0 items-center justify-center rounded border border-line font-mono text-[9px] font-medium text-ink-2">
      MD
    </span>
  );
}

// Copy-page menu: Copy page text, View as Markdown (raw MDX source),
// Edit on GitHub. Floats top-right of the title like the reference
// sites — rendered by the layout, zero per-page edits.
export function PageActions() {
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setOpen(false);
  }, [pathname]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const onClick = (e: MouseEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onClick);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onClick);
    };
  }, [open ]);

  const copyPage = async () => {
    const main = document.getElementById("docs-main");
    if (!main) return;
    try {
      await navigator.clipboard.writeText(main.innerText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard unavailable — selection still works.
    }
    setOpen(false);
  };

  const btn =
    "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left no-underline transition-colors hover:bg-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus";

  return (
    <div ref={boxRef} className="relative float-right mb-2 ml-4 mt-2">
      <div className="flex rounded-lg border border-line">
        <button
          type="button"
          onClick={() => void copyPage()}
          className="inline-flex cursor-pointer items-center gap-2 rounded-l-lg px-3 py-1.5 text-[13.5px] text-ink transition-colors hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        >
          <CopyIcon />
          {copied ? "Copied" : "Copy page"}
        </button>
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          aria-label="More page actions"
          className="cursor-pointer rounded-r-lg border-l border-line px-2 py-1.5 text-ink-2 transition-colors hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
            <path d="M2.5 4.5 6 8l3.5-3.5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      </div>
      {open ? (
        <div className="absolute right-0 top-full z-50 mt-2 w-[19rem] overflow-hidden rounded-2xl border border-line bg-canvas p-2 shadow-2xl" role="menu">
          <button type="button" role="menuitem" onClick={() => void copyPage()} className={btn}>
            <span className={rowIcon}><CopyIcon /></span>
            <span>
              <span className="block text-[14px] text-ink">Copy page</span>
              <span className="block text-[12.5px] text-ink-2">Copy page text for LLMs</span>
            </span>
          </button>
          <a href={rawUrl(pathname)} role="menuitem" className={btn}>
            <span className={rowIcon}><MdBadge /></span>
            <span>
              <span className="block text-[14px] text-ink">View as Markdown</span>
              <span className="block text-[12.5px] text-ink-2">Open the MDX source</span>
            </span>
          </a>
          <a href={editUrl(pathname)} role="menuitem" className={btn}>
            <span className={rowIcon}><GitHubIcon /></span>
            <span>
              <span className="block text-[14px] text-ink">Edit this page on GitHub</span>
              <span className="block text-[12.5px] text-ink-2">Suggest changes to this page</span>
            </span>
          </a>
        </div>
      ) : null}
    </div>
  );
}
