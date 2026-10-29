import type { Metadata } from "next";

import { SitesCatalog } from "./sites-catalog";
import { readFirstPage } from "./list-data";

export const metadata: Metadata = {
  title: "Sites · agent-webmcp",
  description: "Websites your agent can work with. Scanned live, with proof.",
};

// Static shell, fresh data every few minutes. Row data comes from
// the worker (cached 300s there); sites.json is fallback only.
export const revalidate = 300;

export default async function SitesPage() {
  const { sites, cursor, counts } = await readFirstPage();
  return (
    <>
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 rounded-md border border-line bg-canvas px-3 py-2 text-sm text-ink no-underline"
      >
        Skip to content
      </a>
      <SitesCatalog initialSites={sites} initialCursor={cursor} initialCounts={counts} />
    </>
  );
}
