#!/bin/bash
# Scan one submission and post the result back to the intake Worker.
#   scan-job.sh <id> <entry> <callback-url> <token>
set -u
ID="${1:?id}"
ENTRY="${2:?entry}"
CALLBACK="${3:?callback}"
TOKEN="${4:?token}"

OUT="/var/lib/awm-scan/jobs/$ID"
mkdir -p "$OUT"
/opt/awm-scan/run-scan.sh "$ENTRY" "$OUT" 3

if [ ! -s "$OUT/result.json" ]; then
  # The scanner crashed before printing a record. Send a valid error record so
  # the request completes instead of waiting for the timeout.
  HOST=$(echo "$ENTRY" | cut -d/ -f1 | sed 's/^www\.//')
  printf '{"host":"%s","status":"error","checked_at":"%s","pages_checked":[],"tools":[],"note":"the scan produced no record","missed":0,"engines":["agent-browser"]}\n' \
    "$HOST" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$OUT/result.json"
fi

curl -sS -m 90 -X POST "$CALLBACK?id=$ID&token=$TOKEN" \
  -H 'content-type: application/json' \
  --data-binary "@$OUT/result.json" >/dev/null 2>&1
