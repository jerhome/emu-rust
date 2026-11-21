"""Self-contained tests for mkcpc.py: run `python3 -I test_mkcpc.py`."""
import hashlib
import io
import os
import subprocess
import sys
import tempfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
MKCPC = os.path.join(HERE, "mkcpc.py")
MKC64 = os.path.join(HERE, "mkc64.py")
HELLO_SHA256 = "212ddb91b40091737b55536d286eb431f178cc642bf38ee273515d6bc9409359"


def run(script, *args):
    subprocess.run([sys.executable, "-I", script, *args], check=True)


def directory(img):
    """File names of a 40-track, 9-sector DSK written by mkcpc.py (directory = sectors C1..C4 of track 0)."""
    assert img[:34] == b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n"
    track0 = img[256 + 256 : 256 + 256 + 4 * 512]
    return sorted(
        track0[e + 1 : e + 9].decode().strip() + "." + track0[e + 9 : e + 12].decode()
        for e in range(0, 2048, 32)
        if track0[e] == 0
    )


def main():
    with tempfile.TemporaryDirectory() as d:
        hello = os.path.join(d, "hello.dsk")
        run(MKCPC, hello)
        img = open(hello, "rb").read()
        assert hashlib.sha256(img).hexdigest() == HELLO_SHA256, "hello.dsk is pinned"
        assert len(img) == 256 + 40 * (256 + 9 * 512) and directory(img) == ["HELLO.BAS"]

        out = os.path.join(d, "set")
        d64 = os.path.join(d, "hello.d64")
        run(MKC64, d)  # writes hello.prg and hello.d64 into d
        run(MKCPC, "--set", out, "--d64", d64)
        names = sorted(os.listdir(out))
        assert names == [
            "command.m3u",
            "faces (Side A).dsk",
            "faces (Side B).dsk",
            "faces.m3u",
            "hello.dsk",
            "keys.dsk",
            "mixed.zip",
            "save.dsk",
            "two.dsk",
        ], names
        read = lambda n: open(os.path.join(out, n), "rb").read()
        assert read("hello.dsk") == img, "same hello.dsk in the set"
        assert directory(read("keys.dsk")) == ["KEYS.BAS"] and b"INKEY$" in read("keys.dsk")
        assert directory(read("save.dsk")) == ["HELLO.BAS"] and b'SAVE"OUT",A' in read("save.dsk")
        assert directory(read("two.dsk")) == ["HELLO.BAS", "SECOND.BAS"]
        assert read("command.m3u") == b'#COMMAND:RUN"SECOND"\ntwo.dsk\n'
        assert directory(read("faces (Side A).dsk")) == ["A.BAS"]
        assert directory(read("faces (Side B).dsk")) == ["B.BAS"]
        assert b"SIDE A" in read("faces (Side A).dsk") and b"SIDE B" in read("faces (Side B).dsk")
        assert read("faces.m3u").decode().splitlines() == [
            "faces (Side A).dsk|Side A",
            "faces (Side B).dsk|Side B",
        ]
        with zipfile.ZipFile(io.BytesIO(read("mixed.zip"))) as z:
            assert z.namelist() == ["hello.dsk", "hello.d64"], "one disk of each system"
            assert z.read("hello.dsk") == img and z.read("hello.d64") == open(d64, "rb").read()

        again = os.path.join(d, "again")
        run(MKCPC, "--set", again, "--d64", d64)
        for n in names:
            assert open(os.path.join(again, n), "rb").read() == read(n), f"{n} is reproducible"

        no_zip = os.path.join(d, "nozip")
        run(MKCPC, "--set", no_zip)
        assert "mixed.zip" not in os.listdir(no_zip)
    print("mkcpc OK")


main()
