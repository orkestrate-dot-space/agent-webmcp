"use server";

import { revalidatePath, revalidateTag, updateTag } from "next/cache";

// On-demand invalidation after a successful worker scan. updateTag
// makes the next data fetch wait for fresh rows (immediate, no
// stale serve); revalidateTag clears the client router cache so
// back-navigation to /sites doesn't replay the old payload.
// (revalidateTag alone is stale-while-revalidate; updateTag alone
// leaves the router cache — the pair covers both layers.)
// Called from DomainScan/RescanButton.
export async function revalidateAfterScan(host?: string): Promise<{ revalidated: boolean }> {
  updateTag("sites-list");
  revalidateTag("sites-list", "max");
  updateTag("sites-counts");
  revalidateTag("sites-counts", "max");
  if (host) {
    updateTag(host);
    revalidateTag(host, "max");
    revalidatePath(`/sites/${host}`, "page");
  }
  return { revalidated: true };
}
