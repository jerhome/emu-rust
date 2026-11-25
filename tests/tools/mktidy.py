#!/usr/bin/env python3
"""Generate a "loose" roms/ tree for the tidying tests (ADR 0023 4, design v7.1 5), plus the expected outcome.

usage (python3 -I mktidy.py ...):
  mktidy.py OUTDIR [--no-symlink]
      writes OUTDIR/roms/ (the loose tree) and OUTDIR/manifest.json (expectations); OUTDIR/roms must not exist.

Everything is synthetic or original: minimal headers only (no Nintendo logo, no Sony text, no BIOS), written for this
project. Output is byte-for-byte deterministic (fixed content, fixed mtime 1700000000, sorted manifest); mkcd.py,
mkadf.py and mkdos.py are reused as libraries (SOURCE_DATE_EPOCH is pinned while they run).

manifest.json (version 1)
  units[]   one entry per expected tidy unit (ADR 0023 4.2):
    id            stable slug
    title         human title
    files         every file of the unit, relative to roms/, sorted ("/" separators)
    status        Sure | Suggested | Ambiguous | Blocked | Unknown   (TidyStatus)
    reason        Sure/Suggested: TidyReason name (Library, Header, DatHash, ArchiveContent, DatName, Extension,
                  DosFolder); Blocked: TidyBlock variant name; Ambiguous / Unknown: null
    system        destination system id, or null
    candidates    systems the user may choose from (Ambiguous), else []
    moves[]       {"from", "to"} relative to roms/, one per file moved; [] when nothing moves. A Blocked unit whose
                  system is known (collision, folder named like a system) keeps the moves it would have made
    whole_dir     {"from", "to"} when the unit moves as one directory rename, else null
    destination   "to" of the file or folder the unit ends in ("snes/Game.sfc", "psx/Game/"), else null
    text          why, in plain words
    requires      "unix" for the symlink unit, else null (skip the unit elsewhere / with --no-symlink)
    windows       optional override {"status", "reason"} where Windows differs (path longer than 240 chars)
    confidence    "adr" (follows the ADR text, or the choice FB1a made and the architect confirmed: see note)
    note          the decision, when the ADR text alone does not give it
  firmware_names[]  file names to give TidyOptions::with_firmware_names (left in place, listed in ignored_paths)
  ignored[]  {"path", "reason"} files that are not units: "already_sorted" (anywhere below a first-level roms/<id>/,
             never analysed) or "firmware_name" (a file named like a known BIOS) or
             "not_a_game" (no known extension: listed in ignored_paths, never a unit)
  tree[]     every path of the tree (files, symlinks) relative to roms/, sorted: the "before" snapshot
  platform   {"symlink": bool}
"""
import argparse
import hashlib
import importlib.util
import io
import json
import os
import shutil
import struct
import sys
import tempfile
import types
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
EPOCH = 1700000000
os.environ["SOURCE_DATE_EPOCH"] = str(EPOCH)


def _load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, name + ".py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


mkcd = _load("mkcd")
mkadf = _load("mkadf")
mkdos = _load("mkdos")

# ---------------------------------------------------------------------------------------------- headers


def pad(text: str, n: int) -> bytes:
    return text.encode("ascii").ljust(n, b" ")[:n]


def snes(title="EMU-RUST TEST SNES", copier=False) -> bytes:
    """32 KiB LoROM with a valid-looking header at 0x7FC0 (no logo exists for SNES)."""
    rom = bytearray(0x8000)
    h = 0x7FC0
    rom[h : h + 21] = pad(title, 21)
    rom[h + 0x15] = 0x20  # LoROM, slow
    rom[h + 0x17] = 0x05  # 32 KiB
    rom[h + 0x1C : h + 0x1E] = struct.pack("<H", 0xFFFF ^ 0x0000)
    rom[h + 0x1E : h + 0x20] = struct.pack("<H", 0x0000)
    return (b"\x00" * 512 if copier else b"") + bytes(rom)


def megadrive(title="EMU-RUST TEST MD", magic="SEGA MEGA DRIVE ") -> bytes:
    rom = bytearray(0x400)
    rom[0x100:0x110] = pad(magic, 16)
    rom[0x110:0x120] = pad("(C)EMUR 2026.NOV", 16)
    rom[0x120:0x150] = pad(title, 48)
    rom[0x150:0x180] = pad(title, 48)
    rom[0x180:0x18E] = pad("GM 00000000-00", 14)
    rom[0x1F0:0x200] = pad("JUE", 16)
    return bytes(rom)


