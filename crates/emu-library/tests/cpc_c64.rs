//! CPC and C64 in the library (design v5.0 §3.4, §6): shared `.tap`, folder-only extensions, tape and side groups,
//! mixed archives, two CPC DATs. Generated fixtures only, no ROM, no network.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use emu_library::{
    Dat, DatEntry, DatSource, Game, Identification, Library, ScanOptions, ScanReport, hash_reader,
};

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn zip_of(path: &Path, files: &[(&str, &[u8])]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn blob(seed: u8) -> Vec<u8> {
    (0..2048usize)
        .map(|i| (i as u8).wrapping_mul(7) ^ seed)
        .collect()
}

fn scan(lib: &mut Library, root: &Path) -> ScanReport {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn entry(game: &str, rom: &str, data: &[u8]) -> DatEntry {
    let h = hash_reader(data, data.len() as u64, None, u64::MAX).unwrap();
    DatEntry {
        game: game.into(),
        rom_name: rom.into(),
        size: h.full.size,
        crc32: h.full.crc32,
        sha1: Some(h.full.sha1),
    }
}

fn dat(entries: Vec<DatEntry>) -> Dat {
    Dat {
        name: "test".into(),
        version: None,
        entries,
    }
}

fn find<'a>(games: &'a [Game], name: &str) -> Option<&'a Game> {
    games
        .iter()
        .find(|g| g.path.file_name().is_some_and(|n| n == name))
}

fn system_of(lib: &Library, name: &str) -> Option<String> {
    find(&lib.games().unwrap(), name).map(|g| g.system.clone())
}

#[test]
fn extensions_and_folder_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("a.dsk"), &blob(1));
    write(&root.join("b.cdt"), &blob(2));
    write(&root.join("c.d64"), &blob(3));
    write(&root.join("d.prg"), &blob(4));
    write(&root.join("e.tap"), &blob(5));
    write(&root.join("cpc/f.tap"), &blob(6));
    write(&root.join("cpc/g.sna"), &blob(7));
    write(&root.join("h.sna"), &blob(8));
    write(&root.join("i.voc"), &blob(9));
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    for (name, system) in [
        ("a.dsk", "cpc"),
        ("b.cdt", "cpc"),
        ("c.d64", "c64"),
        ("d.prg", "c64"),
        ("e.tap", "c64"),
        ("f.tap", "cpc"),
        ("g.sna", "cpc"),
    ] {
        assert_eq!(system_of(&lib, name).as_deref(), Some(system), "{name}");
    }
    assert_eq!(system_of(&lib, "h.sna"), None);
    assert_eq!(system_of(&lib, "i.voc"), None);
    assert_eq!(report.unclassified, 2, "{report:?}");
}

#[test]
fn sides_and_tapes_group_and_playlists_follow_their_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("Quest (Side A).dsk"), &blob(1));
    write(&root.join("Quest (Side B).dsk"), &blob(2));
    write(&root.join("Saga (Tape 1 of 2).cdt"), &blob(3));
    write(&root.join("Saga (Tape 2 of 2).cdt"), &blob(4));
    write(&root.join("Run (Tape 1).tap"), &blob(5));
    write(&root.join("Run (Tape 2).tap"), &blob(6));
    write(&root.join("pl.m3u"), b"x.d64\ny.d64\n");
    write(&root.join("x.d64"), &blob(7));
    write(&root.join("y.d64"), &blob(8));
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    let games = lib.games().unwrap();
    let quest = games.iter().find(|g| g.title == "Quest").unwrap();
    assert_eq!((quest.system.as_str(), quest.media.len()), ("cpc", 2));
    let saga = games.iter().find(|g| g.title == "Saga").unwrap();
    assert_eq!((saga.system.as_str(), saga.media.len()), ("cpc", 2));
    let run = games.iter().find(|g| g.title == "Run").unwrap();
    assert_eq!((run.system.as_str(), run.media.len()), ("c64", 2));
    assert_eq!(system_of(&lib, "pl.m3u").as_deref(), Some("c64"));
}

#[test]
fn mixed_archive_is_ignored_and_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    zip_of(
        &root.join("mixed.zip"),
        &[("a.dsk", &blob(1)), ("b.d64", &blob(2))],
    );
    zip_of(
        &root.join("cpc.zip"),
        &[("a.dsk", &blob(3)), ("b.cdt", &blob(4))],
    );
    zip_of(&root.join("c64.zip"), &[("hello.d64", &blob(5))]);
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.ignored_mixed_archives, 1);
    assert_eq!(system_of(&lib, "mixed.zip"), None);
    assert_eq!(system_of(&lib, "cpc.zip").as_deref(), Some("cpc"));
    assert_eq!(system_of(&lib, "c64.zip").as_deref(), Some("c64"));
    // Counted again at each scan, never stored.
    let again = scan(&mut lib, &root);
    assert_eq!((again.ignored_mixed_archives, again.unchanged), (1, 2));
}

