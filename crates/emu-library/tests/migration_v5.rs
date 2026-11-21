//! Schema v4 (3.0 / 2.1) -> v5 (4.0): `game.is_dir` is the only change, nothing is lost. A v4 base is a real 2.0
//! base (`tests/data/library/`) taken through the current code, then put back to the v4 layout.

use std::fs;
use std::path::{Path, PathBuf};

use emu_library::{Library, OpenOutcome, SCHEMA_VERSION};
use rusqlite::Connection;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/library/library-2.0.sqlite")
}

fn user_version(db: &Path) -> i64 {
    Connection::open(db)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

/// Every column of `game` except `is_dir`, as text.
fn rows(db: &Path, table_sql: &str) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn.prepare(table_sql).unwrap();
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

const GAME_SQL: &str = "SELECT id, path, entry, system, size, file_size, mtime_unix, crc32, sha1, crc32_nh, \
                        sha1_nh, title, identification, favorite, last_played_unix, play_count, pinned \
                        FROM game ORDER BY id";

/// Columns of `dat_entry` up to v5.
const ENTRY_SQL: &str =
    "SELECT system, source, game, rom_name, size, crc32, sha1 FROM dat_entry ORDER BY rowid";

#[test]
fn a_v4_base_gains_is_dir_without_loss() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    fs::copy(fixture(), &db).unwrap();
    drop(Library::open(&db).unwrap());
    assert_eq!(SCHEMA_VERSION, 6);
    // Back to the v4 layout (the v6 columns go too: the steps after v4 are replayed).
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "ALTER TABLE game DROP COLUMN region;
             ALTER TABLE dat_entry DROP COLUMN serial;
             ALTER TABLE dat_entry DROP COLUMN region;
             ALTER TABLE game DROP COLUMN is_dir;",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 4).unwrap();
    }
    assert_eq!(user_version(&db), 4);
    let games = rows(&db, GAME_SQL);
    let media = rows(&db, "SELECT * FROM game_media ORDER BY game_id, idx");
    let covers = rows(&db, "SELECT * FROM cover ORDER BY game, kind");
    let dat = rows(&db, "SELECT * FROM dat_info ORDER BY system, source");
    let entries = rows(&db, ENTRY_SQL);
    assert_eq!(games.len(), 7);

    let (lib, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    let all = lib.games().unwrap();
    assert_eq!(all.len(), 7);
    assert!(all.iter().all(|g| !g.is_dir));
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
    // The new column: every old row is a file, and the migration is applied once.
    assert_eq!(
        rows(&db, "SELECT DISTINCT is_dir FROM game"),
        ["Integer(0)"]
    );
    let (_, again) = Library::open(&db).unwrap();
    assert_eq!(again, OpenOutcome::Opened);
    assert_eq!(rows(&db, GAME_SQL), games);
}
