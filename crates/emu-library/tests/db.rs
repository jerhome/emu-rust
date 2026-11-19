//! `library.sqlite`: migrations, newer schema, corruption and quarantine, user data persistence.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use emu_library::{
    CoverKind, CoverRecord, CoverState, Library, LibraryError, OpenOutcome, SCHEMA_VERSION,
    ScanOptions,
};

fn user_version(path: &Path) -> i64 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

/// A base holding a few games, closed (WAL checkpointed into the main file).
fn populated(dir: &Path) -> PathBuf {
    let roms = dir.join("roms");
    fs::create_dir_all(&roms).unwrap();
    for i in 0..200u32 {
        fs::write(roms.join(format!("game-{i:04}.md")), i.to_le_bytes()).unwrap();
    }
    let db = dir.join("library.sqlite");
    let (mut lib, _) = Library::open(&db).unwrap();
    lib.scan(
        &ScanOptions {
            roots: vec![roms],
            ..Default::default()
        },
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    drop(lib);
    db
}

fn quarantined_files(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.contains(".corrupt-"))
        .collect();
    v.sort();
    v
}

fn assert_recovered(db: &Path) {
    let (lib, outcome) = Library::open(db).unwrap();
    let OpenOutcome::Recovered { quarantined } = outcome else {
        panic!("expected Recovered, got {outcome:?}");
    };
    assert!(quarantined.exists());
    assert!(
        quarantined
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("library.sqlite.corrupt-")
    );
    assert!(lib.games().unwrap().is_empty());
    drop(lib);
    assert_eq!(user_version(db), i64::from(SCHEMA_VERSION));
    let (_, again) = Library::open(db).unwrap();
    assert_eq!(again, OpenOutcome::Opened);
}

#[test]
fn creates_then_reopens() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    let (lib, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Created);
    assert!(lib.games().unwrap().is_empty());
    drop(lib);
    assert_eq!(user_version(&db), i64::from(SCHEMA_VERSION));
    let (_, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);

    // An empty file is a fresh base, not a corrupt one.
    let empty = tmp.path().join("empty.sqlite");
    fs::write(&empty, b"").unwrap();
    assert_eq!(Library::open(&empty).unwrap().1, OpenOutcome::Created);
}

#[test]
fn newer_schema_is_left_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE future (x); PRAGMA user_version = 99;")
            .unwrap();
    }
    let before = fs::read(&db).unwrap();
    match Library::open(&db) {
        Err(LibraryError::NewerSchema { found, supported }) => {
            assert_eq!((found, supported), (99, SCHEMA_VERSION));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fs::read(&db).unwrap(), before);
    assert!(quarantined_files(tmp.path()).is_empty());
}

/// A newer version left its changes in `-wal` (crash, or still running elsewhere): the read-write connection that
/// would checkpoint them into the main file on close is never opened.
#[test]
fn newer_schema_in_wal_is_left_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    let copy = tmp.path().join("copy");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&copy).unwrap();
    let db = origin.join("library.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    conn.execute_batch(
        "CREATE TABLE future (x); INSERT INTO future VALUES (1); PRAGMA user_version = 99;",
    )
    .unwrap();
    // Copied while the writer is open: the change lives only in the WAL.
    for name in ["library.sqlite", "library.sqlite-wal"] {
        fs::copy(origin.join(name), copy.join(name)).unwrap();
    }
    drop(conn);
    let db = copy.join("library.sqlite");
    let wal = copy.join("library.sqlite-wal");
    let before = (fs::read(&db).unwrap(), fs::read(&wal).unwrap());
    assert!(!before.1.is_empty());
    match Library::open(&db) {
        Err(LibraryError::NewerSchema { found, .. }) => assert_eq!(found, 99),
        other => panic!("{other:?}"),
    }
    assert_eq!((fs::read(&db).unwrap(), fs::read(&wal).unwrap()), before);
    assert!(quarantined_files(&copy).is_empty());
}

#[test]
fn random_bytes_are_quarantined() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let bytes: Vec<u8> = (0..8192)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    fs::write(&db, &bytes).unwrap();
    assert_recovered(&db);
    let q = quarantined_files(tmp.path());
    assert_eq!(q.len(), 1, "{q:?}");
    assert_eq!(fs::read(tmp.path().join(&q[0])).unwrap(), bytes);
}

