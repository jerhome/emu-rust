use std::collections::BTreeMap;
use std::io::Read;

use super::*;

/// Files of a folder (relative `/` path ⇒ bytes; folders as `name/` ⇒ empty) plus their modification times.
fn tree(root: &Path) -> BTreeMap<String, (Vec<u8>, Option<std::time::SystemTime>)> {
    fn visit(
        root: &Path,
        dir: &Path,
        out: &mut BTreeMap<String, (Vec<u8>, Option<std::time::SystemTime>)>,
    ) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                out.insert(format!("{rel}/"), (Vec::new(), None));
                visit(root, &path, out);
            } else if meta.is_file() {
                out.insert(rel, (fs::read(&path).unwrap(), meta.modified().ok()));
            } else {
                out.insert(format!("{rel}@link"), (Vec::new(), None));
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn write(root: &Path, rel: &str, data: &[u8]) -> PathBuf {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, data).unwrap();
    path
}

/// A small DOS-like game: root executable, nested data, an empty folder.
fn game(root: &Path) -> PathBuf {
    let dir = root.join("Game");
    write(&dir, "GAME.EXE", b"MZ fake executable");
    write(&dir, "SETUP.EXE", b"MZ setup");
    write(&dir, "DATA/LEVEL1.DAT", &(0..=255u8).collect::<Vec<_>>());
    write(&dir, "DATA/SUB/LEVEL2.DAT", &[1u8; 64]);
    write(&dir, "caf\u{e9}.txt", b"non ascii");
    fs::create_dir_all(dir.join("SAVES")).unwrap();
    dir
}

fn build(dir: &Path, snapshot: &Path) -> Result<SnapshotReport, RuntimeError> {
    snapshot_folder(
        dir,
        snapshot,
        &SnapshotLimits::DEFAULT,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
}

/// Entries of a zip: name ⇒ (bytes, stored?).
fn zip_entries(path: &Path) -> BTreeMap<String, (Vec<u8>, bool)> {
    let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.insert(
            String::from_utf8(entry.name_raw().to_vec()).unwrap(),
            (data, entry.compression() == CompressionMethod::Stored),
        );
    }
    out
}

/// Names in `dir` (the cache folder): no temporary file may stay behind.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|r| {
            r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn snapshot_path_is_one_sanitized_file_name_without_hash() {
    let cache = Path::new("cache/dosbox_pure");
    assert_eq!(
        snapshot_path(cache, "Monkey Island"),
        cache.join("Monkey Island.zip")
    );
    assert_eq!(snapshot_path(cache, "G (2)"), cache.join("G (2).zip"));
    assert_eq!(snapshot_path(cache, "a#b/c"), cache.join("ab_c.zip"));
    assert_eq!(snapshot_path(cache, "../x"), cache.join(".._x.zip"));
    assert_eq!(snapshot_path(cache, "#"), cache.join("game.zip"));
    assert_eq!(snapshot_path(cache, ".."), cache.join("game.zip"));
    assert_eq!(snapshot_path(cache, "CON"), cache.join("_CON.zip"));
    assert_eq!(
        manifest_path(&cache.join("G.zip")),
        cache.join("G.zip.manifest")
    );
}

