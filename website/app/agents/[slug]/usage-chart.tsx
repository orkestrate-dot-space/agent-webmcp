"use client";

import { useEffect, useState } from "react";

import { Area } from "../../../components/dither-kit/area";
import { AreaChart } from "../../../components/dither-kit/area-chart";
import { Grid } from "../../../components/dither-kit/grid";
import { XAxis } from "../../../components/dither-kit/x-axis";
import { YAxis } from "../../../components/dither-kit/y-axis";

interface DayCount {
  day: string;
  count: number;
}

const RANGES = [
  { id: "today", label: "Today", days: 1, caption: "get_agent calls · last 24 hours" },
  { id: "7d", label: "7d", days: 7, caption: "get_agent calls · last 7 days" },
  { id: "30d", label: "30d", days: 30, caption: "get_agent calls · last 30 days" },
  { id: "all", label: "All time", days: 90, caption: "get_agent calls · all time" },
] as const;

type RangeId = (typeof RANGES)[number]["id"];

export function UsageChart({ domain }: { domain: string }) {
  const [range, setRange] = useState<RangeId>("today");
  const [days, setDays] = useState<Array<DayCount> | null>(null);
  const [total, setTotal] = useState<number | null>(null);

  const active = RANGES.find((r) => r.id === range) ?? RANGES[2];

  useEffect(() => {
    let live = true;
    setDays(null);
    fetch(`/api/use?domain=${encodeURIComponent(domain)}&days=${active.days}`)
      .then((r) => (r.ok ? r.json() : null))
      .then((j) => {
        if (!live || !j) return;
        setDays(j.days as Array<DayCount>);
        setTotal(j.total as number);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [domain, active.days]);

  return (
    <div>
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <div className="flex items-baseline gap-3">
          <p className="font-mono text-3xl leading-9 text-ink">{total ?? "–"}</p>
          <p className="text-sm leading-5 text-ink-2">{active.caption}</p>
        </div>
        <div className="inline-flex gap-1 rounded-lg border border-line p-1" role="tablist" aria-label="Usage range">
          {RANGES.map((r) => (
            <button
              key={r.id}
              type="button"
              role="tab"
              aria-selected={range === r.id}
              onClick={() => setRange(r.id)}
              className={`cursor-pointer rounded-md px-3 py-1 text-sm leading-5 transition ${
                range === r.id ? "bg-hover font-medium text-ink" : "font-normal text-ink-2 hover:text-ink"
              }`}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>
      <div className="mt-3 rounded-lg border border-line bg-raised p-4">
        {days === null ? (
          <div className="h-44 animate-pulse rounded-lg bg-hover" aria-label="Loading usage" />
        ) : (
          <AreaChart
            data={days}
            config={{ count: { label: "calls", color: "green" } }}
            className="h-44 w-full"
            interactive={false}
          >
            <Grid />
            <XAxis
              dataKey="day"
              tickFormatter={(_: unknown, i: number) =>
                i % Math.ceil(days.length / 6) === 0 ? days[i].day : ""
              }
            />
            <YAxis tickCount={3} tickFormatter={(t: number) => String(Math.round(t))} />
            <Area dataKey="count" />
          </AreaChart>
        )}
      </div>
    </div>
  );
}
