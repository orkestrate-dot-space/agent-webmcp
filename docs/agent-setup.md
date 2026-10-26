# Set up agent-webmcp for your user

Paste this into your agent. It installs the engine, wires your harness,
and verifies the install. No `install.sh` command needed on your part —
the agent runs every step and reports back.

```
Set up agent-webmcp for me by following https://raw.githubusercontent.com/orkestrate-dot-space/agent-webmcp/main/docs/agent-setup.md
```

## 1. Check the platform and Bun

```sh
uname -s
bun --version
```

- Continue on macOS (`Darwin`) or Linux. Stop on anything else and say so.
- Continue if `bun --version` prints `1.x`. Otherwise stop and ask: the
  user installs Bun themselves (https://bun.sh), you never install
  runtimes unasked.

## 2. Get the source and build it

```sh
git clone https://github.com/orkestrate-dot-space/agent-webmcp ~/agent-webmcp
cd ~/agent-webmcp && ./install.sh
```

- The script refuses on a dirty tree; a fresh clone is never dirty.
- It builds `dist/`, installs `~/.local/bin/agent-webmcp`, and syncs
  `~/.local/share/agent-webmcp/skills/`. If live MCP servers hold the old
  binary, the script asks them to exit once and retries — they respawn on
  next use.
- The last line prints the MCP entry and the verify command. Keep both.

## 3. Wire your harness

Add the entry the script printed to the client you run in. Ask the user
if you aren't sure which client that is.

**opencode** (`~/.config/opencode/opencode.json`):

```json
{ "mcp": { "agent-webmcp": {
  "type": "local",
  "command": ["/home/YOU/.local/bin/agent-webmcp", "mcp", "serve"]
} } }
```

**Claude Code**: `claude mcp add --scope user agent-webmcp -- /home/YOU/.local/bin/agent-webmcp mcp serve`

**Any MCP client**: stdio server, command `/home/YOU/.local/bin/agent-webmcp`,
args `["mcp", "serve"]`. No environment variables, no auth — the OS user
is the auth.

Ask before writing user-level or global config the user didn't mention.
Tell the user what you changed when you finish.

## 4. Reach a browser (driver's job, your check)

The engine never launches browsers. It attaches to one your driver owns:

- Preferred: an agent-browser daemon (`agent-browser open about:blank`
  in your driver's session). Daemon discovery is the default attach path.
- Any Chromium with `--remote-debugging-port` plus WebMCP flags works.
  Desktop chat panels need `--window-size=1366,900`.

Your driver's own skill owns its setup flags — read that skill, don't
guess them. Then hand its endpoint to `open` as `cdp`, or omit `cdp`
and let discovery find the daemon.

## 5. Verify (green means done)

```sh
~/.local/bin/agent-webmcp doctor
```

- `browser.reachable: true` — a browser answered. If false, the `fix`
  line names the exact next step (start a daemon or pass `--cdp`).
- Then, from your harness: `skill action=show`. It returns the drive
  loop. `skill action=search` with no query lists installed agents.
- Report all three to the user: binary version, `doctor`, and the
  `skill show` head. Stop here — authoring starts with recon, not setup.
