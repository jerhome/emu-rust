//! Amiga in the library (design v3.0 §3.4, §3.5.2): shared and folder-only extensions, `.m3u` system, two DAT
//! sources. Generated fixtures only, no ROM, no network.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use emu_library::{
    Dat, DatEntry, DatSource, Game, Identification, Library, LibraryError, MAX_UNCLASSIFIED_PATHS,
    ScanOptions, ScanReport, hash_reader,
};

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn disk(seed: u8) -> Vec<u8> {
    (0..1024usize)
        .map(|i| (i as u8).wrapping_mul(13) ^ seed)
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

fn file_names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn ipf_outside_folders_needs_a_dat() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("loose.ipf"), &disk(1));
    write(&root.join("st-dat.ipf"), &disk(2));
    write(&root.join("amiga-dat.ipf"), &disk(3));
    write(&root.join("amiga/in-folder.ipf"), &disk(4));
    write(&root.join("atarist/in-folder-st.ipf"), &disk(5));
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat(
        "atarist",
        &dat(vec![entry("ST Game", "x.ipf", &disk(2))]),
        None,
    )
    .unwrap();
    lib.import_dat(
        "amiga",
        &dat(vec![entry("Amiga Game", "y.ipf", &disk(3))]),
        None,
    )
    .unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.unclassified, 1);
    assert_eq!(file_names(&report.unclassified_paths), ["loose.ipf"]);
    assert_eq!(system_of(&lib, "loose.ipf"), None);
    assert_eq!(system_of(&lib, "st-dat.ipf").as_deref(), Some("atarist"));
    assert_eq!(system_of(&lib, "amiga-dat.ipf").as_deref(), Some("amiga"));
    assert_eq!(system_of(&lib, "in-folder.ipf").as_deref(), Some("amiga"));
    assert_eq!(
        system_of(&lib, "in-folder-st.ipf").as_deref(),
        Some("atarist")
    );
    let games = lib.games().unwrap();
    assert_eq!(find(&games, "amiga-dat.ipf").unwrap().title, "Amiga Game");

    // Rescan: the unclassified file is reported again, the others are unchanged.
    let again = scan(&mut lib, &root);
    assert_eq!(
        (again.unclassified, again.unchanged, again.added),
        (1, 4, 0)
    );

    // A DAT entry appearing later classifies it at the next scan.
    lib.import_dat("amiga", &dat(vec![entry("Late", "z.ipf", &disk(1))]), None)
        .unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.unclassified, 1);
    assert_eq!(system_of(&lib, "loose.ipf").as_deref(), Some("amiga"));
    // The previous Amiga entry is gone (same source replaced): that file is re-evaluated, now unclassified.
    assert_eq!(system_of(&lib, "amiga-dat.ipf"), None);
    assert_eq!(file_names(&report.unclassified_paths), ["amiga-dat.ipf"]);
}

#[test]
fn an_ipf_in_both_dats_is_unclassified() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("both.ipf"), &disk(1));
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat("atarist", &dat(vec![entry("A", "a.ipf", &disk(1))]), None)
        .unwrap();
    lib.import_dat("amiga", &dat(vec![entry("B", "b.ipf", &disk(1))]), None)
        .unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.unclassified, 1);
    assert!(lib.games().unwrap().is_empty());
}

#[test]
fn a_stored_ipf_leaving_the_dat_is_reevaluated() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("game.ipf"), &disk(1));
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat("atarist", &dat(vec![entry("A", "a.ipf", &disk(1))]), None)
        .unwrap();
    scan(&mut lib, &root);
    let g = lib.games().unwrap()[0].clone();
    assert_eq!(
        (g.system.as_str(), g.identification),
        ("atarist", Identification::Sha1)
    );
    lib.set_favorite(g.id, true).unwrap();
    // DAT import: the row keeps its system as "unknown" until the next scan.
    lib.import_dat("atarist", &dat(vec![]), None).unwrap();
    let g = lib.games().unwrap()[0].clone();
    assert_eq!(
        (g.system.as_str(), g.identification),
        ("atarist", Identification::Unknown)
    );
    let report = scan(&mut lib, &root);
    assert_eq!(report.unclassified, 1);
    assert!(lib.games().unwrap().is_empty());
}

