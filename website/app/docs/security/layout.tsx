import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Security · agent-webmcp docs",
  description: "Untrusted page data, deliberate writes, and the process and file guards behind close and the registry.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
