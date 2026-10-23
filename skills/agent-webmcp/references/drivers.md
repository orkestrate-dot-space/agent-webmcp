# Drivers (agent-webmcp reference)

The engine never drives browsers. Pair every session with a driver via the
`cdpUrl` from `open`. One appendix table — recon is observe → act → mine
traffic everywhere.

| Driver | See | Act | Mine traffic | Notes |
|---|---|---|---|---|
| agent-browser (default) | `snapshot` | `click/fill/type/press` | HAR via network commands | Daemon discovery is the default attach path; `webmcp list/invoke` consumes native verbs directly |
| Playwright MCP | `snapshot` | `click/fill/type` | `network_requests`/HAR | `connectOverCDP` to the session's `cdpUrl` |
| Chrome DevTools MCP | `take_snapshot` | `click/fill` | `list_network_requests` | Same CDP, different verbs |
| Stagehand | `observe/extract` | `act` | request interception | AI assertions; replay artifacts as evidence |

Rules: the driver sees and touches, never persists. Page bodies run in the
page; driver calls run outside it — never paste one as the other. Auth
lives in the browser profile (or the driver's vault); secrets never enter
tool args, specs, or evidence files. Per-call `cdp` overrides discovery
when the driver knows a better endpoint (Steel, cloud sandboxes).
