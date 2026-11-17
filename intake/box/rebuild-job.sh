#!/bin/bash
# Scan one shard of the weekly rebuild and post the partial index back.
#   rebuild-job.sh <id> <callback-url> <token> <worker-url> <pointer-url> <shard> <count>
set -u
ID="${1:?id}"
CALLBACK="${2:?callback}"
TOKEN="${3:?token}"
WORKER="${4:?worker url}"
POINTER="${5:?pointer url}"
SHARD="${6:?shard}"
COUNT="${7:?count}"

WORK="/var/lib/awm-scan/rebuild/$ID-$SHARD"
mkdir -p "$WORK"
cd /opt/awm-scan || exit 90
export AGENT_BROWSER_ARGS="--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport"
export HOME=/home/scanner

curl -sS -m 60 "$WORKER/seeds.txt?shard=$SHARD&count=$COUNT" -o "$WORK/seeds.txt" || exit 91
[ -s "$WORK/seeds.txt" ] || exit 91

# The previous index lets one missed check keep a site's tools.
if curl -sS -m 60 "$POINTER" -o "$WORK/pointer.json"; then
  URL=$(node -e 'const p=require(process.argv[1]);console.log(new URL(p.url,process.argv[2]).href)' \
    "$WORK/pointer.json" "$POINTER" 2>/dev/null)
  if [ -n "${URL:-}" ]; then
    curl -sS -m 120 "$URL" -o "$WORK/previous.json" || rm -f "$WORK/previous.json"
  fi
fi

PREV_ARG=""
[ -s "$WORK/previous.json" ] && PREV_ARG="--previous $WORK/previous.json"

# A rebuild can lean on the carry rule: one fast pass, no extra looks.
node tools/build-index/build.mjs \
  --seeds "$WORK/seeds.txt" \
  --pages 2 \
  --workers 4 \
  --retries 0 \
  --settle 1200 \
  --skip-networkidle \
  --engines agent-browser \
  $PREV_ARG \
  --out "$WORK/out.json" >"$WORK/build.log" 2>&1 || exit 92

curl -sS -m 300 -X POST "$CALLBACK?token=$TOKEN&shard=$SHARD" \
  -H 'content-type: application/json' \
  --data-binary "@$WORK/out.json" >/dev/null 2>&1
