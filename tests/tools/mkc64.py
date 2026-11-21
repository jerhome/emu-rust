#!/usr/bin/env python3
"""Generate tiny original C64 test media (public domain, written for this project): no third-party data.

usage: mkc64.py OUTDIR         ->  OUTDIR/hello.prg  OUTDIR/hello.d64   (SHA-256 pinned, do not change)
       mkc64.py OUTDIR --all   ->  also joy.prg, second.prg, two.d64 (HELLO + SECOND), second.m3u ("two.d64:SECOND"),
                                   hello.t64, cart.crt (Normal cartridge, 8 KB) and mixed.zip (hello.d64 + hello.dsk
                                   when OUTDIR/hello.dsk exists, e.g. written by mkcpc.py --set)
hello.prg is a one-line BASIC V2 listing:  10 PRINT"EMU-RUST C64 TEST":POKE53280,2:POKE53281,5
joy.prg shows the CIA1 joystick ports ($DC01 = port 1, $DC00 = port 2) and which one is active.
"""
import io
import struct
import sys
import zipfile
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
    return d64_files([(name, prg)])


def d64_files(files: list[tuple[str, bytes]]) -> bytes:
    """Files are laid out one after the other from track 17 sector 0, going down (never on track 18)."""
    img = bytearray(174848)
    free = {t: set(range(SPT[t - 1])) for t in range(1, 36)}
    b = offset(18, 0)
    img[b : b + 4] = bytes([18, 1, 0x41, 0])
    sectors = [(t, s) for t in range(17, 0, -1) for s in range(SPT[t - 1])]
    chains = []
    for _, prg in files:
        chunks = [prg[i : i + 254] for i in range(0, len(prg), 254)]
        secs, sectors = sectors[: len(chunks)], sectors[len(chunks) :]
        assert len(secs) == len(chunks), "disk full"
        chains.append((secs, chunks))
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
    assert len(files) <= 8, "one directory sector"
    d = offset(18, 1)
    img[d], img[d + 1] = 0, 0xFF
    for n, ((name, _), (secs, chunks)) in enumerate(zip(files, chains)):
        e = d + 32 * n
        img[e + 2] = 0x82  # PRG, closed
        img[e + 3], img[e + 4] = secs[0]
        img[e + 5 : e + 21] = nm(name)
        img[e + 0x1E : e + 0x20] = struct.pack("<H", len(chunks))
    return bytes(img)


# Tokens of the BASIC V2 keywords used by the generated programs (longest match, outside quotes).
TOKENS = {
    "PRINT": 0x99, "POKE": 0x97, "PEEK": 0xC2, "AND": 0xAF, "IF": 0x8B, "THEN": 0xA7, "GOTO": 0x89,
    "CHR$": 0xC7, "REM": 0x8F, "<": 0xB3, "=": 0xB2,
}  # fmt: skip


def basic(lines: list[tuple[int, str]]) -> bytes:
    """Tokenizes `(number, text)` lines into a PRG at $0801."""
    start = 0x0801
    out = b""
    for number, text in lines:
        body = b""
        i = 0
        quoted = False
        while i < len(text):
            if text[i] == '"':
                quoted = not quoted
            if not quoted:
                tok = next((k for k in sorted(TOKENS, key=len, reverse=True) if text.startswith(k, i)), None)
                if tok:
                    body += bytes([TOKENS[tok]])
                    i += len(tok)
                    continue
            body += text[i].encode("ascii")
            i += 1
        nxt = start + len(out) + 2 + 2 + len(body) + 1
        out += struct.pack("<HH", nxt, number) + body + b"\x00"
    return struct.pack("<H", start) + out + b"\x00\x00"


JOY_LINES = [
    (10, 'PRINT CHR$(147);"JOY TEST"'),
    (20, "A=PEEK(56321)AND31:B=PEEK(56320)AND31"),
    (30, 'PRINT CHR$(19);CHR$(17);CHR$(17);"JOY1:";A;" JOY2:";B;"   "'),
    (40, 'IF A<31 THEN PRINT "PORT 1 ACTIVE ":GOTO 20'),
    (50, 'IF B<31 THEN PRINT "PORT 2 ACTIVE ":GOTO 20'),
    (60, 'PRINT "NO PORT ACTIVE":GOTO 20'),
]


def joy_prg() -> bytes:
    return basic(JOY_LINES)


def second_prg() -> bytes:
    return basic([(10, 'PRINT "EMU-RUST C64 SECOND"')])


def t64(prg: bytes, name: str = "HELLO", tape_name: str = "EMU-RUST TEST") -> bytes:
    """T64 container (version 1.01) with one PRG entry."""
    load, body = struct.unpack("<H", prg[:2])[0], prg[2:]
    max_entries = 4
    head = b"C64S tape image file".ljust(32, b"\0") + struct.pack("<HHHH", 0x0101, max_entries, 1, 0)
    head += tape_name.encode("ascii").ljust(24, b" ")
    data_at = 64 + 32 * max_entries
    entry = bytes([1, 0x82]) + struct.pack("<HHHII", load, load + len(body), 0, data_at, 0)
    entry += name.encode("ascii").ljust(16, b" ")
    return head + entry.ljust(32 * max_entries, b"\0") + body


# 8 KB cartridge at $8000: cold / warm start vectors, "CBM80", then the standard reset sequence and a text loop.
_MSG = b"EMU-RUST C64 CART TEST\r\x00"
_CODE = (
    bytes.fromhex("78" "8E16D0" "20A3FD" "2050FD" "2015FD" "205BFF" "58" "A200")
    + bytes([0xBD, 0x2B, 0x80])  # LDA $802B,X
    + bytes.fromhex("F007" "20D2FF" "E8" "4C1C80" "4C2880")
    + _MSG
)  # fmt: skip


def crt() -> bytes:
    rom = bytes.fromhex("0980" "0980" "C3C2CD3830") + _CODE
    assert rom[0x2B:] == _MSG
    rom = rom.ljust(0x2000, b"\xff")
    head = b"C64 CARTRIDGE   " + struct.pack(">IHHBB", 0x40, 0x0100, 0, 0, 1) + bytes(6)  # EXROM=0, GAME=1
    head += b"EMU-RUST TEST".ljust(32, b"\0")
    chip = b"CHIP" + struct.pack(">IHHHH", 0x2010, 0, 0, 0x8000, 0x2000) + rom
    return head + chip


def mixed_zip(files: dict[str, bytes]) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, body in files.items():
            info = zipfile.ZipInfo(name, (2020, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, body)
    return buf.getvalue()


def build_all(out: Path) -> None:
    prg, second = basic_prg(), second_prg()
    (out / "hello.prg").write_bytes(prg)
    (out / "hello.d64").write_bytes(d64(prg))
    (out / "joy.prg").write_bytes(joy_prg())
    (out / "second.prg").write_bytes(second)
    (out / "two.d64").write_bytes(d64_files([("HELLO", prg), ("SECOND", second)]))
    (out / "second.m3u").write_bytes(b"two.d64:SECOND\n")
    (out / "hello.t64").write_bytes(t64(prg))
    (out / "cart.crt").write_bytes(crt())
    if (out / "hello.dsk").exists():
        (out / "mixed.zip").write_bytes(mixed_zip({"hello.dsk": (out / "hello.dsk").read_bytes(), "hello.d64": d64(prg)}))


if __name__ == "__main__":
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    if sys.argv[2:3] == ["--all"]:
        build_all(out)
    else:
        prg = basic_prg()
        (out / "hello.prg").write_bytes(prg)
        (out / "hello.d64").write_bytes(d64(prg))
