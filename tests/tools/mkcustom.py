#!/usr/bin/env python3
"""Generate the synthetic data of the 8.0 custom systems tests (ADR 0025, design v8.0 section 3), no commercial data.

usage (python3 -I mkcustom.py ...):
  mkcustom.py OUTDIR [--no-special]
      writes OUTDIR/ (must not exist) and OUTDIR/manifest.json. --no-special skips the symlink and the FIFO (the
      cases are then absent from the manifest too); on Windows they are skipped automatically.

Layout (everything below OUTDIR; output is deterministic, fixed mtime 1700000000, sorted manifest)
  info/<name>.info          synthetic libretro core-info files (one per case, see manifest "info")
  definitions/<case>/       one directory per case = one <data>/systems/ directory (call Catalog::load on it);
                            files are <id>.toml
  official/systems.toml     synthetic official catalog (Catalog::load_with_official), format of emu-library/data/systems.toml
  official/cores.toml       synthetic cores.toml for the switch tests (format of emu-assets/data/cores.toml)
  keys/                     key profiles for the switch ("official profile already present and different")
  trees/<case>/             data dirs (roms/<id>/..., systems/.retired/...) with tiny synthetic files

manifest.json (version 1)
  info[]         {"file", "case", "expect": {...}}  hints on what parse_info / addability must give
  definitions[]  {"case", "dir", "files", "expect"}  expect["ok"] = ids loaded, expect["errors"] = [{"id",
                 "error": DefinitionError variant name}]; optional "ignored" (files beyond the 256 bound),
                 "requires": "unix" (symlink / FIFO), "api_only": true (not reachable from a file), "note"
  trees[]        {"case", "dir", "expect": "..."}    expectation in words (reserved folders, conflicts)
  platform       {"special": bool}

Every DefinitionError variant is covered by a file except DuplicateId (two files cannot share a stem: reachable only
through validate_custom(creating = true); case "duplicate-id" is api_only). Cases marked with a note about the
loader's choice (first file wins for DuplicateDatabase, Syntax for unknown fields) follow the design text and are to
be confirmed by lot PB2.
"""
import argparse
import json
import os
import sys

EPOCH = 1700000000
BEL = chr(7)
RLO = chr(0x202E)
T_BEL = chr(92) + "u0007"  # TOML escapes: raw control characters would be a syntax error instead
T_RLO = chr(92) + "u202E"


def toml_def(**o):
    """A valid format-1 definition, overridable per field. Values are pre-rendered TOML (use q() for strings)."""
    f = {
        "format": "1", "render": '"software"', "id": '"zzdemo"', "name": '"Demo System"',
        "database": '"Demo - Fictional - Console"', "extensions": '["dmo"]', "dat": '["no-intro"]',
        "screenscraper": "9001", "igdb": "9002", "cores": '["demo_core"]', "ignored_firmware": "[]",
        "origin_core": '"demo_core"', "info_name": '"Demo (Fictional Core)"', "created_by": '"8.0.0"',
        "created_unix": "1764028800",
    }
    f.update(o)
    lines = ["format = %s" % f["format"], "render = %s" % f["render"], "", "[system]", "id = %s" % f["id"],
             "name = %s" % f["name"]]
    if f["database"] is not None:
        lines.append("database = %s" % f["database"])
    lines += ["extensions = %s" % f["extensions"], "dat = %s" % f["dat"]]
    if f["screenscraper"] is not None:
        lines.append("screenscraper = %s" % f["screenscraper"])
    if f["igdb"] is not None:
        lines.append("igdb = %s" % f["igdb"])
    lines += ["", "[cores]", "list = %s" % f["cores"], "ignored_firmware = %s" % f["ignored_firmware"],
              "", "[origin]", "core = %s" % f["origin_core"], "info_name = %s" % f["info_name"],
              "created_by = %s" % f["created_by"], "created_unix = %s" % f["created_unix"], ""]
    return "\n".join(lines)


def q(s):
    return '"%s"' % s


