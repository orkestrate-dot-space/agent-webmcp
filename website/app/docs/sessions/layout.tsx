import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Sessions · agent-webmcp docs",
  description: "Owned versus borrowed browser model: lifecycle, close semantics, disk records, and multi-session discipline.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
