"""Self-contained tests for mksaves.py: run `python3 -I test_mksaves.py`."""
import importlib.util
import os
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("mksaves", os.path.join(HERE, "mksaves.py"))
mk = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(mk)


def tree(root):
    """Relative path -> bytes; manifests hold absolute source paths, so the root is replaced by `@`."""
    out = {}
    for d, _, names in os.walk(root):
        for n in names:
            p = os.path.join(d, n)
            with open(p, "rb") as f:
                out[os.path.relpath(p, root)] = f.read().replace(root.encode(), b"@") if n == ".emu-rust-disks" else f.read()
    return out


def decode_state(b):
    assert b[:8] == b"EMURSTAT"
    version, hlen = struct.unpack_from("<II", b, 8)
    assert version == 1
    header = dict(l.split("=", 1) for l in b[16:16 + hlen].decode().splitlines())
    (plen,) = struct.unpack_from("<Q", b, 16 + hlen)
    payload = b[24 + hlen:24 + hlen + plen]
    (ck,) = struct.unpack_from("<Q", b, 24 + hlen + plen)
    assert ck == mk.fnv1a64(payload) and len(b) == 32 + hlen + plen
    return header, payload


class MkSaves(unittest.TestCase):
    def gen(self, profile):
        d = tempfile.mkdtemp()
        self.addCleanup(lambda: subprocess.run(["rm", "-rf", d]))
        subprocess.run([sys.executable, "-I", os.path.join(HERE, "mksaves.py"), profile, d], check=True)
        return d

    def test_profiles_are_nested_and_deterministic(self):
        trees = {p: tree(self.gen(p)) for p in mk.PROFILES}
        for a, b in zip(mk.PROFILES, mk.PROFILES[1:]):
            self.assertTrue(set(trees[a]) < set(trees[b]))
            for name, data in trees[a].items():
                self.assertEqual(data, trees[b][name], name)
        self.assertEqual(trees["rc1"], tree(self.gen("rc1")))

    def test_states_follow_the_2_0_container_without_game_fp(self):
        t = tree(self.gen("1.0"))
        header, payload = decode_state(t["data/states/default/Zelda.state0"])
        self.assertEqual((header["core_id"], header["game"], len(payload)), ("default", "Zelda", mk.STATE_SIZE))
        self.assertNotIn("game_fp", header)
        self.assertEqual(len(t["data/saves/default/Zelda.srm"]), mk.SRAM_SIZE)

    def test_homonyms_differ_and_share_files(self):
        t = tree(self.gen("1.0"))
        self.assertNotEqual(t["roms/a/Twin.bin"], t["roms/b/Twin.bin"])
        self.assertIn("data/saves/default/Twin.srm", t)

    def test_manifest_matches_sources_and_leftovers(self):
        d = self.gen("rc1")
        t = tree(d)
        text = t["data/saves/default/Game.disks/.emu-rust-disks"].decode()
        lines = text.splitlines()
        self.assertEqual(lines[0], mk.MANIFEST_HEADER)
        self.assertEqual([l.split("\t")[0] for l in lines[1:]], ["set", "added"])
        for line in lines[1:]:
            origin, copy, entry, size, mtime, crc, source = line.split("\t")
            with open(source.replace("@", d), "rb") as f:
                src = f.read()
            self.assertEqual((int(size), crc), (len(src), f"{zlib.crc32(src):08x}"))
        self.assertIn("data/saves/default/Game.disks/02 X.st", t)
        self.assertIn("data/saves/default/Game.disks/Game.m3u", t)
        self.assertNotIn("data/saves/default/Old.disks/.emu-rust-disks", t)
        self.assertIn("data/saves/default/Old.disks/02 Y.st", t)

    def test_unknown_profile_is_refused(self):
        with self.assertRaises(ValueError):
            mk.build("3.0", tempfile.mkdtemp())


if __name__ == "__main__":
    unittest.main()
