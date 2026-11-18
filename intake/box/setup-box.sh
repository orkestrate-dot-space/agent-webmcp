#!/bin/bash
# Prepare an instabox box to scan strangers' pages. Idempotent; run as the box
# user (it uses sudo), with the repo's tools/ and intake/box/ already copied to
# /data/home.
#
#   setup-box.sh
#
# What it does:
# - creates an unprivileged `scanner` user with its own HOME (no credentials);
# - installs the scanner and job scripts under /opt/awm-scan (root-owned);
# - gives the scanner writable state under /var/lib/awm-scan;
# - blocks the scanner user from private, loopback and metadata ranges at the
#   network layer, so a redirect cannot make the browser fetch them.
set -eu

HERE="${HERE:-/data/home}"
USER_NAME=scanner

sudo useradd -m -s /bin/bash "$USER_NAME" 2>/dev/null || true
sudo mkdir -p /opt/awm-scan /var/lib/awm-scan/jobs /var/lib/awm-scan/rebuild

sudo rm -rf /opt/awm-scan/tools
sudo cp -r "$HERE/tools" /opt/awm-scan/
sudo cp "$HERE"/awm/scan-job.sh "$HERE"/awm/rebuild-job.sh "$HERE"/awm/run-scan.sh /opt/awm-scan/ 2>/dev/null \
  || sudo cp "$HERE"/intake/box/*.sh /opt/awm-scan/
# guard.sh lives next to this script in both layouts; it must always land in
# /opt/awm-scan because the Worker runs it before every job.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
sudo cp "$SCRIPT_DIR/guard.sh" /opt/awm-scan/
sudo chmod 755 /opt/awm-scan/*.sh
sudo chown -R root:root /opt/awm-scan
sudo chmod -R a+rX /opt/awm-scan

sudo chown -R "$USER_NAME:$USER_NAME" /var/lib/awm-scan

# Network guard: Chrome runs as the scanner user, so these rules cover every
# fetch it makes, including one reached by a redirect or a rebound DNS answer.
# The rule set lives in guard.sh so the job start can re-apply it: the rules
# do not survive a reboot.
export GUARD_USER="$USER_NAME"
# shellcheck source=guard.sh
. "$SCRIPT_DIR/guard.sh"
apply_guard

echo "scanner ready: $(sudo -u "$USER_NAME" bash -lc 'node --version')"
