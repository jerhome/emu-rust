#!/usr/bin/env bash
# System resources for project follow-up (Linux/WSL2 and macOS). Read-only.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

count() { pgrep -x "$1" 2>/dev/null | wc -l | tr -d ' '; }
running() { pgrep -x "$1" >/dev/null 2>&1 && echo "en cours" || echo "arrêté"; }

echo "Ressources système — $(date '+%Y-%m-%d %H:%M')"
if [ "$(uname)" = Darwin ]; then
  read -r _ l1 l5 l15 _ < <(sysctl -n vm.loadavg)
  echo "  CPU     : charge $l1 / $l5 / $l15 (1/5/15 min) sur $(sysctl -n hw.ncpu) threads"
  total=$(( $(sysctl -n hw.memsize) / 1048576 ))
  # Available ≈ free + inactive + speculative pages.
  avail=$(vm_stat | awk -v ps="$(pagesize)" '/Pages (free|inactive|speculative)/ {gsub("\\.","",$NF); s+=$NF} END {printf "%d", s*ps/1048576}')
  used=$(( total - avail ))
  echo "  Mémoire : $used / $total Mo utilisés ($(( used * 100 / total )) %), $avail Mo disponibles"
  sysctl -n vm.swapusage | awk '{printf "  Swap    : %s / %s\n", $6, $3}'
  # Degrees need an SMC reader (brew install osx-cpu-temp); throttling via pmset needs no privileges.
  if command -v osx-cpu-temp >/dev/null; then
    fan=$(osx-cpu-temp -f 2>/dev/null | awk '/Fan 0/ {r=$(NF-2); print (r < 500 ? "illisible" : r " tr/min")}')
    echo "  Temp.   : CPU $(osx-cpu-temp -c 2>/dev/null)${fan:+, ventilateur $fan}"
  else
    echo "  Temp.   : n/d (installer : brew install osx-cpu-temp)"
  fi
  lim=$(pmset -g therm | awk -F= '/CPU_Speed_Limit/ {gsub(/ /,"",$2); print $2}')
  [ -n "$lim" ] && { [ "$lim" -lt 100 ] && echo "  Thermique : ⚠ CPU bridé à $lim % (surchauffe)" || echo "  Thermique : pas de bridage (vitesse 100 %)"; }
  top_cmd() { ps -Ao pcpu,pmem,comm -r | sed -n '2,4p'; }
else
  read -r l1 l5 l15 _ < /proc/loadavg
  echo "  CPU     : charge $l1 / $l5 / $l15 (1/5/15 min) sur $(nproc) cœurs"
  free -m | awk '/^Mem:/  {printf "  Mémoire : %d / %d Mo utilisés (%d %%), %d Mo disponibles\n", $3, $2, $3*100/$2, $7}
                 /^Swap:/ {if ($2>0) printf "  Swap    : %d / %d Mo\n", $3, $2}'
  if command -v sensors >/dev/null; then
    sensors 2>/dev/null | awk '/^(Package id 0|Tctl|CPU):/ {print "  Temp.   : CPU " $NF; exit}' | sed 's/[()]//g'
  elif t=$(cat /sys/class/thermal/thermal_zone*/temp 2>/dev/null | sort -n | tail -1) && [ -n "$t" ]; then
    echo "  Temp.   : $(( t / 1000 )) °C (zone la plus chaude)"
  else
    echo "  Temp.   : n/d (WSL/conteneur : voir l'hôte)"
  fi
  top_cmd() { ps -eo pcpu,pmem,comm --sort=-pcpu | sed -n '2,4p'; }
fi
df -h "$ROOT" | awk 'NR==2 {printf "  Disque  : %s / %s utilisés (%s), %s libres\n", $3, $2, $5, $4}'
[ -d "$ROOT/target" ] && echo "  target/ : $(du -sh "$ROOT/target" 2>/dev/null | cut -f1)"
echo "  cargo   : $(count cargo) processus"
echo "  rustc   : $(count rustc) processus"
echo "  Xvfb    : $(running Xvfb)"
echo "  emu-rust: $(running emu-rust)"
echo "  Top processus (CPU) :"
top_cmd | awk '{c=$3; for(i=4;i<=NF;i++) c=c" "$i; n=split(c,p,"/"); printf "            %5s %% CPU  %5s %% RAM  %s\n", $1, $2, p[n]}'
