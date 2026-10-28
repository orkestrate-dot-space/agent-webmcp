import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Installation · agent-webmcp docs",
  description: "Build the engine from source, put it on PATH, verify the version.",
};

export default function InstallationLayout({ children }: { children: ReactNode }) {
  return children;
}
