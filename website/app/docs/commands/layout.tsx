import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Commands · agent-webmcp docs",
  description: "Every verb and door: lifecycle, authoring, MCP-only composition, utilities, flags, outputs, exits.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