def tlist(items):
    return "[" + ", ".join(q(i) for i in items) + "]"


def write(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data if isinstance(data, bytes) else data.encode("utf-8"))
    os.utime(path, (EPOCH, EPOCH))


# ---- core-info files -------------------------------------------------------------------------------------------

def info_cases():
    base = 'display_name = "%s"\ncorename = "%s"\nlicense = "Public Domain"\n'

    def mk(name, extra):
        return base % (name, name) + extra

    return [
        ("cartridge", mk("Demo Cartridge", 'supported_extensions = "dmo|dm2"\ndatabase = "Demo - Fictional - Console"\n'
                        'systemname = "Demo Console"\nhw_render = "false"\nsupports_no_game = "false"\n'),
         {"addability": "Cartridge", "render": "Software", "databases": ["Demo - Fictional - Console"]}),
        ("multi-mixed", mk("Demo Mixed", 'supported_extensions = "dmo|cue|chd|m3u"\n'
                          'database = "Demo - Fictional - Console|Demo - Fictional - Console CD"\n'
                          'systemname = "Demo Console / CD"\nhw_render = "false"\n'),
         {"addability": "one entry per database (Q5): cartridge + disc", "databases": 2}),
        ("hw-three-apis", mk("Demo Hardware", 'supported_extensions = "dmo"\ndatabase = "Demo - Fictional - 3D"\n'
                            'hw_render = "true"\nrequired_hw_api = "OpenGL Core >= 3.3 | Vulkan >= 1.0 | Direct3D >= 11.0"\n'),
         {"render": "Hardware(3 apis)", "addability": "not addable in 8.0 (hardware rendering)"}),
        ("hw-no-api", mk("Demo Hardware Any", 'supported_extensions = "dmo"\ndatabase = "Demo - Fictional - 3D"\n'
                        'hw_render = "true"\n'),
         {"render": "Hardware([])", "addability": "not addable in 8.0"}),
        ("cd-only", mk("Demo CD", 'supported_extensions = "cue|ccd|chd|toc|m3u"\ndatabase = "Demo - Fictional - CD"\n'
                      'hw_render = "false"\n'),
         {"addability": "disc only: grayed (cartridge_extensions is empty)"}),
        ("arcade", mk("Demo Arcade", 'supported_extensions = "zip"\nhw_render = "false"\nneeds_fullpath = "true"\n'),
         {"addability": "archive only: arcade tier, no database"}),
        ("no-database", mk("Demo No Database", 'supported_extensions = "dmo"\nhw_render = "false"\n'),
         {"databases": [], "addability": "addable without database (no DAT, no libretro covers)"}),
        ("parasitic-values", mk("Demo Parasites", 'supported_extensions = "dmo|/|.|V32"\ndatabase = "/|.|V32"\n'
                               'systemname = "V32"\nhw_render = "false"\n'),
         {"note": "values '/', '.' and 'V32' in extensions, database and systemname must not crash or become paths"}),
        ("long-text-control", mk("Demo Long", 'supported_extensions = "dmo"\ndatabase = "Demo - Fictional - Console"\n'
                                'systemname = "' + "L" * 400 + BEL + RLO + 'end"\n'
                                'categories = "' + "|".join(["C" * 50] * 20) + '"\nhw_render = "false"\n'),
         {"system_name": "<= 128 chars, control and bidi removed", "categories": "<= 8 entries of <= 32 chars",
          "note": "the control and bidi characters are written as UTF-8, not escapes (core-info has no escapes)"}),
        ("slash-name", mk("Demo Slash Name", 'supported_extensions = "dmo"\n'
                         'database = "Demo - Fictional - Console"\nsystemname = "Demo 8/16 bit (various)"\n'
                         'hw_render = "false"\n'),
         {"system_name": "Demo 8/16 bit (various)", "normalize_name": "Demo 8-16 bit (various)"}),
        ("unknown-render", mk("Demo Unknown Render", 'supported_extensions = "dmo"\ndatabase = "Demo - Fictional - Console"\n'),
         {"render": "Unknown", "addability": "addable (the launch decides)"}),
    ]


