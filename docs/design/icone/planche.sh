#!/usr/bin/env bash
# Comparison sheet of the icon variants (ImageMagick only; its internal SVG renderer is enough for flat shapes).
set -euo pipefail
D=$1; S=$2; F=DejaVu-Sans
for v in a b c d; do V=$(echo $v | tr a-d A-D)
 for s in 256 64 32 16; do convert -background none "$D/variante-$v.svg" -resize ${s}x${s} "$S/$v-$s.png"; done
 convert -size 520x300 xc:'#1b1b1b' "$S/$v-256.png" -geometry +16+28 -composite "$S/$v-64.png" -geometry +296+28 -composite "$S/$v-32.png" -geometry +384+28 -composite "$S/$v-16.png" -geometry +440+28 -composite \
   -fill '#e6e6e6' -draw "rectangle 288,120 504,220" "$S/$v-64.png" -geometry +296+138 -composite "$S/$v-32.png" -geometry +384+138 -composite "$S/$v-16.png" -geometry +440+138 -composite \
   -font "$F" -pointsize 20 -fill '#96c8ff' -annotate +296+262 "Variante $V" -pointsize 12 -fill '#b4b4b4' -annotate +296+284 "256 / 64 / 32 / 16 px" "$S/row-$v.png"; done
convert \( "$S/row-a.png" "$S/row-b.png" +append \) \( "$S/row-c.png" "$S/row-d.png" +append \) -append -depth 8 -strip "$D/planche.png"
