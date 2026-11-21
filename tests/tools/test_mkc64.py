"""Self-contained tests for mkc64.py: run `python3 -I test_mkc64.py`."""
import hashlib
import io
import os
import struct
import subprocess
import sys
import tempfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
MKC64 = os.path.join(HERE, "mkc64.py")
MKCPC = os.path.join(HERE, "mkcpc.py")
PINNED = {
    "hello.prg": "cf6a533abf9a406b4c37bfd2b8654847b1902b9ea019d4e80c6a64a18eaf9e08",
    "hello.d64": "28c4f7a47105d71feaff9ee083609ea1b3b2c7a3c5f7786158cef2c916d24237",
}
SPT = [21] * 17 + [19] * 7 + [18] * 6 + [17] * 5


def run(script, *args):
    subprocess.run([sys.executable, "-I", script, *args], check=True)


def sector(img, track, sec):
    o = (sum(SPT[: track - 1]) + sec) * 256
    return img[o : o + 256]


def d64_files(img):
    """{name: PRG bytes} read back by following the directory and the sector chains."""
    assert len(img) == 174848
    out = {}
    d = sector(img, 18, 1)
    for e in range(0, 256, 32):
        if d[e + 2] != 0x82:
            continue
        name = d[e + 5 : e + 21].rstrip(b"\xa0").decode()
        t, s = d[e + 3], d[e + 4]
        data = b""
        while t:
            blk = sector(img, t, s)
            if blk[0]:
                data += blk[2:]
                t, s = blk[0], blk[1]
            else:
                data += blk[2 : blk[1] + 1]
                t = 0
        assert struct.unpack("<H", d[e + 0x1E : e + 0x20])[0] == -(-len(data) // 254), "block count"
        out[name] = data
    return out


def listing(prg):
    """[(line number, body bytes)] of a BASIC PRG at $0801 (checks the next-line links)."""
    assert prg[:2] == b"\x01\x08"
    pos, lines = 2, []
    while prg[pos : pos + 2] != b"\x00\x00":
        nxt, num = struct.unpack("<HH", prg[pos : pos + 4])
        end = prg.index(b"\x00", pos + 4)
        assert nxt == 0x0801 + end + 1 - 2, "link to the next line"
        lines.append((num, prg[pos + 4 : end]))
        pos = end + 1
    assert pos + 2 == len(prg)
    return lines


def main():
    with tempfile.TemporaryDirectory() as d:
        run(MKC64, d)
        assert sorted(os.listdir(d)) == ["hello.d64", "hello.prg"], "default mode writes two files"
        for name, sha in PINNED.items():
            assert hashlib.sha256(open(os.path.join(d, name), "rb").read()).hexdigest() == sha, name
        hello = open(os.path.join(d, "hello.prg"), "rb").read()
        assert d64_files(open(os.path.join(d, "hello.d64"), "rb").read()) == {"HELLO": hello}

        out = os.path.join(d, "all")
        run(MKCPC, "--set", out)  # provides hello.dsk for the mixed archive
        run(MKC64, out, "--all")
        read = lambda n: open(os.path.join(out, n), "rb").read()
        for name, sha in PINNED.items():
            assert hashlib.sha256(read(name)).hexdigest() == sha, f"{name} unchanged by --all"

        # BASIC tokenizer: reproduces the hand-built hello.prg
        assert listing(hello) == [(10, b'\x99"EMU-RUST C64 TEST":\x97' b"53280,2:\x97" b"53281,5")]
        joy = read("joy.prg")
        lines = dict(listing(joy))
        assert b"\xc2(56321)\xaf31" in lines[20] and b"\xc2(56320)\xaf31" in lines[20], "reads $DC01 then $DC00"
        assert b"PORT 1 ACTIVE" in lines[40] and b"PORT 2 ACTIVE" in lines[50]
        assert listing(read("second.prg")) == [(10, b'\x99 "EMU-RUST C64 SECOND"')]

        two = d64_files(read("two.d64"))
        assert two == {"HELLO": hello, "SECOND": read("second.prg")}
        assert read("second.m3u") == b"two.d64:SECOND\n"

        t = read("hello.t64")
        assert t[:20] == b"C64S tape image file" and struct.unpack("<HHH", t[32:38]) == (0x0101, 4, 1)
        e = t[64:96]
        load, end, _, off = struct.unpack("<HHHI", e[2:12])
        assert e[0] == 1 and e[1] == 0x82 and e[16:32].rstrip() == b"HELLO"
        assert load == 0x0801 and t[off:] == hello[2:] and end == load + len(hello) - 2

        c = read("cart.crt")
        assert c[:16] == b"C64 CARTRIDGE   " and struct.unpack(">IHH", c[16:24]) == (0x40, 0x0100, 0)
        assert (c[24], c[25]) == (0, 1), "Normal cartridge, 8 KB: EXROM 0, GAME 1"
        assert c[0x40:0x44] == b"CHIP" and struct.unpack(">IHHHH", c[0x44:0x50]) == (0x2010, 0, 0, 0x8000, 0x2000)
        rom = c[0x50:]
        assert len(c) == 0x40 + 0x10 + 0x2000 and len(rom) == 0x2000
        assert rom[4:9] == b"\xc3\xc2\xcd\x38\x30", "CBM80 signature"
        assert rom[0:2] == rom[2:4] == b"\x09\x80" and rom[9] == 0x78, "cold and warm start at $8009 (SEI)"
        assert b"EMU-RUST C64 CART TEST" in rom

        with zipfile.ZipFile(io.BytesIO(read("mixed.zip"))) as z:
            assert z.namelist() == ["hello.dsk", "hello.d64"]
            assert z.read("hello.d64") == read("hello.d64")
        assert read("mixed.zip") == open(os.path.join(d, "all", "mixed.zip"), "rb").read()
        same = os.path.join(d, "same")
        run(MKCPC, "--set", same)
        run(MKC64, same, "--all")
        for n in os.listdir(out):
            assert open(os.path.join(same, n), "rb").read() == read(n), f"{n} is reproducible"
    print("mkc64 OK")


main()