def gb(title="EMUTEST", color=False) -> bytes:
    """Header without the boot logo (the scan does not probe .gb): title, CGB flag, cart type, header checksum."""
    rom = bytearray(0x8000)
    rom[0x134 : 0x134 + len(title)] = title.encode("ascii")
    rom[0x143] = 0x80 if color else 0x00
    rom[0x147] = 0x00
    rom[0x148] = 0x00
    rom[0x14D] = (-sum(rom[0x134:0x14D]) - 25) & 0xFF
    return bytes(rom)


def gba(title="EMUTEST") -> bytes:
    rom = bytearray(0x400)
    rom[0xA0 : 0xA0 + len(title)] = title.encode("ascii")
    rom[0xAC:0xB0] = b"EMUT"
    rom[0xB0:0xB2] = b"01"
    rom[0xB2] = 0x96
    rom[0xBD] = (-sum(rom[0xA0:0xBD]) - 0x19) & 0xFF
    return bytes(rom)


def nes(prg=1, chr_=1, fill=0) -> bytes:
    return b"NES\x1a" + bytes([prg, chr_, 0, 0]) + bytes(8) + bytes([fill]) * (prg * 16384 + chr_ * 8192)


def sega8(region=0x4, game_gear=False) -> bytes:
    """Master System / Game Gear: "TMR SEGA" at 0x7FF0, region nibble at 0x7FFF."""
    rom = bytearray(0x8000)
    rom[0x7FF0:0x7FF8] = b"TMR SEGA"
    rom[0x7FFF] = ((0x6 if game_gear else 0x4) << 4) | 0x0
    return bytes(rom)


def atari_st(label="EMUTEST") -> bytes:
    img = bytearray(737280)
    img[0:3] = b"\xeb\x3c\x90"
    img[3:11] = b"EMURUST "
    struct.pack_into("<HBHBHHBHHHII", img, 11, 512, 2, 1, 2, 112, 1440, 0xF9, 3, 9, 2, 0, 0)
    img[510:512] = b"\x55\xaa"
    return bytes(img)


def fake_chd() -> bytes:
    """CHD v5 magic + length/version fields, zero-filled: not a valid image, only recognisable by extension."""
    return b"MComprHD" + struct.pack(">II", 124, 5) + bytes(112)


def fake_sbi() -> bytes:
    return b"SBI\x00" + bytes(16)


def make_zip(entries: dict) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name in sorted(entries):
            zi = zipfile.ZipInfo(name, (2000, 1, 1, 0, 0, 0))
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = 0o100644 << 16
            z.writestr(zi, entries[name])
    return buf.getvalue()


# ---------------------------------------------------------------------------------------------- CD helpers


def megacd_files(name, audio=0, discs=1, region="E", serial="T-00000 -00") -> dict:
    """{filename: bytes} of one or more Mega CD discs (.cue + tracks), never an .m3u."""
    out = {}
    for k in range(discs):
        n = name if discs == 1 else "%s (Disc %d)" % (name, k + 1)
        a = types.SimpleNamespace(
            disc_name=n, region=region, title=n.upper()[:40], serial=serial, sector=2352, data_sectors=24,
            audio_tracks=audio, audio_seconds=0.1, audio="silence", layout="multi", pregap=0, iso=False,
        )
        out.update(mkcd.build_cd(a))
    return out


def psx_files(name, region="E", serial=None, audio=0, discs=1) -> dict:
    """PlayStation discs; with audio > 0 the data track becomes "NAME (Track 1).bin" plus audio tracks."""
    out = {}
    for k in range(discs):
        n = name if discs == 1 else "%s (Disc %d)" % (name, k + 1)
        a = types.SimpleNamespace(
            disc_name=n, region=region, serial=serial, mode=2, iso=False, boot_data=None,
            no_system_cnf=False, no_license=False,
        )
        files = mkcd.psx_files(a)
        if audio:
            data = files.pop(n + ".bin")
            files.pop(n + ".cue")
            cue = ['FILE "%s (Track 1).bin" BINARY' % n, "  TRACK 01 MODE2/2352", "    INDEX 01 00:00:00"]
            files[n + " (Track 1).bin"] = data
            for t in range(2, audio + 2):
                files["%s (Track %d).bin" % (n, t)] = mkcd.audio_track(8, "silence")
                cue += ['FILE "%s (Track %d).bin" BINARY' % (n, t), "  TRACK %02d AUDIO" % t, "    INDEX 01 00:00:00"]
            files[n + ".cue"] = ("\r\n".join(cue) + "\r\n").encode()
        out.update(files)
    return out


def opera_files(name, discs=1) -> dict:
    out = {}
    for k in range(discs):
        n = name if discs == 1 else "%s (Disc %d)" % (name, k + 1)
        out.update(mkcd.fake_3do_files(n, 2352, 16))
    return out


