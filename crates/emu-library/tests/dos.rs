//! DOS library rules (design v4.0 §3.4, §10 #7 / #8): game folders, loose executables, DOS zips, disc images
//! shared with the Mega CD, `.m3u8`, DAT identification. Files from `tests/tools/mkdos.py` and hand-made bytes.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;

use emu_library::{
    Dat, DatEntry, Game, Identification, Library, ScanOptions, ScanReport, hash_reader,
};

fn mkdos(out: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/tools/mkdos.py");
    let status = Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(out)
        .output()
        .expect("python3 is required for the DOS fixtures");
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
}

fn scan(lib: &mut Library, root: &Path) -> ScanReport {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            fs::copy(e.path(), target).unwrap();
        }
    }
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

/// `(system, path relative to root, is_dir)` of every game.
fn listed(lib: &Library, root: &Path) -> BTreeSet<(String, String, bool)> {
    lib.games()
        .unwrap()
        .into_iter()
        .map(|g| {
            let rel = g.path.strip_prefix(root).unwrap().to_string_lossy();
            (g.system, rel.replace('\\', "/"), g.is_dir)
        })
        .collect()
}

fn item(system: &str, path: &str, dir: bool) -> (String, String, bool) {
    (system.to_owned(), path.to_owned(), dir)
}

fn by_path<'a>(games: &'a [Game], root: &Path, rel: &str) -> &'a Game {
    games
        .iter()
        .find(|g| g.path == root.join(rel))
        .unwrap_or_else(|| panic!("{rel} not listed"))
}

fn crc_of(data: &[u8]) -> u32 {
    hash_reader(data, data.len() as u64, None, u64::MAX)
        .unwrap()
        .full
        .crc32
}

#[test]
fn game_folders_and_loose_executables() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    mkdos(&gen_dir);
    let root = tmp.path().join("roms");
    let dos = root.join("dos");
    for name in ["hello", "multi", "nested", "lowercase", "empty"] {
        copy_dir(&gen_dir.join(name), &dos.join(name));
    }
    fs::create_dir_all(dos.join("empty")).unwrap();
    // Loose programs in the root are not games; elsewhere they are not ours (not even reported).
    write(&dos.join("LOOSE.EXE"), b"MZ-loose");
    write(&dos.join("TOOL.COM"), b"tool");
    write(&root.join("misc/stray.exe"), b"MZ-stray");

    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.ignored_loose_executables, 2, "{report:?}");
    assert_eq!(report.unclassified, 0, "{report:?}");
    assert_eq!(
        listed(&lib, &root),
        BTreeSet::from([
            item("dos", "dos/hello", true),
            item("dos", "dos/multi", true),
            item("dos", "dos/lowercase", true),
            // `nested/` and `nested/GAMES/` have no executable at their root: walked, not games.
            item("dos", "dos/nested/GAMES/DEEP", true),
        ])
    );
    let games = lib.games().unwrap();
    let multi = by_path(&games, &root, "dos/multi");
    assert_eq!(
        (multi.title.as_str(), multi.identification),
        ("multi", Identification::Unknown)
    );
    assert!(multi.entry.is_none() && multi.sha1.is_none() && multi.media.is_empty());
    // The files of a game folder are not listed on their own.
    assert!(games.iter().all(|g| !g.path.ends_with("GAME.EXE")));

    let again = scan(&mut lib, &root);
    assert_eq!(
        (again.added, again.updated, again.removed, again.unchanged),
        (0, 0, 0, 4)
    );
    assert_eq!(again.ignored_loose_executables, 2);
    // A changed file in the root of a game folder is noticed.
    write(&dos.join("multi/README.TXT"), b"changed and longer\r\n");
    let third = scan(&mut lib, &root);
    assert_eq!((third.updated, third.unchanged), (1, 3), "{third:?}");
    // Removing the folder removes the game.
    fs::remove_dir_all(dos.join("hello")).unwrap();
    let fourth = scan(&mut lib, &root);
    assert_eq!(fourth.removed, 1);
}

#[test]
fn a_scan_rooted_at_the_dos_folder_works_the_same() {
    let tmp = tempfile::tempdir().unwrap();
    let dos = tmp.path().join("dos");
    write(&dos.join("Game One/GAME.EXE"), b"MZ1");
    write(&dos.join("LOOSE.EXE"), b"MZ2");
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &dos);
    assert_eq!(report.ignored_loose_executables, 1);
    assert_eq!(
        listed(&lib, &dos),
        BTreeSet::from([item("dos", "Game One", true)])
    );
    // A folder name with a dot keeps it whole.
    write(&dos.join("v1.5 Deluxe/RUN.BAT"), b"echo");
    scan(&mut lib, &dos);
    let titles: Vec<String> = lib.games().unwrap().into_iter().map(|g| g.title).collect();
    assert!(titles.contains(&"v1.5 Deluxe".to_owned()), "{titles:?}");
}

