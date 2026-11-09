# Copy prompt (website button source)

Paste this into Claude Code, OpenCode, Cursor, or Codex:

```text
Set up agent-webmcp so you can call website tools from your browser driver. Do all steps yourself, don't ask me to run commands.

1. Install CLI if missing: npm i -g @orkestrate/agent-webmcp (fallback: prefix every command with npx -y @orkestrate/agent-webmcp)
2. Install skill: npx skills add orkestrate-dot-space/agent-webmcp --global
3. Verify: agent-webmcp search "docs" and agent-webmcp get docs.stripe.com — both must return tools, no setup needed
4. Detect driver: check for agent-browser, Browse MCP, or Playwright, report which you'll use

From now on: search before snapshot. Toolkit-first: run search, then get, execute via the how_to_run block get returns. Snapshot only on a toolkit miss.

Reply when ready with version + driver + 1 verified tool.
```
