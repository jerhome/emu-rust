#!/usr/bin/env bash
# Regenerate assets/icon-*.png, icon.ico and icon.icns from assets/icon.svg.
# Rasterizer (first found): resvg, rsvg-convert, ImageMagick (magick), macOS qlmanage.
# Resizing: sips (macOS) or ImageMagick. .icns: iconutil (macOS only; kept as-is elsewhere).
# .ico: built with python3 (PNG-compressed entries 16..256), no extra dependency.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
a="$root/assets"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
sizes=(16 32 48 128 256 512)

raster() { # size out
  if command -v resvg >/dev/null; then resvg -w "$1" -h "$1" "$a/icon.svg" "$2"
  elif command -v rsvg-convert >/dev/null; then rsvg-convert -w "$1" -h "$1" "$a/icon.svg" -o "$2"
  elif command -v magick >/dev/null; then magick -background none -density 384 "$a/icon.svg" -resize "${1}x${1}" "$2"
  elif command -v qlmanage >/dev/null; then
    # qlmanage only renders reliably at large sizes; downscale afterwards.
    if [ ! -f "$tmp/big.png" ]; then
      qlmanage -t -s 1024 -o "$tmp" "$a/icon.svg" >/dev/null 2>&1
      mv "$tmp/icon.svg.png" "$tmp/big.png"
    fi
    if command -v sips >/dev/null; then sips -z "$1" "$1" "$tmp/big.png" --out "$2" >/dev/null
    else magick "$tmp/big.png" -resize "${1}x${1}" "$2"; fi
  else echo "no SVG rasterizer found (resvg, rsvg-convert, magick, qlmanage)" >&2; exit 1; fi
}

raster 1024 "$tmp/1024.png"
for s in "${sizes[@]}"; do
  raster "$s" "$a/icon-$s.png"
done

python3 -I - "$a" <<'PY'
import struct, sys
a = sys.argv[1]
sizes = [16, 32, 48, 128, 256]
imgs = [open(f"{a}/icon-{s}.png", "rb").read() for s in sizes]
out = struct.pack("<HHH", 0, 1, len(sizes))
off = 6 + 16 * len(sizes)
for s, d in zip(sizes, imgs):
    out += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(d), off)
    off += len(d)
open(f"{a}/icon.ico", "wb").write(out + b"".join(imgs))
PY

if command -v iconutil >/dev/null; then
  set_dir="$tmp/icon.iconset"; mkdir "$set_dir"
  cp "$a/icon-16.png"  "$set_dir/icon_16x16.png"
  cp "$a/icon-32.png"  "$set_dir/icon_16x16@2x.png"
  cp "$a/icon-32.png"  "$set_dir/icon_32x32.png"
  raster 64 "$set_dir/icon_32x32@2x.png"
  cp "$a/icon-128.png" "$set_dir/icon_128x128.png"
  cp "$a/icon-256.png" "$set_dir/icon_128x128@2x.png"
  cp "$a/icon-256.png" "$set_dir/icon_256x256.png"
  cp "$a/icon-512.png" "$set_dir/icon_256x256@2x.png"
  cp "$a/icon-512.png" "$set_dir/icon_512x512.png"
  cp "$tmp/1024.png"   "$set_dir/icon_512x512@2x.png"
  iconutil -c icns "$set_dir" -o "$a/icon.icns"
else
  echo "iconutil not found: assets/icon.icns left unchanged (run on macOS)" >&2
fi
echo "icons regenerated in $a"
