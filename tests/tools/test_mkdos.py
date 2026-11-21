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

    def test_programs_bytes_are_pinned(self):
        # Hand-assembled bytes: any change must be deliberate (the commented source is mkdos.py).
        self.assertEqual(mkdos.KEYS_COM.hex(), "b400cd163c1b740888c2b402cd21ebf0b8004ccd21")
        self.assertEqual(len(mkdos.MOUSE_COM), 80)
        self.assertEqual(len(mkdos.MIDI_COM), 112)
        for prog in (mkdos.KEYS_COM, mkdos.MOUSE_COM, mkdos.MIDI_COM):
            self.assertLessEqual(len(prog), 256)  # tiny .COM, no relocation needed
            self.assertTrue(prog.endswith(b"\xc3") or prog.endswith(b"\xcd\x21"))
        self.assertEqual((mkdos.keys_com(), mkdos.mouse_com(), mkdos.midi_com()),
                         (mkdos.KEYS_COM, mkdos.MOUSE_COM, mkdos.MIDI_COM), "deterministic")

    def test_assembler_fixups(self):
        # nop / x: jmp short x / call x
        a = mkdos.Asm().b(0x90).label("x").jmp8(0xEB, "x").call("x")
        self.assertEqual(a.bytes(), bytes([0x90, 0xEB, 0xFE, 0xE8, 0xFB, 0xFF]))
        with self.assertRaises(AssertionError):
            mkdos.Asm().label("far").b(*([0x90] * 200)).jmp8(0xEB, "far").bytes()

    def test_program_cases(self):
        c = mkdos.all_cases()
        for prog, data in (("keys", mkdos.KEYS_COM), ("mouse", mkdos.MOUSE_COM), ("midi", mkdos.MIDI_COM)):
            self.assertEqual(c[prog], {prog.upper() + ".COM": data})
            self.assertEqual(names(c[prog + ".zip"]), [prog.upper() + ".COM"])

    def test_floppy_fat12(self):
        img = mkdos.all_cases()["hello.img"]
        self.assertEqual(img, mkdos.all_cases()["hello.ima"])
        self.assertEqual(len(img), 1474560)
        self.assertEqual(img[510:512], b"\x55\xaa")
        ss, spc, res, nfat, root_entries, total, media, spf = (
            int.from_bytes(img[11:13], "little"), img[13], int.from_bytes(img[14:16], "little"), img[16],
            int.from_bytes(img[17:19], "little"), int.from_bytes(img[19:21], "little"), img[21],
            int.from_bytes(img[22:24], "little"))
        self.assertEqual((ss, spc, res, nfat, root_entries, total, media, spf), (512, 1, 1, 2, 224, 2880, 0xF0, 9))
        self.assertEqual(img[res * ss : (res + spf) * ss], img[(res + spf) * ss : (res + 2 * spf) * ss], "both FATs equal")

        def fat(i):
            p = i * 3 // 2
            v = img[res * ss + p] | img[res * ss + p + 1] << 8
            return (v >> 4 if i % 2 else v) & 0xFFF

        self.assertEqual((fat(0), fat(1)), (0xFF0, 0xFFF))
        root = (res + nfat * spf) * ss
        data = root + root_entries * 32
        files = {}
        for k in range(root_entries):
            e = img[root + k * 32 : root + k * 32 + 32]
            if e[0] == 0:
                break
            first = int.from_bytes(e[26:28], "little")
            size = int.from_bytes(e[28:32], "little")
            out, c = b"", first
            while True:
                out += img[data + (c - 2) * ss : data + (c - 1) * ss]
                if fat(c) == 0xFFF:
                    break
                c = fat(c)
            files[(e[:8].decode().strip() + "." + e[8:11].decode().strip())] = out[:size]
        self.assertEqual(files, {"HELLO.COM": mkdos.HELLO_COM, "README.TXT": b"EMU-RUST FLOPPY TEST\r\n"})

    def test_floppy_multi_cluster_chain(self):
        big = bytes(range(256)) * 10  # 2560 bytes = 5 clusters
        img = mkdos.fat12_floppy({"BIG.BIN": big, "EMPTY.TXT": b""})
        root = (1 + 2 * 9) * 512
        entries = [img[root + k * 32 : root + k * 32 + 32] for k in range(2)]
        by_name = {e[:11]: e for e in entries}
        e = by_name[b"BIG     BIN"]
        self.assertEqual(int.from_bytes(e[28:32], "little"), len(big))
        data = root + 224 * 32
        first = int.from_bytes(e[26:28], "little")
        self.assertEqual(img[data + (first - 2) * 512 : data + (first - 2) * 512 + 512], big[:512])
        self.assertEqual(int.from_bytes(by_name[b"EMPTY   TXT"][26:28], "little"), 0)


if __name__ == "__main__":
    unittest.main()