#[test]
fn snapshot_holds_the_folder_stored_and_tracks_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let snap = tmp.path().join("cache/dosbox_pure/Game.zip");
    assert_eq!(
        snapshot_status(&dir, &snap).unwrap(),
        SnapshotStatus::Missing
    );

    let mut calls = Vec::new();
    let report = snapshot_folder(
        &dir,
        &snap,
        &SnapshotLimits::DEFAULT,
        &mut |done, total| calls.push((done, total)),
        &AtomicBool::new(false),
    )
    .unwrap();
    let bytes = 18 + 8 + 256 + 64 + 9;
    assert_eq!(
        report,
        SnapshotReport {
            files: 5,
            bytes,
            rebuilt: true
        }
    );
    assert_eq!(calls.first(), Some(&(0, bytes)));
    assert_eq!(calls.last(), Some(&(bytes, bytes)));
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0));

    let entries = zip_entries(&snap);
    let expected: Vec<&str> = vec![
        "DATA/",
        "DATA/LEVEL1.DAT",
        "DATA/SUB/",
        "DATA/SUB/LEVEL2.DAT",
        "GAME.EXE",
        "SAVES/",
        "SETUP.EXE",
        "caf\u{e9}.txt",
    ];
    assert_eq!(
        entries.keys().map(String::as_str).collect::<Vec<_>>(),
        expected
    );
    assert!(entries.values().all(|(_, stored)| *stored));
    for (name, (data, _)) in &entries {
        if !name.ends_with('/') {
            assert_eq!(data, &fs::read(dir.join(name)).unwrap(), "{name}");
        }
    }
    assert_eq!(
        snapshot_status(&dir, &snap).unwrap(),
        SnapshotStatus::Current
    );
    assert_eq!(
        names(snap.parent().unwrap()),
        ["Game.zip", "Game.zip.manifest"]
    );

    // Already current: nothing written.
    let before = fs::metadata(&snap).unwrap().modified().unwrap();
    assert!(!build(&dir, &snap).unwrap().rebuilt);
    assert_eq!(fs::metadata(&snap).unwrap().modified().unwrap(), before);

    // A new file, then a changed size: stale, rebuilt.
    write(&dir, "DATA/NEW.DAT", b"new");
    assert_eq!(snapshot_status(&dir, &snap).unwrap(), SnapshotStatus::Stale);
    assert!(build(&dir, &snap).unwrap().rebuilt);
    assert_eq!(
        snapshot_status(&dir, &snap).unwrap(),
        SnapshotStatus::Current
    );
    assert_eq!(zip_entries(&snap)["DATA/NEW.DAT"].0, b"new");
    write(&dir, "DATA/NEW.DAT", b"newer");
    assert_eq!(snapshot_status(&dir, &snap).unwrap(), SnapshotStatus::Stale);

    // Manifest missing or foreign: stale.
    build(&dir, &snap).unwrap();
    fs::write(manifest_path(&snap), b"emu-rust snapshot 1\nnope\n").unwrap();
    assert_eq!(snapshot_status(&dir, &snap).unwrap(), SnapshotStatus::Stale);
    fs::remove_file(manifest_path(&snap)).unwrap();
    assert_eq!(snapshot_status(&dir, &snap).unwrap(), SnapshotStatus::Stale);
}

#[test]
fn snapshot_is_reproducible() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let a = tmp.path().join("a/Game.zip");
    let b = tmp.path().join("b/Other.zip");
    build(&dir, &a).unwrap();
    build(&dir, &b).unwrap();
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    assert_eq!(
        fs::read(manifest_path(&a)).unwrap(),
        fs::read(manifest_path(&b)).unwrap()
    );
}

#[test]
fn source_folder_is_never_modified() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let before = tree(&dir);
    let snap = tmp.path().join("cache/Game.zip");
    build(&dir, &snap).unwrap();
    snapshot_status(&dir, &snap).unwrap();
    folder_fingerprint(&dir, &SnapshotLimits::DEFAULT).unwrap();
    write(&dir, "X.TXT", b"x");
    build(&dir, &snap).unwrap();
    fs::remove_file(dir.join("X.TXT")).unwrap();
    assert_eq!(tree(&dir), before);
}

#[cfg(unix)]
#[test]
fn read_only_folder_is_accepted() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let before = tree(&dir);
    let set_mode = |dir_mode: u32, file_mode: u32| {
        for (rel, _) in tree(&dir) {
            let path = dir.join(rel.trim_end_matches('/'));
            let mode = if rel.ends_with('/') {
                dir_mode
            } else {
                file_mode
            };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        }
        fs::set_permissions(&dir, fs::Permissions::from_mode(dir_mode)).unwrap();
    };
    set_mode(0o555, 0o444);
    let snap = tmp.path().join("cache/Game.zip");
    let result = build(&dir, &snap);
    set_mode(0o755, 0o644);
    assert_eq!(result.unwrap().files, 5);
    assert_eq!(tree(&dir), before);
}

