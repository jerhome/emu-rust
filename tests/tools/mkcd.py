#!/usr/bin/env python3
"""Generate fake CD images for tests: no commercial content, original data (public domain, written for this project).

usage (python3 -I mkcd.py ...):
  mkcd.py megacd OUTDIR [--name N] [--region J|U|E] [--title T] [--serial S] [--sector 2048|2352]
                        [--data-sectors N] [--audio-tracks N] [--audio-seconds S] [--audio silence|sine]
                        [--layout multi|mono] [--pregap SECTORS] [--iso] [--discs N] [--m3u]
  mkcd.py orphan OUT.bin        data track alone (SEGADISCSYSTEM, MODE1/2352), no .cue
  mkcd.py 32x OUT.32x           fake 32X ROM header ("SEGA 32X" at 0x100); not bootable
  mkcd.py iso9660 OUT.iso [--label L] [--file NAME=PATH ...]   minimal ISO 9660 (PC / DOS discs)

Output is deterministic; SOURCE_DATE_EPOCH (if set) only changes the ISO 9660 timestamps.

Sega CD sector-0 layout written (user data, offsets from the start of the 2048-byte sector):
  0x000 "SEGADISCSYSTEM  " (16)      disc header; the library probe accepts it at 0 (2048) or 0x10 (2352, after sync+header)
  0x010 volume name (11), 0x020 "SEGA MEGA DRIVE " ... only approximate, readers ignore the rest
  0x100 "SEGA MEGA DRIVE " (16)      Mega Drive style header (design v6.0 3.4: serial read at 0x180)
  0x110 copyright/date (16), 0x120 domestic title (48), 0x150 overseas title (48)
  0x180 "GM <serial>" (type + serial, e.g. "GM T-93175 -00"), 0x1F0 region field ("J", "U" or "E", then spaces)
  0x20B region letter (J/U/E): the byte genesis_plus_gx inspects for CD region auto-detection.
Sources: docs/design/v6.0-gba-32x-megacd.md 3.4 (probe at 0/0x10, serial at 0x180); the standard Mega Drive header
layout (0x100/0x180/0x1F0); 0x20B is recalled from genesis_plus_gx (loadrom.c) and NOT verified here.
MODE1/2352 sectors: sync, BCD MSF header, 2048 data, EDC (CRC, poly 0xD8018001), 8 zero bytes, ECC left at zero
(emulators do not check it).
"""
import argparse
import math
import os
import struct
import sys
import time

SYNC = b"\x00" + b"\xff" * 10 + b"\x00"
RAW = 2352
USER = 2048
FPS = 44100 // 75  # audio frames (stereo samples) per sector = 588


def _edc_table():
    t = []
    for i in range(256):
        c = i
        for _ in range(8):
            c = (c >> 1) ^ (0xD8018001 if c & 1 else 0)
        t.append(c)
    return t


_EDC = _edc_table()


def edc(data: bytes) -> int:
    c = 0
    for b in data:
        c = (c >> 8) ^ _EDC[(c ^ b) & 0xFF]
    return c