#[test]
fn dat_identification_of_folders_and_zips_and_reidentification_after_import() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let dos = root.join("dos");
    let game = b"MZ-game-exe-bytes".as_slice();
    let data = b"level data".as_slice();
    let tool = b"MZ-shared-install".as_slice();
    write(&dos.join("Alpha/GAME.EXE"), game);
    write(&dos.join("Alpha/INSTALL.EXE"), tool);
    write(&dos.join("Beta/INSTALL.EXE"), tool);
    write(&dos.join("Gamma/other.exe"), b"MZ-unknown");
    zip_of(
        &dos.join("alpha-zipped.zip"),
        &[("game.exe", game), ("level.dat", data)],
    );
    let line = |g: &str, rom: &str, bytes: &[u8]| DatEntry {
        game: g.to_owned(),
        rom_name: rom.to_owned(),
        size: bytes.len() as u64,
        crc32: crc_of(bytes),
        sha1: None,
    };
    let dat = Dat {
        name: "DOS".into(),
        version: None,
        entries: vec![
            line("Alpha (1992)", "GAME.EXE", game),
            line("Alpha (1992)", "LEVEL.DAT", data),
            line("Alpha (1992)", "INSTALL.EXE", tool),
            line("Beta (1993)", "INSTALL.EXE", tool),
        ],
    };
    let mut lib = Library::open_in_memory().unwrap();
    // Scanned before the DAT exists: names only.
    scan(&mut lib, &root);
    assert!(
        lib.games()
            .unwrap()
            .iter()
            .all(|g| g.identification == Identification::Unknown)
    );
    // Importing the DAT makes the next scan analyse the DOS games again (once).
    lib.import_dat("dos", &dat, None).unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.updated, 4, "{report:?}");
    let games = lib.games().unwrap();
    let alpha = by_path(&games, &root, "dos/Alpha");
    assert_eq!(
        (alpha.title.as_str(), alpha.identification),
        ("Alpha (1992)", Identification::Crc32Size)
    );
    let zipped = by_path(&games, &root, "dos/alpha-zipped.zip");
    assert_eq!(
        (zipped.title.as_str(), zipped.identification),
        ("Alpha (1992)", Identification::Crc32Size)
    );
    // Only the shared installer matches: two games tie, the folder name is kept (it is not a DAT name either).
    let beta = by_path(&games, &root, "dos/Beta");
    assert_eq!(
        (beta.title.as_str(), beta.identification),
        ("Beta", Identification::Unknown)
    );
    let gamma = by_path(&games, &root, "dos/Gamma");
    assert_eq!(gamma.identification, Identification::Unknown);
    let third = scan(&mut lib, &root);
    assert_eq!((third.updated, third.unchanged), (0, 4), "{third:?}");
}

#[test]
fn dos_zips_and_discs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let iso = vec![0x20u8; 4096];
    // Under `dos/`: always a DOS game, a CD inside is fine; nothing is read (no hash).
    zip_of(
        &root.join("dos/withcd.zip"),
        &[("GAME.EXE", b"MZ"), ("game.iso", &iso)],
    );
    zip_of(&root.join("dos/onlyiso.zip"), &[("disc.iso", &iso)]);
    // Elsewhere: a program inside makes it a DOS game, otherwise the 2.1 refusal stays.
    zip_of(
        &root.join("games/exe-and-cd.zip"),
        &[("RUN.EXE", b"MZ"), ("d.cue", b"FILE")],
    );
    zip_of(&root.join("games/plain-cd.zip"), &[("d.iso", &iso)]);
    zip_of(&root.join("games/text.zip"), &[("readme.txt", b"hi")]);
    write(
        &root.join("games/pack.dosz"),
        b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    );
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    let games = lib.games().unwrap();
    assert_eq!(
        listed(&lib, &root),
        BTreeSet::from([
            item("dos", "dos/withcd.zip", false),
            item("dos", "dos/onlyiso.zip", false),
            item("dos", "games/exe-and-cd.zip", false),
        ]),
        "{report:?}"
    );
    for g in &games {
        assert!(
            g.entry.is_none() && g.sha1.is_none() && g.crc32.is_none(),
            "{:?}",
            g.path
        );
    }
    assert_eq!(by_path(&games, &root, "dos/withcd.zip").title, "withcd");
    let errors: Vec<String> = report
        .errors
        .iter()
        .map(|(p, m)| format!("{}: {m}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with("plain-cd.zip: Les CD ne se lisent pas")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.starts_with("text.zip")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.starts_with("pack.dosz")),
        "{errors:?}"
    );
    let again = scan(&mut lib, &root);
    assert_eq!(
        (again.added, again.updated, again.removed, again.unchanged),
        (0, 0, 0, 3)
    );
}

