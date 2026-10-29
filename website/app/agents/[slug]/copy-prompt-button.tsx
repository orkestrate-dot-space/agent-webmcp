"use client";

import { useRef, useState } from "react";
import { Check, Copy } from "lucide-react";

import type { SignalItem } from "../scan";

const has = (signals: Array<SignalItem>, key: string): boolean =>
  signals.some((s) => s.key === key && s.ok);

export function buildPrompt(host: string, signals: Array<SignalItem>): string {
  const parts: Array<string> = [];
  if (has(signals, "llms")) parts.push("documentation");
  if (signals.some((s) => (s.key === "skillsIndex" || s.key === "skillMd") && s.ok))
    parts.push("agent skills");
  if (signals.some((s) => (s.key === "apiCatalog" || s.key === "openapi") && s.ok))
    parts.push("API specifications");
  if (has(signals, "mcp")) parts.push("MCP server");
  if (has(signals, "native")) parts.push("callable page tools");
  if (has(signals, "oauth")) parts.push("authentication requirements");
  if (has(signals, "agentCard")) parts.push("agent identity");
  if (has(signals, "ask")) parts.push("queryable docs");
  const what =
    parts.length === 0
      ? "public pages"
      : parts.length === 1
        ? parts[0]
        : `${parts.slice(0, -1).join(", ")}, and ${parts[parts.length - 1]}`;
  return `Use agent-webmcp to explore ${host}. Review its available ${what}. For my task: [describe task]. Prefer verified, accessible surfaces; cite the sources used and state when a capability is unavailable.`;
}

// Action-row copy control: builds the prompt from the domain's
// actual found signals and copies it, with confirmation.
export function CopyPromptButton({ host, signals }: { host: string; signals: Array<SignalItem> }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(buildPrompt(host, signals));
    } catch {
      return;
    }
    setCopied(true);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <button
      type="button"
      onClick={() => void copy()}
      aria-live="polite"
      className="inline-flex cursor-pointer items-center gap-2 text-[15px] leading-6 text-ink-2 no-underline transition hover:text-ink"
    >
      {copied ? <Check size={15} aria-hidden="true" /> : <Copy size={15} aria-hidden="true" />}
      {copied ? "Copied" : "Copy agent prompt"}
    </button>
  );
}
