//! Scanner on generated fixtures (tempdir): no real ROM, no network.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use emu_library::{
    CoverKind, CoverState, Dat, DatEntry, Identification, Library, LibraryError, ScanOptions,
    ScanReport, hash_reader,
};

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn set_mtime(path: &Path, secs: u64) {
    let f = fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

fn rom(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ seed)
        .collect()
}

fn entry(game: &str, data: &[u8], with_sha1: bool) -> DatEntry {
    let h = hash_reader(data, data.len() as u64, None, u64::MAX)
        .unwrap()
        .full;
    DatEntry {
        game: game.into(),
        rom_name: format!("{game}.bin"),
        size: h.size,
        crc32: h.crc32,
        sha1: with_sha1.then_some(h.sha1),
        serial: None,
        region: None,
    }
}

fn dat(entries: Vec<DatEntry>) -> Dat {
    Dat {
        name: "test".into(),
        version: Some("1".into()),
        entries,
    }
}

/// Stored-only zip with full control over names and declared sizes (malicious archives).
fn build_zip(entries: &[(&[u8], &[u8], Option<u32>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, declared) in entries {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let size = declared.unwrap_or(data.len() as u32);
        let mut common = Vec::new();
        common.extend_from_slice(&20u16.to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());
        common.extend_from_slice(&0u32.to_le_bytes());
        common.extend_from_slice(&crc.to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&(name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&common);
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&common);
        central.extend_from_slice(&[0u8; 10]); // comment len, disk, attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn options(roots: &[&Path]) -> ScanOptions {
    ScanOptions {
        roots: roots.iter().map(|p| p.to_path_buf()).collect(),
        ..Default::default()
    }
}

fn scan(lib: &mut Library, opts: &ScanOptions) -> ScanReport {
    lib.scan(opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn by_file(lib: &Library, name: &str) -> emu_library::Game {
    lib.games()
        .unwrap()
        .into_iter()
        .find(|g| g.path.file_name().unwrap() == name)
        .unwrap_or_else(|| panic!("{name} not in library"))
}

#[test]
fn identifies_by_sha1_crc_headerless_and_name() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let gb_a = rom(1, 4096);
    let gb_b = rom(2, 4096);
    write(&roms.join("gb").join("a.gb"), &gb_a);
    write(&roms.join("gb").join("b.gb"), &gb_b);
    write(&roms.join("gb").join("Gamma Land (World).gb"), &rom(3, 100));
    write(&roms.join("gb").join("unknown.gb"), &rom(4, 100));
    let nes_body = rom(5, 8192);
    let mut nes = b"NES\x1A".to_vec();
    nes.extend_from_slice(&[0u8; 12]);
    nes.extend_from_slice(&nes_body);
    write(&roms.join("nes").join("c.nes"), &nes);
    let snes_body = rom(6, 2048);
    let mut snes = vec![0u8; 512];
    snes.extend_from_slice(&snes_body);
    write(&roms.join("d.smc"), &snes);

    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat(
        "gb",
        &dat(vec![
            entry("Alpha (World)", &gb_a, true),
            entry("Beta (Europe)", &gb_b, false),
            DatEntry {
                game: "Gamma Land (World)".into(),
                rom_name: "x".into(),
                size: 1,
                crc32: 1,
                sha1: None,
                serial: None,
                region: None,
            },
        ]),
        Some("\"etag\""),
    )
    .unwrap();
    lib.import_dat(
        "nes",
        &dat(vec![entry("Charlie (USA)", &nes_body, true)]),
        None,
    )
    .unwrap();
    lib.import_dat(
        "snes",
        &dat(vec![entry("Delta (Japan)", &snes_body, false)]),
        None,
    )
    .unwrap();

    let mut calls = 0;
    let report = lib
        .scan(&options(&[&roms]), &AtomicBool::new(false), &mut |_| {
            calls += 1
        })
        .unwrap();
    assert!(calls >= 1);
    assert_eq!(report.added, 6, "{report:?}");
    assert!(report.errors.is_empty(), "{report:?}");

    let a = by_file(&lib, "a.gb");
    assert_eq!(
        (a.title.as_str(), a.identification),
        ("Alpha (World)", Identification::Sha1)
    );
    assert_eq!(a.system, "gb");
    assert_eq!(a.size, 4096);
    let b = by_file(&lib, "b.gb");
    assert_eq!(
        (b.title.as_str(), b.identification),
        ("Beta (Europe)", Identification::Crc32Size)
    );
    let g = by_file(&lib, "Gamma Land (World).gb");
    assert_eq!(g.identification, Identification::Name);
    let u = by_file(&lib, "unknown.gb");
    assert_eq!(
        (u.title.as_str(), u.identification),
        ("unknown", Identification::Unknown)
    );
    let c = by_file(&lib, "c.nes");
    assert_eq!(
        (c.title.as_str(), c.identification),
        ("Charlie (USA)", Identification::Sha1)
    );
    let d = by_file(&lib, "d.smc");
    assert_eq!(
        (d.system.as_str(), d.identification),
        ("snes", Identification::Crc32Size)
    );
    assert_eq!(
        lib.dat_info("gb").unwrap().unwrap().etag.as_deref(),
        Some("\"etag\"")
    );
    assert_eq!(lib.dat_info("gb").unwrap().unwrap().entries, 3);
    assert!(lib.dat_info("megadrive").unwrap().is_none());
}

#[test]
fn incremental_rescan_and_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let a = roms.join("a.md");
    let b = roms.join("b.gg");
    write(&a, &rom(1, 512));
    write(&b, &rom(2, 512));
    set_mtime(&a, 1_000_000);
    set_mtime(&b, 1_000_000);
    let mut lib = Library::open_in_memory().unwrap();
    let opts = options(&[&roms]);
    assert_eq!(scan(&mut lib, &opts).added, 2);
    let before = by_file(&lib, "a.md");
    lib.set_favorite(before.id, true).unwrap();

    let r = scan(&mut lib, &opts);
    assert_eq!((r.unchanged, r.added, r.updated, r.removed), (2, 0, 0, 0));

    // Same size, new content and mtime: hashed again, favorite kept.
    write(&a, &rom(9, 512));
    set_mtime(&a, 2_000_000);
    let r = scan(&mut lib, &opts);
    assert_eq!((r.unchanged, r.updated), (1, 1));
    let after = by_file(&lib, "a.md");
    assert_eq!(after.id, before.id);
    assert!(after.favorite);
    assert_ne!(after.sha1, before.sha1);

    fs::remove_file(&b).unwrap();
    let r = scan(&mut lib, &opts);
    assert_eq!(r.removed, 1);
    assert_eq!(lib.games().unwrap().len(), 1);
}

#[test]
fn missing_root_keeps_its_games() {
    let tmp = tempfile::tempdir().unwrap();
    let usb = tmp.path().join("usb");
    let local = tmp.path().join("local");
    write(&usb.join("x.sms"), &rom(1, 64));
    write(&local.join("y.sms"), &rom(2, 64));
    let mut lib = Library::open_in_memory().unwrap();
    let opts = options(&[&usb, &local]);
    assert_eq!(scan(&mut lib, &opts).added, 2);

    fs::rename(&usb, tmp.path().join("unplugged")).unwrap();
    let r = scan(&mut lib, &opts);
    assert_eq!(r.missing_roots, vec![usb.clone()]);
    assert_eq!(r.removed, 0);
    assert_eq!(lib.games().unwrap().len(), 2);
}

#[test]
fn folder_and_dat_choose_between_gb_and_gbc() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let color = rom(7, 256);
    write(&roms.join("gbc").join("in-gbc-folder.gb"), &rom(1, 256));
    write(&roms.join("gb").join("plain.gb"), &rom(2, 256));
    write(&roms.join("misc").join("loose.gb"), &rom(3, 256));
    write(&roms.join("misc").join("color-by-dat.gb"), &color);
    write(&roms.join("gb").join("named.gbc"), &rom(4, 256));
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat(
        "gbc",
        &dat(vec![entry("Color Game (World)", &color, true)]),
        None,
    )
    .unwrap();
    scan(&mut lib, &options(&[&roms]));
    assert_eq!(by_file(&lib, "in-gbc-folder.gb").system, "gbc");
    assert_eq!(by_file(&lib, "plain.gb").system, "gb");
    assert_eq!(by_file(&lib, "loose.gb").system, "gb");
    assert_eq!(by_file(&lib, "color-by-dat.gb").system, "gbc");
    // `gb` does not accept `.gbc`: the folder cannot pin it there.
    assert_eq!(by_file(&lib, "named.gbc").system, "gbc");
}

#[test]
fn dat_import_after_scan_reidentifies() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let data = rom(1, 300);
    write(&roms.join("misc").join("late.gb"), &data);
    write(&roms.join("gb").join("pinned.gb"), &data);
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &options(&[&roms]));
    assert_eq!(
        by_file(&lib, "late.gb").identification,
        Identification::Unknown
    );
    lib.import_dat("gbc", &dat(vec![entry("Late (World)", &data, true)]), None)
        .unwrap();
    let late = by_file(&lib, "late.gb");
    assert_eq!(
        (late.system.as_str(), late.title.as_str()),
        ("gbc", "Late (World)")
    );
    // Pinned by its `gb` folder: stays a Game Boy game.
    assert_eq!(by_file(&lib, "pinned.gb").system, "gb");
    // A smaller DAT replaces the previous one.
    assert_eq!(lib.import_dat("gbc", &dat(vec![]), None).unwrap(), 0);
    assert_eq!(
        by_file(&lib, "late.gb").identification,
        Identification::Unknown
    );
    assert!(matches!(
        lib.import_dat("ps2", &dat(vec![]), None),
        Err(LibraryError::UnknownSystem(_))
    ));
}

