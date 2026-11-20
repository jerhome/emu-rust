#!/usr/bin/env python3
"""Generate a 880 KB bootable Amiga ADF whose boot block paints rainbow bars.

Original code (public domain, written for this project): no third-party data.
usage: mkadf.py out.adf
The boot block runs `loop: move.w $DFF006,$DFF180 ; bra.s loop` (VHPOSR -> COLOR00).
"""
import struct
import sys

img = bytearray(901120)
img[0:4] = b"DOS\x00"
# move.w $00DFF006,$00DFF180 ; bra.s -12
img[12:24] = bytes.fromhex("33f900dff00600dff18060f4")
total = 0
for i in range(0, 1024, 4):
    total += struct.unpack_from(">I", img, i)[0]
    if total > 0xFFFFFFFF:
        total = (total & 0xFFFFFFFF) + 1
struct.pack_into(">I", img, 4, (~total) & 0xFFFFFFFF)
open(sys.argv[1], "wb").write(img)
