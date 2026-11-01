"use client";

import { useState } from "react";

async function copyText(text: string): Promise<boolean> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await Promise.race([
        navigator.clipboard.writeText(text),
        new Promise<never>((_, reject) => window.setTimeout(() => reject(new Error("clipboard-timeout")), 1500)),
      ]);
      return true;
    }
  } catch {
    // fall through
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.top = "0";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

function CheckIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M3 8.5 6.5 12 13 4.5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function CopyCodeButton({ code, label, primary, secondary }: { code: string; label?: string; primary?: boolean; secondary?: boolean }) {
  const [state, setState] = useState<"idle" | "ok" | "fail">("idle");
  const text = state === "ok" ? "Copied" : state === "fail" ? "Copy failed" : (label ?? "Copy");
  const run = () => {
    void copyText(code).then((ok) => {
      setState(ok ? "ok" : "fail");
      window.setTimeout(() => setState("idle"), 2000);
    });
  };
  const base =
    "inline-flex h-10 cursor-pointer items-center gap-1.5 rounded-full px-5 text-sm font-medium leading-5 transition active:scale-[0.97] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus";
  if (secondary) {
    return (
      <button type="button" onClick={run} className={`${base} border border-line bg-raised text-ink hover:bg-hover`}>
        {text}
        {state === "ok" ? <CheckIcon /> : null}
      </button>
    );
  }
  if (primary) {
    return (
      <button
        type="button"
        onClick={run}
        className={`${base} bg-ink text-canvas no-underline hover:opacity-90`}
      >
        {text}
        {state === "ok" ? <CheckIcon /> : null}
      </button>
    );
  }
  return (
    <button
      type="button"
      onClick={run}
      className="shrink-0 cursor-pointer rounded-md border border-line px-2.5 py-1 font-mono text-xs leading-5 text-ink-2 transition hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    >
      <span className="inline-flex items-center gap-1.5">
        {text}
        {state === "ok" ? <CheckIcon /> : null}
      </span>
    </button>
  );
}
