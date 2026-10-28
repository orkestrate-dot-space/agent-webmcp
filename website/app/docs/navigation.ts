export interface DocsNavItem {
  name: string;
  href: string;
}

export interface DocsNavSection {
  title: string | null;
  items: DocsNavItem[];
}

/** Single source of truth for the docs sidebar. Add a page here and
 *  create app/docs/<slug>/page.mdx (+ colocated layout.tsx metadata) —
 *  nothing else to wire. */
export const docsNavigation: DocsNavSection[] = [
  {
    title: null,
    items: [
      { name: "Introduction", href: "/docs" },
      { name: "Installation", href: "/docs/installation" },
      { name: "Quick start", href: "/docs/quick-start" },
      { name: "Publishing your site", href: "/docs/publishing" },
    ],
  },
  {
    title: "Authoring",
    items: [
      { name: "Custom tools", href: "/docs/custom-tools" },
      { name: "Codemode", href: "/docs/codemode" },
    ],
  },
  {
    title: "Reference",
    items: [
      { name: "Commands", href: "/docs/commands" },
      { name: "Sessions", href: "/docs/sessions" },
      { name: "Security", href: "/docs/security" },
      { name: "Troubleshooting", href: "/docs/troubleshooting" },
    ],
  },
  {
    title: null,
    items: [{ name: "Changelog", href: "/docs/changelog" }],
  },
];
