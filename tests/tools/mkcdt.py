#!/usr/bin/env python3
"""Generate a tiny original Amstrad CPC tape image (.cdt, TZX container) with one binary file (public domain).

usage: mkcdt.py out.cdt                (hello.cdt, SHA-256 pinned, do not change)
       mkcdt.py --set OUTDIR           hello.cdt + "tape (Tape 1 of 2).cdt" + "tape (Tape 2 of 2).cdt" (each prints its own text)
The Z80 program prints "EMU-RUST CPC TAPE TEST" through the firmware (CALL &BB5A, TXT OUTPUT) then loops forever (a returning program makes the firmware reset); load/entry &4000.
Timings (T-states): pilot 2000, sync 855/855, zero 855, one 1710, i.e. the standard CPC 2000 baud tape.
"""
import struct
import sys
from pathlib import Path

MSG = b"EMU-RUST CPC TAPE TEST\r\n\x00"
PREFIX = bytes.fromhex("210F407EB72806CD5ABB2318F618FE")
CODE = PREFIX + MSG


def crc(data: bytes) -> bytes:
    c = 0xFFFF
    for b in data:
        c ^= b << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x1021) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return struct.pack(">H", c ^ 0xFFFF)


def tape_block(sync: int, payload: bytes) -> bytes:
    # payload is split in 256-byte chunks, each followed by its CRC; 4 trailing 0xFF
    body = bytes([sync])
    for i in range(0, len(payload), 256):
        chunk = payload[i : i + 256].ljust(256, b"\x00")
        body += chunk + crc(chunk)
    return body + b"\xff" * 4


def turbo(data: bytes, pause_ms: int) -> bytes:
    return b"\x11" + struct.pack("<HHHHHHBH", 2000, 855, 855, 855, 1710, 4096, 8, pause_ms) + len(data).to_bytes(3, "little") + data


def cdt(name: bytes = b"HELLO", msg: bytes = MSG) -> bytes:
    code = PREFIX + msg
    header = bytearray(256)  # 64-byte header record padded to a 256-byte chunk
    header[0:16] = name.ljust(16, b"\x00")
    header[16] = 1  # block number
    header[17] = 0xFF  # last block
    header[18] = 2  # binary
    header[19:21] = struct.pack("<H", len(code))
    header[21:23] = struct.pack("<H", 0x4000)  # load address
    header[23] = 0xFF  # first block
    header[24:26] = struct.pack("<H", len(code))
    header[26:28] = struct.pack("<H", 0x4000)  # entry
    out = b"ZXTape!\x1a\x01\x0d"
    out += turbo(tape_block(0x2C, bytes(header)), 10)
    out += turbo(tape_block(0x16, code), 2000)
    return out


def build_set(out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    (out / "hello.cdt").write_bytes(cdt())
    for n in (1, 2):
        text = f"EMU-RUST CPC TAPE {n} OF 2\r\n\x00".encode()
        (out / f"tape (Tape {n} of 2).cdt").write_bytes(cdt(f"TAPE{n}".encode(), text))


if __name__ == "__main__":
    if sys.argv[1:2] == ["--set"]:
        build_set(Path(sys.argv[2]))
    else:
        open(sys.argv[1], "wb").write(cdt())
