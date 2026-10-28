import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Publishing your site · agent-webmcp docs",
  description: "Serve agent-webmcp.json on your domain: skills, agents, hooks. Verify, then list.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