# ---- definitions -----------------------------------------------------------------------------------------------

def definition_cases(special):
    c = []  # (case, {filename: content}, expect, extras)
    d = toml_def

    def ok(case, files, ids, **kw):
        c.append((case, files, dict(ok=sorted(ids), errors=[]), kw))

    def err(case, files, errors, ok_ids=(), **kw):
        c.append((case, files, dict(ok=sorted(ok_ids), errors=errors), kw))

    def one(case, stem, error, **fields):
        err(case, {stem + ".toml": d(**fields)}, [dict(id=stem, error=error)])

    ok("valid-minimal", {"zzdemo.toml": d()}, ["zzdemo"])
    ok("valid-no-database", {"nodb.toml": d(id=q("nodb"), database=None, dat="[]", screenscraper=None, igdb=None)}, ["nodb"])
    ok("valid-pcengine", {"pcengine.toml": d(id=q("pcengine"), name=q("PC Engine"),
        database=q("NEC - PC Engine - TurboGrafx 16"), extensions='["pce"]', cores='["mednafen_pce_fast"]',
        origin_core=q("mednafen_pce_fast"), info_name=q("NEC - PC Engine / CD (Beetle PCE FAST)"),
        screenscraper="31", igdb="86")}, ["pcengine"])
    ok("valid-limits", {"a" * 24 + ".toml": d(id=q("a" * 24), name=q("N" * 64), database=q("D" * 128),
        extensions=tlist(["e%07d" % i for i in range(64)]), dat='["no-intro", "tosec", "libretro"]',
        cores=tlist(["core_%d" % i for i in range(1, 8)] + ["demo_core"]),
        ignored_firmware=tlist(["firm/f%d.bin" % i for i in range(64)]),
        info_name=q("I" * 128), created_by=q("B" * 32))}, ["a" * 24])
    ok("valid-name-punctuation", {"punct.toml": d(id=q("punct"), name=q("Demo + Co., Ltd. 'X' & (Y)! v_2"),
        database=q("Demo - Fictional (A, B) & Co's - Console"))}, ["punct"])
    ok("valid-info-name-slash", {"slash.toml": d(id=q("slash"), info_name=q("Demo 8/16 bit (various) / CD"))}, ["slash"])
    ok("valid-256", {"s%03d.toml" % i: d(id=q("s%03d" % i), database=q("Demo - Many - %03d" % i),
                                         extensions='["m%02d"]' % (i % 100)) for i in range(256)},
       ["s%03d" % i for i in range(256)])
    many = {"t%03d.toml" % i: d(id=q("t%03d" % i), database=q("Demo - Many - %03d" % i)) for i in range(300)}
    c.append(("many-300", many, dict(ok=sorted("t%03d" % i for i in range(256)),
                   errors=[dict(id="t%03d" % i, error="TooManyDefinitions") for i in range(256, 300)]),
              dict(ignored=sorted("t%03d.toml" % i for i in range(256, 300)),
                   note="the 256 first by name are read; each of the other 44 is a TooManyDefinitions problem")))

    err("e-syntax", {"bad.toml": "format = 1\n[system\nid = \n"}, [dict(id="bad", error="Syntax")])
    err("e-unknown-field", {"unk.toml": d(id=q("unk")).replace("[origin]", "surprise = 1\n\n[origin]")},
        [dict(id="unk", error="Syntax", note="deny_unknown_fields: reported as a syntax error (to confirm in PB2)")])
    err("e-toolarge", {"big.toml": d(id=q("big")) + "# " + "x" * (64 * 1024) + "\n"}, [dict(id="big", error="TooLarge")])
    err("e-newer-format", {"fmt2.toml": d(id=q("fmt2"), format="2")}, [dict(id="fmt2", error="NewerFormat")],
        note="the file stays intact")
    one("e-render-hardware", "hw", "UnsupportedRender", id=q("hw"), render=q("hardware"))
    one("e-invalid-id-upper", "BAD", "InvalidId", id=q("BAD"))
    one("e-invalid-id-long", "x" * 25, "InvalidId", id=q("x" * 25))
    one("e-invalid-id-dash", "a-b", "InvalidId", id=q("a-b"))
    one("e-reserved-windows", "con", "ReservedId", id=q("con"))
    one("e-reserved-folder", "roms", "ReservedId", id=q("roms"))
    one("e-reserved-table-id", "psx", "ReservedId", id=q("psx"), database=q("Demo - Not Playstation"))
    err("e-id-not-file-name", {"other.toml": d(id=q("zzdemo"))}, [dict(id="other", error="IdNotFileName")])
    c.append(("duplicate-id", {"zzdemo.toml": d()}, dict(ok=["zzdemo"], errors=[]),
              dict(api_only=True, note="validate_custom(draft of zzdemo, catalog containing zzdemo, creating = true) = DuplicateId")))
    err("e-duplicate-database", {"first.toml": d(id=q("first")), "second.toml": d(id=q("second"))},
        [dict(id="second", error="DuplicateDatabase")], ok_ids=["first"],
        note="same database in two files: the first by name wins, the second is in error (to confirm in PB2)")
    one("e-invalid-name-slash", "nm", "InvalidName", id=q("nm"), name=q("Demo/Sys"))
    one("e-invalid-name-dotdot", "nm2", "InvalidName", id=q("nm2"), name=q("a..b"))
    one("e-invalid-name-control", "nm3", "InvalidName", id=q("nm3"), name=q("Demo" + T_BEL))
    one("e-invalid-name-bidi", "nm4", "InvalidName", id=q("nm4"), name=q("Demo" + T_RLO + "evil"))
    one("e-invalid-name-trailing-dot", "nm5", "InvalidName", id=q("nm5"), name=q("Demo."))
    one("e-invalid-name-long", "nm6", "InvalidName", id=q("nm6"), name=q("N" * 65))
    one("e-invalid-database-colon", "db1", "InvalidDatabase", id=q("db1"), database=q("Demo: Fictional"))
    one("e-invalid-database-long", "db2", "InvalidDatabase", id=q("db2"), database=q("D" * 129))
    one("e-invalid-database-leading-space", "db3", "InvalidDatabase", id=q("db3"), database=q(" Demo"))
    one("e-no-extension", "noext", "NoExtension", id=q("noext"), extensions="[]")
    one("e-invalid-extension-upper", "ex1", "InvalidExtension", id=q("ex1"), extensions='["PCE"]')
    one("e-invalid-extension-long", "ex2", "InvalidExtension", id=q("ex2"), extensions='["abcdefghi"]')
    one("e-invalid-extension-dot", "ex3", "InvalidExtension", id=q("ex3"), extensions='[".pce"]')
    one("e-invalid-extension-65", "ex4", "InvalidExtension", id=q("ex4"), extensions=tlist(["e%d" % i for i in range(65)]))
    for ext in ["zip", "7z", "m3u", "m3u8", "cue", "chd", "iso", "ccd", "toc", "gdi", "cdi", "mds", "mdf", "nrg", "pbp",
                "cso", "img", "bin"]:
        one("e-forbidden-extension-" + ext, "fe", "ForbiddenExtension", id=q("fe"), extensions=tlist(["dmo", ext]))
    one("e-unsupported-dat-redump", "dt", "UnsupportedDat", id=q("dt"), dat='["redump"]')
    one("e-no-core", "nc", "NoCore", id=q("nc"), cores="[]")
    one("e-too-many-cores", "tm", "TooManyCores", id=q("tm"), cores=tlist(["c%d" % i for i in range(9)]))
    one("e-invalid-core-upper", "ic", "InvalidCore", id=q("ic"), cores='["Demo Core"]')
    one("e-invalid-core-path", "ic2", "InvalidCore", id=q("ic2"), cores='["../x"]')
    one("e-invalid-origin-long-info", "or1", "InvalidOrigin", id=q("or1"), info_name=q("I" * 129))
    one("e-invalid-origin-created-by", "or2", "InvalidOrigin", id=q("or2"), created_by=q("B" * 33))
    one("e-invalid-origin-core", "or3", "InvalidOrigin", id=q("or3"), origin_core='"Bad Core"')
    one("e-unsafe-firmware-dotdot", "fw1", "UnsafeFirmwarePath", id=q("fw1"), ignored_firmware='["../etc/passwd"]')
    one("e-unsafe-firmware-abs", "fw2", "UnsafeFirmwarePath", id=q("fw2"), ignored_firmware='["/abs.bin"]')
    one("e-unsafe-firmware-backslash", "fw3", "UnsafeFirmwarePath", id=q("fw3"), ignored_firmware="['a\\\\b.bin']")
    one("e-too-many-firmware", "fw4", "TooManyFirmware", id=q("fw4"), ignored_firmware=tlist(["f%d.bin" % i for i in range(65)]))

    ok("conflict-same-id", {"megadrive.toml": d(id=q("megadrive"), database=q("Demo - Another Machine"))}, ["megadrive"],
       note="same id as an official, other database: Uncertain switch, official masked")
    ok("conflict-same-database", {"mymd.toml": d(id=q("mymd"), database=q("Sega - Mega Drive - Genesis"))}, ["mymd"],
       note="same database as megadrive, other id: NeedsMove")
    ok("conflict-both", {"megadrive.toml": d(id=q("megadrive"), database=q("Sega - Mega Drive - Genesis"))}, ["megadrive"],
       note="same id and database: InPlace")
    ok("conflict-no-database-official-id", {"nes.toml": d(id=q("nes"), database=None)}, ["nes"],
       note="no database but an id of the table: validate_custom refuses it when creating (ADR 0025 section 3); "
            "loaded from a file it is an Uncertain conflict with the official nes (to confirm in PB2)")
    if special:
        c.append(("special-symlink", {}, dict(ok=[], errors=[dict(id="link", error="NotRegularFile")]),
                  dict(requires="unix", special="symlink")))
        c.append(("special-fifo", {}, dict(ok=[], errors=[dict(id="pipe", error="NotRegularFile")]),
                  dict(requires="unix", special="fifo")))
    return c


