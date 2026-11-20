#!/usr/bin/env python3
"""Self-contained check of mkadf.py output (size, bootblock, rootblock, bitmap checksums). usage: python3 -I test_mkadf.py"""
import os
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
MKADF = os.path.join(HERE, "mkadf.py")


def longs(b):
    return struct.unpack(">%dI" % (len(b) // 4), b)


def run(*args):
    with tempfile.TemporaryDirectory() as d:
        out = os.path.join(d, "x.adf")
        subprocess.run([sys.executable, "-I", MKADF, *args, out], check=True, env={"SOURCE_DATE_EPOCH": "1700000000"})
        return open(out, "rb").read()


def boot_ok(img):
    total = 0
    for v in longs(img[:1024]):
        total += v
        if total > 0xFFFFFFFF:
            total = (total & 0xFFFFFFFF) + 1
    return total == 0xFFFFFFFF


def block_ok(img, n, off):
    blk = img[n * 512 : (n + 1) * 512]
    return sum(longs(blk)) & 0xFFFFFFFF == 0


def main():
    boot = run()
    assert len(boot) == 901120 and boot[:4] == b"DOS\x00" and boot_ok(boot)
    img = run("--label", "Blank 1")
    assert len(img) == 901120 and img[:4] == b"DOS\x00" and boot_ok(img), "bootblock"
    root = img[880 * 512 : 881 * 512]
    assert struct.unpack(">I", root[0:4])[0] == 2 and struct.unpack(">I", root[12:16])[0] == 72
    assert struct.unpack(">I", root[508:512])[0] == 1, "ST_ROOT"
    assert block_ok(img, 880, 20), "rootblock checksum"
    assert root[432] == 7 and root[433:440] == b"Blank 1", "volume name"
    assert struct.unpack(">i", root[312:316])[0] == -1 and struct.unpack(">I", root[316:320])[0] == 881
    assert block_ok(img, 881, 0), "bitmap checksum"
    bm = longs(img[881 * 512 : 882 * 512])
    free = sum(bin(v).count("1") for v in bm[1:56])
    assert free == 1756, free  # blocks 2..1759 minus root and bitmap
    assert run("--label", "Blank 1") == img, "reproducible"
    for bad in ("", "a/b", "x" * 31):
        r = subprocess.run([sys.executable, "-I", MKADF, "--label", bad, os.devnull], capture_output=True)
        assert r.returncode != 0, bad
    print("mkadf OK")


main()
