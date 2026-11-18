#!/bin/sh
# Install agent-webmcp from GitHub releases (Linux and macOS).
#
#   curl -fsSL https://github.com/orkestrate-dot-space/agent-webmcp/releases/latest/download/install.sh | sh
#
# Options (flags after `sh -s --`, or environment variables):
#   --version X     AGENT_WEBMCP_VERSION       exact version, default latest
#   --dir PATH      AGENT_WEBMCP_INSTALL_DIR   default ~/.local/bin
#
# The download is checked against the release's SHA256SUMS before anything
# is installed. Nothing outside the install dir and agent-webmcp's own data
# dir is touched; no shell profile is edited.
set -eu

REPO="orkestrate-dot-space/agent-webmcp"
VERSION="${AGENT_WEBMCP_VERSION:-latest}"
DIR="${AGENT_WEBMCP_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}"

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --dir) DIR="${2:?--dir needs a value}"; shift 2 ;;
    --dir=*) DIR="${1#*=}"; shift ;;
    -h|--help) sed -n '2,13p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "agent-webmcp install: unknown option $1" >&2; exit 2 ;;
  esac
done

say() { printf '%s\n' "$*"; }
die() { printf 'agent-webmcp install: %s\n' "$*" >&2; exit 1; }

VERSION="${VERSION#v}"
case "$VERSION" in
  latest) ;;
  [0-9]*.[0-9]*.[0-9]*)
    case "$VERSION" in *[!0-9A-Za-z.+-]*) die "not a version: $VERSION" ;; esac ;;
  *) die "not a version: $VERSION (use latest or an exact version like 0.2.2)" ;;
esac

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) die "unsupported OS $(uname -s). On Windows, use: npm i -g @orkestrate/agent-webmcp" ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x64 ;;
  aarch64|arm64) arch=arm64 ;;
  *) die "unsupported CPU $(uname -m)" ;;
esac
# An x64 shell under Rosetta on Apple Silicon still gets the native build.
if [ "$os" = darwin ] && [ "$arch" = x64 ] \
  && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
  arch=arm64
fi
asset="agent-webmcp-$os-$arch.tar.gz"

if [ "$VERSION" = latest ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/v$VERSION"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q --https-only -O "$2" "$1"; }
else
  die "needs curl or wget"
fi

if command -v sha256sum >/dev/null 2>&1; then
  sha() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "needs sha256sum or shasum to verify the download"
fi

tmp="$(mktemp -d 2>/dev/null || mktemp -d -t agent-webmcp)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading $asset ($VERSION)"
fetch "$base/$asset" "$tmp/$asset" || die "download failed: $base/$asset"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "download failed: $base/SHA256SUMS"

want="$(awk -v f="$asset" '$2 == f || $2 == "*"f { print $1 }' "$tmp/SHA256SUMS")"
[ -n "$want" ] || die "SHA256SUMS has no entry for $asset"
got="$(sha "$tmp/$asset")"
[ "$want" = "$got" ] || die "checksum mismatch for $asset (expected $want, got $got)"

tar -xzf "$tmp/$asset" -C "$tmp"
[ -f "$tmp/agent-webmcp" ] || die "$asset does not contain agent-webmcp"

mkdir -p "$DIR"
# Copy next to the target, then rename: a running agent-webmcp (the MCP
# server) keeps its old file and never sees a half-written one.
cp "$tmp/agent-webmcp" "$DIR/.agent-webmcp.new"
chmod 755 "$DIR/.agent-webmcp.new"
mv -f "$DIR/.agent-webmcp.new" "$DIR/agent-webmcp"
# macOS quarantines downloaded files; a binary fetched by curl normally has
# no quarantine flag, but clear it if something added one.
if [ "$os" = darwin ]; then xattr -d com.apple.quarantine "$DIR/agent-webmcp" 2>/dev/null || true; fi

installed="$("$DIR/agent-webmcp" --version 2>/dev/null)" || die "installed, but $DIR/agent-webmcp does not run"

# Receipt: lets `agent-webmcp upgrade` re-run this installer for this copy.
data="${AGENT_WEBMCP_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/agent-webmcp}"
mkdir -p "$data"
printf 'script\n%s\n' "$DIR/agent-webmcp" > "$data/install-receipt"

say "$installed installed to $DIR/agent-webmcp"
case ":${PATH:-}:" in
  *":$DIR:"*) ;;
  *) say ""
     say "$DIR is not on your PATH. Add it, for example:"
     say "  echo 'export PATH=\"$DIR:\$PATH\"' >> ~/.profile" ;;
esac
say ""
say "next: agent-webmcp setup"
