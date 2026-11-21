#!/usr/bin/env python3
"""Generate fake DOS games (folders and zips) for library tests. Original data (public domain, written for this project).

usage (python3 -I mkdos.py): mkdos.py OUTDIR      writes every case below under OUTDIR
                             mkdos.py --list      prints the case names
Folders (game folders):  hello/  menu/  bat/  multi/  longnames/  lowercase/  empty/  nested/
Zips:  hello.zip (flat)  hello-rootdir.zip (one root folder)  menu.zip (two .COM)  bat.zip  multi.zip
       multi-rootdir.zip  longnames.zip  nested.zip  empty.zip (no entries)  notdos.zip (no executable)
       evil-zipslip.zip  evil-absolute.zip  evil-backslash.zip   (hostile entry names: must be refused / never extracted)
Disc: hello.iso (ISO 9660 with HELLO.COM and README.TXT)
Floppies: hello.img / hello.ima (1.44 MB FAT12 with HELLO.COM and README.TXT; same bytes, two extensions)
Programs (hand-made .COM, also written loose as KEYS.COM / MOUSE.COM / MIDI.COM; each is in hello-style folders
          keys/ mouse/ midi/ and zips keys.zip mouse.zip midi.zip): bytes documented next to the code below.
Only HELLO.COM / MENU*.COM / KEYS.COM / MOUSE.COM / MIDI.COM are real programs (hand-made; HELLO.COM bytes from docs/essais-dosbox.md section 4):
  B4 09 BA 10 01 CD 21 31 C0 CD 16 B8 00 4C CD 21   mov ah,9 / mov dx,0110h / int 21h / xor ax,ax / int 16h / exit
  followed by the '$'-terminated text at 0110h (SHA-256 ccd70355...a68d for "HELLO FROM EMU-RUST$").
Every other .EXE/.COM is a placeholder (".EXE" = "MZ" + filler) and must not be run.
Output is deterministic; zip timestamps are 1980-01-01 unless SOURCE_DATE_EPOCH is set (then that time, min 1980).
"""
import importlib.util
import io
import os
import struct
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


class Asm:
    """Minimal 8086 byte assembler for the tiny programs below: raw bytes, labels and short jumps only.
    Every emitted byte sequence is commented at the call site (this is the documentation of the programs)."""

    def __init__(self):
        self.out = bytearray()
        self.labels: dict[str, int] = {}
        self.fix8: list[tuple[int, str]] = []
        self.fix16: list[tuple[int, str]] = []

    def b(self, *data: int):
        self.out += bytes(data)
        return self

    def label(self, name: str):
        self.labels[name] = len(self.out)
        return self

    def jmp8(self, opcode: int, target: str):
        """Short conditional / unconditional jump (opcode + rel8)."""
        self.out += bytes([opcode, 0])
        self.fix8.append((len(self.out) - 1, target))
        return self

    def call(self, target: str):
        """E8 rel16."""
        self.out += bytes([0xE8, 0, 0])
        self.fix16.append((len(self.out) - 2, target))
        return self

    def bytes(self) -> bytes:
        out = bytearray(self.out)
        for pos, target in self.fix8:
            rel = self.labels[target] - (pos + 1)
            assert -128 <= rel <= 127, (target, rel)
            out[pos] = rel & 0xFF
        for pos, target in self.fix16:
            rel = self.labels[target] - (pos + 2)
            out[pos : pos + 2] = (rel & 0xFFFF).to_bytes(2, "little")
        return bytes(out)


def _putc(a: Asm):
    """DL = character: DOS INT 21h function 02h (the character is written to the console; AL is clobbered)."""
    a.b(0xB4, 0x02)  # mov ah,2
    a.b(0xCD, 0x21)  # int 21h


def keys_com() -> bytes:
    """KEYS.COM: echoes every key read with INT 16h function 00h (BIOS keyboard, AL = ASCII) until Esc (1Bh).
    Shows what the keyboard layout (dosbox_pure_keyboard_layout) and the keyboard callback deliver. 21 bytes."""
    a = Asm()
    a.label("loop")
    a.b(0xB4, 0x00)  # mov ah,0       wait for a key
    a.b(0xCD, 0x16)  # int 16h        AL = ASCII, AH = scan code
    a.b(0x3C, 0x1B)  # cmp al,1Bh     Esc ?
    a.jmp8(0x74, "done")  # je done
    a.b(0x88, 0xC2)  # mov dl,al
    _putc(a)  # mov ah,2 / int 21h   echo (CR and BS are interpreted by the console)
    a.jmp8(0xEB, "loop")  # jmp loop
    a.label("done")
    a.b(0xB8, 0x00, 0x4C)  # mov ax,4C00h
    a.b(0xCD, 0x21)  # int 21h        exit
    return a.bytes()


