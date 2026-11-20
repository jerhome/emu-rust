#!/usr/bin/env bash
# Adds THIRD-PARTY-LICENSES.html to a release archive built with NO_LICENSES=1 / without it, next to the
# README (inside the top-level directory), then checks it is there exactly once. Existing entries are not
# rewritten (zip) or only appended (tar), so modes, symlinks and the signed .app are untouched.
# Usage: add-licenses.sh <archive.tar.gz|archive.zip> <THIRD-PARTY-LICENSES.html>
set -euo pipefail

archive=$(realpath "$1")
html=$(realpath "$2")
[[ -f $archive && -f $html ]] || { echo "usage: $0 <archive> <html>" >&2; exit 1; }
base=$(basename "$archive")
top=${base%.tar.gz}
top=${top%.zip}
entry="$top/THIRD-PARTY-LICENSES.html"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

case $archive in
*.tar.gz)
    mtime=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}
    gzip -dc "$archive" > "$work/a.tar"
    list=$(tar -tf "$work/a.tar")
    grep -qxF "$entry" <<< "$list" && { echo "already contains $entry" >&2; exit 1; }
    grep -qxF "$top/README.txt" <<< "$list" || { echo "unexpected layout: no $top/README.txt" >&2; exit 1; }
    mkdir -p "$work/stage/$top"
    install -m 644 "$html" "$work/stage/$entry"
    tar --owner=0 --group=0 --numeric-owner --mtime="@$mtime" -C "$work/stage" -rf "$work/a.tar" "$entry"
    gzip -9n < "$work/a.tar" > "$archive"
    list=$(tar -tzf "$archive")
    [[ $(grep -cxF "$entry" <<< "$list") -eq 1 ]]
    ;;
*.zip)
    list=$(unzip -Z1 "$archive")
    grep -qxF "$entry" <<< "$list" && { echo "already contains $entry" >&2; exit 1; }
    grep -qxF "$top/README.txt" <<< "$list" || { echo "unexpected layout: no $top/README.txt" >&2; exit 1; }
    mkdir -p "$work/stage/$top"
    install -m 644 "$html" "$work/stage/$entry"
    (cd "$work/stage" && zip -X -q "$archive" "$entry")
    list=$(unzip -Z1 "$archive")
    [[ $(grep -cxF "$entry" <<< "$list") -eq 1 ]]
    ;;
*)
    echo "unsupported archive: $archive" >&2; exit 1 ;;
esac
echo "added $entry to $base"
