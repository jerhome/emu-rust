#!/usr/bin/env bash
# Fails if a Mach-O binary requires a macOS newer than MAX (default 12.0: test Mac on Monterey 12.7.6).
# Reads LC_BUILD_VERSION (minos) or, for deployment targets < 10.14, LC_VERSION_MIN_MACOSX (version).
# Usage: check-macos-minos.sh BINARY [MAX]
set -euo pipefail
bin=${1:?usage: check-macos-minos.sh BINARY [MAX]}
max=${2:-12.0}

otool -l "$bin" | grep -A3 -E 'LC_BUILD_VERSION|LC_VERSION_MIN_MACOSX' || true
minos=$(otool -l "$bin" | awk '
    /cmd LC_BUILD_VERSION|cmd LC_VERSION_MIN_MACOSX/ { want = 1; next }
    # No `exit`: closing the pipe early would SIGPIPE otool and fail under pipefail.
    want && !found && ($1 == "minos" || $1 == "version") { print $2; found = 1 }
')
[[ -n $minos ]] || { echo "error: no minimum macOS version found in $bin" >&2; exit 1; }

# Version compare on major.minor[.patch] without sort -V (absent on stock macOS).
ver_gt() {
    local -a a b
    IFS=. read -r -a a <<< "$1"
    IFS=. read -r -a b <<< "$2"
    for i in 0 1 2; do
        (( ${a[i]:-0} > ${b[i]:-0} )) && return 0
        (( ${a[i]:-0} < ${b[i]:-0} )) && return 1
    done
    return 1
}
if ver_gt "$minos" "$max"; then
    echo "error: $bin requires macOS $minos (> $max); set MACOSX_DEPLOYMENT_TARGET" >&2
    exit 1
fi
echo "ok: $bin minimum macOS $minos (<= $max)"
