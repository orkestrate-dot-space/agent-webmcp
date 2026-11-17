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
export AWM_SESSION="awm-rebuild-$ID-$SHARD"

cleanup() {
  for n in 0 1 2 3; do
    agent-browser --session "$AWM_SESSION-$n" close >/dev/null 2>&1 || true
  done
  pkill -P $$ >/dev/null 2>&1 || true
  rm -f "$HOME/.agent-browser/$AWM_SESSION"* >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 143' TERM INT

curl -sS -m 60 "$WORKER/seeds.txt?shard=$SHARD&count=$COUNT" -o "$WORK/seeds.txt" || exit 91
[ -s "$WORK/seeds.txt" ] || exit 91

# The previous index lets one missed check keep a site's tools. If the live
# index exists but cannot be read, fail the shard: publishing without history
# would delist every site that misses one check.
if curl -sS -m 60 "$POINTER" -o "$WORK/pointer.json"; then
  URL=$(node -e 'const p=require(process.argv[1]);console.log(new URL(p.url,process.argv[2]).href)' \
    "$WORK/pointer.json" "$POINTER" 2>/dev/null)
  if [ -z "${URL:-}" ]; then
    echo "pointer has no usable url" >&2
    exit 93
  fi
  curl -sS -m 120 "$URL" -o "$WORK/previous.json" || exit 93
  [ -s "$WORK/previous.json" ] || exit 93
else
  echo "no published pointer; rebuilding without history" >&2
fi

PREV_ARG=""
[ -s "$WORK/previous.json" ] && PREV_ARG="--previous $WORK/previous.json"

# A rebuild can lean on the carry rule: one fast pass, no extra looks.
node tools/build-index/build.mjs \
  --seeds "$WORK/seeds.txt" \
  --pages 2 \
  --workers 4 \
  --retries 0 \
  --engines agent-browser \
  $PREV_ARG \
  --out "$WORK/out.json" >"$WORK/build.log" 2>&1 || exit 92

curl -sS -m 300 -X POST "$CALLBACK?token=$TOKEN&shard=$SHARD" \
  -H 'content-type: application/json' \
  --data-binary "@$WORK/out.json" >/dev/null 2>&1