#[test]
fn zips_are_hashed_on_their_first_accepted_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let data = rom(3, 1000);
    write(
        &roms.join("good.zip"),
        &build_zip(&[
            (b"readme.txt", b"hello", None),
            (b"dir/Game.GB", &data, None),
        ]),
    );
    // Declares 10 bytes, holds 1000: never read past the declared size.
    write(
        &roms.join("bomb.zip"),
        &build_zip(&[(b"bomb.gb", &rom(4, 1000), Some(10))]),
    );
    write(
        &roms.join("slip.zip"),
        &build_zip(&[(b"../../escape.gb", &data, None)]),
    );
    write(
        &roms.join("nothing.zip"),
        &build_zip(&[(b"a.txt", b"x", None)]),
    );
    write(&roms.join("garbage.zip"), b"PK not really a zip");
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat("gb", &dat(vec![entry("Zipped (World)", &data, true)]), None)
        .unwrap();
    let r = scan(&mut lib, &options(&[&roms]));
    assert_eq!(r.added, 2, "{r:?}");
    let failed: Vec<_> = r
        .errors
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(failed, ["bomb.zip", "garbage.zip", "nothing.zip"]);

    let good = by_file(&lib, "good.zip");
    assert_eq!(good.entry.as_deref(), Some("Game.GB"));
    assert_eq!((good.title.as_str(), good.size), ("Zipped (World)", 1000));
    let slip = by_file(&lib, "slip.zip");
    assert_eq!(slip.entry.as_deref(), Some("escape.gb"));
    assert_eq!(slip.identification, Identification::Sha1);
    // Nothing was extracted anywhere.
    assert!(!tmp.path().join("escape.gb").exists());
    assert!(!roms.join("escape.gb").exists());
}

