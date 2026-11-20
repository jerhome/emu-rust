#!/usr/bin/env python3
"""Generate fake DOS games (folders and zips) for library tests. Original data (public domain, written for this project).

usage (python3 -I mkdos.py): mkdos.py OUTDIR      writes every case below under OUTDIR
                             mkdos.py --list      prints the case names
Folders (game folders):  hello/  menu/  bat/  multi/  longnames/  lowercase/  empty/  nested/
Zips:  hello.zip (flat)  hello-rootdir.zip (one root folder)  menu.zip (two .COM)  bat.zip  multi.zip
       multi-rootdir.zip  longnames.zip  nested.zip  empty.zip (no entries)  notdos.zip (no executable)
       evil-zipslip.zip  evil-absolute.zip  evil-backslash.zip   (hostile entry names: must be refused / never extracted)
Disc: hello.iso (ISO 9660 with HELLO.COM and README.TXT)
Only HELLO.COM / MENU*.COM are real programs (hand-made; HELLO.COM bytes from docs/essais-dosbox.md section 4):
  B4 09 BA 10 01 CD 21 31 C0 CD 16 B8 00 4C CD 21   mov ah,9 / mov dx,0110h / int 21h / xor ax,ax / int 16h / exit
  followed by the '$'-terminated text at 0110h (SHA-256 ccd70355...a68d for "HELLO FROM EMU-RUST$").
Every other .EXE/.COM is a placeholder (".EXE" = "MZ" + filler) and must not be run.
Output is deterministic; zip timestamps are 1980-01-01 unless SOURCE_DATE_EPOCH is set (then that time, min 1980).
"""
import importlib.util
import io
import os
import sys
import time
import zipfile

_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("mkcd", os.path.join(_here, "mkcd.py"))
mkcd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(mkcd)


def com(text: str) -> bytes:
    """Tiny .COM printing `text`, waiting for a key, then exiting (same code as HELLO.COM)."""
    return bytes.fromhex("B409BA1001CD2131C0CD16B8004CCD21") + text.encode("ascii") + b"$"


HELLO_COM = com("HELLO FROM EMU-RUST")
assert HELLO_COM == mkcd.HELLO_COM
MENU1 = com("MENU PROGRAM ONE")
MENU2 = com("MENU PROGRAM TWO")
BAT = b"@echo off\r\nver\r\necho EMU-RUST > OUT.TXT\r\ndir\r\n"


def fake_exe(tag: str) -> bytes:
    return (b"MZ" + tag.encode("ascii")).ljust(512, b"\0")


# case name -> {relative path (forward slashes): bytes}; used for folders and, with a root prefix, for zips
GAMES: dict[str, dict[str, bytes]] = {
    "hello": {"HELLO.COM": HELLO_COM},
    "menu": {"MENU1.COM": MENU1, "MENU2.COM": MENU2},
    "bat": {"RUN.BAT": BAT},
    "multi": {
        "GAME.EXE": fake_exe("GAME"),
        "SETUP.EXE": fake_exe("SETUP"),
        "README.TXT": b"fake multi-file game\r\n",
        "DATA/LEVEL1.DAT": bytes(range(256)),
        "DATA/SUB/LEVEL2.DAT": b"\x01" * 64,
    },
    "longnames": {
        "A Very Long Game Name.exe": fake_exe("LONG"),
        "Another Long File Name.txt": b"long name\r\n",
        "dir with spaces/Level One.dat": b"x" * 10,
        "caf\u00e9.txt": b"non ascii name\r\n",
    },
    "lowercase": {"game.exe": fake_exe("low"), "readme.txt": b"lower case 8.3\r\n"},
    "nested": {"GAMES/DEEP/GAME.EXE": fake_exe("DEEP"), "GAMES/INFO.TXT": b"no root executable\r\n"},
}
EMPTY = "empty"


