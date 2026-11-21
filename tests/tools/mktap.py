#!/usr/bin/env python3
"""Generate a C64 .tap (KERNAL turbo-less format) holding the hello.prg BASIC program (public domain, original).

usage: mktap.py hello.prg out.tap [NAME]   (NAME: file name on the tape, default HELLO; the default output is SHA-256 pinned)
Pulse lengths are in units of 8 CPU cycles (S=0x30, M=0x42, L=0x56), as written by the KERNAL SAVE routine.
"""
import struct
import sys

S, M, L = 0x30, 0x42, 0x56


def byte(b: int) -> list[int]:
    out = [L, M]
    ones = 0
    for i in range(8):
        bit = (b >> i) & 1
        ones += bit
        out += [M, S] if bit else [S, M]
    out += [S, M] if ones % 2 else [M, S]  # odd parity
    return out


def block(payload: bytes, header: bool) -> list[int]:
    """Two copies of a block, as the KERNAL writes them (countdown 0x89.. then 0x09..)."""
    out: list[int] = []
    for copy in (0, 1):
        out += [S] * ((0x6A00 if header else 0x1A00) if copy == 0 else 0x4F)
        for c in range(9):
            out += byte((0x89 if copy == 0 else 0x09) - c)
        chk = 0
        for b in payload:
            out += byte(b)
            chk ^= b
        out += byte(chk)
        out += [L, S]  # end-of-data marker
    return out


def tap(prg: bytes, name: str = "HELLO") -> bytes:
    load, body = struct.unpack("<H", prg[:2])[0], prg[2:]
    hdr = bytes([1]) + struct.pack("<HH", load, load + len(body)) + name.encode("ascii")[:16].ljust(16) + b" " * 171
    assert len(hdr) == 192
    pulses = block(hdr, True) + [S] * 0x100 + block(body, False)
    data = bytes(min(p, 255) for p in pulses)
    return b"C64-TAPE-RAW" + bytes([0, 0, 0, 0]) + struct.pack("<I", len(data)) + data


if __name__ == "__main__":
    open(sys.argv[2], "wb").write(tap(open(sys.argv[1], "rb").read(), *sys.argv[3:4]))
