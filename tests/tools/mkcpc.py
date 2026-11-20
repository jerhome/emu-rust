#!/usr/bin/env python3
"""Generate a tiny original Amstrad CPC test disk (public domain, written for this project).

usage: mkcpc.py OUT.dsk
Standard 'MV - CPCEMU' DSK, 40 tracks x 9 sectors x 512 bytes (AMSDOS data format, IDs C1..C9),
one headerless ASCII BASIC file HELLO.BAS.
"""
import sys

PROG = b'10 MODE 1:PRINT "EMU-RUST CPC TEST":BORDER 6\r\n20 PRINT "LINE 20"\r\n'


def dsk(files: dict[str, bytes]) -> bytes:
    tracks, nsec, ssz = 40, 9, 512
    data = [bytearray([0xE5] * ssz) for _ in range(tracks * nsec)]  # logical sectors, linear
    directory = bytearray([0xE5] * 2048)
    nextblock = 2  # blocks 0-1 hold the directory
    ent = 0
    for name, body in files.items():
        stem, _, ext = name.partition(".")
        pad = body + b"\x1a" * (-len(body) % 128)
        records = len(pad) // 128
        nblocks = -(-len(pad) // 1024)
        blocks = list(range(nextblock, nextblock + nblocks))
        nextblock += nblocks
        for i, b in enumerate(blocks):
            chunk = pad[i * 1024 : (i + 1) * 1024].ljust(1024, b"\xe5")
            for k in range(2):
                data[b * 2 + k][:] = chunk[k * 512 : (k + 1) * 512]
        e = bytearray(32)
        e[0] = 0
        e[1:9] = stem.upper().ljust(8).encode()
        e[9:12] = ext.upper().ljust(3).encode()
        e[15] = records
        e[16 : 16 + len(blocks)] = bytes(blocks)
        directory[ent * 32 : ent * 32 + 32] = e
        ent += 1
    for i in range(4):
        data[i][:] = directory[i * 512 : (i + 1) * 512]
    out = bytearray(b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n")
    out += b"emu-rust test".ljust(14, b"\0")
    out += bytes([tracks, 1]) + (256 + nsec * ssz).to_bytes(2, "little")
    out = out.ljust(256, b"\0")
    for t in range(tracks):
        th = bytearray(b"Track-Info\r\n".ljust(16, b"\0"))
        th += bytes([t, 0, 0, 0, 2, nsec, 0x4E, 0xE5])
        for s in range(nsec):
            th += bytes([t, 0, 0xC1 + s, 2, 0, 0]) + ssz.to_bytes(2, "little")
        out += th.ljust(256, b"\0")
        for s in range(nsec):
            out += data[t * nsec + s]
    return bytes(out)


if __name__ == "__main__":
    open(sys.argv[1], "wb").write(dsk({"HELLO.BAS": PROG}))
