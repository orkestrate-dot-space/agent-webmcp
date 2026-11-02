"use client";

import { useEffect, useState } from "react";
import { MessageSquare, Search, SquarePen } from "lucide-react";
import PromptBar from "./prompt-bar";

export interface ShowcaseTool {
  label: string;
  detail: string;
}

export interface ShowcaseExchange {
  tab: string;
  question: string;
  tools: Array<ShowcaseTool>;
  thought: string;
  thoughtTime: string;
  answer: string;
  sources: Array<string>;
}

export function AgentShowcase({ exchanges }: { exchanges: Array<ShowcaseExchange> }) {
  const [selected, setSelected] = useState(0);
  const [question, setQuestion] = useState(exchanges[0]?.question ?? "");
  const [words, setWords] = useState(0);
  const [filter, setFilter] = useState("");

  const current = exchanges[Math.min(selected, exchanges.length - 1)] ?? null;
  const totalWords = current ? current.answer.split(" ").length : 0;
  const streaming = current !== null && words < totalWords;
  const q = filter.trim().toLowerCase();
  const visible = exchanges
    .map((e, i) => ({ e, i }))
    .filter(
      ({ e }) =>
        q === "" ||
        e.question.toLowerCase().includes(q) ||
        e.tab.toLowerCase().includes(q),
    );

  // Client-only: password managers and similar extensions stamp attributes
  // into the DOM pre-hydration (e.g. data-protonpass-form), which breaks
  // SSR hydration. This panel is a scripted demo with no SEO value, so it
  // renders a same-size shell on the server and swaps in after mount.
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    setMounted(true);
  }, []);

  // Word-by-word reveal once a run is open.
  useEffect(() => {
    if (!mounted) return;
    if (current === null || words >= totalWords) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setWords(totalWords);
      return;
    }
    const t = setTimeout(() => setWords((w) => w + 1), 32);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mounted, current, words, totalWords, selected]);

  const open = (i: number) => {
    setSelected(i);
    setQuestion(exchanges[i].question);
    setWords(0);
  };

  if (!mounted) {
    return (
      <div
        aria-hidden="true"
        className="flex min-h-[560px] w-full flex-col self-start overflow-hidden rounded-xl border border-line bg-raised md:flex-row"
      />
    );
  }

  return (
    <div className="flex min-h-[560px] w-full flex-col self-start overflow-hidden rounded-xl border border-line bg-raised md:flex-row">
      {/* side panel — Codex ref: disabled New chat, search, Recents */}
      <aside className="hidden w-60 shrink-0 flex-col gap-0.5 border-r border-line p-3 md:flex">
        <button
          type="button"
          disabled
          aria-disabled="true"
          className="flex cursor-not-allowed items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink-2 opacity-45"
        >
          <SquarePen className="h-4 w-4" aria-hidden="true" />
          New chat
          <kbd className="ml-auto rounded-md border border-line px-1.5 py-0.5 font-mono text-[11px] leading-4 text-ink-2">
            ⌘ N
          </kbd>
        </button>
        <div className="mt-2 flex items-center gap-2 rounded-lg border border-line bg-canvas px-2.5 py-1.5 transition-[border-color] duration-150 focus-within:border-line-strong">
          <Search className="h-3.5 w-3.5 shrink-0 text-ink-2" aria-hidden="true" />
          <input
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            placeholder="Search chats…"
            aria-label="Search chats"
            className="min-w-0 flex-1 bg-transparent text-sm leading-5 text-ink outline-none placeholder:text-ink-2"
          />
          <kbd className="shrink-0 rounded border border-line px-1 font-mono text-[11px] leading-4 text-ink-2">
            /
          </kbd>
        </div>
        <p className="mt-4 px-2 font-mono text-[11px] uppercase leading-4 tracking-wide text-ink-2">
          Recents
        </p>
        <div className="mt-1 flex flex-col gap-0.5">
          {visible.map(({ e, i }) => (
            <button
              key={e.tab}
              type="button"
              onClick={() => open(i)}
              aria-current={selected === i}
              className={`flex cursor-pointer items-center gap-2 truncate rounded-md px-2 py-1.5 text-left text-sm transition-colors duration-150 ${
                selected === i ? "bg-hover font-medium text-ink" : "text-ink-2 hover:bg-hover hover:text-ink"
              }`}
            >
              <MessageSquare
                className={`h-4 w-4 shrink-0 ${selected === i ? "text-ink" : "text-ink-2"}`}
                aria-hidden="true"
              />
              <span className="min-w-0 flex-1 truncate">{e.question}</span>
              {selected === i ? (
                <span className="size-1.5 shrink-0 rounded-full bg-link" aria-hidden="true" />
              ) : null}
            </button>
          ))}
          {visible.length === 0 ? (
            <p className="truncate px-2 py-1.5 text-sm text-ink-2">No chats match.</p>
          ) : null}
        </div>
      </aside>

      {/* main */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* mobile exchange switcher */}
        <div className="flex gap-1 overflow-x-auto border-b border-line p-1.5 md:hidden">
          {exchanges.map((e, i) => (
            <button
              key={e.tab}
              type="button"
              onClick={() => open(i)}
              aria-pressed={selected === i}
              className={`shrink-0 cursor-pointer rounded-md px-3 py-1 text-sm ${
                selected === i ? "bg-hover font-medium text-ink" : "text-ink-2"
              }`}
            >
              {e.tab}
            </button>
          ))}
        </div>

        {current !== null ? (
          <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto px-4 pt-4 pb-1 sm:px-6">
            <div className="flex justify-end pl-14">
              <div
                className="rounded-xl bg-hover px-3 py-1.5 text-[13px] leading-[1.4] text-ink"
                style={{ animation: "showcase-fade-up 300ms var(--ease-out) both" }}
                key={question}
              >
                {question}
              </div>
            </div>
            <div
              className="flex flex-wrap items-center gap-1.5"
              style={{ animation: "showcase-fade-up 300ms var(--ease-out) both" }}
            >
              {current.tools.map((t) => (
                <span
                  key={t.label}
                  className="inline-flex items-center gap-1.5 rounded-md border border-line bg-canvas px-2 py-1 font-mono text-[12px] leading-4 text-ink"
                  title={t.detail}
                >
                  <span className="text-ink-2">›</span>
                  {t.label}
                  <span className="max-w-44 truncate font-sans text-ink-2">{t.detail}</span>
                </span>
              ))}
            </div>
            <p
              className="text-[13px] leading-[1.4] text-ink-2"
              style={{ animation: "showcase-fade-up 300ms var(--ease-out) both" }}
            >
              {current.thought} <span className="font-mono text-[12px]">· {current.thoughtTime}</span>
            </p>
            <p className="text-[13px] leading-normal text-ink">
              {current.answer.split(" ").slice(0, words).join(" ")}
              {streaming ? <span className="ml-0.5 inline-block h-3.5 w-[2px] animate-pulse bg-ink align-middle" /> : null}
            </p>
            {!streaming && current.sources.length > 0 ? (
              <div
                className="flex flex-wrap items-center gap-1.5"
                style={{ animation: "showcase-fade-up 300ms var(--ease-out) both" }}
              >
                {current.sources.map((s) => (
                  <span
                    key={s}
                    className="inline-flex items-center gap-1 rounded-full border border-line px-2 py-0.5 text-[12px] leading-4 text-ink-2"
                  >
                    {s}
                  </span>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}

        {/* composer — typeable Prompt Bar visual; send just clears */}
        <div className="mt-auto shrink-0 p-3 sm:p-4">
          <PromptBar placeholder="Ask anything…" />
        </div>
      </div>
      <style>{`@keyframes showcase-fade-up { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: translateY(0); } }
@keyframes promptbar-pop-in { from { opacity: 0; transform: scale(.98) translateY(4px); } to { opacity: 1; transform: none; } }
@keyframes promptbar-eq { 0%, 100% { transform: scaleY(.35); } 50% { transform: scaleY(1); } }`}</style>
    </div>
  );
}
