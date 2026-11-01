"use client";

import { useState } from "react";

import { CopyCodeButton } from "./copy-code-button";

type TabId = "cli" | "ts" | "py";

const TABS: Array<{ id: TabId; label: string }> = [
  { id: "cli", label: "CLI" },
  { id: "ts", label: "TypeScript" },
  { id: "py", label: "Python" },
];

export function IntegrateTabs({ snippets }: { snippets: Record<TabId, string> }) {
  const [tab, setTab] = useState<TabId>("ts");
  return (
    <div>
      <div className="mt-4 inline-flex gap-1 rounded-lg border border-line p-1">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            aria-pressed={tab === t.id}
            className={`cursor-pointer rounded-md px-4 py-1.5 text-sm leading-5 transition ${
              tab === t.id ? "bg-hover font-medium text-ink" : "font-normal text-ink-2 hover:text-ink"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="mt-3 rounded-lg border border-line bg-raised p-5">
        <div className="flex items-center justify-end">
          <CopyCodeButton code={snippets[tab]} />
        </div>
        <pre className="mt-1 overflow-x-auto whitespace-pre-wrap font-mono text-[13px] leading-6 text-ink">
          {snippets[tab]}
        </pre>
      </div>
    </div>
  );
}
