"""Realistic 1.0 / 2.0 / 2.0.0-rc.1 save trees for the 2.1 game-identity tests (stdlib only, deterministic).

usage: python3 -I mksaves.py PROFILE OUTDIR [CORE_ID]      PROFILE = 1.0 | 2.0 | rc1 (each includes the previous)

Layout under OUTDIR (the shape `EmulatorOptions` expects): roms/, data/saves/<core>/, data/states/<core>/.
CORE_ID defaults to `default` (core id of the mock core `default.so`). Formats mirror crates/emu-runtime:
states = saves.rs `encode_state` without `game_fp` (what 1.0 / 2.0 wrote), manifests = workcopy.rs `encode`.

  1.0   Zelda (own .srm + 2 states), Twin (two different ROMs in roms/a and roms/b that shared Twin.srm/state0).
  2.0   + Game.disks (manifest: set Game.st, added Extra.st), `Game (2).disks` (manifest, another Game.st).
  rc1   + rc.1 leftovers: `02 X.st` copy outside the manifest and a stale Game.m3u in Game.disks, and Old.disks
          (no manifest at all: folder of an earlier rc) holding Old.st, `02 Y.st`, Old.m3u.
"""
import os
import struct
import sys
import zlib

PROFILES = ("1.0", "2.0", "rc1")
SRAM_SIZE = 2048  # mock core SAVE_RAM
STATE_SIZE = 64  # mock core serialize size
CORE_VERSION = "1.0"  # mock core library_version
MANIFEST = ".emu-rust-disks"
MANIFEST_HEADER = "emu-rust working copies 1"


def blob(tag, size):
    """Deterministic, tag-dependent bytes."""
    out, counter = bytearray(), 0
    while len(out) < size:
        out += zlib.crc32(f"{tag}:{counter}".encode()).to_bytes(4, "little")
        counter += 1
    return bytes(out[:size])


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def encode_state(core_id, game, created, payload):
    """STATE_MAGIC | u32 version 1 | u32 header len | header | u64 len | payload | u64 FNV-1a 64."""
    header = f"core_id={core_id}\ncore_version={CORE_VERSION}\ngame={game}\ncreated_unix={created}\n".encode()
    return (
        b"EMURSTAT"
        + struct.pack("<II", 1, len(header))
        + header
        + struct.pack("<Q", len(payload))
        + payload
        + struct.pack("<Q", fnv1a64(payload))
    )


def escape(text):
    return "".join(f"%{ord(c):02X}" if ord(c) < 0x20 or ord(c) == 0x7F or c == "%" else c for c in text)


def manifest(entries):
    """entries: (origin, copy_name, data, source_path). mtime unknown (`-`), as for a copy whose source vanished."""
    lines = [MANIFEST_HEADER]
    for origin, copy, data, source in entries:
        lines.append("\t".join([origin, escape(copy), "", str(len(data)), "-", f"{zlib.crc32(data):08x}", escape(source)]))
    return ("\n".join(lines) + "\n").encode()


def build(profile, root, core_id="default"):
    if profile not in PROFILES:
        raise ValueError(f"unknown profile {profile!r}")
    level = PROFILES.index(profile)
    root = os.path.abspath(root)
    saves = os.path.join(root, "data", "saves", core_id)
    states = os.path.join(root, "data", "states", core_id)
    files = {}

    def put(path, data):
        files[path] = data

    def rom(rel, data):
        path = os.path.join(root, "roms", rel)
        put(path, data)
        return path

    def state(game, slot, tag, created):
        put(os.path.join(states, f"{game}.state{slot}"), encode_state(core_id, game, created, blob(tag, STATE_SIZE)))

    # 1.0
    rom("solo/Zelda.bin", blob("rom-zelda", 4096))
    put(os.path.join(saves, "Zelda.srm"), blob("srm-zelda", SRAM_SIZE))
    state("Zelda", 0, "st-zelda-0", 1_700_000_000)
    state("Zelda", 3, "st-zelda-3", 1_700_000_300)
    rom("a/Twin.bin", blob("rom-twin-a", 4096))
    rom("b/Twin.bin", blob("rom-twin-b", 4096))
    put(os.path.join(saves, "Twin.srm"), blob("srm-twin", SRAM_SIZE))
    state("Twin", 0, "st-twin-0", 1_700_001_000)

    if level >= 1:  # 2.0: working copies with a manifest
        game1 = blob("disk-game", 4096)
        game2 = blob("disk-game-other", 4096)
        extra = blob("disk-extra", 2048)
        src1 = rom("st/Game.st", game1)
        src2 = rom("st2/Game.st", game2)
        srcx = rom("extra/Extra.st", extra)
        d1 = os.path.join(saves, "Game.disks")
        put(os.path.join(d1, "Game.st"), game1 + b"\x00progress")  # the copy has moved on from the source
        put(os.path.join(d1, "Extra.st"), extra)
        put(os.path.join(d1, MANIFEST), manifest([("set", "Game.st", game1, src1), ("added", "Extra.st", extra, srcx)]))
        d2 = os.path.join(saves, "Game (2).disks")
        put(os.path.join(d2, "Game.st"), game2)
        put(os.path.join(d2, MANIFEST), manifest([("set", "Game.st", game2, src2)]))
        put(os.path.join(saves, "Game.srm"), blob("srm-game", SRAM_SIZE))

    if level >= 2:  # rc.1 leftovers
        d1 = os.path.join(saves, "Game.disks")
        put(os.path.join(d1, "02 X.st"), blob("rc1-x", 3072))
        put(os.path.join(d1, "Game.m3u"), b"Game.st\n02 X.st\n")
        old = os.path.join(saves, "Old.disks")
        rom("old/Old.st", blob("disk-old", 4096))
        put(os.path.join(old, "Old.st"), blob("disk-old", 4096))
        put(os.path.join(old, "02 Y.st"), blob("rc1-y", 3072))
        put(os.path.join(old, "Old.m3u"), b"Old.st\n02 Y.st\n")

    for path, data in files.items():
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)
    return sorted(files)


def main(argv):
    if len(argv) not in (3, 4):
        print(__doc__, file=sys.stderr)
        return 2
    build(argv[1], argv[2], argv[3] if len(argv) == 4 else "default")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