#[test]
fn folder_only_extensions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("Game.lha"), &disk(1));
    write(&root.join("disk.hdf"), &disk(2));
    write(&root.join("amiga/whdload/Game.lha"), &disk(3));
    write(&root.join("amiga/system.hdf"), &disk(4));
    write(&root.join("atarist/other.hdz"), &disk(5));
    write(&root.join("Plain.adf"), &disk(6));
    write(&root.join("Plain.st"), &disk(7));
    write(&root.join("Plain.gb"), &disk(8));
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(
        file_names(&report.unclassified_paths),
        ["Game.lha", "disk.hdf", "other.hdz"]
    );
    let games = lib.games().unwrap();
    let mut got: Vec<(String, String)> = games
        .iter()
        .map(|g| {
            let rel = g
                .path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (rel.replace('\\', "/"), g.system.clone())
        })
        .collect();
    got.sort();
    let want: Vec<(String, String)> = [
        ("Plain.adf", "amiga"),
        ("Plain.gb", "gb"),
        ("Plain.st", "atarist"),
        ("amiga/system.hdf", "amiga"),
        ("amiga/whdload/Game.lha", "amiga"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(got, want);
    let lha = find(&games, "Game.lha").unwrap();
    assert!(lha.sha1.is_some());
}

#[test]
fn m3u_takes_the_system_of_its_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("a/d1.adf"), &disk(1));
    write(&root.join("a/d2.adf"), &disk(2));
    write(&root.join("a/Amiga Saga.m3u"), b"d1.adf\nd2.adf\n");
    write(&root.join("s/d1.st"), &disk(3));
    write(&root.join("s/ST Saga.m3u"), b"d1.st\n");
    write(&root.join("i/x.ipf"), &disk(4));
    write(&root.join("i/Loose.m3u"), b"x.ipf\n");
    write(&root.join("m/x.adf"), &disk(5));
    write(&root.join("m/y.st"), &disk(6));
    write(&root.join("m/Mixed.m3u"), b"x.adf\ny.st\n");
    write(&root.join("atarist/x.adf"), &disk(7));
    write(&root.join("atarist/y.st"), &disk(8));
    write(&root.join("atarist/Mixed in ST.m3u"), b"x.adf\ny.st\n");
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(
        file_names(&report.unclassified_paths),
        ["Loose.m3u", "Mixed.m3u"]
    );
    let games = lib.games().unwrap();
    let saga = find(&games, "Amiga Saga.m3u").unwrap();
    assert_eq!(saga.system, "amiga");
    assert_eq!(file_names(&saga.media), ["d1.adf", "d2.adf"]);
    assert_eq!(find(&games, "ST Saga.m3u").unwrap().system, "atarist");
    assert_eq!(find(&games, "Mixed in ST.m3u").unwrap().system, "atarist");
    // Entries of an unclassified playlist stay hidden behind it.
    assert!(find(&games, "x.ipf").is_none() && find(&games, "x.adf").is_none());
    assert_eq!(games.len(), 3, "{games:?}");

    // With a DAT entry, the `.ipf` playlist is classified at the next scan; the others are unchanged.
    lib.import_dat("amiga", &dat(vec![entry("X", "x.ipf", &disk(4))]), None)
        .unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.unclassified, 1);
    assert_eq!(system_of(&lib, "Loose.m3u").as_deref(), Some("amiga"));
    assert!(report.unchanged >= 3, "{report:?}");
}

#[test]
fn two_dat_sources_of_one_system() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("amiga/both.adf"), &disk(1));
    write(&root.join("amiga/whd.lha"), &disk(2));
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    lib.import_dat_source(
        "amiga",
        DatSource::Libretro,
        &dat(vec![
            entry("WHD Name", "both.adf", &disk(1)),
            entry("Game (WHDLoad)", "whd.lha", &disk(2)),
        ]),
        Some("\"w\""),
    )
    .unwrap();
    assert_eq!(system_of(&lib, "both.adf").as_deref(), Some("amiga"));
    let title = |lib: &Library, n: &str| find(&lib.games().unwrap(), n).unwrap().title.clone();
    assert_eq!(title(&lib, "both.adf"), "WHD Name");
    // TOSEC comes first in `dat`: it wins once imported, and does not erase the WHDLoad entries.
    lib.import_dat(
        "amiga",
        &dat(vec![entry("TOSEC Name", "both.adf", &disk(1))]),
        None,
    )
    .unwrap();
    assert_eq!(title(&lib, "both.adf"), "TOSEC Name");
    assert_eq!(title(&lib, "whd.lha"), "Game (WHDLoad)");
    let infos = lib.dat_infos("amiga").unwrap();
    assert_eq!(
        infos
            .iter()
            .map(|d| (d.source, d.entries))
            .collect::<Vec<_>>(),
        [(DatSource::Tosec, 1), (DatSource::Libretro, 2)]
    );
    assert_eq!(
        lib.dat_info("amiga").unwrap().unwrap().source,
        DatSource::Tosec
    );
    assert_eq!(
        lib.dat_info_for("amiga", DatSource::Libretro)
            .unwrap()
            .unwrap()
            .etag
            .as_deref(),
        Some("\"w\"")
    );
    assert!(
        lib.dat_info_for("amiga", DatSource::NoIntro)
            .unwrap()
            .is_none()
    );
    // Replacing one source only.
    lib.import_dat_source("amiga", DatSource::Libretro, &dat(vec![]), None)
        .unwrap();
    assert_eq!(title(&lib, "both.adf"), "TOSEC Name");
    assert_eq!(title(&lib, "whd.lha"), "whd");
    assert!(matches!(
        lib.import_dat_source("amiga", DatSource::NoIntro, &dat(vec![]), None),
        Err(LibraryError::UnknownSystem(_))
    ));
    assert!(matches!(
        lib.import_dat_source("gb", DatSource::Tosec, &dat(vec![]), None),
        Err(LibraryError::UnknownSystem(_))
    ));
    assert!(lib.dat_info("psx").unwrap().is_none());
    assert!(lib.dat_infos("psx").unwrap().is_empty());
}

#[test]
fn unclassified_paths_are_bounded_and_in_scan_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let n = MAX_UNCLASSIFIED_PATHS + 5;
    for i in 0..n {
        write(&root.join(format!("f{i:04}.lha")), &[1]);
    }
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.unclassified, n as u64);
    assert_eq!(report.unclassified_paths.len(), MAX_UNCLASSIFIED_PATHS);
    let names = file_names(&report.unclassified_paths);
    assert_eq!(names[0], "f0000.lha");
    assert!(names.windows(2).all(|w| w[0] < w[1]));
}