def _hex16(a: Asm):
    """Subroutine `hex16`: prints AX as four upper-case hex digits (clobbers AX, CX, DL)."""
    a.label("hex16")
    a.b(0xB9, 0x04, 0x00)  # mov cx,4
    a.label("hex16_digit")
    a.b(0xC1, 0xC0, 0x04)  # rol ax,4         next nibble into the low bits (80186+; DOSBox is always 386+)
    a.b(0x50)  # push ax
    a.b(0x24, 0x0F)  # and al,0Fh
    a.b(0x04, 0x30)  # add al,'0'
    a.b(0x3C, 0x39)  # cmp al,'9'
    a.jmp8(0x76, "hex16_put")  # jbe put
    a.b(0x04, 0x07)  # add al,7           'A'..'F'
    a.label("hex16_put")
    a.b(0x88, 0xC2)  # mov dl,al
    _putc(a)
    a.b(0x58)  # pop ax
    a.jmp8(0xE2, "hex16_digit")  # loop digit      (CX counts the four nibbles)
    a.b(0xC3)  # ret


def mouse_com() -> bytes:
    """MOUSE.COM: INT 33h driver built into DOSBox. Reset + show the cursor, then until a key is pressed prints
    `XXXX YYYY` (hex, CX = x, DX = y in the 640x200 text-mode coordinates) on one line (CR, no LF), so a relative
    motion of the host mouse changes the numbers. Exits on any key (Esc included)."""
    a = Asm()
    a.b(0x31, 0xC0)  # xor ax,ax
    a.b(0xCD, 0x33)  # int 33h          AX=0 reset driver
    a.b(0xB8, 0x01, 0x00)  # mov ax,1
    a.b(0xCD, 0x33)  # int 33h          AX=1 show cursor
    a.label("loop")
    a.b(0xB4, 0x01)  # mov ah,1
    a.b(0xCD, 0x16)  # int 16h          key waiting ? (ZF clear)
    a.jmp8(0x75, "done")  # jnz done
    a.b(0xB8, 0x03, 0x00)  # mov ax,3
    a.b(0xCD, 0x33)  # int 33h          AX=3 position: BX buttons, CX x, DX y
    a.b(0x52)  # push dx
    a.b(0x89, 0xC8)  # mov ax,cx
    a.call("hex16")  # x
    a.b(0xB2, 0x20)  # mov dl,' '
    _putc(a)
    a.b(0x58)  # pop ax           y
    a.call("hex16")
    a.b(0xB2, 0x0D)  # mov dl,0Dh       carriage return: the next line overwrites this one
    _putc(a)
    a.jmp8(0xEB, "loop")  # jmp loop
    a.label("done")
    a.b(0xB4, 0x00)  # mov ah,0         swallow the key
    a.b(0xCD, 0x16)  # int 16h
    a.b(0xB8, 0x00, 0x4C)  # mov ax,4C00h
    a.b(0xCD, 0x21)  # int 21h
    _hex16(a)
    return a.bytes()


MIDI_NOTES = (0x3C, 0x40, 0x43)  # C4 E4 G4


def midi_com() -> bytes:
    """MIDI.COM: drives the MPU-401 (data port 330h, status / command port 331h) in UART mode: reset-to-UART
    command 3Fh, program change 0 (piano) on channel 1, note-on C4 E4 G4 (velocity 7Fh), waits for a key,
    all-notes-off (controller 7Bh) and exits. Audible only with a General MIDI SoundFont chosen in the core."""
    a = Asm()
    a.b(0xBA, 0x31, 0x03)  # mov dx,331h     command / status port
    a.call("wait_write")
    a.b(0xB0, 0x3F)  # mov al,3Fh      UART mode
    a.b(0xEE)  # out dx,al
    a.label("ack")
    a.b(0xEC)  # in al,dx        status: bit 7 set = no data (DSR)
    a.b(0xA8, 0x80)  # test al,80h
    a.jmp8(0x75, "ack")  # jnz ack          wait for the acknowledge byte
    a.b(0x4A)  # dec dx          data port 330h
    a.b(0xEC)  # in al,dx        read the FEh acknowledge
    for byte in (0xC0, 0x00) + sum(((0x90, n, 0x7F) for n in MIDI_NOTES), ()):
        a.b(0xB0, byte)  # mov al,byte
        a.call("send")
    a.b(0xB4, 0x00)  # mov ah,0
    a.b(0xCD, 0x16)  # int 16h         wait for a key
    for byte in (0xB0, 0x7B, 0x00):  # all notes off
        a.b(0xB0, byte)  # mov al,byte
        a.call("send")
    a.b(0xB8, 0x00, 0x4C)  # mov ax,4C00h
    a.b(0xCD, 0x21)  # int 21h
    a.label("send")  # AL = byte to send to the data port
    a.b(0x50)  # push ax
    a.b(0xBA, 0x31, 0x03)  # mov dx,331h
    a.call("wait_write")
    a.b(0x4A)  # dec dx          330h
    a.b(0x58)  # pop ax
    a.b(0xEE)  # out dx,al
    a.b(0xC3)  # ret
    a.label("wait_write")  # DX = 331h: waits until bit 6 (DRR, "output not ready") is clear
    a.b(0xEC)  # in al,dx
    a.b(0xA8, 0x40)  # test al,40h
    a.jmp8(0x75, "wait_write")  # jnz wait_write
    a.b(0xC3)  # ret
    return a.bytes()


