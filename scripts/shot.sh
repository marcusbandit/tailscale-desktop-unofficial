#!/usr/bin/env bash
# Renders the app inside a headless gamescope and saves a PNG to captures/.
# Usage: scripts/shot.sh NAME [VAR=value ...]   e.g. scripts/shot.sh graph TSDU_VIEW=graph
set -euo pipefail
cd "$(dirname "$0")/.."
name=${1:-devices}; shift || true
out="$PWD/captures/$name.png"
rm -f "$out"
before=$(ls /run/user/$UID | grep -E '^gamescope-[0-9]+$' | sort || true)
gamescope --backend headless -W 1400 -H 860 --expose-wayland -- sleep 60 >/tmp/tsdu-gamescope.log 2>&1 &
gs=$!
trap 'kill $gs 2>/dev/null || true' EXIT
for _ in $(seq 50); do
  disp=$(comm -13 <(echo "$before") <(ls /run/user/$UID | grep -E '^gamescope-[0-9]+$' | sort) | tail -1)
  [ -n "$disp" ] && break
  sleep 0.1
done
[ -n "${disp:-}" ] || { echo "gamescope did not start" >&2; exit 1; }
env -u DISPLAY WAYLAND_DISPLAY="$disp" TSDU_SHOT="$out" "$@" timeout 40 target/${PROFILE:-debug}/tailscale-desktop-unofficial
file "$out" | grep -q PNG && echo "$out"
