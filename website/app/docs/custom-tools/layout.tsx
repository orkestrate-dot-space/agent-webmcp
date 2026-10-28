import type { ReactNode } from "react";

// Metadata lives here, not in page.mdx: export const metadata inside MDX
// trips Next issue #91735 under webpack. Colocated layouts use only public
// API and survive bundler switches — prefer this over config patching.
export const metadata = {
  title: "Custom tools · agent-webmcp docs",
  description: "Author page tools with register: discovery first, spec plus plain-JS body, fixture proof, file memory, rot recovery.",
};

export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