def cue_text(*entries) -> bytes:
    """entries: (file, mode); data first."""
    lines = []
    for i, (f, mode) in enumerate(entries, 1):
        lines += ['FILE "%s" BINARY' % f, "  TRACK %02d %s" % (i, mode), "    INDEX 01 00:00:00"]
    return ("\r\n".join(lines) + "\r\n").encode()


# ---------------------------------------------------------------------------------------------- builder


class Tree:
    def __init__(self, root: str, symlink: bool):
        self.root = root
        self.symlink = symlink
        self.files: dict = {}  # rel -> bytes | ("link", target) | ("sparse", size)
        self.units: list = []
        self.ignored: list = []
        self.firmware_names: list = []

    def add(self, rel: str, data):
        assert rel not in self.files, rel
        self.files[rel] = data

    def add_all(self, folder: str, files: dict):
        for n, d in files.items():
            self.add((folder + "/" if folder else "") + n, d)

    def unit(self, uid, title, files, status, reason=None, system=None, candidates=(), moves=None,
             whole_dir=None, text="", requires=None, windows=None, confidence="adr", note=None):
        files = sorted(files)
        for f in files:
            assert f in self.files, (uid, f)
        u = {
            "id": uid, "title": title, "files": files, "status": status, "reason": reason, "system": system,
            "candidates": list(candidates),
            "moves": [{"from": a, "to": b} for a, b in (moves or [])],
            "whole_dir": {"from": whole_dir[0], "to": whole_dir[1]} if whole_dir else None,
            "text": text, "requires": requires, "confidence": confidence,
        }
        if whole_dir:
            u["destination"] = whole_dir[1] + "/"
        elif moves and len(moves) == 1:
            u["destination"] = moves[0][1]
        elif moves:
            u["destination"] = os.path.dirname(moves[0][1]) + "/"
        else:
            u["destination"] = None
        if windows:
            u["windows"] = windows
        if note:
            u["note"] = note
        self.units.append(u)

    def single(self, uid, title, rel, system, reason="Extension", status="Sure", text="", **kw):
        """A single file moved to roms/<system>/<name>."""
        self.unit(uid, title, [rel], status, reason, system, moves=[(rel, "%s/%s" % (system, os.path.basename(rel)))],
                  text=text, **kw)

    def write(self):
        roms = os.path.join(self.root, "roms")
        os.makedirs(roms)
        for rel in sorted(self.files):
            path = os.path.join(roms, *rel.split("/"))
            os.makedirs(os.path.dirname(path), exist_ok=True)
            d = self.files[rel]
            if isinstance(d, tuple) and d[0] == "link":
                os.symlink(d[1], path)
            elif isinstance(d, tuple) and d[0] == "sparse":
                with open(path, "wb") as f:
                    f.truncate(d[1])
            else:
                with open(path, "wb") as f:
                    f.write(d)
        for rel in sorted(self.files):
            path = os.path.join(roms, *rel.split("/"))
            os.utime(path, (EPOCH, EPOCH), follow_symlinks=False)
        # Directories: fixed mtime too (deepest first).
        for dirpath, _dirs, _files in sorted(os.walk(roms), key=lambda t: -len(t[0])):
            os.utime(dirpath, (EPOCH, EPOCH))


