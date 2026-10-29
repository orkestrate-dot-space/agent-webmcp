import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Quick start · agent-webmcp docs",
  description: "The golden path — open, list, register, execute, close — with copy-paste evidence at every step.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
