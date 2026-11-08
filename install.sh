#!/bin/sh
# Install agent-webmcp: binary + init.js + skills into ~/.local.
set -e
ROOT="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"
SHARE="$HOME/.local/share/agent-webmcp"
bun build --compile "$ROOT/src/cli.ts" --outfile "$ROOT/dist/agent-webmcp"
mkdir -p "$BIN" "$SHARE/skills" "$SHARE/sites"
# mv, not cp: the old binary may be running (ETXTBSY on write, rename is fine).
mv -f "$ROOT/dist/agent-webmcp" "$BIN/agent-webmcp"
cp "$ROOT/init.js" "$SHARE/init.js"
cp "$ROOT/init.js" "$BIN/agent-webmcp-init.js"
if [ -d "$ROOT/registry" ]; then cp "$ROOT/registry/index.json" "$SHARE/index.json"; fi
if [ -d "$ROOT/registry/sites" ]; then cp "$ROOT/registry/sites/"*.js "$SHARE/sites/" 2>/dev/null || true; fi
cp "$ROOT/skills/agent-webmcp/SKILL.md" "$SHARE/skills/SKILL.md"
mkdir -p "$SHARE/skills/agent-webmcp"
cp "$ROOT/skills/agent-webmcp/SKILL.md" "$SHARE/skills/agent-webmcp/SKILL.md"
"$BIN/agent-webmcp" init
echo "installed: $BIN/agent-webmcp"
