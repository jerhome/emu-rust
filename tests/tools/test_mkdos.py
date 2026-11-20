"""Self-contained tests for mkdos.py: run `python3 -I test_mkdos.py`."""
import hashlib
import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import unittest
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("mkdos", os.path.join(HERE, "mkdos.py"))
mkdos = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(mkdos)


def names(blob):
    return sorted(zipfile.ZipFile(io.BytesIO(blob)).namelist())


class Dos(unittest.TestCase):
    def test_hello_com_bytes(self):
        self.assertEqual(len(mkdos.HELLO_COM), 36)
        self.assertEqual(hashlib.sha256(mkdos.HELLO_COM).hexdigest(),
                         "ccd70355ebcc1b2438568850ddd6755449905ef7a26ed7e6b9a0ef187491a68d")

    def test_zip_shapes(self):
        c = mkdos.all_cases()
        self.assertEqual(names(c["hello.zip"]), ["HELLO.COM"])
        self.assertEqual(names(c["hello-rootdir.zip"]), ["HELLO/", "HELLO/HELLO.COM"])
        self.assertEqual(names(c["menu.zip"]), ["MENU1.COM", "MENU2.COM"])
        self.assertEqual(names(c["empty.zip"]), [])
        self.assertIn("MULTI/DATA/SUB/LEVEL2.DAT", names(c["multi-rootdir.zip"]))
        self.assertIn("A Very Long Game Name.exe", names(c["longnames.zip"]))
        self.assertFalse(any(n.lower().endswith((".exe", ".com", ".bat")) for n in names(c["notdos.zip"])))
        for blob in c.values():
            if isinstance(blob, bytes) and blob[:2] == b"PK":
                self.assertIsNone(zipfile.ZipFile(io.BytesIO(blob)).testzip())

    def test_hostile_zips_contain_hostile_names(self):
        c = mkdos.all_cases()
        self.assertIn("../evil.txt", names(c["evil-zipslip.zip"]))
        self.assertIn("a/../../evil2.txt", names(c["evil-zipslip.zip"]))
        self.assertIn("/tmp/evil.txt", names(c["evil-absolute.zip"]))
        self.assertTrue(any("\\" in n for n in names(c["evil-backslash.zip"])))

    def test_zip_dates_fixed(self):
        z = zipfile.ZipFile(io.BytesIO(mkdos.all_cases()["hello.zip"]))
        self.assertEqual(z.infolist()[0].date_time[:3], (1980, 1, 1))

    def test_write_all_and_reproducible(self):
        a, b = tempfile.mkdtemp(), tempfile.mkdtemp()
        env = dict(os.environ, SOURCE_DATE_EPOCH="1700000000")
        for d in (a, b):
            subprocess.run([sys.executable, "-I", os.path.join(HERE, "mkdos.py"), d], check=True, env=env)

        def tree(root):
            out = {}
            for dp, dn, fn in os.walk(root):
                for n in fn:
                    p = os.path.join(dp, n)
                    out[os.path.relpath(p, root)] = open(p, "rb").read()
                if not fn and not dn:
                    out[os.path.relpath(dp, root) + "/"] = b""
            return out

        ta, tb = tree(a), tree(b)
        self.assertEqual(ta, tb)
        self.assertEqual(ta["hello/HELLO.COM"], mkdos.HELLO_COM)
        self.assertEqual(ta["multi/DATA/SUB/LEVEL2.DAT"], b"\x01" * 64)
        self.assertTrue(os.path.isdir(os.path.join(a, "empty")))
        # hostile zips are never extracted into folders
        self.assertFalse(any("evil" in k and not k.endswith(".zip") for k in ta))
        self.assertIn("../evil.txt", zipfile.ZipFile(os.path.join(a, "evil-zipslip.zip")).namelist())

    def test_iso(self):
        iso = mkdos.all_cases()["hello.iso"]
        self.assertEqual(iso[16 * 2048 + 1 : 16 * 2048 + 6], b"CD001")
        self.assertIn(b"HELLO.COM;1", iso)
        self.assertIn(mkdos.HELLO_COM, iso)


if __name__ == "__main__":
    unittest.main()
