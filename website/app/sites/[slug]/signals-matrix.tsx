"use client";

import { useState } from "react";
import {
  Bot,
  Braces,
  BookOpen,
  Boxes,
  ChevronDown,
  FileJson,
  FileText,
  Globe,
  KeyRound,
  MessageCircleQuestion,
  MousePointerClick,
  Server,
  type LucideIcon,
} from "lucide-react";

import type { SignalItem } from "../scan";

const ICONS: Record<string, LucideIcon> = {
  file: FileJson,
  llms: BookOpen,
  skillsIndex: Boxes,
  skillMd: FileText,
  apiCatalog: Globe,
  openapi: Braces,
  mcp: Server,
  oauth: KeyRound,
  agentCard: Bot,
  native: MousePointerClick,
  ask: MessageCircleQuestion,
};

const PATHS: Record<string, string> = {
  file: "/.well-known/agent-webmcp.json",
  llms: "/llms.txt",
  skillsIndex: "/.well-known/agent-skills/index.json",
  skillMd: "/skill.md",
  apiCatalog: "/.well-known/api-catalog",
  openapi: "/openapi.json",
  mcp: "/.well-known/mcp/server-card.json",
  oauth: "/.well-known/oauth-protected-resource",
  agentCard: "/.well-known/agent-card.json",
  native: "webmcp.com lookup",
  ask: "page marker · llms.txt · Link header",
};

// The scan report: all eleven probed formats in probe order,
// expandable for the endpoint behind each row.
export function SignalsMatrix({
  host,
  signals,
  verified,
}: {
  host: string;
  signals: Array<SignalItem>;
  verified: string;
}) {
  const [open, setOpen] = useState<Record<string, boolean>>({});
  return (
    <section aria-label="Detected surfaces" className="mt-10">
      <h2 className="text-xl font-medium text-ink">Detected surfaces</h2>
      <p className="mt-1 text-[15px] leading-6 text-ink-2">What agents can discover and use on this site.</p>
      <ul className="mt-4 border-t border-line">
        {signals.map((c) => {
          const Icon = ICONS[c.key] ?? FileText;
          const isOpen = open[c.key] === true;
          const path = PATHS[c.key] ?? c.key;
          return (
            <li key={c.key} className="border-b border-line">
              <button
                type="button"
                onClick={() => setOpen((o) => ({ ...o, [c.key]: !o[c.key] }))}
                aria-expanded={isOpen}
                className="flex w-full cursor-pointer flex-wrap items-center gap-x-4 gap-y-1 py-3.5 text-left"
              >
                <span className={c.ok ? "shrink-0 text-ink-2" : "shrink-0 text-ink-2/60"} aria-hidden="true">
                  <Icon size={20} strokeWidth={1.75} />
                </span>
                <span className={`w-32 shrink-0 text-[15px] leading-5 ${c.ok ? "text-ink" : "text-ink-2"}`}>
                  {c.label}
                </span>
                <span className="min-w-0 flex-1 basis-48 text-sm leading-5 text-ink-2">{c.blurb}</span>
                {c.ok ? (
                  <span className="inline-flex shrink-0 items-center gap-1.5 text-sm leading-5 text-green-500">
                    <span aria-hidden="true">✓</span> Available
                  </span>
                ) : (
                  <span className="inline-flex shrink-0 items-center gap-1.5 text-sm leading-5 text-ink-2">
                    <span aria-hidden="true">✗</span> Unavailable
                  </span>
                )}
                <ChevronDown
                  size={16}
                  aria-hidden="true"
                  className={`shrink-0 text-ink-2 transition-transform ${isOpen ? "rotate-180" : ""}`}
                />
              </button>
              {isOpen ? (
                <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1 pb-3.5 pl-9">
                  <code className="font-mono text-[13px] text-ink-2">
                    {path.startsWith("/") ? `https://${host}${path}` : path}
                  </code>
                  {verified !== "" ? (
                    <span className="text-[13px] leading-5 text-ink-2">Checked {verified}.</span>
                  ) : null}
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
