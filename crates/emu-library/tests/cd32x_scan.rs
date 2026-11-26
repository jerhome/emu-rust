//! Sega CD 32X mark (`Game::needs_32x`) from the Redump name tags on scanned folders (ADR 0024 §1; the probe of
//! the data track has its own tests in `scan.rs` / `disc.rs`). Discs come from `tests/tools/mkcd.py` (original
//! data, no commercial content, no network); `.chd` are dummy files, never read.
//! Needs `python3`.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;

use emu_library::{Dat, DatEntry, DatSource, Game, Library, ScanOptions, ScanReport};

fn mkcd(args: &[&str]) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/tools/mkcd.py");
    let out = Command::new("python3")
        .arg("-I")
        .arg(script)
        .args(args)
        .output()
        .expect("python3 is required for the Mega CD fixtures");
    assert!(
        out.status.success(),
        "mkcd.py {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn scan(lib: &mut Library, root: &Path) -> ScanReport {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    let report = lib
        .scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    report
}

fn game(lib: &Library, file: &str) -> Game {
    lib.games()
        .unwrap()
        .into_iter()
        .find(|g| g.path.file_name().is_some_and(|n| n == file))
        .unwrap_or_else(|| panic!("{file} not listed"))
}

fn chd(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"not read").unwrap();
}

/// `.cue` and `.chd` carrying the tag in their file name are marked; the same discs without tag are not, even
/// when the data track header says `SEGA 32X` (only the `32X Initial Program` boot code counts).
#[test]
fn tag_in_file_name_marks_cue_and_chd() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    mkcd(&[
        "megacd",
        &r("megacd/tagged"),
        "--name",
        "Fake Trap (USA) (Sega CD 32X)",
    ]);
    mkcd(&[
        "megacd",
        &r("megacd/headeronly"),
        "--name",
        "Fake Header (USA)",
    ]);
    mkcd(&["megacd", &r("megacd/plain"), "--name", "Fake Plain (USA)"]);
    chd(&root, "megacd/Fake Chd (Europe) (32X CD).chd");
    chd(&root, "megacd/Fake Dash (Japan) (Mega-CD 32X).chd");
    chd(&root, "megacd/Fake Bare (Europe).chd");
    chd(&root, "megacd/Fake Bracket (USA) [32X].chd");

    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    for (file, expected) in [
        ("Fake Trap (USA) (Sega CD 32X).cue", true),
        ("Fake Chd (Europe) (32X CD).chd", true),
        ("Fake Dash (Japan) (Mega-CD 32X).chd", true),
        ("Fake Header (USA).cue", false),
        ("Fake Plain (USA).cue", false),
        ("Fake Bare (Europe).chd", false),
        ("Fake Bracket (USA) [32X].chd", false),
    ] {
        let g = game(&lib, file);
        assert_eq!(g.system, "megacd", "{file}");
        assert_eq!(g.needs_32x, expected, "{file}");
    }
    // The mark is derived: a second scan changes nothing and the answer is the same.
    let again = scan(&mut lib, &root);
    assert_eq!((again.added, again.updated, again.removed), (0, 0, 0));
    assert!(game(&lib, "Fake Chd (Europe) (32X CD).chd").needs_32x);
}

/// A `.cue` whose file name has no tag but which the reduced Redump DAT identifies (track CRC) under a name that
/// does: the title carries the tag.
#[test]
fn cue_identified_by_dat_with_tagged_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    mkcd(&[
        "megacd",
        &r("megacd/a"),
        "--name",
        "Disc A",
        "--serial",
        "T-111111 -00",
    ]);
    mkcd(&[
        "megacd",
        &r("megacd/b"),
        "--name",
        "Disc B",
        "--serial",
        "T-222222 -00",
    ]);
    let track = |n: &str| {
        fs::read(root.join(format!(
            "megacd/{n}/Disc {} (Track 1).bin",
            n.to_uppercase()
        )))
        .unwrap()
    };
    let entry = |game: &str, data: &[u8]| DatEntry {
        game: game.to_owned(),
        rom_name: format!("{game} (Track 01).bin"),
        size: data.len() as u64,
        crc32: crc32fast::hash(data),
        sha1: None,
        serial: None,
        region: Some("USA".to_owned()),
    };
    let dat = Dat {
        name: "Sega - Mega-CD - Sega CD".into(),
        version: Some("20261125".into()),
        entries: vec![
            entry("Fake Dat Trap (USA) (Sega CD 32X)", &track("a")),
            entry("Fake Dat Plain (USA)", &track("b")),
        ],
    };
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat_source("megacd", DatSource::Redump, &dat, None)
        .unwrap();
    scan(&mut lib, &root);

    let a = game(&lib, "Disc A.cue");
    assert_eq!(a.title, "Fake Dat Trap (USA) (Sega CD 32X)");
    assert!(a.needs_32x);
    let b = game(&lib, "Disc B.cue");
    assert_eq!(b.title, "Fake Dat Plain (USA)");
    assert!(!b.needs_32x);
}