#[test]
fn other_computers_zips_are_not_mixed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    // `.prg` only exists for the C64 and `.voc` only under `cpc/`: neither makes these zips ambiguous.
    zip_of(
        &root.join("st.zip"),
        &[("game.st", &blob(1)), ("game.prg", &blob(2))],
    );
    zip_of(
        &root.join("amiga.zip"),
        &[("game.adf", &blob(3)), ("sound.voc", &blob(4))],
    );
    zip_of(
        &root.join("mixed.zip"),
        &[("a.dsk", &blob(5)), ("b.d64", &blob(6))],
    );
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.ignored_mixed_archives, 1, "{report:?}");
    assert_eq!(system_of(&lib, "st.zip").as_deref(), Some("atarist"));
    assert_eq!(system_of(&lib, "amiga.zip").as_deref(), Some("amiga"));
    assert_eq!(system_of(&lib, "mixed.zip"), None);
}

#[test]
fn two_cpc_dats_and_the_c64_dat_identify_by_crc() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("one.dsk"), &blob(1));
    write(&root.join("two.dsk"), &blob(2));
    write(&root.join("tape.d64"), &blob(3));
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat_source(
        "cpc",
        DatSource::Libretro,
        &dat(vec![entry("Libretro One", "1.dsk", &blob(1))]),
        None,
    )
    .unwrap();
    lib.import_dat_source(
        "cpc",
        DatSource::Tosec,
        &dat(vec![entry("Tosec Two", "2.dsk", &blob(2))]),
        None,
    )
    .unwrap();
    lib.import_dat(
        "c64",
        &dat(vec![entry("No-Intro Three", "3.d64", &blob(3))]),
        None,
    )
    .unwrap();
    scan(&mut lib, &root);
    let games = lib.games().unwrap();
    for (file, title) in [
        ("one.dsk", "Libretro One"),
        ("two.dsk", "Tosec Two"),
        ("tape.d64", "No-Intro Three"),
    ] {
        let g = find(&games, file).unwrap();
        assert_eq!(g.title, title);
        assert_ne!(g.identification, Identification::Unknown);
    }
}

fn game_at<'a>(games: &'a [Game], file: &str) -> &'a Game {
    games
        .iter()
        .find(|g| g.path.file_name().is_some_and(|n| n == file))
        .unwrap_or_else(|| panic!("{file} not in {games:?}"))
}

/// A zip of several floppies is one game whose `media` is the zip itself (emu-runtime extracts every disk it
/// holds); `entry` is the first disk in disk order, not the first of the archive.
#[test]
fn zip_of_disks_is_one_multi_disk_game() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    // Real TOSEC names, archive order scrambled.
    let tosec = |n: u8| format!("B.A.T. (1991)(Ubi Soft)(Disk {n} of 4).dsk");
    zip_of(
        &root.join("bat.zip"),
        &[
            (&tosec(3), &blob(3)),
            (&tosec(1), &blob(1)),
            (&tosec(4), &blob(4)),
            (&tosec(2), &blob(2)),
        ],
    );
    zip_of(
        &root.join("sides.zip"),
        &[("Q (Side B).dsk", &blob(6)), ("Q (Side A).dsk", &blob(5))],
    );
    zip_of(
        &root.join("plain.zip"),
        &[("zeta.dsk", &blob(7)), ("Alpha.dsk", &blob(8))],
    );
    zip_of(
        &root.join("st.zip"),
        &[("G (Disk 2).st", &blob(9)), ("G (Disk 1).st", &blob(10))],
    );
    zip_of(
        &root.join("amiga.zip"),
        &[
            ("H (Disk 2 of 2).adf", &blob(11)),
            ("H (Disk 1 of 2).adf", &blob(12)),
        ],
    );
    zip_of(&root.join("one.zip"), &[("Only.dsk", &blob(13))]);
    // Bare TOSEC files group too.
    for n in 1..=4u8 {
        write(&root.join(format!("bare/{}", tosec(n))), &blob(20 + n));
    }
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    let games = lib.games().unwrap();
    for (file, system, first) in [
        ("bat.zip", "cpc", tosec(1)),
        ("sides.zip", "cpc", "Q (Side A).dsk".to_owned()),
        ("plain.zip", "cpc", "Alpha.dsk".to_owned()),
        ("st.zip", "atarist", "G (Disk 1).st".to_owned()),
        ("amiga.zip", "amiga", "H (Disk 1 of 2).adf".to_owned()),
    ] {
        let g = game_at(&games, file);
        assert_eq!(g.system, system, "{file}");
        assert_eq!(g.entry.as_deref(), Some(first.as_str()), "{file}");
        assert_eq!(g.media, std::slice::from_ref(&g.path), "{file}");
    }
    let one = game_at(&games, "one.zip");
    assert_eq!(one.entry.as_deref(), Some("Only.dsk"));
    assert!(one.media.is_empty());
    let bare: Vec<_> = games
        .iter()
        .filter(|g| g.path.starts_with(root.join("bare")))
        .collect();
    assert_eq!(bare.len(), 1);
    assert_eq!(bare[0].media.len(), 4);
    assert!(bare[0].media[0].to_string_lossy().contains("Disk 1 of 4"));
    // Unchanged files are not analysed again.
    let again = scan(&mut lib, &root);
    assert_eq!(again.unchanged, 7, "{again:?}");
    assert_eq!(lib.games().unwrap(), games);
}
