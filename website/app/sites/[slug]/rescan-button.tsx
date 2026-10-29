"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { RotateCw } from "lucide-react";

import { revalidateAfterScan } from "../actions";
import { SITES_API_VERSION } from "../scan";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

// Manual re-probe: same POST the auto-scan uses, wait until the
// refreshed row is readable (D1 reads can lag the write), then
// invalidate list + record and refresh into the updated record.
export function RescanButton({ host }: { host: string }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const run = async () => {
    if (busy) return;
    setBusy(true);
    setFailed(false);
    try {
      const r = await fetch(`${WORKER}/sites/${encodeURIComponent(host)}/scan`, { method: "POST" });
      if (!r.ok) {
        setBusy(false);
        setFailed(true);
        return;
      }
      let verified: string | null = null;
      try {
        const j = (await r.json()) as { site?: { proof?: { verified?: unknown } } };
        if (typeof j.site?.proof?.verified === "string") verified = j.site.proof.verified;
      } catch {
        // Fall through to the readability poll.
      }
      for (let i = 0; i < 12; i++) {
        try {
          const g = await fetch(`${WORKER}/sites/${encodeURIComponent(host)}?v=${SITES_API_VERSION}`);
          if (g.ok) {
            if (verified === null) break;
            const gj = (await g.json()) as { site?: { proof?: { verified?: unknown } } };
            if (gj.site?.proof?.verified === verified) break;
          }
        } catch {
          // Transient — retry on the next tick.
        }
        await new Promise((x) => setTimeout(x, 2500));
      }
      setBusy(false);
      try {
        await revalidateAfterScan(host);
      } catch {
        // Cache still converges by TTL; the record below is fresh.
      }
      router.refresh();
    } catch {
      setBusy(false);
      setFailed(true);
    }
  };
  return (
    <button
      type="button"
      onClick={run}
      disabled={busy}
      className="inline-flex cursor-pointer items-center gap-1.5 text-[15px] leading-6 text-ink no-underline transition hover:opacity-80 disabled:cursor-default disabled:opacity-60"
    >
      <RotateCw size={15} aria-hidden="true" className={busy ? "animate-spin" : undefined} />
      {failed ? "Try again" : busy ? "Scanning…" : "Rescan"}
    </button>
  );
}