def _bcd(n: int) -> int:
    return (n // 10) << 4 | n % 10


def msf(lba: int) -> tuple[int, int, int]:
    return lba // (75 * 60), lba // 75 % 60, lba % 75


def msf_str(frames: int) -> str:
    return "%02d:%02d:%02d" % msf(frames)


def raw_mode1(lba: int, user: bytes) -> bytes:
    """One MODE1/2352 sector; `lba` is the absolute sector number from the start of the disc (+150 in the header)."""
    assert len(user) == USER
    m, s, f = msf(lba + 150)
    head = SYNC + bytes([_bcd(m), _bcd(s), _bcd(f), 1]) + user
    return head + struct.pack("<I", edc(head)) + bytes(8) + bytes(276)


def _field(text: str, n: int) -> bytes:
    return text.encode("ascii", "replace")[:n].ljust(n, b" ")


def megacd_sector0(region="U", title="EMU-RUST TEST DISC", serial="T-000000 -00") -> bytes:
    if region not in ("J", "U", "E"):
        raise ValueError("region must be J, U or E")
    s = bytearray(USER)
    s[0x000:0x010] = _field("SEGADISCSYSTEM", 16)
    s[0x010:0x01B] = _field("EMURUSTTEST", 11)
    s[0x020:0x030] = _field("SEGA MEGA DRIVE", 16)
    s[0x100:0x110] = _field("SEGA MEGA DRIVE", 16)
    s[0x110:0x120] = _field("(C)EMUR 2026.JAN", 16)
    s[0x120:0x150] = _field(title, 48)
    s[0x150:0x180] = _field(title, 48)
    s[0x180:0x18E] = _field("GM " + serial, 14)
    s[0x1F0:0x200] = _field(region, 16)
    s[0x200:0x20B] = _field("FAKE", 11)
    s[0x20B] = ord(region)
    s[0x20C:0x300] = _field("EMU-RUST FAKE CD, NO COMMERCIAL CONTENT", 0xF4)
    return bytes(s)


def data_track(sector0: bytes, sectors: int, sector_size: int) -> bytes:
    if sectors < 1:
        raise ValueError("a data track needs at least one sector")
    out = bytearray()
    for i in range(sectors):
        user = sector0 if i == 0 else _field("EMU-RUST FAKE DATA SECTOR %d" % i, USER)
        out += user if sector_size == USER else raw_mode1(i, user)
    return bytes(out)


def audio_track(sectors: int, kind: str) -> bytes:
    """16-bit stereo little-endian 44.1 kHz PCM, `sectors` * 2352 bytes."""
    if kind == "silence":
        return bytes(sectors * RAW)
    n = sectors * FPS
    out = bytearray()
    for i in range(n):
        v = int(8000 * math.sin(2 * math.pi * 440 * i / 44100))
        out += struct.pack("<hh", v, v)
    return bytes(out)


def build_cd(a) -> dict[str, bytes]:
    """Return {filename: bytes} for one disc; a: argparse-like namespace (see main)."""
    if a.layout == "mono" and a.sector != RAW and a.audio_tracks:
        raise ValueError("mono layout with audio needs --sector 2352")
    if a.layout == "mono" and a.sector != RAW:
        a.layout = "multi"  # data only: a single file either way
    base = a.disc_name
    s0 = megacd_sector0(a.region, a.title, a.serial)
    data = data_track(s0, a.data_sectors, a.sector)
    asec = round(a.audio_seconds * 75)
    audios = [audio_track(a.pregap + asec, a.audio) for _ in range(a.audio_tracks)]
    mode = "MODE1/%d" % a.sector
    files: dict[str, bytes] = {}
    cue = []
    if a.layout == "mono":
        name = base + ".bin"
        files[name] = data + b"".join(audios)
        cue.append('FILE "%s" BINARY' % name)
        cue += ["  TRACK 01 %s" % mode, "    INDEX 01 00:00:00"]
        pos = a.data_sectors
        for k in range(a.audio_tracks):
            cue.append("  TRACK %02d AUDIO" % (k + 2))
            if a.pregap:
                cue.append("    INDEX 00 %s" % msf_str(pos))
            cue.append("    INDEX 01 %s" % msf_str(pos + a.pregap))
            pos += a.pregap + asec
    else:
        n1 = "%s (Track 1).bin" % base if a.audio_tracks else base + ".bin"
        files[n1] = data
        cue += ['FILE "%s" BINARY' % n1, "  TRACK 01 %s" % mode, "    INDEX 01 00:00:00"]
        for k, au in enumerate(audios):
            nk = "%s (Track %d).bin" % (base, k + 2)
            files[nk] = au
            cue += ['FILE "%s" BINARY' % nk, "  TRACK %02d AUDIO" % (k + 2)]
            if a.pregap:
                cue.append("    INDEX 00 00:00:00")
            cue.append("    INDEX 01 %s" % msf_str(a.pregap))
    files[base + ".cue"] = ("\r\n".join(cue) + "\r\n").encode()
    if a.iso:
        files[base + ".iso"] = data_track(s0, a.data_sectors, USER)
    return files


def iso9660(files: dict[str, bytes], label="EMURUST", when=None) -> bytes:
    """Minimal ISO 9660 level 1: root directory only, names upper-case 8.3 stored as NAME.EXT;1."""
    when = time.gmtime(when if when is not None else 946684800)  # 2000-01-01 unless given
    both32 = lambda v: struct.pack("<I", v) + struct.pack(">I", v)
    both16 = lambda v: struct.pack("<H", v) + struct.pack(">H", v)
    d7 = bytes([when.tm_year - 1900, when.tm_mon, when.tm_mday, when.tm_hour, when.tm_min, when.tm_sec, 0])
    d17 = time.strftime("%Y%m%d%H%M%S", when).encode() + b"00\x00"

    def rec(name: bytes, extent: int, size: int, flags: int) -> bytes:
        r = bytearray([0, 0]) + both32(extent) + both32(size) + d7 + bytes([flags, 0, 0]) + both16(1)
        r += bytes([len(name)]) + name
        if len(r) % 2:
            r += b"\0"
        r[0] = len(r)
        return bytes(r)

    ROOT_LBA = 20
    entries, lba, body = [], 21, bytearray()
    for name in sorted(files):
        if not (name == name.upper() and len(name.split(".")[0]) <= 8):
            raise ValueError("ISO name must be upper-case 8.3: " + name)
        data = files[name]
        entries.append(rec(name.encode() + b";1", lba, len(data), 0))
        body += data.ljust(-(-len(data) // USER) * USER or USER, b"\0")
        lba += max(1, -(-len(data) // USER))
    dirdata = rec(b"\x00", ROOT_LBA, USER, 2) + rec(b"\x01", ROOT_LBA, USER, 2) + b"".join(entries)
    if len(dirdata) > USER:
        raise ValueError("too many files for a one-sector root directory")
    ptab_l = struct.pack("<BBIH", 1, 0, ROOT_LBA, 1) + b"\x00\x00"
    ptab_m = struct.pack(">BBIH", 1, 0, ROOT_LBA, 1) + b"\x00\x00"
    pvd = bytearray(USER)
    pvd[0:8] = b"\x01CD001\x01\x00"
    pvd[8:40] = _field("EMU-RUST", 32)
    pvd[40:72] = _field(label.upper(), 32)
    pvd[80:88] = both32(lba)
    pvd[120:124] = both16(1)
    pvd[124:128] = both16(1)
    pvd[128:132] = both16(USER)
    pvd[132:140] = both32(len(ptab_l))
    pvd[140:144] = struct.pack("<I", 18)
    pvd[148:152] = struct.pack(">I", 19)
    pvd[156:190] = rec(b"\x00", ROOT_LBA, USER, 2)[:34]
    for off in (190, 318, 446, 574):
        pvd[off : off + 128] = b" " * 128
    for off in (702, 739, 776):
        pvd[off : off + 37] = b" " * 37
    for off in (813, 830):
        pvd[off : off + 17] = d17
    for off in (847, 864):
        pvd[off : off + 17] = b"0" * 16 + b"\x00"
    pvd[881] = 1
    term = bytearray(USER)
    term[0:8] = b"\xffCD001\x01\x00"
    out = bytes(16 * USER) + bytes(pvd) + bytes(term)
    out += ptab_l.ljust(USER, b"\0") + ptab_m.ljust(USER, b"\0") + dirdata.ljust(USER, b"\0") + bytes(body)
    return out


def fake_32x() -> bytes:
    rom = bytearray(0x200)
    rom[0x100:0x110] = _field("SEGA 32X", 16)
    rom[0x110:0x120] = _field("(C)EMUR 2026.JAN", 16)
    rom[0x120:0x150] = _field("EMU-RUST FAKE 32X", 48)
    rom[0x150:0x180] = _field("EMU-RUST FAKE 32X", 48)
    rom[0x180:0x18E] = _field("GM 00000000-00", 14)
    rom[0x1F0:0x200] = _field("U", 16)
    return bytes(rom)


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("megacd")
    m.add_argument("outdir")
    m.add_argument("--name", default="fakecd")
    m.add_argument("--region", choices="JUE", default="U")
    m.add_argument("--title", default="EMU-RUST TEST DISC")
    m.add_argument("--serial", default="T-000000 -00")
    m.add_argument("--sector", type=int, choices=(2048, 2352), default=2352)
    m.add_argument("--data-sectors", type=int, default=32)
    m.add_argument("--audio-tracks", type=int, default=1)
    m.add_argument("--audio-seconds", type=float, default=2.0)
    m.add_argument("--audio", choices=("silence", "sine"), default="silence")
    m.add_argument("--layout", choices=("multi", "mono"), default="multi")
    m.add_argument("--pregap", type=int, default=0, help="sectors of INDEX 00..01 before each audio track")
    m.add_argument("--iso", action="store_true", help="also write NAME.iso (2048-byte data track)")
    m.add_argument("--discs", type=int, default=1, help="N > 1 writes 'NAME (Disc k)' images and NAME.m3u")
    m.add_argument("--m3u", action="store_true")
    o = sub.add_parser("orphan")
    o.add_argument("out")
    x = sub.add_parser("32x")
    x.add_argument("out")
    i = sub.add_parser("iso9660")
    i.add_argument("out")
    i.add_argument("--label", default="EMURUST")
    i.add_argument("--file", action="append", default=[], metavar="NAME=PATH")
    a = p.parse_args(argv)
    if a.cmd == "megacd":
        os.makedirs(a.outdir, exist_ok=True)
        names = [a.name] if a.discs == 1 else ["%s (Disc %d)" % (a.name, k + 1) for k in range(a.discs)]
        cues = []
        for n in names:
            a.disc_name = n
            for fn, data in build_cd(a).items():
                with open(os.path.join(a.outdir, fn), "wb") as f:
                    f.write(data)
            cues.append(n + ".cue")
        if a.m3u or a.discs > 1:
            with open(os.path.join(a.outdir, a.name + ".m3u"), "w", newline="") as f:
                f.write("\r\n".join(cues) + "\r\n")
    elif a.cmd == "orphan":
        with open(a.out, "wb") as f:
            f.write(data_track(megacd_sector0(), 16, RAW))
    elif a.cmd == "32x":
        with open(a.out, "wb") as f:
            f.write(fake_32x())
    else:
        files = {"README.TXT": b"EMU-RUST ISO TEST\r\n", "HELLO.COM": HELLO_COM}
        for spec in a.file:
            n, _, path = spec.partition("=")
            files[n.upper()] = open(path, "rb").read()
        epoch = os.environ.get("SOURCE_DATE_EPOCH")
        with open(a.out, "wb") as f:
            f.write(iso9660(files, a.label, int(epoch) if epoch else None))


# docs/essais-dosbox.md section 4: mov ah,9; mov dx,0110h; int 21h; xor ax,ax; int 16h; mov ax,4C00h; int 21h + text
HELLO_COM = bytes.fromhex("B409BA1001CD2131C0CD16B8004CCD21") + b"HELLO FROM EMU-RUST$"

if __name__ == "__main__":
    main(sys.argv[1:])