/// `(Disc 1)` / `(Disc 2)` `.chd` form one game, marked when the group is tagged; an untagged group is not.
#[test]
fn chd_disc_group_is_one_game() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    for k in [1, 2] {
        chd(
            &root,
            &format!("megacd/Fake Duo (USA) (Sega CD 32X) (Disc {k}).chd"),
        );
        chd(&root, &format!("megacd/Fake Solo (USA) (Disc {k}).chd"));
    }
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let games = lib.games().unwrap();
    assert_eq!(
        games.len(),
        2,
        "{:?}",
        games.iter().map(|g| &g.path).collect::<Vec<_>>()
    );
    let duo = game(&lib, "Fake Duo (USA) (Sega CD 32X) (Disc 1).chd");
    assert_eq!(duo.media.len(), 2);
    assert!(duo.needs_32x);
    let solo = game(&lib, "Fake Solo (USA) (Disc 1).chd");
    assert_eq!(solo.media.len(), 2);
    assert!(!solo.needs_32x);
}

/// A `.m3u` whose title is untagged but of which only one entry carries the tag is marked; one with no tagged
/// entry is not.
#[test]
fn m3u_with_a_single_tagged_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let dir = root.join("megacd").join("m3u");
    let d = dir.to_string_lossy().into_owned();
    for name in [
        "Mixed (USA) (Disc 1)",
        "Mixed Alt (USA) (Disc 2) (32X CD)",
        "Clean (USA) (Disc 1)",
        "Clean (USA) (Disc 2)",
    ] {
        mkcd(&["megacd", &d, "--name", name]);
    }
    fs::write(
        dir.join("Mixed (USA).m3u"),
        "Mixed (USA) (Disc 1).cue\r\nMixed Alt (USA) (Disc 2) (32X CD).cue\r\n",
    )
    .unwrap();
    fs::write(
        dir.join("Clean (USA).m3u"),
        "Clean (USA) (Disc 1).cue\r\nClean (USA) (Disc 2).cue\r\n",
    )
    .unwrap();
    // The playlists absorb their discs, the `m3u` written by `mkcd.py` for `--discs 1` does not exist.
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let mixed = game(&lib, "Mixed (USA).m3u");
    assert_eq!(mixed.media.len(), 2);
    assert!(!crate_tag(&mixed.title), "title must be untagged");
    assert!(mixed.needs_32x);
    let clean = game(&lib, "Clean (USA).m3u");
    assert_eq!(clean.media.len(), 2);
    assert!(!clean.needs_32x);
}

fn crate_tag(name: &str) -> bool {
    emu_library::name_needs_32x(name)
}

/// A `.32x` cartridge, even tagged, is a `32x` game; an ordinary Mega CD disc is not marked.
#[test]
fn cartridge_and_ordinary_disc_are_not_marked() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    fs::create_dir_all(root.join("32x")).unwrap();
    mkcd(&["32x", &r("32x/Fake Cart (USA) (32X).32x")]);
    mkcd(&["megacd", &r("megacd/ord"), "--name", "Ordinary (USA)"]);
    chd(&root, "megacd/Ordinary Chd (Europe).chd");

    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let cart = game(&lib, "Fake Cart (USA) (32X).32x");
    assert_eq!(cart.system, "32x");
    assert!(!cart.needs_32x);
    for f in ["Ordinary (USA).cue", "Ordinary Chd (Europe).chd"] {
        let g = game(&lib, f);
        assert_eq!(g.system, "megacd");
        assert!(!g.needs_32x, "{f}");
    }
}

/// The naming of the author's own files (guide 7.2): the tag comes AFTER the disc marker, in both files. They must
/// still form one game and be marked.
#[test]
fn chd_group_with_the_tag_after_the_disc_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    for k in [1, 2] {
        chd(
            &root,
            &format!("megacd/Fake Night (Europe) (Disc {k}) (Mega-CD 32X).chd"),
        );
    }
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let games = lib.games().unwrap();
    assert_eq!(
        games.len(),
        1,
        "{:?}",
        games.iter().map(|g| &g.path).collect::<Vec<_>>()
    );
    assert_eq!(games[0].media.len(), 2);
    assert!(games[0].needs_32x);
}

/// A tagged disc moved by hand (cue folder and `.chd`) then rescanned: the moved-file check keeps the row id and
/// the mark.
#[test]
fn moved_by_hand_keeps_id_and_needs_32x() {
    let tmp = tmp_root();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    mkcd(&[
        "megacd",
        &r("megacd/tagged"),
        "--name",
        "Fake Trap (USA) (Sega CD 32X)",
    ]);
    chd(&root, "megacd/Fake Chd (Europe) (32X CD).chd");
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let cue = game(&lib, "Fake Trap (USA) (Sega CD 32X).cue");
    let chd_game = game(&lib, "Fake Chd (Europe) (32X CD).chd");
    assert!(cue.needs_32x && chd_game.needs_32x);

    fs::create_dir_all(root.join("megacd").join("sorted")).unwrap();
    fs::rename(
        root.join("megacd").join("tagged"),
        root.join("megacd").join("sorted").join("tagged"),
    )
    .unwrap();
    fs::rename(
        root.join("megacd").join("Fake Chd (Europe) (32X CD).chd"),
        root.join("megacd")
            .join("sorted")
            .join("Fake Chd (Europe) (32X CD).chd"),
    )
    .unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.moved, 2, "{report:?}");
    assert_eq!(lib.games().unwrap().len(), 2);
    for (file, id) in [
        ("Fake Trap (USA) (Sega CD 32X).cue", cue.id),
        ("Fake Chd (Europe) (32X CD).chd", chd_game.id),
    ] {
        let g = game(&lib, file);
        assert_eq!(g.id, id, "{file}");
        assert!(g.needs_32x, "{file}");
        assert!(g.path.to_string_lossy().contains("sorted"), "{file}");
    }
}

fn tmp_root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
