//! Schema v5 (4.0 / 5.0) -> v6 (6.0): `game.region`, `dat_entry.serial` and `dat_entry.region` are added, nothing is
//! lost. The v5 base is the real 2.0 base (`tests/data/library/`) taken through the current code, with Mega CD
//! games and a DOS folder game added by a real scan, then put back to the v5 layout.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use emu_library::{DiscRegion, Library, OpenOutcome, SCHEMA_VERSION, ScanOptions};
use rusqlite::Connection;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/library/library-2.0.sqlite")
}

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

fn user_version(db: &Path) -> i64 {
    Connection::open(db)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn rows(db: &Path, sql: &str) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        (0..n)
            .map(|i| r.get_ref(i).map(|v| format!("{v:?}")))
            .collect::<rusqlite::Result<Vec<_>>>()
            .map(|c| c.join("|"))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

/// Every v5 column of `game`.
const GAME_SQL: &str = "SELECT id, path, entry, system, size, file_size, mtime_unix, crc32, sha1, crc32_nh, \
                        sha1_nh, title, identification, favorite, last_played_unix, play_count, pinned, is_dir \
                        FROM game ORDER BY id";
const ENTRY_SQL: &str =
    "SELECT system, source, game, rom_name, size, crc32, sha1 FROM dat_entry ORDER BY rowid";

fn scan(lib: &mut Library, root: &Path) -> emu_library::ScanReport {
    // `/r`, root of the 2.0 games, does not exist (unmounted drive): they are kept.
    let opts = ScanOptions {
        roots: vec![root.to_path_buf(), PathBuf::from("/r")],
        ..Default::default()
    };
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

#[test]
fn a_v5_base_gains_the_region_without_loss_and_the_scan_fills_it() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    fs::copy(fixture(), &db).unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    mkcd(&[
        "megacd",
        &r("megacd/eu"),
        "--name",
        "Dune (Europe)",
        "--region",
        "E",
    ]);
    mkcd(&[
        "megacd",
        &r("megacd/jp"),
        "--name",
        "Header Only",
        "--region",
        "J",
        "--sector",
        "2048",
    ]);
    fs::create_dir_all(root.join("megacd")).unwrap();
    fs::write(root.join("megacd/Named (USA).chd"), b"never read").unwrap();
    fs::write(root.join("megacd/Bare.chd"), b"never read").unwrap();
    fs::create_dir_all(root.join("dos/Keen")).unwrap();
    fs::write(root.join("dos/Keen/KEEN.EXE"), b"MZ fake").unwrap();
    {
        let (mut lib, _) = Library::open(&db).unwrap();
        let report = scan(&mut lib, &root);
        assert!(report.errors.is_empty(), "{report:?}");
        let id = lib
            .games()
            .unwrap()
            .iter()
            .find(|g| g.system == "dos")
            .unwrap()
            .id;
        lib.set_favorite(id, true).unwrap();
        lib.record_played(id, 1_800_000_000).unwrap();
    }
    assert_eq!(SCHEMA_VERSION, 6);
    // Back to the v5 layout, as a 5.0 build left it: the 2.0 games (missing root, kept) and the new ones.
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "ALTER TABLE game DROP COLUMN region;
             ALTER TABLE dat_entry DROP COLUMN serial;
             ALTER TABLE dat_entry DROP COLUMN region;",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();
    }
    assert_eq!(user_version(&db), 5);
    let games = rows(&db, GAME_SQL);
    let media = rows(&db, "SELECT * FROM game_media ORDER BY game_id, idx");
    let covers = rows(&db, "SELECT * FROM cover ORDER BY game, kind");
    let dat = rows(&db, "SELECT * FROM dat_info ORDER BY system, source");
    let entries = rows(&db, ENTRY_SQL);
    assert_eq!(games.len(), 7 + 5, "{games:?}");
    assert!(!entries.is_empty());

    let (lib, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    assert!(lib.games().unwrap().iter().all(|g| g.region.is_none()));
    drop(lib);
    assert_eq!(user_version(&db), 6);
    assert_eq!(rows(&db, GAME_SQL), games);
    assert_eq!(
        rows(&db, "SELECT * FROM game_media ORDER BY game_id, idx"),
        media
    );
    assert_eq!(rows(&db, "SELECT * FROM cover ORDER BY game, kind"), covers);
    assert_eq!(
        rows(&db, "SELECT * FROM dat_info ORDER BY system, source"),
        dat
    );
    assert_eq!(rows(&db, ENTRY_SQL), entries);
    assert_eq!(rows(&db, "SELECT DISTINCT region FROM game"), ["Null"]);
    assert_eq!(
        rows(&db, "SELECT DISTINCT serial, region FROM dat_entry"),
        ["Null|Null"]
    );

    // The next scan leaves every row unchanged (same size and date) but completes the region of the discs.
    let (mut lib, _) = Library::open(&db).unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(
        (report.added, report.updated, report.removed),
        (0, 0, 0),
        "{report:?}"
    );
    assert_eq!(report.unchanged, 5);
    let region = |file: &str| {
        lib.games()
            .unwrap()
            .into_iter()
            .find(|g| g.path.file_name().is_some_and(|n| n == file))
            .unwrap()
            .region
    };
    assert_eq!(region("Dune (Europe).cue"), Some(DiscRegion::Europe));
    assert_eq!(region("Header Only.cue"), Some(DiscRegion::Japan));
    assert_eq!(region("Named (USA).chd"), Some(DiscRegion::Usa));
    assert_eq!(region("Bare.chd"), None);
    assert_eq!(region("Keen"), None);
    drop(lib);
    // Only `region` moved.
    assert_eq!(rows(&db, GAME_SQL), games);
    let (_, again) = Library::open(&db).unwrap();
    assert_eq!(again, OpenOutcome::Opened);
    assert_eq!(user_version(&db), 6);
}
