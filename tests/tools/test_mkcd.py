"""Self-contained tests for mkcd.py: python3 -I -m unittest is not usable with -I, run `python3 -I test_mkcd.py`."""
import hashlib
import importlib.util
import os
import re
import struct
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("mkcd", os.path.join(HERE, "mkcd.py"))
mkcd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(mkcd)


def gen(outdir, *args):
    subprocess.run([sys.executable, "-I", os.path.join(HERE, "mkcd.py"), "megacd", outdir, *args], check=True)


def parse_cue(text):
    """-> list of dicts {file, number, mode, indexes{0/1: frames}}"""
    tracks, cur_file = [], None
    for line in text.splitlines():
        line = line.strip()
        if m := re.fullmatch(r'FILE "([^"]+)" BINARY', line):
            cur_file = m.group(1)
        elif m := re.fullmatch(r"TRACK (\d\d) (MODE1/2048|MODE1/2352|MODE2/2352|AUDIO)", line):
            tracks.append({"file": cur_file, "number": int(m.group(1)), "mode": m.group(2), "indexes": {}})
        elif m := re.fullmatch(r"INDEX (\d\d) (\d\d):(\d\d):(\d\d)", line):
            mm, ss, ff = map(int, m.groups()[1:])
            assert ss < 60 and ff < 75
            tracks[-1]["indexes"][int(m.group(1))] = (mm * 60 + ss) * 75 + ff
        elif line:
            raise AssertionError("unparsable cue line: " + line)
    return tracks


def read(d, name):
    with open(os.path.join(d, name), "rb") as f:
        return f.read()