#[test]
fn text_file_is_quarantined() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    fs::write(&db, b"this is not a database at all, just some text\n").unwrap();
    // A leftover WAL never reaches the new base: SQLite discards an invalid one itself, otherwise it is
    // renamed along with the corrupt file.
    let wal = tmp.path().join("library.sqlite-wal");
    fs::write(&wal, b"stale").unwrap();
    assert_recovered(&db);
    assert!(fs::read(&wal).map_or(true, |w| w != b"stale"));
}

#[test]
fn truncated_file_is_quarantined() {
    let tmp = tempfile::tempdir().unwrap();
    let db = populated(tmp.path());
    let len = fs::metadata(&db).unwrap().len();
    assert!(len > 8192, "{len}");
    let f = fs::OpenOptions::new().write(true).open(&db).unwrap();
    f.set_len(len / 2).unwrap();
    drop(f);
    assert_recovered(&db);
}

#[test]
fn modified_page_is_quarantined() {
    let tmp = tempfile::tempdir().unwrap();
    let db = populated(tmp.path());
    let mut bytes = fs::read(&db).unwrap();
    let page = 4096;
    assert!(bytes.len() >= 4 * page);
    // Overwrite the b-tree page headers and cell pointers of every page after the schema page.
    for start in (page..bytes.len()).step_by(page) {
        for b in &mut bytes[start..start + 64] {
            *b = 0xFF;
        }
    }
    fs::write(&db, &bytes).unwrap();
    assert_recovered(&db);
}

#[test]
fn user_data_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let db = populated(tmp.path());
    let (mut lib, _) = Library::open(&db).unwrap();
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 200);
    let (a, b) = (games[0].id, games[1].id);
    lib.set_favorite(a, true).unwrap();
    lib.record_played(a, 1_700_000_000).unwrap();
    lib.record_played(a, 1_700_000_100).unwrap();
    lib.set_cover(
        a,
        CoverKind::Boxart,
        &CoverState::Cached {
            path: "megadrive/boxart/x.png".into(),
            source: "libretro".into(),
        },
    )
    .unwrap();
    lib.set_cover(
        b,
        CoverKind::Boxart,
        &CoverState::Missing { checked_unix: 100 },
    )
    .unwrap();
    drop(lib);

    let (mut lib, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    let ga = lib.game(a).unwrap().unwrap();
    assert!(ga.favorite);
    assert_eq!(
        (ga.last_played_unix, ga.play_count),
        (Some(1_700_000_100), 2)
    );
    assert!(!lib.game(b).unwrap().unwrap().favorite);
    assert!(matches!(
        lib.cover(a, CoverKind::Boxart).unwrap(),
        Some(CoverRecord { state: CoverState::Cached { ref path, ref source }, .. })
            if path == Path::new("megadrive/boxart/x.png") && source == "libretro"
    ));
    assert_eq!(
        lib.cover(b, CoverKind::Boxart).unwrap().unwrap().state,
        CoverState::Missing { checked_unix: 100 }
    );

    let needing = lib.games_needing_cover(CoverKind::Boxart, 50).unwrap();
    assert_eq!(needing.len(), 198);
    assert!(!needing.contains(&a) && !needing.contains(&b));
    let needing = lib.games_needing_cover(CoverKind::Boxart, 101).unwrap();
    assert_eq!(needing.len(), 199);
    assert!(needing.contains(&b));
    lib.clear_missing_covers().unwrap();
    assert!(lib.cover(b, CoverKind::Boxart).unwrap().is_none());
    assert!(lib.cover(a, CoverKind::Boxart).unwrap().is_some());

    // Unknown ids: no-op for user flags, foreign-key error for covers.
    let ghost = emu_library::GameId(999_999);
    lib.set_favorite(ghost, true).unwrap();
    lib.record_played(ghost, 1).unwrap();
    assert!(lib.game(ghost).unwrap().is_none());
    assert!(matches!(
        lib.set_cover(
            ghost,
            CoverKind::Boxart,
            &CoverState::Missing { checked_unix: 1 }
        ),
        Err(LibraryError::Database(_))
    ));
}

#[test]
fn two_connections_share_the_base() {
    let tmp = tempfile::tempdir().unwrap();
    let db = populated(tmp.path());
    let (reader, _) = Library::open(&db).unwrap();
    let (mut writer, _) = Library::open(&db).unwrap();
    let id = reader.games().unwrap()[0].id;
    writer.set_favorite(id, true).unwrap();
    assert!(reader.game(id).unwrap().unwrap().favorite);
}