#[test]
fn large_files_are_listed_without_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    write(&roms.join("big.md"), &rom(1, 2000));
    write(
        &roms.join("big.zip"),
        &build_zip(&[(b"big.md", &rom(1, 2000), None)]),
    );
    let mut lib = Library::open_in_memory().unwrap();
    let opts = ScanOptions {
        max_file_size: 1000,
        ..options(&[&roms])
    };
    let r = scan(&mut lib, &opts);
    assert_eq!(r.added, 2, "{r:?}");
    for g in lib.games().unwrap() {
        assert_eq!((g.crc32, g.sha1, g.size), (None, None, 2000));
        assert_eq!(g.identification, Identification::Unknown);
    }
}

#[test]
fn depth_limit_and_ignored_files() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let mut deep = roms.clone();
    for i in 0..3 {
        deep.push(format!("d{i}"));
    }
    write(&deep.join("deep.gg"), b"x");
    write(&roms.join("d0").join("shallow.gg"), b"x");
    write(&roms.join("notes.txt"), b"x");
    write(&roms.join("noext"), b"x");
    let mut lib = Library::open_in_memory().unwrap();
    let opts = ScanOptions {
        max_depth: 2,
        ..options(&[&roms])
    };
    assert_eq!(scan(&mut lib, &opts).added, 1);
    let opts = ScanOptions {
        max_depth: 3,
        ..options(&[&roms])
    };
    assert_eq!(scan(&mut lib, &opts).added, 1);
    assert_eq!(lib.games().unwrap().len(), 2);
}

#[test]
fn cancellation_keeps_finished_work() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    for i in 0..300 {
        write(&roms.join(format!("g{i:03}.gg")), &rom(i as u8, 32));
    }
    let mut lib = Library::open_in_memory().unwrap();
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        lib.scan(&options(&[&roms]), &cancel, &mut |_| {}),
        Err(LibraryError::Cancelled)
    ));
    assert!(lib.games().unwrap().is_empty());

    let cancel = AtomicBool::new(false);
    let result = lib.scan(&options(&[&roms]), &cancel, &mut |p| {
        if p.files_seen >= 1 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(result, Err(LibraryError::Cancelled)));
    let partial = lib.games().unwrap().len();
    assert!((1..300).contains(&partial), "{partial}");

    let r = scan(&mut lib, &options(&[&roms]));
    assert_eq!(r.unchanged as usize, partial);
    assert_eq!(lib.games().unwrap().len(), 300);
}

