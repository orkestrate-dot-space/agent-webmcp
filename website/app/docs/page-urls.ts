// Docs URL → content-file URLs. Single home for the "view source"
// mapping so the TOC footer and the Copy-page menu cannot drift apart:
// /docs → app/docs/page.mdx, /docs/<slug> → app/docs/<slug>/page.mdx.
const REPO = "https://github.com/system1970/agent-webmcp";
const RAW = "https://raw.githubusercontent.com/system1970/agent-webmcp";

const contentFile = (pathname: string): string => {
  const slug = pathname.replace(/^\/docs\/?/, "");
  return slug === "" ? "app/docs/page.mdx" : `app/docs/${slug}/page.mdx`;
};

export const editUrl = (pathname: string): string => `${REPO}/blob/main/website/${contentFile(pathname)}`;

export const rawUrl = (pathname: string): string => `${RAW}/main/website/${contentFile(pathname)}`;
