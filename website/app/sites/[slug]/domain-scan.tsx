"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";

import { revalidateAfterScan } from "../actions";
import { SITES_API_VERSION } from "../scan";

const WORKER = "https://agent-webmcp-sites.agent-webmcp.workers.dev";

// Scan-then-render: POSTs a worker scan (which persists the row),
// waits until the row is readable (D1 reads can lag the write),
// invalidates list + record caches, then refreshes into the
// indexed branch. Scanning {host}… doubles as the loading state;
// failure offers a retry.
export function DomainScan({ host }: { host: string }) {
  const router = useRouter();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let on = true;
    setFailed(false);
    const run = async () => {
      try {
        const scan = await fetch(`${WORKER}/sites/${encodeURIComponent(host)}/scan`, {
          method: "POST",
        });
        if (!scan.ok) {
          if (on) setFailed(true);
          return;
        }
        let ready = false;
        for (let i = 0; i < 12 && on; i++) {
          try {
            const r = await fetch(
              `${WORKER}/sites/${encodeURIComponent(host)}?v=${SITES_API_VERSION}`
            );
            if (r.ok) {
              ready = true;
              break;
            }
          } catch {
            // Transient — retry on the next tick.
          }
          await new Promise((x) => setTimeout(x, 2500));
        }
        if (!on) return;
        if (!ready) {
          setFailed(true);
          return;
        }
        try {
          await revalidateAfterScan(host);
        } catch {
          // Cache still converges by TTL; the record below is fresh.
        }
        if (on) router.refresh();
      } catch {
        if (on) setFailed(true);
      }
    };
    run();
    return () => {
      on = false;
    };
  }, [host, attempt, router]);

  if (!failed) {
    return <p className="mt-6 text-[15px] leading-6 text-ink-2">Scanning {host}…</p>;
  }
  return (
    <>
      <p className="mt-6 max-w-[60ch] text-[15px] leading-6 text-ink-2">
        Couldn&apos;t scan {host} just now.
      </p>
      <p className="mt-6">
        <button
          type="button"
          onClick={() => setAttempt((a) => a + 1)}
          className="inline-flex h-10 cursor-pointer items-center justify-center rounded-full bg-ink px-5 text-sm font-medium leading-5 text-canvas transition hover:opacity-90 active:scale-[0.97]"
        >
          Try again →
        </button>
      </p>
    </>
  );
}
