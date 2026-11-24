#!/usr/bin/env bash
# Read-only check: which SNES-like USB pads (VID:PID) have an entry in the SDL_GameControllerDB
# bundled by gilrs, per platform. Usage: check-sdl-pads.sh [VID:PID ...]
#
# SDL GUID layout (32 hex chars, bytes little-endian): BBBB CCCC VVVV 0000 PPPP 0000 xxxx xxxx
#   chars 0-7  bus + CRC (ignored here), chars 8-11 VID LE, 12-15 zero, chars 16-19 PID LE.
# Example 0079:0011 -> 0300 0000 7900 0000 1100 0000 ... ; match regex: ^.{8}<vid_le>0000<pid_le>
set -euo pipefail

db=$(ls ~/.cargo/registry/src/*/gilrs-*/SDL_GameControllerDB/gamecontrollerdb.txt 2>/dev/null | sort | tail -n1 || true)
[ -n "$db" ] || { echo "gamecontrollerdb.txt introuvable dans ~/.cargo/registry/src/*/gilrs-*/" >&2; exit 1; }
echo "Base : $db"

pads=("$@")
[ ${#pads[@]} -gt 0 ] || pads=(0079:0011 0810:e501 081f:e401 12bd:d015)

le() { printf '%s%s' "${1:2:2}" "${1:0:2}"; }  # 4 hex digits -> little-endian byte order

printf '%-10s %-8s %-10s %-6s\n' "VID:PID" "Windows" "Mac OS X" "Linux"
for p in "${pads[@]}"; do
  p=$(printf %s "$p" | tr A-F a-f)
  vid=$(le "${p%%:*}"); pid=$(le "${p##*:}")
  row=()
  for plat in Windows "Mac OS X" Linux; do
    if grep -v '^#' "$db" | grep -i -E "^.{8}${vid}0000${pid}" | grep -q "platform:${plat},\?"; then
      row+=(oui)
    else
      row+=(non)
    fi
  done
  printf '%-10s %-8s %-10s %-6s\n' "$p" "${row[@]}"
done
