import SiteNav from "../site-nav";
import { DocsSidebarNav, MobileDocsNav } from "./sidebar-nav";
import { PageActions } from "./page-actions";
import { Toc } from "./toc";

export const metadata = {
  title: "Docs · agent-webmcp",
  description:
    "agent-webmcp turns live pages into an agent's toolkit: seven verbs, authored tools, MCP-only composition.",
};

export default function DocsLayout({ children }: { children: React.ReactNode }) {
  return (
    <>
      <a
        href="#docs-main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 rounded-md border border-line bg-canvas px-3 py-2 text-sm text-ink no-underline"
      >
        Skip to content
      </a>
      <SiteNav />
      <div className="mx-auto flex w-full max-w-[1280px] flex-col gap-8 px-5 pb-16 pt-6 sm:px-8 lg:flex-row">
        <MobileDocsNav />
        <nav aria-label="Docs" className="hidden w-60 shrink-0 lg:fixed lg:bottom-0 lg:left-[max(2rem,calc(50vw-40rem+2rem))] lg:top-16 lg:block lg:overflow-y-auto lg:overscroll-contain lg:pb-8 lg:pt-6">
          <DocsSidebarNav />
        </nav>
        <main id="docs-main" className="min-w-0 flex-1 pb-16 lg:ml-68">
          <PageActions />
          {children}
        </main>
        <div className="hidden w-48 shrink-0 xl:block">
          <Toc />
        </div>
      </div>
    </>
  );
}
