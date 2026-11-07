import type { MetadataRoute } from "next";

const BASE = "https://web.orkestrate.space/webmcp";

export default function robots(): MetadataRoute.Robots {
  return {
    rules: { userAgent: "*", allow: "/" },
    sitemap: `${BASE}/sitemap.xml`,
  };
}
