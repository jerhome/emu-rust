#!/usr/bin/env bash
# Builds dist/emu-rust-<version>-linux-x86_64.tar.gz from an already built release binary (ADR 0012).
# Env: BIN (default target/release/emu-rust), LICENSES (default dist/THIRD-PARTY-LICENSES.html), OUT_DIR (dist).
# Idempotent: the staging directory and archive are recreated on each run.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=${BIN:-target/release/emu-rust}
LICENSES=${LICENSES:-dist/THIRD-PARTY-LICENSES.html}
# NO_LICENSES=1: leave the HTML out (release.yml adds it afterwards with add-licenses.sh, so builds do not wait for it).
NO_LICENSES=${NO_LICENSES:-}
[[ -n $NO_LICENSES ]] && LICENSES=
OUT_DIR=${OUT_DIR:-dist}
VERSION=${VERSION:-$(scripts/workspace-version.sh)}

name="emu-rust-${VERSION}-linux-x86_64"
for f in "$BIN" ${LICENSES:+"$LICENSES"} packaging/README.txt packaging/THIRD-PARTY-DATA.txt packaging/linux/emu-rust.desktop assets/icon.svg assets/icon-256.png LICENSE-MIT LICENSE-APACHE; do
    [[ -f $f ]] || { echo "missing: $f" >&2; exit 1; }
done
# Guard against shipping a core, BIOS or ROM by mistake (ADR 0001, 0005): only these files go in.
file "$BIN" | grep -q 'ELF 64-bit.*x86-64' || { echo "$BIN is not an x86_64 ELF binary" >&2; exit 1; }

mkdir -p "$OUT_DIR"
stage="$OUT_DIR/$name"
rm -rf "$stage" "$OUT_DIR/$name.tar.gz"
mkdir -p "$stage"
install -m 755 "$BIN" "$stage/emu-rust"
# Icon + launcher: copy icon-256.png/icon.svg to ~/.local/share/icons/hicolor/*/apps/emu-rust.* and the .desktop to ~/.local/share/applications.
install -m 644 packaging/linux/emu-rust.desktop assets/icon.svg assets/icon-256.png "$stage/"
install -m 644 packaging/README.txt packaging/THIRD-PARTY-DATA.txt LICENSE-MIT LICENSE-APACHE ${LICENSES:+"$LICENSES"} "$stage/"

# Reproducible archive: fixed owner, order and mtime (last commit, or now outside git).
mtime=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$mtime" \
    -C "$OUT_DIR" -cf - "$name" | gzip -9n > "$OUT_DIR/$name.tar.gz"
rm -rf "$stage"

echo "$OUT_DIR/$name.tar.gz"
tar -tvzf "$OUT_DIR/$name.tar.gz"
