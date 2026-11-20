#!/usr/bin/env python3
"""Generate a 880 KB bootable Amiga ADF whose boot block paints rainbow bars.

Original code (public domain, written for this project): no third-party data.
usage: mkadf.py out.adf | mkadf.py --label NAME out.adf (blank OFS disk; SOURCE_DATE_EPOCH => reproducible)
The boot block runs `loop: move.w $DFF006,$DFF180 ; bra.s loop` (VHPOSR -> COLOR00).
"""
import struct
import sys
import time

SIZE = 901120  # 80 cylinders x 2 heads x 11 sectors x 512 bytes
ROOT = 880  # rootblock: middle of the disk
BITMAP = 881


def _csum_at(img, base, off):
    """Standard AmigaDOS block checksum: the stored longword makes the sum of all 256 longwords zero."""
    total = 0
    for i in range(0, 512, 4):
        if i != off:
            total = (total + struct.unpack_from(">I", img, base + i)[0]) & 0xFFFFFFFF
    struct.pack_into(">I", img, base + off, (-total) & 0xFFFFFFFF)


def bootblock_checksum(img):
    """Bootblock checksum covers 1024 bytes with end-around carry; stored at offset 4."""
    struct.pack_into(">I", img, 4, 0)
    total = 0
    for i in range(0, 1024, 4):
        total += struct.unpack_from(">I", img, i)[0]
        if total > 0xFFFFFFFF:
            total = (total & 0xFFFFFFFF) + 1
    struct.pack_into(">I", img, 4, (~total) & 0xFFFFFFFF)


def boot_adf():
    img = bytearray(SIZE)
    img[0:4] = b"DOS\x00"
    # move.w $00DFF006,$00DFF180 ; bra.s -12
    img[12:24] = bytes.fromhex("33f900dff00600dff18060f4")
    bootblock_checksum(img)
    return img


def blank_adf(label, now=None):
    """Blank non-bootable OFS disk (DOS\\0) with an empty root directory and the given volume name."""
    name = label.encode("latin-1")
    if not 1 <= len(name) <= 30 or any(c in b"/:" or c < 32 for c in name):
        raise ValueError("volume name must be 1..30 chars without '/', ':' or control characters")
    img = bytearray(SIZE)
    img[0:4] = b"DOS\x00"
    bootblock_checksum(img)
    days, mins, ticks = _amiga_time(now)
    base = ROOT * 512
    struct.pack_into(">I", img, base + 0, 2)  # T_HEADER
    struct.pack_into(">I", img, base + 12, 72)  # hash table size
    struct.pack_into(">i", img, base + 312, -1)  # bitmap valid flag
    struct.pack_into(">I", img, base + 316, BITMAP)  # bitmap page 0
    struct.pack_into(">III", img, base + 420, days, mins, ticks)  # last root alteration
    img[base + 432] = len(name)
    img[base + 433 : base + 433 + len(name)] = name
    struct.pack_into(">III", img, base + 472, days, mins, ticks)  # last disk alteration
    struct.pack_into(">III", img, base + 484, days, mins, ticks)  # creation date
    struct.pack_into(">I", img, base + 508, 1)  # ST_ROOT
    _csum_at(img, base, 20)
    # bitmap: all blocks 2..1759 free except root and bitmap blocks
    bm = BITMAP * 512
    words = bytearray(b"\xff" * 508)
    for blk in (ROOT, BITMAP, 1760, 1761):  # the last two are past the end of the disk (1760 blocks)
        bit = blk - 2
        words[(bit // 32) * 4 + 3 - (bit % 32) // 8] &= ~(1 << (bit % 8)) & 0xFF
    img[bm + 4 : bm + 4 + 220] = words[:220]
    _csum_at(img, bm, 0)
    return img


def _amiga_time(now):
    # Amiga epoch: 1978-01-01; fixed value when SOURCE_DATE_EPOCH is set, for reproducible output
    t = int(now if now is not None else time.time())
    t -= 252460800
    return t // 86400, (t % 86400) // 60, (t % 60) * 50


def main(argv):
    args = argv[1:]
    label = None
    if len(args) == 3 and args[0] == "--label":
        label, args = args[1], args[2:]
    if len(args) != 1:
        sys.exit("usage: mkadf.py out.adf              (bootable rainbow ADF)\n"
                 "       mkadf.py --label NAME out.adf  (blank formatted OFS disk named NAME)")
    if label is None:
        img = boot_adf()
    else:
        import os
        epoch = os.environ.get("SOURCE_DATE_EPOCH")
        img = blank_adf(label, int(epoch) if epoch else None)
    with open(args[0], "wb") as f:
        f.write(img)


if __name__ == "__main__":
    main(sys.argv)
