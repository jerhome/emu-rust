"""Self-contained checks of mktidy.py: python3 -I test_mktidy.py (determinism, manifest vs tree, mkcd probes)."""
import filecmp
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
MKTIDY = os.path.join(HERE, "mktidy.py")
STATUSES = {"Sure", "Suggested", "Ambiguous", "Blocked", "Unknown"}
REASONS = {"Library", "Header", "DatHash", "ArchiveContent", "DatName", "Extension", "DosFolder"}
BLOCKS = {"Conflict", "Duplicate", "MissingReference", "OutsideReference", "SharedTrack", "UngroupedTracks", "Symlink",
          "DiscInArchive", "DestinationNotPlainDir", "OtherVolume", "NoHardLinks", "NonUtf8Path", "PathTooLong",
          "FolderNamedLikeSystem", "ChangedSincePlan"}


def gen(d, *args):
    subprocess.run([sys.executable, "-I", MKTIDY, d, *args], check=True)
    with open(os.path.join(d, "manifest.json"), encoding="utf-8") as f:
        return json.load(f)


class Tidy(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.d = os.path.join(cls.tmp.name, "a")
        cls.m = gen(cls.d)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_deterministic(self):
        other = os.path.join(self.tmp.name, "b")
        gen(other)
        cmp = filecmp.dircmp(self.d, other)
        self.assertFalse(cmp.diff_files or cmp.left_only or cmp.right_only)
        for root, _dirs, files in os.walk(os.path.join(self.d, "roms")):
            for n in files:
                a = os.path.join(root, n)
                b = os.path.join(other, os.path.relpath(a, self.d))
                self.assertEqual(os.lstat(a).st_mtime, os.lstat(b).st_mtime, a)
                if not os.path.islink(a):
                    self.assertTrue(filecmp.cmp(a, b, shallow=False), a)

    def test_refuses_existing_roms(self):
        r = subprocess.run([sys.executable, "-I", MKTIDY, self.d], capture_output=True)
        self.assertNotEqual(r.returncode, 0)

    def test_manifest_vocabulary(self):
        ids = [u["id"] for u in self.m["units"]]
        self.assertEqual(len(ids), len(set(ids)))
        for u in self.m["units"]:
            self.assertIn(u["status"], STATUSES, u["id"])
            if u["status"] in ("Sure", "Suggested"):
                self.assertIn(u["reason"], REASONS, u["id"])
                self.assertTrue(u["system"] and u["moves"], u["id"])
            elif u["status"] == "Blocked":
                self.assertIn(u["reason"], BLOCKS, u["id"])
            elif u["status"] == "Ambiguous":
                self.assertTrue(u["candidates"] and not u["moves"], u["id"])
            else:
                self.assertTrue(u["reason"] is None and u["destination"] is None, u["id"])
            if u["confidence"] != "adr":
                self.assertTrue(u.get("note"), u["id"])

    def test_every_file_is_covered(self):
        in_units = {f for u in self.m["units"] for f in u["files"]}
        ignored = {i["path"] for i in self.m["ignored"]}
        tree = set(self.m["tree"])
        self.assertEqual(in_units & ignored, set())
        self.assertEqual(tree - in_units - ignored, set())
        self.assertEqual((in_units | ignored) - tree, set())
        for rel in tree:
            self.assertTrue(os.path.lexists(os.path.join(self.d, "roms", *rel.split("/"))), rel)

    def test_moves_stay_inside_the_unit(self):
        for u in self.m["units"]:
            self.assertEqual({m["from"] for m in u["moves"]} - set(u["files"]), set(), u["id"])
            for m in u["moves"]:
                self.assertTrue(m["to"].startswith(u["system"] + "/"), u["id"])

    def test_required_content(self):
        have = {u["id"] for u in self.m["units"]}
        for needed in ("snes-sfc", "md-md", "gb-ambiguous", "gbc-gbc", "gba-gba", "nes-nes", "sms-sms", "gg-gg",
                       "x32-32x", "zip-snes", "cd-megacd-multitrack", "cd-psx-multitrack", "multicd-psx-nom3u",
                       "multicd-3do-disc1", "multicd-3do-disc2", "orphan-set-groupable", "chd-sbi", "st-3disks",
                       "adf-2disks", "dos-folder", "bios-name-only", "unknown-file", "collision-conflict",
                       "collision-duplicate", "long-name", "symlink"):
            self.assertIn(needed, have)
        self.assertIn("Mystery Disc.sbi", self.m["tree"])

    def test_headers(self):
        def rd(rel):
            with open(os.path.join(self.d, "roms", rel), "rb") as f:
                return f.read()

        self.assertEqual(rd("Famicom Test.nes")[:4], b"NES\x1a")
        self.assertEqual(rd("Mega Test.md")[0x100:0x104], b"SEGA")
        self.assertEqual(rd("Super Test.32x")[0x100:0x108], b"SEGA 32X")
        self.assertEqual(rd("Master Test.sms")[0x7FF0:0x7FF8], b"TMR SEGA")
        self.assertEqual(rd("Advance Test.gba")[0xB2], 0x96)
        self.assertEqual(rd("Chrono Disc (Track 1).bin")[16:30], b"SEGADISCSYSTEM")
        self.assertEqual(rd("Mystery Disc.chd")[:8], b"MComprHD")
        self.assertEqual(rd("Opera Single.bin")[16:23], b"\x01ZZZZZ\x01")

    def test_symlink_and_long_name(self):
        link = os.path.join(self.d, "roms", "Link to SNES.sfc")
        self.assertTrue(os.path.islink(link))
        long_unit = next(u for u in self.m["units"] if u["id"] == "long-name")
        self.assertEqual(len(long_unit["files"][0]), 230)
        self.assertLess(len(long_unit["files"][0]), 255)

    def test_no_symlink_option(self):
        d = os.path.join(self.tmp.name, "n")
        m = gen(d, "--no-symlink")
        self.assertNotIn("symlink", {u["id"] for u in m["units"]})
        self.assertFalse(m["platform"]["symlink"])


if __name__ == "__main__":
    unittest.main()
