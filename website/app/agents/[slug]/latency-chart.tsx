"use client";

import { AreaChart } from "@/components/dither-kit/area-chart";
import { Area } from "@/components/dither-kit/area";
import { XAxis } from "@/components/dither-kit/x-axis";
import { YAxis } from "@/components/dither-kit/y-axis";
import { Tooltip } from "@/components/dither-kit/tooltip";

export interface RunPoint {
  label: string;
  ms: number;
}

export function LatencyChart({ runs }: { runs: Array<RunPoint> }) {
  const data = runs.map((r) => ({ run: r.label, seconds: Math.round((r.ms / 1000) * 10) / 10 }));
  const config = { seconds: { label: "Seconds", color: "green" as const } };
  return (
    <AreaChart data={data} config={config}>
      <XAxis dataKey="run" />
      <YAxis />
      <Tooltip labelKey="run" />
      <Area dataKey="seconds" variant="gradient" />
    </AreaChart>
  );
}