def build(t: Tree):
    # ---- console ROMs at the root (extension unique to one system) --------------------------------------------
    t.add("Test Quest.sfc", snes("TEST QUEST"))
    t.single("snes-sfc", "Test Quest", "Test Quest.sfc", "snes", text="extension .sfc belongs to snes only")
    t.add("Copier Game.smc", snes("COPIER GAME", copier=True))
    t.single("snes-smc-copier", "Copier Game", "Copier Game.smc", "snes", text="512-byte copier header, .smc is snes only")
    t.add("Mega Test.md", megadrive("MEGA TEST"))
    t.single("md-md", "Mega Test", "Mega Test.md", "megadrive", text=".md belongs to megadrive only")
    t.add("Cart Bin.bin", megadrive("CART BIN"))
    t.single("md-bin-header", "Cart Bin", "Cart Bin.bin", "megadrive", reason="Header",
             text="SEGA MEGA DRIVE header at 0x100 in a .bin (not a CD track)", confidence="adr",
             note="a .bin with a Sega / 32X header is Header, never Extension (FB1a)")
    t.add("Bin 32X.bin", mkcd.fake_32x())
    t.single("x32-bin-header", "Bin 32X", "Bin 32X.bin", "32x", reason="Header", text="SEGA 32X signature at 0x100 in a .bin")
    t.add("Super Test.32x", mkcd.fake_32x())
    t.single("x32-32x", "Super Test", "Super Test.32x", "32x", text=".32x belongs to 32x only")
    t.add("Boy Test.gbc", gb("BOYCOLOR", color=True))
    t.single("gbc-gbc", "Boy Test", "Boy Test.gbc", "gbc", text=".gbc belongs to gbc only")
    t.add("Pocket Test.gb", gb("POCKET"))
    t.unit("gb-ambiguous", "Pocket Test", ["Pocket Test.gb"], "Ambiguous", None, None, candidates=["gb", "gbc"],
           text=".gb is listed by gb and gbc; without a DAT the user chooses",
           confidence="adr",
           note="candidates in catalog order; a CGB-flagged .gb (0x143) is not used by the scan")
    t.add("Advance Test.gba", gba("ADVANCE"))
    t.single("gba-gba", "Advance Test", "Advance Test.gba", "gba", text=".gba belongs to gba only")
    t.add("Famicom Test.nes", nes(1, 1))
    t.single("nes-nes", "Famicom Test", "Famicom Test.nes", "nes", text=".nes belongs to nes only (iNES header present)")
    t.add("Master Test.sms", sega8())
    t.single("sms-sms", "Master Test", "Master Test.sms", "mastersystem", text=".sms belongs to mastersystem only")
    t.add("Handheld Test.gg", sega8(game_gear=True))
    t.single("gg-gg", "Handheld Test", "Handheld Test.gg", "gamegear", text=".gg belongs to gamegear only")

    # ---- zips ------------------------------------------------------------------------------------------------
    t.add("Zip SNES.zip", make_zip({"Zipped Quest.sfc": snes("ZIPPED")}))
    t.single("zip-snes", "Zip SNES", "Zip SNES.zip", "snes", reason="ArchiveContent", text="one .sfc entry")
    t.add("Zip Mega Drive.zip", make_zip({"Zipped Mega.md": megadrive("ZIPPED")}))
    t.single("zip-md", "Zip Mega Drive", "Zip Mega Drive.zip", "megadrive", reason="ArchiveContent", text="one .md entry")
    t.add("Zip Advance.zip", make_zip({"Zipped Advance.gba": gba("ZIPPED")}))
    t.single("zip-gba", "Zip Advance", "Zip Advance.zip", "gba", reason="ArchiveContent", text="one .gba entry")
    t.add("Zip NES.zip", make_zip({"Zipped Famicom.nes": nes(1, 0, 1)}))
    t.single("zip-nes", "Zip NES", "Zip NES.zip", "nes", reason="ArchiveContent", text="one .nes entry")
    t.add("Zip Mixed.zip", make_zip({"Mixed A.dsk": b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n" + bytes(256), "Mixed B.d64": bytes(174848)}))
    t.unit("zip-mixed", "Zip Mixed", ["Zip Mixed.zip"], "Unknown", None, None,
           text="entries of two computers (.dsk and .d64): the scan ignores it (MixedArchive); the tidy leaves it in place",
           confidence="adr",
           note="a zip whose entries belong to two computer systems is Unknown (the scan's MixedArchive)")
    t.add("Zip DOS.zip", make_zip({"GAME.EXE": mkdos.fake_exe("ZDOS"), "README.TXT": b"zipped dos game\r\n"}))
    t.single("zip-dos", "Zip DOS", "Zip DOS.zip", "dos", reason="ArchiveContent", status="Suggested",
             text="zip taken for DOS (ADR 4.3: Suggested, shown unchecked)", confidence="adr",
             note="a zip taken for DOS is Suggested(ArchiveContent), a .dosz is Sure(Extension)")
    t.add("Zip CD.zip", make_zip({"Zipped CD.cue": cue_text(("Zipped CD.bin", "MODE1/2352")),
                                  "Zipped CD.bin": mkcd.data_track(mkcd.megacd_sector0(), 8, 2352)}))
    t.unit("zip-cd", "Zip CD", ["Zip CD.zip"], "Blocked", "DiscInArchive", None,
           text="a CD image inside a zip is refused (rule 2.1)")

    # ---- CD images: multi-track .cue/.bin ---------------------------------------------------------------------
    f = megacd_files("Chrono Disc", audio=2, serial="T-11111 -00")
    t.add_all("", f)
    t.unit("cd-megacd-multitrack", "Chrono Disc", list(f), "Sure", "Header", "megacd",
           moves=[(n, "megacd/Chrono Disc/" + n) for n in sorted(f)],
           text="SEGADISCSYSTEM probe; .cue + 3 tracks move into a folder named after the .cue")
    f = psx_files("Playstation Mix", audio=2, serial="SLES-99901")
    t.add_all("", f)
    t.unit("cd-psx-multitrack", "Playstation Mix", list(f), "Sure", "Header", "psx",
           moves=[(n, "psx/Playstation Mix/" + n) for n in sorted(f)],
           text="PLAYSTATION descriptor probe; .cue + data + 2 audio tracks into a folder named after the .cue")
    f = psx_files("Playstation Mono", serial="SLUS-99902", region="U")
    t.add_all("", f)
    t.unit("cd-psx-mono", "Playstation Mono", list(f), "Sure", "Header", "psx",
           moves=[(n, "psx/Playstation Mono/" + n) for n in sorted(f)], text="single-bin PS1 disc, folder created")
    f = opera_files("Opera Single")
    t.add_all("", f)
    t.unit("cd-3do-single", "Opera Single", list(f), "Sure", "Header", "3do",
           moves=[(n, "3do/Opera Single/" + n) for n in sorted(f)], text="Opera volume header probe")

    # ---- multi-CD without .m3u: (Disc 1) / (Disc 2) -----------------------------------------------------------
    f = psx_files("Twin Quest", discs=2, serial=None)
    t.add_all("", f)
    t.unit("multicd-psx-nom3u", "Twin Quest", list(f), "Sure", "Header", "psx",
           moves=[(n, "psx/Twin Quest/" + n) for n in sorted(f)],
           text="ADR 4.2: the whole (Disc N) group is ONE unit, ONE folder; the scan then shows one game",
           confidence="adr",
           note="folder of a (Disc N) group = name without the marker: psx/Twin Quest/")
    f = megacd_files("Mega Twin", discs=2, audio=1, serial="T-22222 -00")
    t.add_all("", f)
    t.unit("multicd-megacd-nom3u", "Mega Twin", list(f), "Sure", "Header", "megacd",
           moves=[(n, "megacd/Mega Twin/" + n) for n in sorted(f)],
           text="same rule on Mega CD (group_discs true), 2 cues + 4 tracks in one folder", confidence="adr",
           note="same rule on Mega CD: megacd/Mega Twin/")
    f = opera_files("Opera Twin", discs=2)
    t.add_all("", f)
    for k in (1, 2):
        names = ["Opera Twin (Disc %d).bin" % k, "Opera Twin (Disc %d).cue" % k]
        t.unit("multicd-3do-disc%d" % k, "Opera Twin (Disc %d)" % k, names, "Sure", "Header", "3do",
               moves=[(n, "3do/Opera Twin (Disc %d)/%s" % (k, n)) for n in names],
               text="3do has group_discs = false: one unit per disc")

    # ---- multi-CD WITH .m3u ----------------------------------------------------------------------------------
    f = psx_files("Playlist Quest", discs=2, serial=None)
    f["Playlist Quest.m3u"] = b"Playlist Quest (Disc 1).cue\r\nPlaylist Quest (Disc 2).cue\r\n"
    t.add_all("", f)
    t.unit("multicd-psx-m3u", "Playlist Quest", list(f), "Sure", "Header", "psx",
           moves=[(n, "psx/Playlist Quest/" + n) for n in sorted(f)],
           text=".m3u + its 2 discs and their tracks into a folder named after the .m3u", confidence="adr",
           note="an .m3u of discs is Header, through the .cue it lists")

    # ---- single-unit folder ----------------------------------------------------------------------------------
    f = psx_files("Folder CD", serial="SLES-99903")
    t.add_all("Folder CD", f)
    t.unit("folder-single-cd", "Folder CD", ["Folder CD/" + n for n in f], "Sure", "Header", "psx",
           whole_dir=("Folder CD", "psx/Folder CD"),
           moves=[("Folder CD/" + n, "psx/Folder CD/" + n) for n in sorted(f)],
           text="a folder holding exactly one folder-unit moves as is (one rename), under its own name",
           confidence="adr", note="moves[] is the file-level view; whole_dir (present) means one rename of the folder")

    # ---- two games, same track names, different folders -------------------------------------------------------
    for tag in ("A", "B"):
        d = megacd_files("same", audio=1, serial="T-3333%d -00" % (1 if tag == "A" else 2))
        t.add("Same Tracks %s/Track 1.bin" % tag, d["same (Track 1).bin"])
        t.add("Same Tracks %s/Track 2.bin" % tag, d["same (Track 2).bin"])
        t.add("Same Tracks %s/Game %s.cue" % (tag, tag),
              cue_text(("Track 1.bin", "MODE1/2352"), ("Track 2.bin", "AUDIO")))
        t.unit("same-tracks-" + tag.lower(), "Game " + tag,
               ["Same Tracks %s/%s" % (tag, n) for n in ("Track 1.bin", "Track 2.bin", "Game %s.cue" % tag)],
               "Sure", "Header", "megacd", whole_dir=("Same Tracks " + tag, "megacd/Same Tracks " + tag),
               moves=[("Same Tracks %s/%s" % (tag, n), "megacd/Same Tracks %s/%s" % (tag, n))
                      for n in sorted(("Track 1.bin", "Track 2.bin", "Game %s.cue" % tag))],
               text="same track file names in two folders never clash: each folder moves whole",
               confidence="adr", note="moves[] is the file-level view; whole_dir (present) means one rename of the folder")

    # ---- orphan .bin tracks (no .cue) ------------------------------------------------------------------------
    d = megacd_files("Orphan Set", audio=2, serial="T-44444 -00")
    for n in ("Orphan Set (Track 1).bin", "Orphan Set (Track 2).bin", "Orphan Set (Track 3).bin"):
        t.add(n, d[n])
    names = ["Orphan Set (Track %d).bin" % k for k in (1, 2, 3)]
    for k, n in enumerate(names, 1):
        t.unit("orphan-set-track-%d" % k, "Orphan Set (Track %d)" % k, [n], "Blocked", "UngroupedTracks", None,
               text="Sega CD track with no .cue: grouping is only done for PS1 / 3DO with data and audio tracks",
               confidence="adr",
               note="no megacd/Orphan Set folder: one Blocked(UngroupedTracks) unit per track, nothing moves")
    t.add("Lonely (Track 2).bin", mkcd.audio_track(8, "silence"))
    t.unit("orphan-audio-alone", "Lonely (Track 2)", ["Lonely (Track 2).bin"], "Blocked", "UngroupedTracks", None,
           text="an audio track with no data track: grouping impossible", confidence="adr",
           note="may be Unknown instead (a .bin without header = megadrive by extension?): FB1a / architect")
    t.add("Plain Orphan.bin", mkcd.data_track(mkcd.megacd_sector0(), 16, 2352))
    t.unit("orphan-megacd-alone", "Plain Orphan", ["Plain Orphan.bin"], "Blocked", "UngroupedTracks", None,
           text="a lone Sega CD data track without .cue", confidence="adr",
           note="Blocked(UngroupedTracks) vs Unknown vs grouped as a one-file CD: not decided by the ADR")
    ps = psx_files("tmp", serial="SLES-99904")
    t.add("Psx Orphan.bin", ps["tmp.bin"])
    t.unit("orphan-psx-alone", "Psx Orphan", ["Psx Orphan.bin"], "Sure", "Header", "psx",
           moves=[("Psx Orphan.bin", "psx/Psx Orphan.bin")],
           text="a PlayStation data track alone is a game for the scan (PLAYSTATION descriptor)",
           confidence="adr", note="a lone PS1 data track is a flat file psx/x.bin (a folder only when audio tracks of the same radical exist)")

    # ---- fake .chd + .sbi ------------------------------------------------------------------------------------
    t.add("Mystery Disc.chd", fake_chd())
    t.add("Mystery Disc.sbi", fake_sbi())
    t.unit("chd-sbi", "Mystery Disc", ["Mystery Disc.chd", "Mystery Disc.sbi"], "Ambiguous", None, None,
           candidates=["megacd", "dos", "psx", "3do"],
           text=".chd is shared by four systems and cannot be probed here; the .sbi travels with it",
           confidence="adr", note="candidates [megacd, dos, psx, 3do]; the moves are empty until a system is chosen")

    # ---- floppy groups ---------------------------------------------------------------------------------------
    names = ["Rainbow Quest (Disk %d of 3).st" % k for k in (1, 2, 3)]
    for n in names:
        t.add(n, atari_st())
    t.unit("st-3disks", "Rainbow Quest", names, "Sure", "Extension", "atarist",
           moves=[(n, "atarist/" + n) for n in names], text="group_media: 3 disks side by side, flat")
    names = ["Space Demo (Disk %d of 2).adf" % k for k in (1, 2)]
    for k, n in enumerate(names, 1):
        t.add(n, mkadf.blank_adf("SPACE%d" % k, EPOCH))
    t.unit("adf-2disks", "Space Demo", names, "Sure", "Extension", "amiga",
           moves=[(n, "amiga/" + n) for n in names], text="group_media: 2 ADF disks side by side, flat")

    # ---- DOS game folder -------------------------------------------------------------------------------------
    t.add("Dos Game/GAME.EXE", mkdos.fake_exe("GAME"))
    t.add("Dos Game/DATA/LEVEL1.DAT", bytes(range(256)))
    t.add("Dos Game/README.TXT", b"fake DOS game folder\r\n")
    dos = ["Dos Game/DATA/LEVEL1.DAT", "Dos Game/GAME.EXE", "Dos Game/README.TXT"]
    t.unit("dos-folder", "Dos Game", dos, "Suggested", "DosFolder", "dos", whole_dir=("Dos Game", "dos/Dos Game"),
           moves=[(n, "dos/" + n) for n in dos],
           text="folder with an .exe at its root: Suggested (unchecked), indivisible, moved by one rename",
           confidence="adr", note="moves[] is the file-level view; whole_dir (present) means one rename of the folder")

    # ---- any other folder: files one by one ------------------------------------------------------------------
    t.add("Misc Folder/Alpha.nes", nes(1, 1, 3))
    t.add("Misc Folder/Beta.sms", sega8())
    t.single("misc-alpha", "Alpha", "Misc Folder/Alpha.nes", "nes", text="folder is neither CD nor DOS: file by file")
    t.single("misc-beta", "Beta", "Misc Folder/Beta.sms", "mastersystem", text="the emptied folder is removed")

    # ---- BIOS-like by name only, unknown ---------------------------------------------------------------------
    t.add("scph1001.bin", bytes((i * 7 + 3) & 0xFF for i in range(512 * 1024)))
    t.firmware_names.append("scph1001.bin")
    t.ignored.append({"path": "scph1001.bin", "reason": "firmware_name"})
    t.add("Headerless.bin", bytes((i * 11 + 5) & 0xFF for i in range(4096)))
    t.unit("headerless-bin", "Headerless", ["Headerless.bin"], "Suggested", "Extension", "megadrive",
           moves=[("Headerless.bin", "megadrive/Headerless.bin")],
           text="a .bin without a Sega header is never Sure: Suggested(Extension), shown unchecked", confidence="adr",
           note="deviation from ADR rule 5 chosen in FB1a; BIOS names are handled by firmware_names, not by a block")
    t.add("kick34005.A500", bytes((i * 5 + 1) & 0xFF for i in range(4096)))
    t.ignored.append({"path": "kick34005.A500", "reason": "not_a_game"})
    t.add("notes.txt", b"some notes, not a game\r\n")
    t.ignored.append({"path": "notes.txt", "reason": "not_a_game"})
    t.add("mystery.xyz", bytes(range(200)))
    t.ignored.append({"path": "mystery.xyz", "reason": "not_a_game"})

    # ---- name collisions -------------------------------------------------------------------------------------
    same = snes("DUPE GAME")
    t.add("Dupe Game.sfc", same)
    t.add("snes/Dupe Game.sfc", same)
    t.ignored.append({"path": "snes/Dupe Game.sfc", "reason": "already_sorted"})
    t.unit("collision-duplicate", "Dupe Game", ["Dupe Game.sfc"], "Blocked", "Duplicate", "snes",
           moves=[("Dupe Game.sfc", "snes/Dupe Game.sfc")],
           text="destination snes/Dupe Game.sfc exists with the same size and CRC32: stays, reported as a duplicate")
    t.add("Clash Game.nes", nes(1, 1, 4))
    t.add("nes/Clash Game.nes", nes(1, 1, 5))
    t.ignored.append({"path": "nes/Clash Game.nes", "reason": "already_sorted"})
    t.unit("collision-conflict", "Clash Game", ["Clash Game.nes"], "Blocked", "Conflict", "nes",
           moves=[("Clash Game.nes", "nes/Clash Game.nes")],
           text="destination nes/Clash Game.nes exists with other content: stays, conflict")
    t.add("CASE Game.sfc", snes("CASE UPPER"))
    t.add("snes/case game.sfc", snes("CASE LOWER"))
    t.ignored.append({"path": "snes/case game.sfc", "reason": "already_sorted"})
    t.unit("collision-case", "CASE Game", ["CASE Game.sfc"], "Blocked", "Conflict", "snes",
           moves=[("CASE Game.sfc", "snes/CASE Game.sfc")],
           text="destination differs only by case from snes/case game.sfc: compared case-insensitively, other content")
    t.add("Sub A/Twin.sms", sega8())
    t.add("Sub B/Twin.sms", bytes(sega8())[:-1] + b"\x01")
    t.unit("collision-plan-a", "Twin", ["Sub A/Twin.sms"], "Sure", "Extension", "mastersystem",
           moves=[("Sub A/Twin.sms", "mastersystem/Twin.sms")],
           text="two units target mastersystem/Twin.sms: the first in plan order keeps it", confidence="adr",
           note="only the 2nd unit is blocked")
    t.unit("collision-plan-b", "Twin", ["Sub B/Twin.sms"], "Blocked", "Conflict", "mastersystem",
           moves=[("Sub B/Twin.sms", "mastersystem/Twin.sms")],
           text="see collision-plan-a", confidence="adr", note="only the 2nd unit is blocked")
    t.add("gba/Sorted Already.gba", gba("SORTED"))
    t.ignored.append({"path": "gba/Sorted Already.gba", "reason": "already_sorted"})

    # ---- blocked CDs -----------------------------------------------------------------------------------------
    t.add("Broken Missing/ghost.cue", cue_text(("ghost.bin", "MODE1/2352")))
    t.unit("cue-missing-ref", "ghost", ["Broken Missing/ghost.cue"], "Blocked", "MissingReference", None,
           text=".cue naming a track that does not exist", confidence="adr",
           note="OutsideReference is checked before MissingReference; a cue whose first track is missing has no system")
    t.add("Broken Absolute/abs.cue", cue_text(("/abs/path/track.bin", "MODE1/2352")))
    t.unit("cue-absolute-ref", "abs", ["Broken Absolute/abs.cue"], "Blocked", "OutsideReference", None,
           text=".cue with an absolute FILE reference", confidence="adr",
           note="OutsideReference comes before MissingReference (an absolute path that does not exist is Outside)")
    sh = megacd_files("shared", serial="T-55555 -00")
    t.add("Shared Tracks/shared.bin", sh["shared.bin"])
    t.add("Shared Tracks/Shared One.cue", cue_text(("shared.bin", "MODE1/2352")))
    t.add("Shared Tracks/Shared Two.cue", cue_text(("shared.bin", "MODE1/2352")))
    t.unit("cue-shared-track-1", "Shared One", ["Shared Tracks/Shared One.cue", "Shared Tracks/shared.bin"], "Blocked",
           "SharedTrack", None, text="track cited by two .cue: both units blocked", confidence="adr",
           note="shared.bin appears in both units' files; a file belongs to one unit, so which unit lists it is "
                "FB1a's choice")
    t.unit("cue-shared-track-2", "Shared Two", ["Shared Tracks/Shared Two.cue", "Shared Tracks/shared.bin"], "Blocked",
           "SharedTrack", None, text="see cue-shared-track-1", confidence="adr", note="see cue-shared-track-1")
    sysname = psx_files("nes", serial="SLES-99905")
    t.add_all("", sysname)
    t.unit("folder-named-like-system", "nes", list(sysname), "Blocked", "FolderNamedLikeSystem", "psx",
           moves=[("nes.cue", "psx/nes/nes.cue"), ("nes.bin", "psx/nes/nes.bin")],
           text="the folder to create, psx/nes/, is named after a system id: it would change the scan's folder rule",
           confidence="adr", note="the folder to create, psx/nes/, is named after a system id")

    # ---- symbolic link ---------------------------------------------------------------------------------------
    t.add("Real SNES.sfc", snes("REAL SNES"))
    t.single("snes-real", "Real SNES", "Real SNES.sfc", "snes", text="target of the link below: a normal file")
    if t.symlink:
        t.add("Link to SNES.sfc", ("link", "Real SNES.sfc"))
        t.unit("symlink", "Link to SNES.sfc", ["Link to SNES.sfc"], "Blocked", "Symlink", None,
               text="never followed, never moved: left in place and reported", requires="unix")

    # ---- very long name (Windows 240-char limit) -------------------------------------------------------------
    stem = ("Very Long Name " * 20)[:225] + "X"
    longname = stem + ".sfc"
    assert len(longname) == 230
    t.add(longname, snes("LONG"))
    t.unit("long-name", "Very Long Name", [longname], "Sure", "Extension", "snes",
           moves=[(longname, "snes/" + longname)], windows={"status": "Blocked", "reason": "PathTooLong"},
           text="230-character name: fine on Unix; on Windows 'root + snes/ + name' exceeds 240 characters",
           confidence="adr",
           note="PathTooLong only applies on Windows (root included in the length)")


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("outdir")
    p.add_argument("--no-symlink", action="store_true", help="skip the symbolic link (forced on non-Unix systems)")
    a = p.parse_args(argv)
    if os.path.exists(os.path.join(a.outdir, "roms")):
        sys.exit("%s/roms already exists" % a.outdir)
    os.makedirs(a.outdir, exist_ok=True)
    symlink = os.name == "posix" and not a.no_symlink
    t = Tree(a.outdir, symlink)
    build(t)
    t.write()
    ids = [u["id"] for u in t.units]
    assert len(ids) == len(set(ids)), "duplicate unit ids"
    manifest = {
        "version": 1,
        "units": sorted(t.units, key=lambda u: u["id"]),
        "firmware_names": sorted(t.firmware_names),
        "ignored": sorted(t.ignored, key=lambda i: i["path"]),
        "tree": sorted(t.files),
        "platform": {"symlink": symlink},
    }
    with open(os.path.join(a.outdir, "manifest.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(manifest, f, indent=2, sort_keys=True, ensure_ascii=False)
        f.write("\n")


if __name__ == "__main__":
    main(sys.argv[1:])