class MegaCd(unittest.TestCase):
    def test_raw_sector_structure(self):
        d = tempfile.mkdtemp()
        gen(d, "--region", "E", "--serial", "T-93175 -00", "--title", "TEST")
        b = read(d, "fakecd (Track 1).bin")
        self.assertEqual(len(b) % 2352, 0)
        self.assertEqual(b[:12], b"\x00" + b"\xff" * 10 + b"\x00")
        self.assertEqual(b[12:16], bytes([0, 2, 0, 1]))  # MSF 00:02:00 (BCD), mode 1
        self.assertEqual(b[16:30], b"SEGADISCSYSTEM")
        user = b[16:16 + 2048]
        self.assertEqual(user[0x180:0x18E], b"GM T-93175 -00")
        self.assertEqual(user[0x1F0:0x1F1], b"E")
        self.assertEqual(user[0x20B], 0x64)
        self.assertEqual(user[0x100:0x110], b"SEGA MEGA DRIVE ")
        self.assertEqual(struct.unpack("<I", b[2064:2068])[0], mkcd.edc(b[:2064]))
        m, s, f = b[2352 + 12 : 2352 + 15]
        self.assertEqual((m, s, f), (0, 2, 1))

    def test_regions(self):
        for r in "JUE":
            s = mkcd.megacd_sector0(r)
            self.assertEqual((s[0x1F0], s[0x20B]), (ord(r), {"E": 0x64, "J": 0xA1, "U": 0x7A}[r]))
        with self.assertRaises(ValueError):
            mkcd.megacd_sector0("X")

    def test_2048_and_iso(self):
        d = tempfile.mkdtemp()
        gen(d, "--sector", "2048", "--iso", "--audio-tracks", "0", "--data-sectors", "8")
        b = read(d, "fakecd.bin")
        self.assertEqual(len(b), 8 * 2048)
        self.assertEqual(b[:14], b"SEGADISCSYSTEM")
        self.assertEqual(read(d, "fakecd.iso"), b)
        cue = parse_cue(read(d, "fakecd.cue").decode())
        self.assertEqual([(t["mode"], t["file"]) for t in cue], [("MODE1/2048", "fakecd.bin")])

    def test_multi_file_cue_and_durations(self):
        d = tempfile.mkdtemp()
        gen(d, "--audio-tracks", "2", "--audio-seconds", "3", "--pregap", "150", "--data-sectors", "20")
        t = parse_cue(read(d, "fakecd.cue").decode())
        self.assertEqual([x["number"] for x in t], [1, 2, 3])
        self.assertEqual([x["mode"] for x in t], ["MODE1/2352", "AUDIO", "AUDIO"])
        self.assertEqual(len({x["file"] for x in t}), 3)
        self.assertEqual(len(read(d, t[0]["file"])), 20 * 2352)
        for x in t[1:]:
            self.assertEqual(x["indexes"], {0: 0, 1: 150})
            self.assertEqual(len(read(d, x["file"])), (150 + 3 * 75) * 2352)  # pre-gap + 3 s

    def test_mono_cue_offsets_match_file(self):
        d = tempfile.mkdtemp()
        gen(d, "--layout", "mono", "--audio-tracks", "2", "--audio-seconds", "1", "--pregap", "150", "--data-sectors", "16")
        t = parse_cue(read(d, "fakecd.cue").decode())
        self.assertEqual({x["file"] for x in t}, {"fakecd.bin"})
        total = len(read(d, "fakecd.bin")) // 2352
        self.assertEqual(total, 16 + 2 * (150 + 75))
        self.assertEqual(t[1]["indexes"], {0: 16, 1: 166})
        self.assertEqual(t[2]["indexes"], {0: 241, 1: 391})
        self.assertLess(t[2]["indexes"][1] + 75, total + 1)

    def test_mono_without_pregap_has_no_index00(self):
        d = tempfile.mkdtemp()
        gen(d, "--layout", "mono", "--data-sectors", "4")
        t = parse_cue(read(d, "fakecd.cue").decode())
        self.assertEqual(t[1]["indexes"], {1: 4})

    def test_mono_2048_with_audio_rejected(self):
        d = tempfile.mkdtemp()
        r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "mkcd.py"), "megacd", d, "--layout", "mono", "--sector", "2048"],
                           capture_output=True)
        self.assertNotEqual(r.returncode, 0)

    def test_audio_content(self):
        sil = mkcd.audio_track(2, "silence")
        sin = mkcd.audio_track(2, "sine")
        self.assertEqual((len(sil), len(sin)), (2 * 2352, 2 * 2352))
        self.assertEqual(set(sil), {0})
        self.assertTrue(any(sin))
        l, r = struct.unpack_from("<hh", sin, 4 * 50)
        self.assertEqual(l, r)

    def test_m3u_multi_disc(self):
        d = tempfile.mkdtemp()
        gen(d, "--discs", "2", "--name", "game", "--data-sectors", "2")
        lines = read(d, "game.m3u").decode().splitlines()
        self.assertEqual(lines, ["game (Disc 1).cue", "game (Disc 2).cue"])
        for l in lines:
            self.assertTrue(os.path.exists(os.path.join(d, l)))
            for x in parse_cue(read(d, l).decode()):
                self.assertTrue(os.path.exists(os.path.join(d, x["file"])))

    def test_orphan_and_32x(self):
        d = tempfile.mkdtemp()
        subprocess.run([sys.executable, "-I", os.path.join(HERE, "mkcd.py"), "orphan", d + "/o.bin"], check=True)
        subprocess.run([sys.executable, "-I", os.path.join(HERE, "mkcd.py"), "32x", d + "/g.32x"], check=True)
        self.assertEqual(read(d, "o.bin")[16:30], b"SEGADISCSYSTEM")
        self.assertEqual(read(d, "g.32x")[0x100:0x108], b"SEGA 32X")

    def test_reproducible(self):
        a, b = tempfile.mkdtemp(), tempfile.mkdtemp()
        env = dict(os.environ, SOURCE_DATE_EPOCH="1700000000")
        for d in (a, b):
            subprocess.run([sys.executable, "-I", os.path.join(HERE, "mkcd.py"), "megacd", d, "--iso", "--audio", "sine", "--discs", "2"],
                           check=True, env=env)
        self.assertEqual(sorted(os.listdir(a)), sorted(os.listdir(b)))
        for n in os.listdir(a):
            self.assertEqual(read(a, n), read(b, n), n)