def officials():
    systems = '''# Synthetic official catalog for the switch tests (ADR 0025 section 10). Never shipped.
[[system]]
id = "synth"
name = "Synthetic Official"
extensions = ["syn", "sy2"]
database = "Synthetic - Official - Console"
screenscraper = 9100
igdb = 9101

[[system]]
id = "synthb"
name = "Synthetic Official B"
extensions = ["sb"]
database = "Synthetic - Official - Console B"
'''
    cores = '''# Synthetic cores.toml for the switch tests.
[systems."Synthetic - Official - Console"]
cores = ["synth_core", "demo_core"]

[systems."Synthetic - Official - Console B"]
cores = ["synth_core_b"]
'''
    return systems, cores


def keys_profiles():
    same = '[bindings]\nup = "ArrowUp"\ndown = "ArrowDown"\nfire = "Space"\n'
    other = '[bindings]\nup = "W"\ndown = "S"\nfire = "Enter"\n'
    return {"official-profile.toml": same, "user-profile-different.toml": other, "user-profile-identical.toml": same}


def trees():
    t = []

    def b(n):
        return b"DEMO" + bytes(n)  # tag + zeros: never a real header

    t.append(("custom-folder-destination-conflict", {
        "roms/zzdemo/Game A.dmo": b(8), "roms/synth/Game A.dmo": b(8) + b"X",
        "systems/zzdemo.toml": toml_def(database=q("Synthetic - Official - Console")),
    }, "roms/zzdemo/Game A.dmo and roms/synth/Game A.dmo differ: a move of zzdemo to synth is not proposable (conflict)"))
    t.append(("reserved-folders", {
        "systems/.retired/gone-1700000000.toml": toml_def(id=q("gone"), database=q("Demo - Gone")),
        "systems/broken.toml": "not toml [",
        "roms/gone/Game.dmo": b(4), "roms/gone/disc.bin": b(16),
        "roms/broken/Game.dmo": b(4), "roms/broken/disc.bin": b(16),
        "roms/orphan/Game.dmo": b(4),
    }, "gone (retired), broken (in error), orphan (id absent from the catalog): tidy takes nothing, puts nothing, "
       "never removes; Restore finds every game"))
    t.append(("active-id-never-reserved", {
        "systems/megadrive.toml": "not toml [",
        "systems/.retired/megadrive-1700000001.replaced.toml": toml_def(id=q("megadrive")),
        "roms/megadrive/Game.md": b(4),
    }, "megadrive is an active official id (invalid custom file, replaced file): never reserved"))
    t.append(("retired-names", {
        "systems/.retired/zzdemo-1700000000.toml": toml_def(),
        "systems/.retired/zzdemo-1700000002.replaced.toml": toml_def(),
        "systems/.retired/zzdemo-1700000003.bak": b"x",
        "systems/.retired/ZZ-1.toml": toml_def(),
        "systems/.retired/zzdemo-notanumber.toml": toml_def(),
    }, "accepted: the Removed and the Replaced names only; refused (not listed): .bak, ZZ-1.toml, zzdemo-notanumber.toml"))
    t.append(("conflict-uncertain-rows", {
        "roms/megadrive/Game.md": b(4), "roms/megadrive/Other.md": b(8),
        "systems/megadrive.toml": toml_def(id=q("megadrive"), database=q("Demo - Another Machine")),
    }, "uncertain conflict: rows with id megadrive belong to the official; deletion refused while a game stays under "
       "roms/megadrive/, then forget_system(megadrive, Keep)"))
    return t


