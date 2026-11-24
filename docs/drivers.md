# Driver setup

`agent-webmcp` supports three real run drivers—`agent-browser`, `browse`, and `lightpanda`—all of which execute tools directly via `agent-webmcp call` and also support `call --plan`. Setting `driver: "none"` emits generic driver-agnostic steps.

Install `agent-webmcp` and run first-run setup once:

```bash
curl -fsSL https://web.orkestrate.space/webmcp/install | bash   # Linux, macOS
npm i -g @orkestrate/agent-webmcp                               # any OS

agent-webmcp setup
```

`setup` fetches the index, installs or updates the skill for detected clients, and checks which run drivers are available (printing manual install lines if none are ready). Setup never edits client MCP configs or installs browser drivers automatically.

Run `agent-webmcp status` at any time to see detected drivers, the selected driver, and any active session lock.

## Check a site before you open it

```bash
agent-webmcp find cal.com
```

`find` answers whether a site registers WebMCP tools, which page they're on, and prints a ready-to-run `call` command.

## agent-browser (default run driver)

`agent-browser` `>= 0.43.0` is the primary run driver. `agent-webmcp call` launches it with `--enable-features=WebMCP`, manages a per-session lock so concurrent calls don't collide, waits for late tool registration (`--settle`), and detects blocked pages.

Useful `call` flags and config keys (`drivers.agent-browser` in [docs/config.md](config.md)):

- `--session <name>` (default `"agent-webmcp"`): reuse a named session so logins persist across calls.
- `--fresh`: use a throwaway session closed after the call.
- `--headed`: show the browser window.
- `profile`, `cdp`, `extraArgs` (user config only): attach to an existing Chrome profile or CDP endpoint.

## OpenCode Browse (real Chrome run driver)

`browse` drives your own logged-in Chrome through the `browse` CLI and extension—opening background tabs without stealing focus.

- `agent-webmcp status` checks `browse status --json` and marks `browse` ready when the extension is connected.
- Under `driver: "auto"`, `browse` is used automatically when `agent-browser` is missing and the `browse` extension is connected; or select it explicitly with `--driver browse` / `"driver": "browse"`.
- `browse` opens a background tab per `call`/`inspect` and closes it automatically when finished on a best-effort basis (leaving it open on timeout so an in-flight action is not interrupted, and closure is not guaranteed if the extension disconnects), while Chrome's cookies/session persist across tabs. If tabs stop closing, reinstall the extension path for `browse` versions below 0.2.1 (`browse` CLI `>= 0.2.1` is required for reliable tab cleanup).

## Lightpanda (experimental run driver)

Lightpanda is an experimental headless fallback when `agent-browser` and `browse` are not ready (used automatically by `driver: "auto"`, or explicitly with `--driver lightpanda`).

Install with `curl -fsSL https://pkg.lightpanda.io/install.sh | bash -s "1.0.4"` or configure `drivers.lightpanda.command` in your user config.

## Check it works

Run a search against a real indexed demo site:

```bash
agent-webmcp find "search flights" --json
```

You should see `searchFlights` on `https://googlechromelabs.github.io/webmcp-tools/demos/react-flightsearch/`. Test a call (or `--plan` to inspect steps without running):

```bash
agent-webmcp call googlechromelabs.github.io/searchFlights '{"origin":"SFO","destination":"JFK","tripType":"one-way","outboundDate":"2026-12-01","inboundDate":"","passengers":1}'
```