def _date():
    e = os.environ.get("SOURCE_DATE_EPOCH")
    t = time.gmtime(int(e)) if e else time.gmtime(315532800)
    return max((t.tm_year, t.tm_mon, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec // 2 * 2), (1980, 1, 1, 0, 0, 0))


def make_zip(entries: dict[str, bytes], root: str = "") -> bytes:
    """Zip with fixed timestamps, deflated, names written verbatim (hostile names allowed on purpose)."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        names = {}
        for path, data in entries.items():
            names[root + path] = data
        dirs = set()
        for n in names:
            parts = n.split("/")[:-1]
            if ".." in parts or n.startswith("/"):
                continue  # hostile names get no directory entries
            for i in range(1, len(parts) + 1):
                d = "/".join(parts[:i])
                if d and d not in dirs:
                    dirs.add(d)
        for d in sorted(dirs):
            zi = zipfile.ZipInfo(d + "/", _date())
            zi.external_attr = (0o40755 << 16) | 0x10
            z.writestr(zi, b"")
        for n in sorted(names):
            zi = zipfile.ZipInfo(n, _date())
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = 0o100644 << 16
            z.writestr(zi, names[n])
    return buf.getvalue()


def all_cases() -> dict[str, bytes | dict]:
    """{name: bytes} for files, {name: {path: bytes}} for folders."""
    out: dict = {}
    for g, files in GAMES.items():
        out[g] = files
    out[EMPTY] = {}
    out["hello.zip"] = make_zip(GAMES["hello"])
    out["hello-rootdir.zip"] = make_zip(GAMES["hello"], "HELLO/")
    out["menu.zip"] = make_zip(GAMES["menu"])
    out["bat.zip"] = make_zip(GAMES["bat"])
    out["multi.zip"] = make_zip(GAMES["multi"])
    out["multi-rootdir.zip"] = make_zip(GAMES["multi"], "MULTI/")
    out["longnames.zip"] = make_zip(GAMES["longnames"])
    out["nested.zip"] = make_zip(GAMES["nested"])
    out["empty.zip"] = make_zip({})
    out["notdos.zip"] = make_zip({"README.TXT": b"no executable here\r\n", "DATA.BIN": b"\0" * 32})
    out["evil-zipslip.zip"] = make_zip({"../evil.txt": b"zip-slip\r\n", "a/../../evil2.txt": b"zip-slip 2\r\n", "OK.COM": HELLO_COM})
    out["evil-absolute.zip"] = make_zip({"/tmp/evil.txt": b"absolute\r\n", "OK.COM": HELLO_COM})
    out["evil-backslash.zip"] = make_zip({"..\\evil.txt": b"backslash\r\n", "C:\\evil.txt": b"drive\r\n", "OK.COM": HELLO_COM})
    out["hello.iso"] = mkcd.iso9660({"HELLO.COM": HELLO_COM, "README.TXT": b"EMU-RUST ISO TEST\r\n"}, "HELLO", _epoch())
    return out


def _epoch():
    e = os.environ.get("SOURCE_DATE_EPOCH")
    return int(e) if e else None


def write_all(outdir: str) -> list[str]:
    written = []
    for name, v in all_cases().items():
        path = os.path.join(outdir, name)
        if isinstance(v, dict):
            os.makedirs(path, exist_ok=True)
            for rel, data in v.items():
                fp = os.path.join(path, *rel.split("/"))
                os.makedirs(os.path.dirname(fp), exist_ok=True)
                with open(fp, "wb") as f:
                    f.write(data)
        else:
            os.makedirs(outdir, exist_ok=True)
            with open(path, "wb") as f:
                f.write(v)
        written.append(name)
    return written


def main(argv):
    if argv == ["--list"]:
        print("\n".join(all_cases()))
    elif len(argv) == 1 and not argv[0].startswith("-"):
        write_all(argv[0])
    else:
        sys.exit(__doc__.split("\n")[2])


if __name__ == "__main__":
    main(sys.argv[1:])