/// A file moved by hand, then a scan cancelled after its first batch: the next scan still recognises the move
/// (same id, favourite, history and cover kept).
#[test]
fn moved_by_hand_then_cancelled_scan_keeps_the_game() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    write(&roms.join("a.gg"), &rom(200, 64));
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &options(&[&roms]));
    let before = by_file(&lib, "a.gg");
    lib.set_favorite(before.id, true).unwrap();
    lib.record_played(before.id, 1_000).unwrap();
    let cover = CoverState::Cached {
        path: PathBuf::from("gamegear/a.png"),
        source: "libretro".into(),
    };
    lib.set_cover(before.id, CoverKind::Boxart, &cover).unwrap();

    // The moved file comes first in the walk (directory `aaa` before the files `z*`).
    fs::create_dir_all(roms.join("aaa")).unwrap();
    fs::rename(roms.join("a.gg"), roms.join("aaa").join("a.gg")).unwrap();
    for i in 0..300 {
        write(&roms.join(format!("z{i:03}.gg")), &rom(i as u8, 32));
    }
    let cancel = AtomicBool::new(false);
    let result = lib.scan(&options(&[&roms]), &cancel, &mut |p| {
        if p.files_seen >= 1 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(result, Err(LibraryError::Cancelled)));
    let partial = lib.games().unwrap().len();
    assert!(partial < 301, "{partial}");

    let r = scan(&mut lib, &options(&[&roms]));
    assert_eq!(lib.games().unwrap().len(), 301, "{r:?}");
    let after = by_file(&lib, "a.gg");
    assert!(after.path.ends_with("aaa/a.gg"));
    assert_eq!(after.id, before.id, "{r:?}");
    assert!(after.favorite);
    assert_eq!(after.play_count, 1);
    assert_eq!(
        lib.cover(after.id, CoverKind::Boxart)
            .unwrap()
            .unwrap()
            .state,
        cover
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_unreadable_files() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let outside = tmp.path().join("outside");
    write(&roms.join("ok.sms"), &rom(1, 64));
    write(&outside.join("linked.sms"), &rom(2, 64));
    write(&outside.join("in-linked-dir.sms"), &rom(3, 64));
    symlink(&roms, roms.join("loop")).unwrap();
    symlink(&outside, roms.join("dirlink")).unwrap();
    symlink(outside.join("linked.sms"), roms.join("filelink.sms")).unwrap();
    symlink(tmp.path().join("nowhere.sms"), roms.join("dangling.sms")).unwrap();
    let locked = roms.join("locked.sms");
    write(&locked, &rom(4, 64));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let really_locked = fs::File::open(&locked).is_err();

    let mut lib = Library::open_in_memory().unwrap();
    let r = scan(&mut lib, &options(&[&roms]));
    let mut names: Vec<_> = lib
        .games()
        .unwrap()
        .iter()
        .map(|g| g.path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    if really_locked {
        assert_eq!(names, ["filelink.sms", "ok.sms"]);
        assert_eq!(r.errors.len(), 1, "{r:?}");
        assert_eq!(r.errors[0].0, locked);
    }
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn identical_roots_are_scanned_once() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    write(&roms.join("a.gg"), b"x");
    let mut lib = Library::open_in_memory().unwrap();
    let r = scan(&mut lib, &options(&[&roms, &roms]));
    assert_eq!(r.added, 1);
    let _: PathBuf = by_file(&lib, "a.gg").path;
}

/// The scan holds no write lock while it reads files: another connection (the UI, the cover worker) writes
/// between any two files. The progress callback runs on the scan thread between files, like a slow read would.
#[test]
fn other_connections_write_while_files_are_hashed() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    for i in 0..300u32 {
        write(&roms.join(format!("g{i:03}.gg")), &i.to_le_bytes());
    }
    let db = tmp.path().join("library.sqlite");
    let (mut lib, _) = Library::open(&db).unwrap();
    let ui = rusqlite::Connection::open(&db).unwrap();
    // No waiting: any lock held by the scan fails the write at once.
    ui.busy_timeout(Duration::ZERO).unwrap();
    let mut calls = 0;
    let mut failures = Vec::new();
    lib.scan(&options(&[&roms]), &AtomicBool::new(false), &mut |p| {
        calls += 1;
        if let Err(e) = ui.execute_batch(
            "BEGIN IMMEDIATE; UPDATE game SET favorite = 1 WHERE id = (SELECT min(id) FROM game); COMMIT;",
        ) {
            failures.push((p.files_seen, e.to_string()));
            let _ = ui.execute_batch("ROLLBACK");
        }
        // Slow first files: the next ones report progress too, some of them with results not yet stored.
        if calls <= 3 {
            std::thread::sleep(Duration::from_millis(110));
        }
    })
    .unwrap();
    assert!(calls >= 4, "{calls}");
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(lib.games().unwrap().len(), 300);
}

/// The re-identification after a DAT import applies the scan's folder rule (folders below the parent of the
/// root), not "any folder of the absolute path".
#[test]
fn dat_import_uses_the_folder_rule_of_the_scan() {
    let tmp = tempfile::tempdir().unwrap();
    // `gb` is above the root's parent: the scan ignores it.
    let roms = tmp.path().join("gb").join("media").join("roms");
    let data = rom(9, 300);
    write(&roms.join("color.gb"), &data);
    write(&roms.join("gb").join("pinned.gb"), &data);
    let opts = options(&[&roms]);
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &opts);
    assert_eq!(by_file(&lib, "color.gb").system, "gb");
    lib.import_dat("gbc", &dat(vec![entry("Color (World)", &data, true)]), None)
        .unwrap();
    assert_eq!(by_file(&lib, "color.gb").system, "gbc");
    assert_eq!(by_file(&lib, "pinned.gb").system, "gb");

    // A fresh scan agrees with the re-identification.
    let mut fresh = Library::open_in_memory().unwrap();
    fresh
        .import_dat("gbc", &dat(vec![entry("Color (World)", &data, true)]), None)
        .unwrap();
    scan(&mut fresh, &opts);
    assert_eq!(by_file(&fresh, "color.gb").system, "gbc");
    assert_eq!(by_file(&fresh, "pinned.gb").system, "gb");

    // Rescanning from a higher root makes `gb` a scan folder: the unchanged file is analysed again and pinned.
    let r = scan(&mut lib, &options(&[&tmp.path().join("gb")]));
    assert_eq!((r.updated, r.unchanged), (1, 1), "{r:?}");
    assert_eq!(by_file(&lib, "color.gb").system, "gb");
    let r = scan(&mut lib, &options(&[&tmp.path().join("gb")]));
    assert_eq!(r.unchanged, 2, "{r:?}");
}

