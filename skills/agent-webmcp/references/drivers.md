# Drivers (agent-webmcp reference)

The engine attaches; the driver sees and touches. Pair every session
with a driver via the `cdpUrl` from `open`. No `cdpUrl` answers?
Bootstrap a daemon-owned browser, then hand its endpoint to `open`:

```bash
AGENT_BROWSER_SESSION=work agent-browser open https://example.com  # daemon owns it
agent-browser get cdp-url                                          # hand this to open's cdp
```

Recon discipline — stale tabs are the #1 failure mode:

1. Fresh driver session per task. Verify an old tab with a snapshot before acting.
2. Viewport first for desktop-only UI (chat rails hide below ~1280px). Headless needs `--window-size=1366,900`.
3. Snapshot → act by `@ref` → snapshot. Refs beat selectors; eval is last resort.
4. Close the driver session when done. Engine tabs and driver tabs never meet; leaked driver tabs are yours.

Any WebMCP-capable browser works — its own skill owns the launch flags.
When a page cannot take authored tools the engine names the flag
fail-loud; look it up in the driver's docs, not here. Page bodies run in
the page, driver calls run outside it. Auth lives in the browser profile
(or the driver's vault); secrets never enter args, specs, or evidence.
Per-call `cdp` overrides discovery when the driver knows better.
