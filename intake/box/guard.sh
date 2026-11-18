#!/bin/bash
# Egress guard for the unprivileged `scanner` user: blocks private, loopback,
# link-local, v4-mapped and metadata ranges on IPv4 and IPv6, so a redirect
# or a rebound DNS answer cannot make the browser fetch them.
#
# Used two ways:
#   setup-box.sh sources this file and calls apply_guard (needs sudo);
#   the job start command runs `sudo /opt/awm-scan/guard.sh`, which applies
#   every rule idempotently and then verifies each one with iptables -C /
#   ip6tables -C. If any rule is still missing it exits 40 with a message,
#   and the job never hands off to the scanner user (fail closed: the
#   request stays scanning, the Worker pump retries, /health flags it).
set -eu

GUARD_USER="${GUARD_USER:-scanner}"
GUARD_FAIL_CODE=40

# Loopback needs an exception. agent-browser talks to Chrome over a loopback
# CDP port that Chrome picks at random from the ephemeral range, so blocking
# 127.0.0.0/8 or ::1/128 outright makes every launch fail with "Connection
# refused". Allow loopback only to that range; the REJECT below still covers
# the box's own services on every other port.
guard4() { # guard4 <net> <accept-ephemeral: 0|1>
  local net="$1"
  if [ "$2" = 1 ]; then
    sudo iptables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -p tcp --dport 32768:65535 -j ACCEPT 2>/dev/null \
      || sudo iptables -I OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -p tcp --dport 32768:65535 -j ACCEPT
  fi
  sudo iptables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -j REJECT 2>/dev/null \
    || sudo iptables -A OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -j REJECT
}

# Same treatment on v6: ::1/128 mirrors 127.0.0.0/8 (CDP loopback), and the
# v6 private/link-local/v4-mapped ranges are rejected like their v4 cousins.
guard6() { # guard6 <net> <accept-ephemeral: 0|1>
  local net="$1"
  if [ "$2" = 1 ]; then
    sudo ip6tables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -p tcp --dport 32768:65535 -j ACCEPT 2>/dev/null \
      || sudo ip6tables -I OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -p tcp --dport 32768:65535 -j ACCEPT
  fi
  sudo ip6tables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -j REJECT 2>/dev/null \
    || sudo ip6tables -A OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -j REJECT
}

apply_guard() {
  local net
  for net in 169.254.0.0/16 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16; do guard4 "$net" 0; done
  guard4 127.0.0.0/8 1
  for net in fe80::/10 fc00::/7 ::ffff:0:0/96; do guard6 "$net" 0; done
  guard6 ::1/128 1
}

verify_guard() {
  # Prints each missing rule; returns nonzero when anything is missing.
  local missing=0 net
  for net in 169.254.0.0/16 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 127.0.0.0/8; do
    if ! sudo iptables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -j REJECT 2>/dev/null; then
      echo "guard: missing iptables rule for $net" >&2
      missing=1
    fi
  done
  if ! sudo iptables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d 127.0.0.0/8 -p tcp --dport 32768:65535 -j ACCEPT 2>/dev/null; then
    echo "guard: missing iptables CDP exception for 127.0.0.0/8" >&2
    missing=1
  fi
  for net in fe80::/10 fc00::/7 ::ffff:0:0/96 ::1/128; do
    if ! sudo ip6tables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d "$net" -j REJECT 2>/dev/null; then
      echo "guard: missing ip6tables rule for $net" >&2
      missing=1
    fi
  done
  if ! sudo ip6tables -C OUTPUT -m owner --uid-owner "$GUARD_USER" -d ::1/128 -p tcp --dport 32768:65535 -j ACCEPT 2>/dev/null; then
    echo "guard: missing ip6tables CDP exception for ::1/128" >&2
    missing=1
  fi
  return "$missing"
}

# Executed (not sourced): heal, then verify; fail closed.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  apply_guard
  if ! verify_guard; then
    echo "guard: egress rules could not be applied; refusing to scan" >&2
    exit "$GUARD_FAIL_CODE"
  fi
  echo "guard: ok"
fi