/// The stored path does not depend on how the root is spelled (`.` components, trailing or `/` separators on
/// Windows): rescanning the same folder under another spelling finds the same, unchanged games.
#[test]
fn root_spelling_does_not_change_the_stored_paths() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("x").join("roms").join("a.gb"),
        &rom(9, 300),
    );
    let mut lib = Library::open_in_memory().unwrap();
    let r = scan(&mut lib, &options(&[&tmp.path().join("x/./roms/")]));
    assert_eq!(r.added, 1, "{r:?}");
    let r = scan(&mut lib, &options(&[&tmp.path().join("x").join("roms")]));
    assert_eq!((r.added, r.unchanged, r.removed), (0, 1, 0), "{r:?}");
    let r = scan(&mut lib, &options(&[&tmp.path().join("x")]));
    assert_eq!((r.added, r.unchanged, r.removed), (0, 1, 0), "{r:?}");
}

#[test]
fn empty_files_and_empty_zip_entries_are_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    write(&roms.join("empty.gb"), b"");
    write(
        &roms.join("emptyzip.zip"),
        &build_zip(&[(b"inner.gb", b"", None)]),
    );
    write(
        &roms.join("path.zip"),
        &build_zip(&[(b"../dir/Name.gb", &rom(9, 600), None)]),
    );
    let mut lib = Library::open_in_memory().unwrap();
    let r = scan(&mut lib, &options(&[&roms]));
    assert_eq!((r.added, r.ignored_empty), (1, 2), "{r:?}");
    assert!(r.errors.is_empty(), "{r:?}");
    let g = by_file(&lib, "path.zip");
    // The title comes from the archive name, the entry is a base name: no path component survives.
    assert_eq!(g.title, "path");
    assert_eq!(g.entry.as_deref(), Some("Name.gb"));

    // A file emptied after a first scan leaves the list.
    write(&roms.join("path.zip"), b"");
    let r = scan(&mut lib, &options(&[&roms]));
    assert_eq!((r.removed, r.ignored_empty), (1, 3), "{r:?}");
    assert!(lib.games().unwrap().is_empty());
}
