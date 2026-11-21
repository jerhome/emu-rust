"""Self-contained tests for mktap.py: run `python3 -I test_mktap.py`."""
import hashlib
import os
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
MKTAP = os.path.join(HERE, "mktap.py")
MKC64 = os.path.join(HERE, "mkc64.py")
HELLO_TAP_SHA256 = "dd024ffccf902b4b6b35149a4718622cd9c72d002e941abddc8db8fa9506e34a"


def run(script, *args):
    subprocess.run([sys.executable, "-I", script, *args], check=True)


def main():
    with tempfile.TemporaryDirectory() as d:
        run(MKC64, d, "--all")
        tap = os.path.join(d, "hello.tap")
        run(MKTAP, os.path.join(d, "hello.prg"), tap)
        data = open(tap, "rb").read()
        assert hashlib.sha256(data).hexdigest() == HELLO_TAP_SHA256, "hello.tap is pinned"
        assert data[:12] == b"C64-TAPE-RAW" and data[12] == 0, "version 0"
        assert struct.unpack("<I", data[16:20])[0] == len(data) - 20, "data size"

        joy = os.path.join(d, "joy.tap")
        run(MKTAP, os.path.join(d, "joy.prg"), joy, "JOY")
        jd = open(joy, "rb").read()
        assert jd[:12] == b"C64-TAPE-RAW" and struct.unpack("<I", jd[16:20])[0] == len(jd) - 20
        assert jd != data and len(jd) > len(data), "a bigger program makes a longer tape"
        named = os.path.join(d, "named.tap")
        run(MKTAP, os.path.join(d, "hello.prg"), named, "OTHER")
        assert open(named, "rb").read() != data, "the NAME argument changes the header block"
        again = os.path.join(d, "again.tap")
        run(MKTAP, os.path.join(d, "hello.prg"), again)
        assert open(again, "rb").read() == data
    print("mktap OK")


main()
