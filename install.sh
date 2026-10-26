#!/usr/bin/env sh
# agent-webmcp installer: binary + skills, nothing else. No config, no
# browser stages (drivers own browsers), no writes beyond the two targets.
# Interactive when TTY, env-driven otherwise. Refuses on a dirty tree when
# run from a checkout (set INSTALL_DIRTY_OK=1 to override).
set -eu

BIN_DIR="${INSTALL_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${INSTALL_DATA_DIR:-$HOME/.local/share/agent-webmcp}"
SRC_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

if [ -e "$SRC_DIR/.git" ] && [ -z "${INSTALL_DIRTY_OK:-}" ]; then
  if git -C "$SRC_DIR" status --porcelain | grep -q .; then
    echo "refusing: dirty tree at $SRC_DIR (commit it, or set INSTALL_DIRTY_OK=1)" >&2
    exit 1
  fi
fi

need() { command -v "$1" >/dev/null 2>&1 || { echo "need $1 on PATH" >&2; exit 1; }; }
need bun

echo "building agent-webmcp..."
(cd "$SRC_DIR" && bun run compile)

mkdir -p "$BIN_DIR" "$DATA_DIR/skills"
for i in 1 2 3 4 5 6; do
  if cp "$SRC_DIR/dist/agent-webmcp" "$BIN_DIR/agent-webmcp" 2>/dev/null; then break; fi
  # Live MCP servers hold the old binary (Text file busy): ask them to
  # exit once, then the copy lands. They respawn on next use.
  fuser -k "$BIN_DIR/agent-webmcp" >/dev/null 2>&1
  sleep 2
done
cmp -s "$SRC_DIR/dist/agent-webmcp" "$BIN_DIR/agent-webmcp" \
  || { echo "installed binary differs from dist (holders?) — retry install" >&2; exit 1; }
chmod +x "$BIN_DIR/agent-webmcp"
rm -rf "$DATA_DIR/skills/agent-webmcp"
cp -r "$SRC_DIR/skills/." "$DATA_DIR/skills/"

"$BIN_DIR/agent-webmcp" --version
echo "installed: $BIN_DIR/agent-webmcp + $DATA_DIR/skills"
echo "skills served from: $DATA_DIR/skills (what the binary serves — reinstall after skill edits)"
echo "wire your harness (opencode example):"
echo "{ \"mcp\": { \"agent-webmcp\": { \"type\": \"local\", \"command\": [\"$BIN_DIR/agent-webmcp\", \"mcp\", \"serve\"] } } }"
echo "then verify: $BIN_DIR/agent-webmcp doctor"
