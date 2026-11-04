// Per-agent example tasks for the "Try it out" list.
// Curated per agent — no generic fallback: agents without entries get no
// list. Every entry here is a cold deep-link question that the site's own
// assistant actually answered. Questions stay plain text, self-contained,
// and free of apostrophes/code fences (both break Mintlify prefill).
const BY_HOST: Record<string, Array<string>> = {
  "mintlify.com": [
    "Document a POST /v1/payments endpoint with a docs.json entry and a manual API page.",
    "My docs page returns 404 after I add a new MDX file, even though the file exists. What usually causes this in Mintlify?",
    "We still use mint.json. What changes when we migrate to docs.json?",
  ],
  "claude.com": [
    "We want Claude to read our internal Postgres database. Should we build an MCP server or use a connector? What is the difference and which do you recommend?",
    "How do I add a custom connector for an MCP server that uses OAuth?",
  ],
  "auth0.com": [
    "How do I require MFA with push notifications for all users in a tenant?",
    "We run staging and production. Should we use separate Auth0 tenants or one tenant per environment?",
    "How do I add a custom claim to an access token only for users in a specific organization?",
  ],
};

export function askExamplesFor(host: string): Array<string> {
  return BY_HOST[host] ?? [];
}
