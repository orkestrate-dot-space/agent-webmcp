#!/bin/bash
# Scan one entry and write result.json + check.log into the given directory.
#   run-scan.sh <entry> <out-dir> [pages] [extras] [job-id]
# Runs as the unprivileged scanner user: no credentials. Each job has its own
# agent-browser session (named from the job id) and cleans up only what it
# started, so a killed or crashed job cannot break the next one and two jobs
# on one box cannot touch each other.
set -u
ENTRY="${1:?entry}"
OUT="${2:?out dir}"
PAGES="${3:-3}"
EXTRAS="${4:-}"
# Session names allow only letters, digits, hyphens and underscores.
JOB="$(printf '%s' "${5:-$$}" | tr -c 'A-Za-z0-9_-' '_')"

mkdir -p "$OUT"
cd /opt/awm-scan || exit 90
export AGENT_BROWSER_ARGS="--enable-features=WebMCP,WebMCPTesting,DevToolsWebMCPSupport"
export HOME=/home/scanner
export AWM_SESSION="awm-$JOB"

cleanup() {
  agent-browser --session "$AWM_SESSION" close >/dev/null 2>&1 || true
  # Only this job's own children, never another job's browser.
  pkill -P $$ >/dev/null 2>&1 || true
  rm -f "$HOME/.agent-browser/$AWM_SESSION"* >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 143' TERM INT

# A leftover session of this same job id (a retry) starts clean.
agent-browser --session "$AWM_SESSION" close >/dev/null 2>&1 || true
rm -f "$HOME/.agent-browser/$AWM_SESSION"* >/dev/null 2>&1 || true

node tools/check-site.mjs "$ENTRY" --pages "$PAGES" --extra "$EXTRAS" > "$OUT/result.json" 2> "$OUT/check.log"
CODE=$?
echo "exit=$CODE"
tail -3 "$OUT/check.log"
exit "$CODE"