#[test]
fn limits_are_enforced_during_the_walk_and_leave_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let cache = tmp.path().join("cache");
    let snap = cache.join("Game.zip");
    let try_with = |limits: SnapshotLimits| {
        snapshot_folder(
            &dir,
            &snap,
            &limits,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
    };
    let d = SnapshotLimits::DEFAULT;
    let cases = [
        (
            SnapshotLimits {
                max_bytes: 300,
                ..d
            },
            300,
        ),
        (SnapshotLimits { max_files: 7, ..d }, 7),
        (SnapshotLimits { max_depth: 1, ..d }, 1),
    ];
    for (limits, limit) in cases {
        match try_with(limits) {
            Err(RuntimeError::SnapshotTooLarge { limit: l }) => assert_eq!(l, limit),
            other => panic!("{limits:?}: {other:?}"),
        }
        assert!(!snap.exists());
        assert!(names(&cache).is_empty(), "{:?}", names(&cache));
    }
    // Exactly at the limits: accepted (355 bytes, 8 entries, depth 2).
    let exact = SnapshotLimits {
        max_bytes: 355,
        max_files: 8,
        max_depth: 2,
    };
    assert!(try_with(exact).unwrap().rebuilt);
    assert!(matches!(
        folder_fingerprint(&dir, &SnapshotLimits { max_files: 2, ..d }),
        Err(RuntimeError::SnapshotTooLarge { limit: 2 })
    ));
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let outside = write(tmp.path(), "secret.txt", b"outside the game");
    std::os::unix::fs::symlink(&outside, dir.join("LINK.TXT")).unwrap();
    std::os::unix::fs::symlink(tmp.path(), dir.join("LOOP")).unwrap();
    let snap = tmp.path().join("cache/Game.zip");
    assert_eq!(build(&dir, &snap).unwrap().files, 5);
    let entries = zip_entries(&snap);
    assert!(
        !entries
            .keys()
            .any(|k| k.starts_with("LINK") || k.starts_with("LOOP"))
    );
}

#[cfg(unix)]
#[test]
fn unrepresentable_names_are_refused_before_anything_is_written() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let mut hostile_names = vec![OsStr::new("NEW\nLINE.TXT"), OsStr::new("back\\slash.txt")];
    // APFS (macOS) refuses non-UTF-8 names itself.
    if cfg!(target_os = "linux") {
        hostile_names.push(OsStr::from_bytes(b"latin1-\xe9.txt"));
    }
    for bad in hostile_names {
        let tmp = tempfile::tempdir().unwrap();
        let dir = game(tmp.path());
        let hostile = dir.join("DATA").join(bad);
        fs::write(&hostile, b"x").unwrap();
        let cache = tmp.path().join("cache");
        let snap = cache.join("Game.zip");
        match build(&dir, &snap) {
            Err(RuntimeError::UnrepresentableName(path)) => assert_eq!(path, hostile),
            other => panic!("{bad:?}: {other:?}"),
        }
        assert!(names(&cache).is_empty());
        assert!(matches!(
            folder_fingerprint(&dir, &SnapshotLimits::DEFAULT),
            Err(RuntimeError::UnrepresentableName(_))
        ));
    }
}

#[test]
fn cancellation_leaves_no_residue_and_keeps_the_previous_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let cache = tmp.path().join("cache");
    let snap = cache.join("Game.zip");
    let cancelled = AtomicBool::new(true);
    let r = snapshot_folder(
        &dir,
        &snap,
        &SnapshotLimits::DEFAULT,
        &mut |_, _| {},
        &cancelled,
    );
    assert!(matches!(r, Err(RuntimeError::SnapshotCancelled)));
    assert!(names(&cache).is_empty());

    build(&dir, &snap).unwrap();
    let kept = fs::read(&snap).unwrap();
    write(&dir, "DATA/BIG.DAT", &vec![7u8; 3 * CHUNK]);
    // Cancelled in the middle of a large file, from the progress callback (as the UI's Cancel button would).
    let cancel = AtomicBool::new(false);
    let r = snapshot_folder(
        &dir,
        &snap,
        &SnapshotLimits::DEFAULT,
        &mut |done, _| {
            if done > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(r, Err(RuntimeError::SnapshotCancelled)));
    assert_eq!(fs::read(&snap).unwrap(), kept, "previous snapshot intact");
    assert_eq!(names(&cache), ["Game.zip", "Game.zip.manifest"]);
    assert_eq!(snapshot_status(&dir, &snap).unwrap(), SnapshotStatus::Stale);
}

