# Drivers (agent-webmcp reference)

The engine never drives browsers. Pair every session with a driver via the
`cdpUrl` from `open`. Bootstrap a daemon-owned browser when none answers:

```bash
AGENT_BROWSER_SESSION=work agent-browser open https://example.com  # daemon owns it
agent-browser get cdp-url                                          # hand this to open's cdp
```

Recon discipline — stale tabs are the #1 failure mode. Every recon:

1. Fresh driver session per task (`session: <task>` or `AGENT_BROWSER_SESSION=<task>`). Never reuse a tab spawned ages ago — verify with a snapshot before acting.
2. Check viewport first for desktop-only UI (chat rails hide below ~1280px). Set `--window-size=1366,900` when launching headless.
3. Snapshot → act by `@ref` → snapshot. Refs beat selectors; eval is the last resort, never the first move.
4. Close the driver session when done. The engine never sees driver tabs; leaked tabs are yours.

One appendix table — recon is observe → act → mine traffic everywhere.

| Driver | See | Act | Mine traffic | Page tools? | Notes |
|---|---|---|---|---|---|
| agent-browser (default) | `snapshot` | `click/fill/type/press` | HAR via network commands | Consumes (`webmcp list/invoke`); opt-out `AGENT_BROWSER_NO_WEBMCP=1` | Daemon discovery is the default attach path; CLI+MCP parity, tiny context |
| Playwright MCP | snapshot-after-every-action | `click/fill/type` | network list/detail, tracing | Consumes as `webmcp_<tool>`, default-on; opt-out `--no-webmcp` | Needs `--enable-features=WebMCP`; Firefox: `dom.modelcontext.enabled` |
| Chrome DevTools MCP | `take_snapshot` | `click/fill` | `list_network_requests`, traces | Consumes, opt-in `--categoryExperimentalWebmcp=true` | Best login story (`autoConnect` inherits your live browser) |
| Stagehand v4 | `observe/extract` | `act` (AI) or `page.*` (code) | request interception | Consumes and **prefers** over `act`; needs `WebMCPTesting,DevToolsWebMCPSupport` | Structured extraction with Zod; cloud stealth via Browserbase |
| Hyperbrowser | agent runtimes | hosted agents | session APIs | Consumes (details thin) | Zero-infra cloud; persistent auth |
| browser-use / Steel | AX tree / CDP | observe→act loop / bring-your-own | screenshots, history | Ignore (browser-use) / infra-only (Steel) | Bring a supporting driver for page tools |

Rules: the driver sees and touches, never persists. Any WebMCP-capable
browser works the same — launch flags, per driver docs:
`--enable-features=WebMCPTesting,DevToolsWebMCPSupport` (Chrome/Chromium;
Firefox: `dom.modelcontext.enabled`), which is also what our fail-loud
fix names when a page cannot take authored tools. Page bodies run in the
page; driver calls run outside it — never paste one as the other. Auth
lives in the browser profile (or the driver's vault); secrets never enter
tool args, specs, or evidence files. Per-call `cdp` overrides discovery
when the driver knows a better endpoint (Steel, cloud sandboxes).
