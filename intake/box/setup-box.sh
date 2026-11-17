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
sudo chmod 755 /opt/awm-scan/*.sh
sudo chown -R root:root /opt/awm-scan
sudo chmod -R a+rX /opt/awm-scan

sudo chown -R "$USER_NAME:$USER_NAME" /var/lib/awm-scan

# Network guard: Chrome runs as the scanner user, so these rules cover every
# fetch it makes, including one reached by a redirect.
#
# Loopback needs care. agent-browser talks to Chrome over a loopback CDP port
# that Chrome picks at random from the ephemeral range, so blocking 127.0.0.0/8
# outright makes every launch fail with "Connection refused". Allow loopback
# only to that range, and reject it below, where the box's own services live.
guard() { # guard <net> <accept-ephemeral: 0|1>
  net="$1"
  if [ "$2" = 1 ]; then
    sudo iptables -C OUTPUT -m owner --uid-owner "$USER_NAME" -d "$net" -p tcp --dport 32768:65535 -j ACCEPT 2>/dev/null \
      || sudo iptables -I OUTPUT -m owner --uid-owner "$USER_NAME" -d "$net" -p tcp --dport 32768:65535 -j ACCEPT
  fi
  sudo iptables -C OUTPUT -m owner --uid-owner "$USER_NAME" -d "$net" -j REJECT 2>/dev/null \
    || sudo iptables -A OUTPUT -m owner --uid-owner "$USER_NAME" -d "$net" -j REJECT
}
for net in 169.254.0.0/16 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16; do guard "$net" 0; done
guard 127.0.0.0/8 1

echo "scanner ready: $(sudo -u "$USER_NAME" bash -lc 'node --version')"
