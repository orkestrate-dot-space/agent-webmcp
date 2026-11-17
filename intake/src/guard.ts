import type { SiteRecord } from "./env";

/// A rebuild that sees a site with far fewer tools than before is more likely
/// a slow or overloaded scan than a real change (shop.dev went from 22 tools to
/// 7 that way). Such a site keeps its previous record until a scan agrees.
export const KEEP_RATIO = 2 / 3;

function keyOf(site: SiteRecord): string {
  return site.pages_checked[0] ?? site.host;
}

export interface GuardResult {
  sites: SiteRecord[];
  kept: string[];
}

/// Only a verified site that is still verified but lost more than a third of
/// its tools is held back. A site that stopped showing tools at all follows the
/// scanner's own two-misses rule, so a real removal still goes through.
export function holdBackToolLoss(merged: SiteRecord[], previous: SiteRecord[]): GuardResult {
  const before = new Map<string, SiteRecord>();
  for (const site of previous) before.set(keyOf(site), site);
  const kept: string[] = [];
  const sites = merged.map((site) => {
    const old = before.get(keyOf(site));
    if (!old || old.status !== "verified" || site.status !== "verified") return site;
    if (old.tools.length === 0 || site.tools.length >= old.tools.length * KEEP_RATIO) return site;
    kept.push(`${site.host} (${site.tools.length} of ${old.tools.length} tools)`);
    return old;
  });
  return { sites, kept };
}
