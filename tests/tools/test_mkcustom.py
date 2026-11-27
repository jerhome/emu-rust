"""Self-contained checks of mkcustom.py: python3 -I test_mkcustom.py (determinism, manifest vs tree, TOML validity)."""
import filecmp
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest

try:
    import tomllib
except ImportError:  # Python < 3.11: TOML checks are skipped
    tomllib = None

HERE = os.path.dirname(os.path.abspath(__file__))
MKCUSTOM = os.path.join(HERE, "mkcustom.py")
VARIANTS = {"Syntax", "TooLarge", "NotRegularFile", "NewerFormat", "UnsupportedRender", "InvalidId", "ReservedId",
            "IdNotFileName", "DuplicateId", "DuplicateDatabase", "InvalidName", "InvalidDatabase", "NoExtension",
            "InvalidExtension", "ForbiddenExtension", "UnsupportedDat", "NoCore", "TooManyCores", "InvalidCore",
            "InvalidOrigin", "UnsafeFirmwarePath", "TooManyFirmware", "TooManyDefinitions"}
SPECIAL = os.name == "posix"


def gen(d, *args):
    subprocess.run([sys.executable, "-I", MKCUSTOM, d, *args], check=True)
    with open(os.path.join(d, "manifest.json"), encoding="utf-8") as f:
        return json.load(f)


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


class Custom(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.d = os.path.join(cls.tmp.name, "a")
        cls.m = gen(cls.d)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def case(self, name):
        return [c for c in self.m["definitions"] if c["case"] == name][0]

    def test_deterministic(self):
        other = os.path.join(self.tmp.name, "b")
        gen(other)
        cmp = filecmp.dircmp(self.d, other)
        self.assertFalse(cmp.diff_files or cmp.left_only or cmp.right_only)
        self.assertEqual(read(os.path.join(self.d, "manifest.json")), read(os.path.join(other, "manifest.json")))

    def test_refuses_existing_output(self):
        r = subprocess.run([sys.executable, "-I", MKCUSTOM, self.d], capture_output=True)
        self.assertNotEqual(r.returncode, 0)

    def test_every_variant_but_duplicate_id_has_a_file(self):
        covered = {e["error"] for c in self.m["definitions"] for e in c["expect"]["errors"]}
        self.assertEqual(VARIANTS - covered, {"DuplicateId"})

    def test_duplicate_id_is_api_only(self):
        self.assertTrue(self.case("duplicate-id")["api_only"])

    def test_files_match_manifest(self):
        for c in self.m["definitions"]:
            on_disk = sorted(os.listdir(os.path.join(self.d, c["dir"])))
            self.assertEqual(on_disk, c["files"], c["case"])
        many = self.case("many-300")
        self.assertEqual(len(many["files"]), 300)
        self.assertEqual(len(many["ignored"]), 44)
        self.assertEqual(len(many["expect"]["ok"]), 256)
        for i in self.m["info"]:
            self.assertTrue(os.path.isfile(os.path.join(self.d, i["file"])), i["file"])
        for t in self.m["trees"]:
            self.assertTrue(os.path.isdir(os.path.join(self.d, t["dir"])), t["case"])

    def test_errors_name_the_file_stem(self):
        for c in self.m["definitions"]:
            stems = {f[:-5] for f in c["files"] if f.endswith(".toml")}
            for e in c["expect"]["errors"]:
                self.assertIn(e["id"], stems, c["case"])
            if c["case"] != "duplicate-id":
                for ident in c["expect"]["ok"]:
                    self.assertIn(ident, stems, c["case"])

    def test_special_files_are_not_regular(self):
        if not SPECIAL:
            self.skipTest("posix only")
        link = os.path.join(self.d, self.case("special-symlink")["dir"], "link.toml")
        pipe = os.path.join(self.d, self.case("special-fifo")["dir"], "pipe.toml")
        self.assertTrue(stat.S_ISLNK(os.lstat(link).st_mode))
        self.assertTrue(stat.S_ISFIFO(os.lstat(pipe).st_mode))
        self.assertEqual(self.case("special-fifo")["requires"], "unix")

    def test_no_special_skips_them(self):
        d = os.path.join(self.tmp.name, "nospecial")
        m = gen(d, "--no-special")
        names = {c["case"] for c in m["definitions"]}
        self.assertFalse({"special-symlink", "special-fifo"} & names)
        self.assertFalse(m["platform"]["special"])

    def test_sizes(self):
        path = os.path.join(self.d, self.case("e-toolarge")["dir"], "big.toml")
        self.assertGreater(os.path.getsize(path), 64 * 1024)
        for c in self.m["definitions"]:
            if c["case"] == "e-toolarge":
                continue
            for f in c["files"]:
                p = os.path.join(self.d, c["dir"], f)
                if os.path.isfile(p) and not os.path.islink(p):
                    self.assertLess(os.path.getsize(p), 64 * 1024, (c["case"], f))

    def test_toml_validity(self):
        if tomllib is None:
            self.skipTest("tomllib needs Python 3.11")
        for c in self.m["definitions"]:
            for f in c["files"]:
                p = os.path.join(self.d, c["dir"], f)
                if not os.path.isfile(p) or os.path.islink(p):
                    continue
                text = read(p)
                if c["case"] == "e-syntax":
                    with self.assertRaises(tomllib.TOMLDecodeError):
                        tomllib.loads(text)
                else:
                    self.assertIn("system", tomllib.loads(text), (c["case"], f))  # errors are semantic, not TOML
        for name in ("official/systems.toml", "official/cores.toml", "keys/official-profile.toml"):
            tomllib.loads(read(os.path.join(self.d, name)))

    def test_control_characters_are_toml_escapes(self):
        if tomllib is None:
            self.skipTest("tomllib needs Python 3.11")
        for case, ch in (("e-invalid-name-control", chr(7)), ("e-invalid-name-bidi", chr(0x202E))):
            c = self.case(case)
            doc = tomllib.loads(read(os.path.join(self.d, c["dir"], c["files"][0])))
            self.assertIn(ch, doc["system"]["name"])

    def test_info_cases_are_synthetic(self):
        for i in self.m["info"]:
            text = read(os.path.join(self.d, i["file"]))
            self.assertIn("Demo", text)
            self.assertIn('license = "Public Domain"', text)

    def test_hardware_info_cases(self):
        hw = read(os.path.join(self.d, "info", "hw-three-apis.info"))
        self.assertEqual(hw.count("|"), 2)
        self.assertIn('hw_render = "true"', hw)
        self.assertNotIn("required_hw_api", read(os.path.join(self.d, "info", "hw-no-api.info")))


if __name__ == "__main__":
    unittest.main()
