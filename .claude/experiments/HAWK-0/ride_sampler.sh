#!/bin/sh
# HAWK-1b ride trace without DLL changes: once a developer game with the console is up, send `pos_save` once a second (it only
# records the position values to nrpvp.log, it moves nothing) until the game exits or MAX seconds pass.
# Usage: ride_sampler.sh <repo> [max_seconds=1500]
REPO="$1"; MAX="${2:-1500}"; NRCON="$REPO/dist/nrcon.exe"
i=0
until "$NRCON" status >/dev/null 2>&1; do
    i=$((i + 5)); [ "$i" -ge "$MAX" ] && { echo "no game after $MAX s"; exit 1; }
    sleep 5
done
echo "game up after ${i}s; sampling"
n=0
while "$NRCON" status >/dev/null 2>&1 && [ "$i" -lt "$MAX" ]; do
    "$NRCON" pos_save >/dev/null 2>&1 && n=$((n + 1))
    sleep 1; i=$((i + 1))
done
echo "stopped after ${i}s, $n samples"
