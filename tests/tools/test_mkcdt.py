"""Self-contained tests for mkcdt.py: run `python3 -I test_mkcdt.py`."""
import hashlib
import os
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
MKCDT = os.path.join(HERE, "mkcdt.py")
HELLO_SHA256 = "9461a692e6a517f45122b4b07235c7b43fb60b000b0ef4448cfebae9e57f7eca"


def run(*args):
    subprocess.run([sys.executable, "-I", MKCDT, *args], check=True)


def blocks(cdt):
    """Block 0x11 (turbo) payloads of a TZX 1.13 file."""
    assert cdt[:10] == b"ZXTape!\x1a\x01\x0d"
    pos, out = 10, []
    while pos < len(cdt):
        assert cdt[pos] == 0x11, "only turbo blocks"
        length = int.from_bytes(cdt[pos + 16 : pos + 19], "little")
        out.append(cdt[pos + 19 : pos + 19 + length])
        pos += 19 + length
    assert pos == len(cdt)
    return out


def main():
    with tempfile.TemporaryDirectory() as d:
        hello = os.path.join(d, "hello.cdt")
        run(hello)
        data = open(hello, "rb").read()
        assert hashlib.sha256(data).hexdigest() == HELLO_SHA256, "hello.cdt is pinned"
        header, code = blocks(data)
        assert header[0] == 0x2C and code[0] == 0x16 and b"EMU-RUST CPC TAPE TEST" in code

        out = os.path.join(d, "set")
        run("--set", out)
        names = sorted(os.listdir(out))
        assert names == ["hello.cdt", "tape (Tape 1 of 2).cdt", "tape (Tape 2 of 2).cdt"], names
        assert open(os.path.join(out, "hello.cdt"), "rb").read() == data
        for n in (1, 2):
            tape = open(os.path.join(out, f"tape (Tape {n} of 2).cdt"), "rb").read()
            header, code = blocks(tape)
            assert header[1:6] == f"TAPE{n}".encode(), "file name in the header record"
            assert f"EMU-RUST CPC TAPE {n} OF 2".encode() in code
            assert struct.unpack("<H", header[1 + 21 : 1 + 23])[0] == 0x4000, "load address"
        again = os.path.join(d, "again")
        run("--set", again)
        for n in names:
            assert open(os.path.join(again, n), "rb").read() == open(os.path.join(out, n), "rb").read()
    print("mkcdt OK")


main()