KEYS_COM = keys_com()
MOUSE_COM = mouse_com()
MIDI_COM = midi_com()


def fake_exe(tag: str) -> bytes:
    return (b"MZ" + tag.encode("ascii")).ljust(512, b"\0")


# case name -> {relative path (forward slashes): bytes}; used for folders and, with a root prefix, for zips
GAMES: dict[str, dict[str, bytes]] = {
    "hello": {"HELLO.COM": HELLO_COM},
    "menu": {"MENU1.COM": MENU1, "MENU2.COM": MENU2},
    "bat": {"RUN.BAT": BAT},
    "keys": {"KEYS.COM": KEYS_COM},
    "mouse": {"MOUSE.COM": MOUSE_COM},
    "midi": {"MIDI.COM": MIDI_COM},
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
    for prog in ("keys", "mouse", "midi"):
        out[prog + ".zip"] = make_zip(GAMES[prog])
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
    floppy = fat12_floppy({"HELLO.COM": HELLO_COM, "README.TXT": b"EMU-RUST FLOPPY TEST\r\n"}, "HELLO")
    out["hello.img"] = floppy
    out["hello.ima"] = floppy
    return out


def fat12_floppy(files: dict[str, bytes], label: str = "EMURUST") -> bytes:
    """1.44 MB FAT12 floppy (2880 sectors of 512 bytes, 1 sector per cluster, 224 root entries), files at the root,
    8.3 upper-case names, fixed timestamp 1980-01-01. Not bootable (the boot sector only carries the BPB)."""
    ss, total, spf, nfat, root_entries = 512, 2880, 9, 2, 224
    img = bytearray(total * ss)
    img[0:3] = b"\xeb\x3c\x90"
    img[3:11] = b"EMURUST "
    # BPB: bytes/sector, sectors/cluster, reserved, FATs, root entries, total sectors, media, sectors/FAT,
    # sectors/track, heads, hidden, big total
    struct.pack_into("<HBHBHHBHHHII", img, 11, ss, 1, 1, nfat, root_entries, total, 0xF0, spf, 18, 2, 0, 0)
    struct.pack_into("<BBBI11s8s", img, 36, 0, 0, 0x29, 0x454D5552, label.ljust(11).encode("ascii"), b"FAT12   ")
    img[510:512] = b"\x55\xaa"
    fat = [0xFF0, 0xFFF] + [0] * (total - 1 - nfat * spf - root_entries * 32 // ss - 1)
    root = (1 + nfat * spf) * ss
    data = root + root_entries * 32
    next_free = 2
    entries = []
    for name, content in sorted(files.items()):
        stem, _, ext = name.upper().partition(".")
        assert len(stem) <= 8 and len(ext) <= 3, name
        clusters = max(1, -(-len(content) // ss)) if content else 0
        first = next_free if clusters else 0
        for i in range(clusters):
            c = next_free + i
            fat[c] = 0xFFF if i == clusters - 1 else c + 1
            off = data + (c - 2) * ss
            chunk = content[i * ss : (i + 1) * ss]
            img[off : off + len(chunk)] = chunk
        next_free += clusters
        entries.append(
            (stem.ljust(8) + ext.ljust(3)).encode("ascii")
            + bytes([0x20])
            + bytes(10)
            + struct.pack("<HHHI", 0, 0x0021, first, len(content))  # 00:00:00, 1980-01-01
        )
    assert next_free <= len(fat)
    assert len(entries) <= root_entries
    for i, e in enumerate(entries):
        img[root + i * 32 : root + i * 32 + 32] = e
    packed = bytearray(spf * ss)
    for i, v in enumerate(fat):
        p = i * 3 // 2
        if i % 2 == 0:
            packed[p] |= v & 0xFF
            packed[p + 1] |= (v >> 8) & 0x0F
        else:
            packed[p] |= (v & 0x0F) << 4
            packed[p + 1] = (v >> 4) & 0xFF
    for k in range(nfat):
        img[(1 + k * spf) * ss : (1 + (k + 1) * spf) * ss] = packed
    return bytes(img)


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
