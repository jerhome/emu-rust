#!/usr/bin/env bash
# Regenerates every icon file from packaging/icon/emu-rust.svg (ADR 0023 §2). Outputs are committed, so a
# normal build needs no graphics tool. Needs ImageMagick (`convert`, internal SVG renderer) and python3
# (standard library only). Idempotent: same inputs and tool versions give the same files.
# Outputs: packaging/icon/png/<16..1024>.png, emu-rust.ico, emu-rust.icns, SOURCE.sha256,
#          packaging/linux/hicolor/<size>/apps/emu-rust.png, hicolor/scalable/apps/emu-rust.svg.
# packaging/linux/emu-rust.desktop is hand-written and only checked here.
set -euo pipefail
cd "$(dirname "$0")/.."

svg=packaging/icon/emu-rust.svg
out=packaging/icon
hicolor=packaging/linux/hicolor
[[ -f $svg ]] || { echo "missing: $svg" >&2; exit 1; }
command -v convert >/dev/null || { echo "ImageMagick (convert) is required" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }
[[ -f packaging/linux/emu-rust.desktop ]] || { echo "missing: packaging/linux/emu-rust.desktop" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Render once at 1024 px, then downscale with a Lanczos filter: rasterising the SVG directly at tiny
# sizes gives dark fringes with the internal renderer. -strip and the date chunks off keep PNGs byte-stable.
convert -background none -density 144 "$svg" -resize 1024x1024 -depth 8 PNG32:"$work/master.png"
render() { # size dest
    convert "$work/master.png" -filter Lanczos -resize "${1}x${1}" \
        -define png:exclude-chunk=date,time,tIME,tEXt -strip -depth 8 PNG32:"$2"
}

mkdir -p "$out/png"
for s in 16 24 32 48 64 128 256 512 1024; do
    render "$s" "$work/$s.png"
done
for s in 16 32 48 64 128 256 512 1024; do
    install -m 644 "$work/$s.png" "$out/png/$s.png"
done

# .ico: 16, 24, 32, 48, 64, 256 (24 is not kept as a PNG file).
convert "$work/16.png" "$work/24.png" "$work/32.png" "$work/48.png" "$work/64.png" "$work/256.png" \
    -strip "$work/emu-rust.ico"
install -m 644 "$work/emu-rust.ico" "$out/emu-rust.ico"

# .icns: container of PNG payloads (big-endian: magic, total length, then type + length + data).
python3 -I - "$work" "$out/emu-rust.icns" <<'PY'
import struct, sys
work, dest = sys.argv[1:3]
# (OSType, pixel size); the @2x types reuse the next size up.
types = [("icp4", 16), ("icp5", 32), ("icp6", 64), ("ic07", 128), ("ic08", 256), ("ic09", 512),
         ("ic10", 1024), ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512)]
body = b""
for t, size in types:
    data = open(f"{work}/{size}.png", "rb").read()
    body += t.encode("ascii") + struct.pack(">I", 8 + len(data)) + data
open(dest, "wb").write(b"icns" + struct.pack(">I", 8 + len(body)) + body)
PY

# Linux hicolor tree (copied into the tar.gz by package-linux.sh).
for s in 16 32 48 64 128 256 512; do
    mkdir -p "$hicolor/${s}x${s}/apps"
    install -m 644 "$work/$s.png" "$hicolor/${s}x${s}/apps/emu-rust.png"
done
mkdir -p "$hicolor/scalable/apps"
install -m 644 "$svg" "$hicolor/scalable/apps/emu-rust.svg"

# Fingerprint of the source: CI only checks that it matches the committed SVG.
(cd "$out" && sha256sum emu-rust.svg > SOURCE.sha256)

echo "icons regenerated from $svg"
