#!/usr/bin/env bash
# Builds dist/emu-rust-<version>-macos-x86_64.zip: emu-rust.app (ad hoc signed, not notarized) plus README and
# licenses next to it (ADR 0012). macOS only (plutil, codesign, ditto). No dylib is embedded.
# Env: BIN (default target/x86_64-apple-darwin/release/emu-rust), LICENSES, OUT_DIR, VERSION.
set -euo pipefail
cd "$(dirname "$0")/.."

[[ $(uname -s) == Darwin ]] || { echo "package-macos.sh runs on macOS only" >&2; exit 1; }

BIN=${BIN:-target/x86_64-apple-darwin/release/emu-rust}
LICENSES=${LICENSES:-dist/THIRD-PARTY-LICENSES.html}
# NO_LICENSES=1: leave the HTML out (release.yml adds it afterwards with add-licenses.sh, so builds do not wait for it).
NO_LICENSES=${NO_LICENSES:-}
[[ -n $NO_LICENSES ]] && LICENSES=
OUT_DIR=${OUT_DIR:-dist}
VERSION=${VERSION:-$(scripts/workspace-version.sh)}

name="emu-rust-${VERSION}-macos-x86_64"
for f in "$BIN" ${LICENSES:+"$LICENSES"} packaging/README.txt packaging/THIRD-PARTY-DATA.txt packaging/macos/Info.plist assets/icon.icns LICENSE-MIT LICENSE-APACHE; do
    [[ -f $f ]] || { echo "missing: $f" >&2; exit 1; }
done
lipo -archs "$BIN" | grep -qx x86_64 || { echo "$BIN is not an x86_64-only Mach-O binary" >&2; exit 1; }
# Minimum OS recorded by the linker (MACOSX_DEPLOYMENT_TARGET=10.13): must run on the Monterey test Mac.
scripts/check-macos-minos.sh "$BIN" 12.0

mkdir -p "$OUT_DIR"
stage="$OUT_DIR/$name"
rm -rf "$stage" "$OUT_DIR/$name.zip"
app="$stage/emu-rust.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
install -m 755 "$BIN" "$app/Contents/MacOS/emu-rust"
install -m 644 assets/icon.icns "$app/Contents/Resources/icon.icns"
install -m 644 packaging/README.txt packaging/THIRD-PARTY-DATA.txt LICENSE-MIT LICENSE-APACHE ${LICENSES:+"$LICENSES"} "$stage/"

# Ad hoc signature (ADR 0012): seals Info.plist to the executable; no Developer ID, no notarization.
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict --verbose=2 "$app"

ditto -c -k --keepParent "$stage" "$OUT_DIR/$name.zip"
rm -rf "$stage"

echo "$OUT_DIR/$name.zip"
unzip -l "$OUT_DIR/$name.zip"
