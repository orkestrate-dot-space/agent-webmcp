# Drivers reference (agent-webmcp v0.4.0)

Run drivers execute `call` and `inspect`. Plan mode (`--plan`,
`--driver none`) prints steps without driving a browser.

## Driver matrix

| Driver | Kind | Install |
|---|---|---|
| `agent-browser >=0.43.0` | run (default) | `npm i -g agent-browser && agent-browser install` |
| `browse` | run (your logged-in Chrome) | install `browse`, pair extension (`browse status --json`) |
| `lightpanda` | run, experimental | `curl -fsSL https://pkg.lightpanda.io/install.sh \| bash -s "1.0.4"` |
| `none` / `--plan` | plan-only | no install; prints driver-agnostic steps |

`driver: auto` tries `agent-browser`, then `browse` (when its
extension is connected), then `lightpanda`, else returns `no_driver`.
Naming a driver never silently falls back to another.

## Session and run flags

- `--driver <auto|agent-browser|browse|lightpanda|none>`
- `--session <name>` (default `agent-webmcp`): reuse a logged-in
  `agent-browser` session across calls.
- `--fresh`: throwaway `agent-browser` session closed after the call
  (`browse` always opens and closes a background tab per call).
- `--headed`: show the browser window.
- `--timeout <ms>`: max milliseconds per call (default `60000`).
- `--settle <ms>`: wait for late tool registration (default `8000`,
  `0` waits not at all, clamped to `30000`).

Config keys live under `drivers.agent-browser` and `call`
(see `--ref config`).

## Manual driver commands

Use these only with `--plan` output or when calling a driver directly:

| Driver | List | Call |
|---|---|---|
| agent-browser 0.43+ | `agent-browser webmcp list` | `agent-browser webmcp invoke <tool> --params '{...}'` |
| OpenCode Browse | `browse webmcp --tab <id> --action list` | `browse webmcp --tab <id> --action call --name <tool> --input '{...}'` |
| Lightpanda | `navigator.modelContext.getTools()` | `page.callPageTool({name, arguments})` |

Check readiness any time with `agent-webmcp status`.