class Iso9660(unittest.TestCase):
    def test_parse_back(self):
        files = {"HELLO.COM": mkcd.HELLO_COM, "README.TXT": b"x" * 5000, "EMPTY.DAT": b""}
        img = mkcd.iso9660(files, "TESTVOL", 1700000000)
        self.assertEqual(len(img) % 2048, 0)
        self.assertEqual(img[16 * 2048 + 1 : 16 * 2048 + 6], b"CD001")
        self.assertEqual(img[17 * 2048 : 17 * 2048 + 6], b"\xffCD001")
        pvd = img[16 * 2048 :]
        self.assertEqual(pvd[40:48], b"TESTVOL ")
        self.assertEqual(struct.unpack_from("<I", pvd, 80)[0] * 2048, len(img))
        self.assertEqual(struct.unpack_from("<H", pvd, 128)[0], 2048)
        root = struct.unpack_from("<I", pvd, 156 + 2)[0]
        self.assertEqual(struct.unpack_from(">I", pvd, 156 + 6)[0], root)
        d = img[root * 2048 : root * 2048 + 2048]
        found, pos = {}, 0
        while pos < 2048 and d[pos]:
            ln = d[pos]
            ext, size = struct.unpack_from("<I", d, pos + 2)[0], struct.unpack_from("<I", d, pos + 10)[0]
            name = d[pos + 33 : pos + 33 + d[pos + 32]]
            if name not in (b"\x00", b"\x01"):
                found[name.decode()] = img[ext * 2048 : ext * 2048 + size]
            pos += ln
        self.assertEqual(found, {k + ";1": v for k, v in files.items()})
        self.assertEqual(hashlib.sha256(found["HELLO.COM;1"]).hexdigest(),
                         "ccd70355ebcc1b2438568850ddd6755449905ef7a26ed7e6b9a0ef187491a68d")

    def test_bad_name(self):
        with self.assertRaises(ValueError):
            mkcd.iso9660({"Long Name.txt": b""})


def gen_cmd(cmd, outdir, *args):
    subprocess.run([sys.executable, "-I", os.path.join(HERE, "mkcd.py"), cmd, outdir, *args], check=True)


def iso_files(iso):
    """Root directory of an image written by mkcd (extent 20): {name without ;1: (lba, size)}."""
    root = iso[20 * 2048 : 21 * 2048]
    out, pos = {}, 0
    while pos < len(root) and root[pos]:
        n = root[pos]
        rec = root[pos : pos + n]
        name = rec[33 : 33 + rec[32]]
        if name not in (b"\x00", b"\x01"):
            out[name.decode().split(";")[0]] = (struct.unpack("<I", rec[2:6])[0], struct.unpack("<I", rec[10:14])[0])
        pos += n
    return out