#[test]
fn folder_fingerprint_ignores_dates_but_not_names_or_sizes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    let fp = |d: &Path| folder_fingerprint(d, &SnapshotLimits::DEFAULT).unwrap();
    let first = fp(&dir);
    assert_eq!(first.0, 355);
    // Same names and sizes, other bytes and dates: same fingerprint (no file is read).
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(&dir, "GAME.EXE", b"MZ FAKE EXECUTABLE");
    assert_eq!(fp(&dir), first);
    // A copy elsewhere has the same fingerprint (moved folder).
    let copy = tmp.path().join("copy");
    for (rel, (data, _)) in tree(&dir) {
        if rel.ends_with('/') {
            fs::create_dir_all(copy.join(rel)).unwrap();
        } else {
            write(&copy, &rel, &data);
        }
    }
    assert_eq!(fp(&copy), first);
    fs::rename(dir.join("SETUP.EXE"), dir.join("INSTALL.EXE")).unwrap();
    assert_ne!(fp(&dir), first);
    fs::rename(dir.join("INSTALL.EXE"), dir.join("SETUP.EXE")).unwrap();
    write(&dir, "SETUP.EXE", b"MZ setup!");
    assert_eq!(fp(&dir).0, 356);
    assert_ne!(fp(&dir).1, first.1);
}

#[test]
fn start_program_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = game(tmp.path());
    write(&dir, "run.bat", b"@echo off");
    write(&dir, "TOOL.COM", b"\xc3");
    write(&dir, "README.TXT", b"text");
    write(&dir, "LONGNAME1.EXE", b"MZ");
    fs::create_dir_all(dir.join("DIR.EXE")).unwrap();
    assert_eq!(validate_start(&dir, "GAME.EXE").unwrap(), "GAME.EXE");
    assert_eq!(validate_start(&dir, "game.exe").unwrap(), "GAME.EXE");
    assert_eq!(validate_start(&dir, "RUN.BAT").unwrap(), "RUN.BAT");
    assert_eq!(validate_start(&dir, "Tool.Com").unwrap(), "TOOL.COM");
    for bad in [
        "",
        "README.TXT",
        "MISSING.EXE",
        "LONGNAME1.EXE",
        "DIR.EXE",
        "GAME",
        ".EXE",
        "GA ME.EXE",
        "G#.EXE",
        "A:GAME.EXE",
        "DATA/LEVEL1.EXE",
        "..\\GAME.EXE",
        "GAME.EXE.EXE",
        "GAME.EX",
        "GAME.EXEC",
        "J\u{e9}U.EXE",
        "GAME.EXE\n",
    ] {
        assert!(
            matches!(validate_start(&dir, bad), Err(RuntimeError::InvalidStart(s)) if s == bad),
            "{bad:?}"
        );
    }
}

#[test]
fn dos_dates() {
    let ns = |secs: u64| Some(u128::from(secs) * 1_000_000_000);
    let parts = |d: DateTime| {
        (
            d.year(),
            d.month(),
            d.day(),
            d.hour(),
            d.minute(),
            d.second(),
        )
    };
    assert_eq!(parts(dos_time(None)), (1980, 1, 1, 0, 0, 0));
    assert_eq!(parts(dos_time(ns(0))), (1980, 1, 1, 0, 0, 0), "before 1980");
    // 2000-02-29 12:34:56 UTC.
    assert_eq!(parts(dos_time(ns(951_827_696))), (2000, 2, 29, 12, 34, 56));
    // 1980-01-01 00:00:01 UTC: seconds rounded down to even by the zip format.
    assert_eq!(parts(dos_time(ns(315_532_801))), (1980, 1, 1, 0, 0, 0));
    assert_eq!(parts(dos_time(Some(u128::MAX))).0, 2107);
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
    assert_eq!(civil_from_days(20_819), (2027, 1, 1));
}
