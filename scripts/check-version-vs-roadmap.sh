#!/usr/bin/env bash
# Fails if the workspace version (major.minor) differs from the latest version
# marked "✅ vX.Y" in the first column of docs/roadmap.md. Works with bash 3.2.
# WORKSPACE_VERSION overrides scripts/workspace-version.sh (tests).
set -euo pipefail
cd "$(dirname "$0")/.."

roadmap=docs/roadmap.md
latest=$(sed -n -E 's/^\| ✅ v([0-9]+\.[0-9]+).*/\1/p' "$roadmap" | sort -t. -k1,1n -k2,2n | tail -n 1)
[ -n "$latest" ] || { echo "no '✅ vX.Y' marker found in $roadmap" >&2; exit 1; }

version=${WORKSPACE_VERSION:-$(scripts/workspace-version.sh)}
# Drop patch and any -rc.N / +build suffix.
current=$(printf '%s\n' "$version" | sed -E 's/^([0-9]+\.[0-9]+).*/\1/')

if [ "$current" != "$latest" ]; then
  echo "version mismatch: workspace is $version but the roadmap's latest done version is $latest." >&2
  echo "Update [workspace.package] version in Cargo.toml (and Cargo.lock) or the ✅ marker in $roadmap." >&2
  exit 1
fi
echo "version OK: $version matches roadmap v$latest"
