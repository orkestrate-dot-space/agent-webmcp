import type { ShowcaseExchange } from "./agent-showcase";

// Scripted demo runs. Timings marked ~ are from verified live runs
// (claude.com ~10s, auth0.com ~15s); everything else is illustrative.
const GENERIC: Array<ShowcaseExchange> = [
  {
    tab: "Getting started",
    question: "How do I get started?",
    tools: [
      { label: "get_agent", detail: "ask_site" },
      { label: "search_docs", detail: "getting started" },
      { label: "read_page", detail: "/quickstart" },
    ],
    thought: "Read 1 file · Searched for 1 pattern",
    thoughtTime: "~10s",
    answer:
      "Three steps with links: install, configure, deploy. Every claim cites the page it came from.",
    sources: ["Quickstart"],
  },
  {
    tab: "Follow-up",
    question: "How is that different from the alternative?",
    tools: [
      { label: "get_agent", detail: "ask_site" },
      { label: "search_docs", detail: "comparison" },
      { label: "read_page", detail: "/comparison" },
    ],
    thought: "Thread kept · no re-search needed",
    thoughtTime: "~8s",
    answer:
      "Resolved “that” from the previous answer — no repetition needed. Side-by-side, scoped to what's actually in these docs.",
    sources: ["Comparison"],
  },
  {
    tab: "Out of scope",
    question: "Write me a poem instead?",
    tools: [{ label: "get_agent", detail: "ask_site" }],
    thought: "Scoped agent check",
    thoughtTime: "~2s",
    answer: "Declined with a redirect — this agent answers from its docs, and says so instead of guessing.",
    sources: [],
  },
];

const BY_HOST: Record<string, Array<ShowcaseExchange>> = {
  "mintlify.com": [
    {
      tab: "Getting started",
      question: "How do I get started?",
      tools: [
        { label: "get_agent", detail: "ask_mintlify" },
        { label: "search_docs", detail: "get started" },
        { label: "read_page", detail: "/quickstart" },
      ],
      thought: "Read 1 file · Searched for 1 pattern",
      thoughtTime: "~10s",
      answer:
        "Edit in the web editor or clone and run mint dev, then publish from your dashboard. Every step links its source.",
      sources: ["Quickstart", "CLI reference"],
    },
    {
      tab: "Assistant widget",
      question: "What is the assistant widget?",
      tools: [
        { label: "get_agent", detail: "ask_mintlify" },
        { label: "search_docs", detail: "assistant widget" },
        { label: "read_page", detail: "/assistant" },
      ],
      thought: "Read 1 file · Searched for 2 patterns",
      thoughtTime: "~11s",
      answer:
        "A docs chatbot grounded in your pages, with citations and follow-up threads out of the box.",
      sources: ["Assistant", "Widget reference"],
    },
    {
      tab: "Out of scope",
      question: "Write me a poem?",
      tools: [{ label: "get_agent", detail: "ask_mintlify" }],
      thought: "Scoped agent check",
      thoughtTime: "~2s",
      answer: "Politely declined — it answers from Mintlify docs, and says so instead of guessing.",
      sources: [],
    },
  ],
  "claude.com": [
    {
      tab: "Connectors",
      question: "What is a connector?",
      tools: [
        { label: "get_agent", detail: "ask_claude" },
        { label: "search_docs", detail: "connector" },
        { label: "read_page", detail: "/connectors/getting-started" },
      ],
      thought: "Read 1 file · Searched for 1 pattern",
      thoughtTime: "~10s",
      answer:
        "A connector links Claude to an outside app or service — calendar, documents, issue tracker. Add one from Customize > Connectors, sign in, approve the access.",
      sources: ["Get started with connectors", "Connector directory"],
    },
    {
      tab: "Follow-up",
      question: "How is that different from an MCP server?",
      tools: [
        { label: "get_agent", detail: "ask_claude" },
        { label: "search_docs", detail: "connector vs MCP" },
      ],
      thought: "Thread kept · “that” resolved in context",
      thoughtTime: "~9s",
      answer:
        "A connector is the user-facing presentation, the MCP server is what it connects to — directory vs custom, same infrastructure.",
      sources: ["Connectors", "MCP reference"],
    },
    {
      tab: "Out of scope",
      question: "Write me a poem?",
      tools: [{ label: "get_agent", detail: "ask_claude" }],
      thought: "Scoped agent check",
      thoughtTime: "~2s",
      answer: "Declined with a redirect back to connectors documentation.",
      sources: [],
    },
  ],
  "auth0.com": [
    {
      tab: "What is Auth0?",
      question: "What is Auth0 and how do I get started?",
      tools: [
        { label: "get_agent", detail: "ask_auth0" },
        { label: "search_docs", detail: "what is auth0 getting started" },
        { label: "read_page", detail: "/overview" },
      ],
      thought: "Read 1 file · Searched for 2 patterns",
      thoughtTime: "~15s",
      answer:
        "Auth0 is an identity platform — drop-in authentication and authorization for your apps and APIs. To get started: create a tenant, register your app, follow a stack quickstart.",
      sources: ["Auth0 Overview", "Introduction to Auth0"],
    },
    {
      tab: "Tenants",
      question: "What is a tenant?",
      tools: [
        { label: "get_agent", detail: "ask_auth0" },
        { label: "search_docs", detail: "tenant" },
        { label: "read_page", detail: "/tenants" },
      ],
      thought: "Read 1 file · Searched for 1 pattern",
      thoughtTime: "~12s",
      answer:
        "A tenant is your isolated Auth0 environment — users, apps, and connections live inside it.",
      sources: ["Tenants overview"],
    },
    {
      tab: "MFA",
      question: "Does it support MFA?",
      tools: [
        { label: "get_agent", detail: "ask_auth0" },
        { label: "search_docs", detail: "MFA" },
      ],
      thought: "Thread kept · “it” resolved to Auth0",
      thoughtTime: "~10s",
      answer: "Yes — one-time codes, push, biometrics, and more, enforced per app or per user.",
      sources: ["MFA guide"],
    },
  ],
};

export function showcaseFor(host: string): Array<ShowcaseExchange> {
  return BY_HOST[host] ?? GENERIC;
}
