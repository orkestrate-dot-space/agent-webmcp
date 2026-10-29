import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Changelog · agent-webmcp docs",
  description: "Engine v2 history, starting at 0.1.0.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
