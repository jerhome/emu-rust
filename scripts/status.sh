#!/usr/bin/env bash
# System resources (WSL2) for project follow-up. Read-only.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "Ressources système — $(date '+%Y-%m-%d %H:%M')"
read -r l1 l5 l15 _ < /proc/loadavg
echo "  CPU     : charge $l1 / $l5 / $l15 (1/5/15 min) sur $(nproc) cœurs"
free -m | awk '/^Mem:/  {printf "  Mémoire : %d / %d Mo utilisés (%d %%), %d Mo disponibles\n", $3, $2, $3*100/$2, $7}
               /^Swap:/ {if ($2>0) printf "  Swap    : %d / %d Mo\n", $3, $2}'
df -h "$ROOT" | awk 'NR==2 {printf "  Disque  : %s / %s utilisés (%s), %s libres\n", $3, $2, $5, $4}'
[ -d "$ROOT/target" ] && echo "  target/ : $(du -sh "$ROOT/target" 2>/dev/null | cut -f1)"
n=$(pgrep -c -x cargo || true); echo "  cargo   : ${n:-0} processus"
n=$(pgrep -c -x rustc || true); echo "  rustc   : ${n:-0} processus"
pgrep -x Xvfb >/dev/null && echo "  Xvfb    : en cours" || echo "  Xvfb    : arrêté"
pgrep -x emu-rust >/dev/null && echo "  emu-rust: en cours" || echo "  emu-rust: arrêté"
echo "  Top processus (CPU) :"
ps -eo pcpu,pmem,comm --sort=-pcpu | sed -n '2,4p' | awk '{printf "            %5s %% CPU  %5s %% RAM  %s\n", $1, $2, $3}'
