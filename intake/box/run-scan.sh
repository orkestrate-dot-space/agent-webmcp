#!/bin/bash
# Scan one entry and write result.json + check.log into the given directory.
#   run-scan.sh <entry> <out-dir> [pages]
# Runs as an unprivileged scanner user: no credentials, its own HOME.
set -u
ENTRY="${1:?entry}"
OUT="${2:?out dir}"
PAGES="${3:-3}"
mkdir -p "$OUT"
cd /opt/awm-scan || exit 90
export AGENT_BROWSER_ARGS="--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport"
export HOME=/home/scanner
node tools/check-site.mjs "$ENTRY" --pages "$PAGES" > "$OUT/result.json" 2> "$OUT/check.log"
CODE=$?
echo "exit=$CODE"
tail -3 "$OUT/check.log"
