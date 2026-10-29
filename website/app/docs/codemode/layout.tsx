import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Codemode · agent-webmcp docs",
  description: "Compose page tools across sessions with search and execute: shapes, budgets, envelopes, failure codes.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