def build(out, special):
    manifest = {"version": 1, "info": [], "definitions": [], "trees": [], "platform": {"special": special}}
    for name, text, expect in info_cases():
        write(os.path.join(out, "info", name + ".info"), text)
        manifest["info"].append({"file": "info/%s.info" % name, "case": name, "expect": expect})
    for case, files, expect, extra in definition_cases(special):
        d = os.path.join(out, "definitions", case)
        os.makedirs(d, exist_ok=True)
        for fname, content in files.items():
            write(os.path.join(d, fname), content)
        entry = {"case": case, "dir": "definitions/" + case, "files": sorted(files), "expect": expect}
        entry.update({k: v for k, v in extra.items() if k != "special"})
        if extra.get("special") == "symlink":
            os.symlink(os.path.join("..", "..", "info", "cartridge.info"), os.path.join(d, "link.toml"))
            entry["files"] = ["link.toml"]
        elif extra.get("special") == "fifo":
            os.mkfifo(os.path.join(d, "pipe.toml"))
            entry["files"] = ["pipe.toml"]
        manifest["definitions"].append(entry)
    s, c = officials()
    write(os.path.join(out, "official", "systems.toml"), s)
    write(os.path.join(out, "official", "cores.toml"), c)
    for n, text in keys_profiles().items():
        write(os.path.join(out, "keys", n), text)
    for case, files, expect in trees():
        for rel, content in files.items():
            write(os.path.join(out, "trees", case, *rel.split("/")), content)
        manifest["trees"].append({"case": case, "dir": "trees/" + case, "expect": expect})
    for k in ("definitions", "info", "trees"):
        manifest[k].sort(key=lambda e: e["case"])
    write(os.path.join(out, "manifest.json"), json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir")
    ap.add_argument("--no-special", action="store_true")
    a = ap.parse_args()
    if os.path.exists(a.outdir):
        sys.exit("%s already exists" % a.outdir)
    build(a.outdir, not a.no_special and os.name == "posix")


if __name__ == "__main__":
    main()