#[test]
fn cds_under_dos_are_classified_by_folder_and_their_tracks_are_hidden() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let dos = root.join("dos");
    // A PC CD: cue + data bin + audio img, in the root of `dos/` (not a game folder: no executable).
    write(&dos.join("PC Game.cue"), b"FILE \"PC Game.bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\nFILE \"track02.img\" BINARY\r\n  TRACK 02 AUDIO\r\n");
    write(&dos.join("PC Game.bin"), &vec![1u8; 4096]);
    write(&dos.join("track02.img"), &vec![2u8; 4096]);
    write(&dos.join("Setup.iso"), &vec![3u8; 4096]);
    write(&dos.join("Big.chd"), b"MComprHD");
    write(&dos.join("Install.ins"), b"x");
    write(&dos.join("disk.vhd"), &vec![4u8; 1024]);
    // Outside any system folder: nothing to probe, tracks are still hidden, the disc is unclassified.
    write(&root.join("other/Music.cue"), b"FILE \"m.bin\" BINARY\r\n");
    write(&root.join("other/m.bin"), &vec![5u8; 4096]);
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.hidden_tracks, 3, "{report:?}");
    assert_eq!(report.unclassified, 1, "{report:?}");
    assert!(report.unclassified_paths[0].ends_with("Music.cue"));
    assert_eq!(
        listed(&lib, &root),
        BTreeSet::from([
            item("dos", "dos/PC Game.cue", false),
            item("dos", "dos/Setup.iso", false),
            item("dos", "dos/Big.chd", false),
            item("dos", "dos/Install.ins", false),
            item("dos", "dos/disk.vhd", false),
        ])
    );
    for g in lib.games().unwrap() {
        assert!(
            g.sha1.is_none() && g.crc32.is_none(),
            "{:?} was hashed",
            g.path
        );
    }
    let again = scan(&mut lib, &root);
    assert_eq!(
        (again.added, again.updated, again.unchanged),
        (0, 0, 5),
        "{again:?}"
    );
    assert_eq!(again.hidden_tracks, 3);
}

#[test]
fn small_images_are_matched_on_crc_and_playlists_group_them() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let d1 = vec![7u8; 2048];
    let d2 = vec![8u8; 2048];
    write(&root.join("dos/Quest (Disk 1 of 2).ima"), &d1);
    write(&root.join("dos/Quest (Disk 2 of 2).ima"), &d2);
    write(&root.join("dos/Solo.ima"), &d1);
    // A playlist of any spelling: the entries are hidden, the playlist is the game.
    write(&root.join("dos/Saga.m3u8"), b"Saga 1.img\nSaga 2.img\n");
    write(&root.join("dos/Saga 1.img"), &d1);
    write(&root.join("dos/Saga 2.img"), &d2);
    let dat = Dat {
        name: "DOS".into(),
        version: None,
        entries: vec![DatEntry {
            game: "Solo Game (1991)".into(),
            rom_name: "solo.ima".into(),
            size: d1.len() as u64,
            crc32: crc_of(&d1),
            sha1: None,
        }],
    };
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat("dos", &dat, None).unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    let games = lib.games().unwrap();
    assert_eq!(
        listed(&lib, &root),
        BTreeSet::from([
            item("dos", "dos/Quest (Disk 1 of 2).ima", false),
            item("dos", "dos/Solo.ima", false),
            item("dos", "dos/Saga.m3u8", false),
        ])
    );
    assert_eq!(
        by_path(&games, &root, "dos/Quest (Disk 1 of 2).ima")
            .media
            .len(),
        2
    );
    assert_eq!(by_path(&games, &root, "dos/Saga.m3u8").media.len(), 2);
    let solo = by_path(&games, &root, "dos/Solo.ima");
    assert_eq!(
        (solo.title.as_str(), solo.identification),
        ("Solo Game (1991)", Identification::Crc32Size)
    );
    let again = scan(&mut lib, &root);
    assert_eq!(
        (again.added, again.updated, again.unchanged),
        (0, 0, 3),
        "{again:?}"
    );
}

#[test]
fn a_playlist_of_a_dos_image_outside_the_folder_is_still_dos() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("pc/a.ima"), &vec![1u8; 512]);
    write(&root.join("pc/list.m3u8"), b"a.ima\n");
    // `.m3u` alone is shared by three computers: classified by its entries, here a DOS image.
    write(&root.join("pc/old.m3u"), b"a.ima\n");
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let systems: BTreeSet<String> = lib.games().unwrap().into_iter().map(|g| g.system).collect();
    assert_eq!(systems, BTreeSet::from(["dos".to_owned()]));
}
