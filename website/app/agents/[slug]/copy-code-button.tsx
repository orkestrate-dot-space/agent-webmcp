"use client";

import { useState } from "react";

export function CopyCodeButton({ code, label, primary, secondary }: { code: string; label?: string; primary?: boolean; secondary?: boolean }) {
  const [copied, setCopied] = useState(false);
  const text = copied ? "Copied" : (label ?? "Copy");
  if (secondary) {
    return (
      <button
        type="button"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(code);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          } catch {
            // Clipboard unavailable — select the text manually.
          }
        }}
        className="inline-flex h-9 cursor-pointer items-center rounded-full border border-line bg-raised px-4 text-sm font-medium leading-5 text-ink transition hover:bg-hover active:scale-[0.97]"
      >
        {text}
      </button>
    );
  }
  if (primary) {
    return (
      <button
        type="button"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(code);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          } catch {
            // Clipboard unavailable — select the text manually.
          }
        }}
        className="inline-flex h-9 cursor-pointer items-center rounded-full bg-ink px-4 text-sm font-medium leading-5 text-canvas transition hover:opacity-90 active:scale-[0.97]"
      >
        {text}
      </button>
    );
  }
  return (
    <button
      type="button"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(code);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        } catch {
          // Clipboard unavailable — select the text manually.
        }
      }}
      className="shrink-0 cursor-pointer rounded-md border border-line px-2.5 py-1 font-mono text-xs leading-5 text-ink-2 transition hover:text-ink"
    >
      {text}
    </button>
  );
}
