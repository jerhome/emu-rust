#!/usr/bin/env python3
"""Generate tiny original C64 test media (public domain, written for this project): no third-party data.

usage: mkc64.py OUTDIR   ->  OUTDIR/hello.prg  OUTDIR/hello.d64
The program is a one-line BASIC V2 listing:  10 PRINT"EMU-RUST C64 TEST":POKE53280,2:POKE53281,5
"""
import struct
import sys
from pathlib import Path


def basic_prg() -> bytes:
    body = bytes([0x99]) + b'"EMU-RUST C64 TEST"' + b":" + bytes([0x97]) + b"53280,2:" + bytes([0x97]) + b"53281,5"
    start = 0x0801
    nxt = start + 2 + 2 + len(body) + 1
    line = struct.pack("<HH", nxt, 10) + body + b"\x00"
    return struct.pack("<H", start) + line + b"\x00\x00"


SPT = [21] * 17 + [19] * 7 + [18] * 6 + [17] * 5  # sectors per track 1..35


def offset(track: int, sector: int) -> int:
    return (sum(SPT[: track - 1]) + sector) * 256


def d64(prg: bytes, name: str = "HELLO") -> bytes:
    img = bytearray(174848)
    free = {t: set(range(SPT[t - 1])) for t in range(1, 36)}
    # BAM
    b = offset(18, 0)
    img[b : b + 4] = bytes([18, 1, 0x41, 0])
    # file sectors: 254 data bytes each, starting at track 17 sector 0 going up
    chunks = [prg[i : i + 254] for i in range(0, len(prg), 254)]
    secs = [(17, i) for i in range(len(chunks))]
    for i, ((t, s), c) in enumerate(zip(secs, chunks)):
        free[t].discard(s)
        o = offset(t, s)
        if i + 1 < len(chunks):
            img[o], img[o + 1] = secs[i + 1]
        else:
            img[o], img[o + 1] = 0, len(c) + 1
        img[o + 2 : o + 2 + len(c)] = c
    free[18].discard(0)
    free[18].discard(1)
    for t in range(1, 36):
        bits = 0
        for s in free[t]:
            bits |= 1 << s
        o = b + 4 * t
        img[o] = len(free[t])
        img[o + 1 : o + 4] = bits.to_bytes(3, "little")
    nm = lambda s: s.encode("ascii").ljust(16, b"\xa0")
    img[b + 0x90 : b + 0xA0] = nm("EMU-RUST TEST")
    img[b + 0xA0 : b + 0xAB] = b"\xa0\xa0" + b"EM" + b"\xa0" + b"2A" + b"\xa0" * 4
    d = offset(18, 1)
    img[d], img[d + 1] = 0, 0xFF
    img[d + 2] = 0x82  # PRG, closed
    img[d + 3], img[d + 4] = secs[0]
    img[d + 5 : d + 21] = nm(name)
    img[d + 0x1E : d + 0x20] = struct.pack("<H", len(chunks))
    return bytes(img)


if __name__ == "__main__":
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    prg = basic_prg()
    (out / "hello.prg").write_bytes(prg)
    (out / "hello.d64").write_bytes(d64(prg))
