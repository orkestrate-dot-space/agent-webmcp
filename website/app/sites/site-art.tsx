"use client";

import { useState } from "react";

function GlobeIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="6" stroke="currentColor" strokeWidth="1.4" />
      <path
        d="M2 8h12M8 2c-3.5 3.2-3.5 8.8 0 12 3.5-3.2 3.5-8.8 0-12Z"
        stroke="currentColor"
        strokeWidth="1.2"
      />
    </svg>
  );
}

export function OriginFavicon({ host, size = 24 }: { host: string; size?: number }) {
  const [failed, setFailed] = useState(false);
  if (host === "" || !host.includes(".") || failed) {
    return (
      <span aria-hidden="true" className="flex shrink-0 items-center justify-center text-ink-2" style={{ width: size, height: size }}>
        <GlobeIcon size={size} />
      </span>
    );
  }
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src={`https://www.google.com/s2/favicons?domain=${encodeURIComponent(host)}&sz=64`}
      alt=""
      aria-hidden="true"
      width={size}
      height={size}
      loading="lazy"
      onError={() => setFailed(true)}
      className="shrink-0 object-contain"
      style={{ width: size, height: size }}
    />
  );
}
