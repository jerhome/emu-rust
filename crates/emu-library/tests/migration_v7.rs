//! Schema v7 (7.x) -> v8 (8.0): `game.scan_pending` is added, nothing is lost. The v7 base is made by a real scan
//! (a favourite, a played game, a cover) and put back to the v7 layout.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use emu_library::{CoverKind, CoverState, Library, OpenOutcome, SCHEMA_VERSION, ScanOptions};
use rusqlite::Connection;

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

/// Every v7 column of `game`.
const GAME_SQL: &str = "SELECT id, path, entry, system, size, file_size, mtime_unix, crc32, sha1, crc32_nh, \
                        sha1_nh, title, identification, favorite, last_played_unix, play_count, pinned, is_dir, \
                        region, needs_32x FROM game ORDER BY id";

fn scan(lib: &mut Library, root: &Path) -> emu_library::ScanReport {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

#[test]
fn a_v7_base_gains_the_scan_mark_without_loss() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    let root = tmp.path().join("roms");
    fs::create_dir_all(root.join("gb")).unwrap();
    for (name, seed) in [("A.gb", 1u8), ("B.gb", 2), ("C.gb", 3)] {
        fs::write(root.join("gb").join(name), vec![seed; 4096]).unwrap();
    }
    {
        let (mut lib, _) = Library::open(&db).unwrap();
        let report = scan(&mut lib, &root);
        assert_eq!(report.added, 3, "{report:?}");
        let games = lib.games().unwrap();
        lib.set_favorite(games[0].id, true).unwrap();
        lib.record_played(games[1].id, 1_800_000_000).unwrap();
        lib.set_cover(
            games[2].id,
            CoverKind::Boxart,
            &CoverState::Missing { checked_unix: 1 },
        )
        .unwrap();
    }
    assert_eq!(SCHEMA_VERSION, 8);
    // Back to the v7 layout, as a 7.x build left it.
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("ALTER TABLE game DROP COLUMN scan_pending;")
            .unwrap();
        conn.pragma_update(None, "user_version", 7).unwrap();
    }
    assert_eq!(user_version(&db), 7);
    let games = rows(&db, GAME_SQL);
    let media = rows(&db, "SELECT * FROM game_media ORDER BY game_id, idx");
    let covers = rows(&db, "SELECT * FROM cover ORDER BY game, kind");
    assert_eq!(games.len(), 3, "{games:?}");

    let (lib, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    assert_eq!(lib.games().unwrap().len(), 3);
    drop(lib);
    assert_eq!(user_version(&db), 8);
    assert_eq!(rows(&db, GAME_SQL), games);
    assert_eq!(
        rows(&db, "SELECT * FROM game_media ORDER BY game_id, idx"),
        media
    );
    assert_eq!(rows(&db, "SELECT * FROM cover ORDER BY game, kind"), covers);
    assert_eq!(
        rows(&db, "SELECT DISTINCT scan_pending FROM game"),
        ["Null"]
    );

    // The next scan leaves every row unchanged and unmarked.
    let (mut lib, _) = Library::open(&db).unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(
        (
            report.added,
            report.updated,
            report.removed,
            report.unchanged
        ),
        (0, 0, 0, 3),
        "{report:?}"
    );
    assert!(!report.cautious);
    drop(lib);
    assert_eq!(rows(&db, GAME_SQL), games);
    assert_eq!(
        rows(&db, "SELECT DISTINCT scan_pending FROM game"),
        ["Null"]
    );
    let (_, again) = Library::open(&db).unwrap();
    assert_eq!(again, OpenOutcome::Opened);
    assert_eq!(user_version(&db), 8);
}