class Psx(unittest.TestCase):
    def test_iso_structure(self):
        d = tempfile.mkdtemp()
        gen_cmd("psx", d, "--iso", "--region", "E")
        iso = read(d, "fakepsx.iso")
        self.assertEqual(len(iso) % 2048, 0)
        self.assertEqual(iso[16 * 2048 : 16 * 2048 + 6], b"\x01CD001")
        self.assertEqual(iso[16 * 2048 + 8 : 16 * 2048 + 19], b"PLAYSTATION")
        self.assertIn(b"Euro pe", iso[4 * 2048 : 5 * 2048])
        self.assertNotIn(b"Sony", iso[: 16 * 2048])
        self.assertNotIn(b"Licensed", iso[: 16 * 2048])
        files = iso_files(iso)
        self.assertEqual(sorted(files), ["SLES_999.01", "SYSTEM.CNF"])
        lba, size = files["SYSTEM.CNF"]
        cnf = iso[lba * 2048 : lba * 2048 + size].decode()
        self.assertTrue(cnf.startswith("BOOT = cdrom:\\SLES_999.01;1\r\n"), cnf)

    def test_regions_and_serial(self):
        for region, token, prefix in (("U", b"Amer  ica", "SLUS_999.01"), ("J", b"Inc.", "SLPS_999.01"), ("E", b"Euro pe", "SLES_999.01")):
            iso = mkcd.psx_iso(region)
            self.assertIn(token, iso[4 * 2048 : 5 * 2048])
            self.assertIn(prefix, iso_files(iso))
        iso = mkcd.psx_iso("U", serial="SLUS-00001")
        lba, size = iso_files(iso)["SYSTEM.CNF"]
        self.assertIn(b"cdrom:\\SLUS_000.01;1", iso[lba * 2048 : lba * 2048 + size])
        for bad in ("SLES99901", "sles-99901", "SLES-999"):
            with self.assertRaises(ValueError):
                mkcd.psx_iso("E", serial=bad)
        with self.assertRaises(ValueError):
            mkcd.psx_iso("X")

    def test_variants_without_cnf_or_license(self):
        iso = mkcd.psx_iso("E", system_cnf=False, license_area=False)
        self.assertEqual(sorted(iso_files(iso)), ["SLES_999.01"])
        self.assertEqual(iso[: 16 * 2048], bytes(16 * 2048))
        self.assertEqual(iso[16 * 2048 + 8 : 16 * 2048 + 19], b"PLAYSTATION")

    def test_raw_mode2_and_cue(self):
        d = tempfile.mkdtemp()
        gen_cmd("psx", d, "--iso")
        iso, b = read(d, "fakepsx.iso"), read(d, "fakepsx.bin")
        self.assertEqual(len(b), len(iso) // 2048 * 2352)
        self.assertEqual(b[:12], b"\x00" + b"\xff" * 10 + b"\x00")
        self.assertEqual(b[15], 2)
        self.assertEqual(b[16:24], bytes([0, 0, 8, 0, 0, 0, 8, 0]))
        for lba in (0, 4, 16, 17):
            self.assertEqual(b[lba * 2352 + 24 : lba * 2352 + 24 + 2048], iso[lba * 2048 : (lba + 1) * 2048])
        self.assertEqual(b[16 * 2352 + 24 + 8 : 16 * 2352 + 24 + 19], b"PLAYSTATION")
        sec = b[2352:2 * 2352]
        self.assertEqual(struct.unpack("<I", sec[2072:2076])[0], mkcd.edc(sec[16:2072]))
        t = parse_cue(read(d, "fakepsx.cue").decode())
        self.assertEqual(t[0]["file"], "fakepsx.bin")

    def test_mode1(self):
        d = tempfile.mkdtemp()
        gen_cmd("psx", d, "--mode", "1")
        b = read(d, "fakepsx.bin")
        self.assertEqual(b[15], 1)
        self.assertEqual(b[16 * 2352 + 16 : 16 * 2352 + 22], b"\x01CD001")
        self.assertIn(b"MODE1/2352", read(d, "fakepsx.cue"))

    def test_boot_exe_is_stored(self):
        d = tempfile.mkdtemp()
        exe = os.path.join(d, "t.exe")
        payload = b"PS-X EXE" + bytes(range(256)) * 20
        with open(exe, "wb") as f:
            f.write(payload)
        gen_cmd("psx", d, "--psx-boot", exe, "--iso", "--region", "U", "--serial", "SLUS-00001")
        iso = read(d, "fakepsx.iso")
        lba, size = iso_files(iso)["SLUS_000.01"]
        self.assertEqual(size, len(payload))
        self.assertEqual(iso[lba * 2048 : lba * 2048 + size], payload)

    def test_two_discs_and_m3u(self):
        d = tempfile.mkdtemp()
        gen_cmd("psx", d, "--name", "Game", "--discs", "2")
        self.assertEqual(
            read(d, "Game.m3u"), b"Game (Disc 1).cue\r\nGame (Disc 2).cue\r\n"
        )
        for k in (1, 2):
            self.assertTrue(os.path.exists(os.path.join(d, "Game (Disc %d).bin" % k)))
            t = parse_cue(read(d, "Game (Disc %d).cue" % k).decode())
            self.assertEqual(t[0]["file"], "Game (Disc %d).bin" % k)

    def test_reproducible(self):
        a, b = tempfile.mkdtemp(), tempfile.mkdtemp()
        gen_cmd("psx", a, "--iso")
        gen_cmd("psx", b, "--iso")
        for n in ("fakepsx.bin", "fakepsx.cue", "fakepsx.iso"):
            self.assertEqual(hashlib.sha256(read(a, n)).digest(), hashlib.sha256(read(b, n)).digest())


class ThreeDo(unittest.TestCase):
    def test_raw(self):
        d = tempfile.mkdtemp()
        gen_cmd("3do", d)
        b = read(d, "fake3do.bin")
        self.assertEqual(len(b) % 2352, 0)
        self.assertEqual(b[16:23], b"\x01ZZZZZ\x01")
        self.assertEqual(b[12:16], bytes([0, 2, 0, 1]))
        self.assertIn(b"MODE1/2352", read(d, "fake3do.cue"))
        self.assertNotIn(b"CD001", b[: 40 * 2352])  # not mistaken for a PlayStation disc

    def test_2048_and_no_portfolio_content(self):
        d = tempfile.mkdtemp()
        gen_cmd("3do", d, "--sector", "2048")
        iso = read(d, "fake3do.iso")
        self.assertEqual(iso[:7], b"\x01ZZZZZ\x01")
        self.assertEqual(iso[7:17], b"EMURUST3DO")
        self.assertEqual(len(iso) % 2048, 0)
        self.assertFalse(os.path.exists(os.path.join(d, "fake3do.cue")))


if __name__ == "__main__":
    unittest.main()
