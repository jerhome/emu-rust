#!/usr/bin/env python3
"""Generate a tiny original Amstrad CPC test disk (public domain, written for this project).

usage: mkcpc.py OUT.dsk                         one disk with HELLO.BAS (SHA-256 pinned, do not change)
       mkcpc.py --set OUTDIR [--d64 HELLO.D64]  the CQ1 set, all deterministic:
         save.dsk                HELLO.BAS saves "OUT" (ASCII) to its own disk, then prints "SAVED" (write test)
         keys.dsk                KEYS.BAS echoes INKEY$ (keyboard layout tests)
         two.dsk                 HELLO.BAS + SECOND.BAS (prints "EMU-RUST CPC SECOND")
         command.m3u             "#COMMAND:RUN"SECOND"" then two.dsk (frontend launch line)
         faces (Side A).dsk, faces (Side B).dsk   one program each (A.BAS / B.BAS), for insert_next
         faces.m3u               both faces, labelled
         mixed.zip               hello.dsk + hello.d64 (only with --d64: a mixed-system archive)
Standard 'MV - CPCEMU' DSK, 40 tracks x 9 sectors x 512 bytes (AMSDOS data format, IDs C1..C9),
one headerless ASCII BASIC file HELLO.BAS.
"""
import io
import sys
import zipfile
from pathlib import Path

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


def prog(text: str) -> bytes:
    return f'10 MODE 1:PRINT "{text}"\r\n'.encode()


SAVER = b'10 MODE 1:PRINT "EMU-RUST CPC SAVE"\r\n20 SAVE"OUT",A\r\n30 PRINT "SAVED"\r\n'
KEYS = b'10 MODE 1:PRINT "KEYS"\r\n20 A$=INKEY$:IF A$="" THEN 20\r\n30 PRINT A$;:GOTO 20\r\n'
FIXED_ZIP_DATE = (2020, 1, 1, 0, 0, 0)


def mixed_zip(files: dict[str, bytes]) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, body in files.items():
            info = zipfile.ZipInfo(name, FIXED_ZIP_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, body)
    return buf.getvalue()


def build_set(out: Path, d64: Path | None) -> None:
    out.mkdir(parents=True, exist_ok=True)
    hello = dsk({"HELLO.BAS": PROG})
    (out / "hello.dsk").write_bytes(hello)
    (out / "save.dsk").write_bytes(dsk({"HELLO.BAS": SAVER}))
    (out / "keys.dsk").write_bytes(dsk({"KEYS.BAS": KEYS}))
    (out / "two.dsk").write_bytes(dsk({"HELLO.BAS": PROG, "SECOND.BAS": prog("EMU-RUST CPC SECOND")}))
    (out / "command.m3u").write_bytes(b'#COMMAND:RUN"SECOND"\ntwo.dsk\n')
    (out / "faces (Side A).dsk").write_bytes(dsk({"A.BAS": prog("EMU-RUST CPC SIDE A")}))
    (out / "faces (Side B).dsk").write_bytes(dsk({"B.BAS": prog("EMU-RUST CPC SIDE B")}))
    (out / "faces.m3u").write_bytes(b"faces (Side A).dsk|Side A\nfaces (Side B).dsk|Side B\n")
    if d64 is not None:
        (out / "mixed.zip").write_bytes(mixed_zip({"hello.dsk": hello, "hello.d64": d64.read_bytes()}))


if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["--set"]:
        build_set(Path(args[1]), Path(args[3]) if args[2:3] == ["--d64"] else None)
    else:
        open(args[0], "wb").write(dsk({"HELLO.BAS": PROG}))
