"use client";

import { useState } from "react";

export function CopyCodeButton({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);
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
      {copied ? "Copied" : "Copy"}
    </button>
  );
}
