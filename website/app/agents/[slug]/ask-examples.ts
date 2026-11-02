// Per-agent example tasks for the "Example tasks" list.
// Curated per agent — no generic fallback: agents without entries get no
// list. Concrete jobs with specifics, not FAQ questions.
const BY_HOST: Record<string, Array<string>> = {
  "mintlify.com": [
    "Get a Mintlify docs site live from the quickstart",
    "Add the assistant widget to my docs",
    "Publish a docs change from the Mintlify dashboard",
  ],
  "claude.com": [
    "Connect Claude to my team's Slack workspace",
    "Explain connectors vs MCP servers for a custom integration",
    "Add the GitHub connector to Claude",
  ],
  "auth0.com": [
    "Set up Auth0 authentication for a new app",
    "Isolate my staging users in a separate tenant",
    "Enforce MFA with push notifications on my tenant",
  ],
  "launchdarkly.com": [
    "Roll out a feature flag to 10% of production traffic",
    "Target a flag at Team-plan users in the EU",
    "Set up a LaunchDarkly project for staging vs production",
  ],
  "bunny.net": [
    "Put my site behind a bunny.net pull zone",
    "Purge the CDN cache after a deploy",
    "Estimate bunny.net costs for 50TB of video delivery",
  ],
};

export function askExamplesFor(host: string): Array<string> {
  return BY_HOST[host] ?? [];
}
