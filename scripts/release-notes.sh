#!/usr/bin/env bash
# Prints the GitHub release notes for VERSION: the `## [VERSION]` section of CHANGELOG.md preceded by an optional
# packaging/release-notes-X.Y.Z.md and followed by packaging/release-notes.md (test checklist, core URLs).
# Usage: release-notes.sh VERSION [--allow-missing-changelog]
set -euo pipefail
cd "$(dirname "$0")/.."

version=${1:?usage: release-notes.sh VERSION [--allow-missing-changelog]}
allow_missing=${2:-}

section=""
if [[ -f CHANGELOG.md ]]; then
    # From the version heading (exclusive) to the next `## ` heading.
    section=$(awk -v v="$version" '
        index($0, "## [" v "]") == 1 { on = 1; next }
        on && /^## / { exit }
        on { print }
    ' CHANGELOG.md)
fi
if [[ -z ${section//[[:space:]]/} ]]; then
    if [[ $allow_missing == --allow-missing-changelog ]]; then
        echo "warning: no '## [$version]' section in CHANGELOG.md" >&2
        section="_(CHANGELOG.md : section [$version] absente — dry run)_"
    else
        echo "error: no '## [$version]' section in CHANGELOG.md" >&2
        exit 1
    fi
fi

# Optional per-release header (warning, highlights): packaging/release-notes-X.Y.Z.md, X.Y.Z = version without any
# pre-release suffix (8.0.0-rc.1 reads release-notes-8.0.0.md). Absent for most releases.
header="packaging/release-notes-${version%%-*}.md"
if [[ -f $header ]]; then
    sed "s/@VERSION@/$version/g" "$header"
    printf '\n'
fi
printf '%s\n\n' "$section"
sed "s/@VERSION@/$version/g" packaging/release-notes.md
