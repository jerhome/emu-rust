#!/usr/bin/env bash
# Prints the workspace version (single source: [workspace.package] version, ADR 0012).
set -euo pipefail
cd "$(dirname "$0")/.."
# `cargo pkgid` prints e.g. `path+file:///src/crates/emu-app#emu-app@0.1.0`.
pkgid=$(cargo pkgid --offline -p emu-app 2>/dev/null || cargo pkgid -p emu-app)
version=${pkgid##*[#@]}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+].*)?$ ]] || { echo "unexpected pkgid: $pkgid" >&2; exit 1; }
printf '%s\n' "$version"
