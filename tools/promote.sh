#!/bin/sh
# Promote a tagged release to everyone, in the one safe order:
#   1. publish the draft GitHub release (curl installs and script upgrades
#      start getting it)
#   2. move npm's `latest` to it (npm installs and npm upgrades)
#   3. republish the index pointer, whose latest_cli follows npm `latest`
#      (users see "run: agent-webmcp upgrade" only once both installs work)
#
#   tools/promote.sh 0.2.2
#
# Needs: gh (repo write), the npm token at ~/.config/agent-webmcp/npm-ci-token,
# and the KV access tools/publish-index.mjs --kv uses.
set -eu
V="${1:?usage: tools/promote.sh <version>}"
V="${V#v}"
REPO=orkestrate-dot-space/agent-webmcp
PKG=@orkestrate/agent-webmcp
cd "$(dirname "$0")/.."

for p in "" -linux-x64 -linux-arm64 -darwin-arm64 -darwin-x64 -win32-x64; do
  got="$(npm view --prefer-online "$PKG$p@$V" version 2>/dev/null || true)"
  [ "$got" = "$V" ] || { echo "npm has no $PKG$p@$V yet (replication can lag a minute)"; exit 1; }
done

echo "1/3 GitHub release v$V"
gh release edit "v$V" --repo "$REPO" --draft=false --latest
curl -fsSL -o /dev/null "https://github.com/$REPO/releases/download/v$V/install.sh"

echo "2/3 npm latest -> $V"
rc="$(mktemp)"
trap 'rm -f "$rc"' EXIT
umask 077
printf '//registry.npmjs.org/:_authToken=%s\n' "$(cat ~/.config/agent-webmcp/npm-ci-token)" > "$rc"
npm --userconfig "$rc" dist-tag add "$PKG@$V" latest

echo "3/3 index pointer latest_cli=$V"
node tools/publish-index.mjs --kv --latest-cli="$V"

echo "promoted $V. Snapshot worker/public/index.json in a PR."
