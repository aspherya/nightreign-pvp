#!/usr/bin/env bash
# Dev loop for puppet/orchestrator work: stop the session, rebuild this checkout's dev DLL, launch it offline and press through to the
# Roundtable. Needs `nro desk` running in a normal terminal. Usage: tools/cycle.sh [extra nro launch args]
set -e
here="$(cd "$(dirname "$0")/.." && pwd)"
"$here/target/debug/nro.exe" stop >/dev/null 2>&1 || true
(cd "$here" && powershell -NoProfile -File scripts/build.ps1 | tail -1)
"$here/target/debug/nro.exe" launch --world --set 'mode="offline"' "$@" | grep -E '"launched"|world loaded|refused|error' || true
