//! The "scan in progress" marker (7.6): a scan killed between two batches leaves it, and the next scan then
//! merges moved files but never purges, so a favourite is not lost with the old row of a file moved by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use emu_library::{CoverKind, CoverState, Library, LibraryError, ScanOptions, ScanReport};
use rusqlite::Connection;

fn noise(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

fn scan(lib: &mut Library, root: &Path) -> ScanReport {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn marker(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(".scanning");
    PathBuf::from(name)
}

/// Rows for `Hero.sfc` (favourite) and its copy `vrac/Hero.sfc` stored by a first scan, then the original
/// removed: the state a scan killed after storing the new side of a move would leave.
struct Fx {
    _tmp: tempfile::TempDir,
    db: PathBuf,
    roms: PathBuf,
}

fn interrupted_move() -> (Fx, Library) {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    fs::create_dir_all(roms.join("vrac")).unwrap();
    fs::write(roms.join("Hero.sfc"), noise(4096)).unwrap();
    fs::write(roms.join("vrac").join("Hero.sfc"), noise(4096)).unwrap();
    let db = tmp.path().join("library.db");
    let (mut lib, _) = Library::open(&db).unwrap();
    scan(&mut lib, &roms);
    let hero = lib
        .games()
        .unwrap()
        .into_iter()
        .find(|g| g.path == roms.join("Hero.sfc"))
        .unwrap();
    lib.set_favorite(hero.id, true).unwrap();
    fs::remove_file(roms.join("Hero.sfc")).unwrap();
    (
        Fx {
            _tmp: tmp,
            db,
            roms,
        },
        lib,
    )
}

#[test]
fn without_a_marker_the_old_row_is_purged() {
    let (fx, mut lib) = interrupted_move();
    let report = scan(&mut lib, &fx.roms);
    assert!(!report.cautious);
    assert_eq!(report.removed, 1);
    assert!(lib.games().unwrap().iter().all(|g| !g.favorite));
}

#[test]
fn a_leftover_marker_merges_the_moved_file_and_keeps_the_favourite() {
    let (fx, mut lib) = interrupted_move();
    fs::write(marker(&fx.db), b"left by a killed scan").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!((report.moved, report.removed), (1, 0));
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 1);
    assert!(games[0].favorite);
    assert_eq!(games[0].path, fx.roms.join("vrac").join("Hero.sfc"));
    assert!(!marker(&fx.db).exists(), "removed at the normal end");
}

#[test]
fn a_leftover_marker_purges_nothing_and_the_next_scan_does() {
    let (fx, mut lib) = interrupted_move();
    fs::remove_file(fx.roms.join("vrac").join("Hero.sfc")).unwrap();
    fs::write(marker(&fx.db), b"left by a killed scan").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!(report.removed, 0);
    assert_eq!(lib.games().unwrap().len(), 2);
    assert!(!marker(&fx.db).exists());
    let report = scan(&mut lib, &fx.roms);
    assert!(!report.cautious);
    assert_eq!(report.removed, 2);
}

#[test]
fn the_marker_file_is_no_longer_written() {
    let (fx, mut lib) = interrupted_move();
    let opts = ScanOptions {
        roots: vec![fx.roms.clone()],
        ..Default::default()
    };
    let mut seen = false;
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {
        seen |= marker(&fx.db).exists();
    })
    .unwrap();
    assert!(!seen);
    assert!(!marker(&fx.db).exists());
}

fn sql(db: &Path, sql: &str) {
    Connection::open(db).unwrap().execute_batch(sql).unwrap();
}

fn marks(db: &Path) -> i64 {
    Connection::open(db)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM game WHERE scan_pending IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

/// Marks the row of `path` as a scan killed between two batches leaves it.
fn mark(db: &Path, path: &Path) {
    let conn = Connection::open(db).unwrap();
    let n = conn
        .execute(
            "UPDATE game SET scan_pending = 1 WHERE path = ?1",
            [path.to_string_lossy().as_ref()],
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn rows_written_by_a_scan_are_marked_until_its_end() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    fs::create_dir_all(&roms).unwrap();
    fs::write(roms.join("A.sfc"), noise(4096)).unwrap();
    fs::write(roms.join("B.sfc"), noise(2048)).unwrap();
    let db = tmp.path().join("library.db");
    let (mut lib, _) = Library::open(&db).unwrap();
    // The trigger records the mark each row carries the moment the scan writes it.
    sql(
        &db,
        "CREATE TABLE written (path TEXT, mark INTEGER);
         CREATE TRIGGER w AFTER INSERT ON game BEGIN
             INSERT INTO written VALUES (NEW.path, NEW.scan_pending);
         END;",
    );
    let report = scan(&mut lib, &roms);
    assert_eq!(report.added, 2);
    let conn = Connection::open(&db).unwrap();
    let written: Vec<Option<i64>> = conn
        .prepare("SELECT mark FROM written")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(written, [Some(1), Some(1)]);
    assert_eq!(marks(&db), 0, "lifted by the end of the scan");
}

#[test]
fn rows_left_marked_by_a_killed_scan_are_merged_then_the_scan_purges_normally() {
    let (fx, mut lib) = interrupted_move();
    // The killed scan had stored the new side of the move (vrac/Hero.sfc) and nothing else.
    mark(&fx.db, &fx.roms.join("vrac").join("Hero.sfc"));
    fs::write(fx.roms.join("Other.sfc"), noise(2048)).unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(!report.cautious, "no 7.6 file: not a cautious scan");
    assert_eq!((report.moved, report.removed, report.added), (1, 0, 1));
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 2);
    let hero = games.iter().find(|g| g.favorite).expect("favourite kept");
    assert_eq!(hero.path, fx.roms.join("vrac").join("Hero.sfc"));
    assert_eq!(marks(&fx.db), 0);
    // Purge is normal again: the next scan removes what disappeared.
    fs::remove_file(fx.roms.join("Other.sfc")).unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert_eq!((report.moved, report.removed), (0, 1));
}

#[test]
fn marked_rows_whose_files_are_gone_are_purged() {
    let (fx, mut lib) = interrupted_move();
    fs::remove_file(fx.roms.join("vrac").join("Hero.sfc")).unwrap();
    mark(&fx.db, &fx.roms.join("vrac").join("Hero.sfc"));
    let report = scan(&mut lib, &fx.roms);
    assert!(!report.cautious);
    assert_eq!(report.removed, 2);
    assert_eq!(lib.games().unwrap().len(), 0);
}

#[test]
fn a_marked_row_that_has_a_favourite_is_never_absorbed() {
    let (fx, mut lib) = interrupted_move();
    let id = keeper_of(&fx, &lib);
    lib.set_favorite(id, true).unwrap();
    mark(&fx.db, &fx.roms.join("vrac").join("Hero.sfc"));
    let report = scan(&mut lib, &fx.roms);
    assert_eq!((report.moved, report.removed), (1, 0));
    let kept = lib.game(id).unwrap().expect("marked row kept");
    assert!(kept.favorite);
    assert_eq!(lib.games().unwrap().len(), 1);
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn a_marked_row_that_has_a_cover_is_never_absorbed() {
    let (fx, mut lib) = interrupted_move();
    let id = keeper_of(&fx, &lib);
    lib.set_cover(
        id,
        CoverKind::Boxart,
        &CoverState::Missing { checked_unix: 1 },
    )
    .unwrap();
    mark(&fx.db, &fx.roms.join("vrac").join("Hero.sfc"));
    let report = scan(&mut lib, &fx.roms);
    assert_eq!((report.moved, report.removed), (1, 0));
    assert!(lib.game(id).unwrap().is_some());
    assert!(lib.cover(id, CoverKind::Boxart).unwrap().is_some());
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn a_marked_row_that_has_play_history_is_never_absorbed() {
    let (fx, mut lib) = interrupted_move();
    let id = keeper_of(&fx, &lib);
    lib.record_played(id, 1_700_000_000).unwrap();
    mark(&fx.db, &fx.roms.join("vrac").join("Hero.sfc"));
    let report = scan(&mut lib, &fx.roms);
    assert_eq!((report.moved, report.removed), (1, 0));
    let kept = lib.game(id).unwrap().expect("marked row kept");
    assert_eq!(kept.play_count, 1);
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn a_cancelled_scan_lifts_the_marks_it_left() {
    let (fx, mut lib) = interrupted_move();
    fs::write(fx.roms.join("New.sfc"), noise(1024)).unwrap();
    fs::write(fx.roms.join("New2.sfc"), noise(1500)).unwrap();
    mark(&fx.db, &fx.roms.join("vrac").join("Hero.sfc"));
    let cancel = AtomicBool::new(false);
    let opts = ScanOptions {
        roots: vec![fx.roms.clone()],
        ..Default::default()
    };
    // Cancelled once the first file has been reached: work already done is kept, but nothing stays marked.
    let r = lib.scan(&opts, &cancel, &mut |_| {
        cancel.store(true, Ordering::Relaxed)
    });
    assert!(matches!(r, Err(LibraryError::Cancelled)));
    assert_eq!(marks(&fx.db), 0);
    assert!(lib.games().unwrap().iter().any(|g| g.favorite));
}

#[test]
fn a_cancelled_scan_keeps_the_7_6_marker_and_a_finished_one_removes_it() {
    let (fx, mut lib) = interrupted_move();
    fs::write(marker(&fx.db), b"").unwrap();
    let opts = ScanOptions {
        roots: vec![fx.roms.clone()],
        ..Default::default()
    };
    let r = lib.scan(&opts, &AtomicBool::new(true), &mut |_| {});
    assert!(matches!(r, Err(LibraryError::Cancelled)));
    assert!(marker(&fx.db).exists());
    scan(&mut lib, &fx.roms);
    assert!(!marker(&fx.db).exists());
}

#[test]
fn a_cautious_scan_never_absorbs_a_row_that_has_a_favourite_or_covers() {
    // Two identical copies; the second one (the one that stays on disk) is the favourite with a cover.
    let (fx, mut lib) = interrupted_move();
    let keeper = lib
        .games()
        .unwrap()
        .into_iter()
        .find(|g| g.path == fx.roms.join("vrac").join("Hero.sfc"))
        .unwrap();
    lib.set_favorite(keeper.id, true).unwrap();
    lib.set_cover(
        keeper.id,
        CoverKind::Boxart,
        &CoverState::Missing { checked_unix: 1 },
    )
    .unwrap();
    fs::write(marker(&fx.db), b"left by a killed scan").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!(report.removed, 0);
    let kept = lib.game(keeper.id).unwrap().expect("row kept");
    assert!(kept.favorite);
    assert!(lib.cover(keeper.id, CoverKind::Boxart).unwrap().is_some());
}

fn keeper_of(fx: &Fx, lib: &Library) -> emu_library::GameId {
    lib.games()
        .unwrap()
        .into_iter()
        .find(|g| g.path == fx.roms.join("vrac").join("Hero.sfc"))
        .unwrap()
        .id
}

#[test]
fn a_cautious_scan_keeps_a_row_that_has_only_a_cover() {
    let (fx, mut lib) = interrupted_move();
    let id = keeper_of(&fx, &lib);
    lib.set_cover(
        id,
        CoverKind::Boxart,
        &CoverState::Missing { checked_unix: 1 },
    )
    .unwrap();
    fs::write(marker(&fx.db), b"left by a killed scan").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!(report.removed, 0);
    assert!(lib.game(id).unwrap().is_some(), "row kept");
    assert!(lib.cover(id, CoverKind::Boxart).unwrap().is_some());
}

#[test]
fn a_cautious_scan_keeps_a_row_that_has_only_play_history() {
    let (fx, mut lib) = interrupted_move();
    let id = keeper_of(&fx, &lib);
    lib.record_played(id, 1_700_000_000).unwrap();
    fs::write(marker(&fx.db), b"left by a killed scan").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!(report.removed, 0);
    let kept = lib.game(id).unwrap().expect("row kept");
    assert!(!kept.favorite);
    assert_eq!(kept.play_count, 1);
    assert_eq!(kept.last_played_unix, Some(1_700_000_000));
}

#[test]
fn a_cautious_scan_leaves_two_pristine_identical_copies_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    fs::create_dir_all(roms.join("vrac")).unwrap();
    fs::write(roms.join("X.sfc"), noise(4096)).unwrap();
    fs::write(roms.join("vrac").join("X.sfc"), noise(4096)).unwrap();
    let db = tmp.path().join("library.db");
    let (mut lib, _) = Library::open(&db).unwrap();
    scan(&mut lib, &roms);
    assert_eq!(lib.games().unwrap().len(), 2);
    fs::write(marker(&db), b"left by a killed scan").unwrap();
    let report = scan(&mut lib, &roms);
    assert!(report.cautious);
    assert_eq!((report.moved, report.removed), (0, 0));
    assert_eq!(lib.games().unwrap().len(), 2);
}

#[test]
fn a_cautious_scan_counts_only_its_own_additions() {
    let (fx, mut lib) = interrupted_move();
    fs::write(fx.roms.join("Other.sfc"), noise(2048)).unwrap();
    fs::write(marker(&fx.db), b"left by a killed scan").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert_eq!((report.moved, report.added), (1, 1));
}

#[test]
fn a_marker_that_cannot_be_written_or_removed_is_reported() {
    let (fx, mut lib) = interrupted_move();
    // A directory in the marker's place: it exists (cautious scan), cannot be written nor removed as a file.
    fs::create_dir(marker(&fx.db)).unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert!(report.marker_failed);
    fs::remove_dir(marker(&fx.db)).unwrap();
    assert!(!scan(&mut lib, &fx.roms).marker_failed);
}

fn old_hero(fx: &Fx, lib: &Library) -> emu_library::GameId {
    lib.games()
        .unwrap()
        .into_iter()
        .find(|g| g.path == fx.roms.join("Hero.sfc"))
        .unwrap()
        .id
}

fn hero_path(fx: &Fx) -> PathBuf {
    fx.roms.join("vrac").join("Hero.sfc")
}

#[test]
fn the_row_that_stays_takes_the_play_history_of_its_vanished_twin() {
    let (fx, mut lib) = interrupted_move();
    let old = old_hero(&fx, &lib);
    lib.record_played(old, 1_000).unwrap();
    let id = keeper_of(&fx, &lib);
    lib.record_played(id, 2_000).unwrap();
    mark(&fx.db, &hero_path(&fx));
    let report = scan(&mut lib, &fx.roms);
    assert_eq!((report.moved, report.removed), (1, 0), "a merge is a move");
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 1);
    let kept = &games[0];
    assert_eq!(kept.id, id);
    assert!(kept.favorite, "the old row was the favourite");
    assert_eq!(kept.play_count, 2);
    assert_eq!(kept.last_played_unix, Some(2_000));
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn the_latest_play_date_wins_and_a_missing_one_is_ignored() {
    let (fx, mut lib) = interrupted_move();
    let old = old_hero(&fx, &lib);
    lib.record_played(old, 9_000).unwrap();
    let id = keeper_of(&fx, &lib);
    lib.set_favorite(id, true).unwrap();
    mark(&fx.db, &hero_path(&fx));
    scan(&mut lib, &fx.roms);
    let kept = lib.game(id).unwrap().unwrap();
    assert_eq!((kept.play_count, kept.last_played_unix), (1, Some(9_000)));
}

#[test]
fn a_favourite_of_the_new_row_survives_when_the_old_one_is_not() {
    let (fx, mut lib) = interrupted_move();
    let old = old_hero(&fx, &lib);
    lib.set_favorite(old, false).unwrap();
    let id = keeper_of(&fx, &lib);
    lib.set_favorite(id, true).unwrap();
    mark(&fx.db, &hero_path(&fx));
    scan(&mut lib, &fx.roms);
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 1);
    assert!(games[0].favorite);
}

#[test]
fn the_cover_of_the_vanished_twin_is_given_when_the_new_row_has_none() {
    let (fx, mut lib) = interrupted_move();
    let old = old_hero(&fx, &lib);
    let cached = CoverState::Cached {
        path: PathBuf::from("a/old.png"),
        source: "libretro".to_owned(),
    };
    lib.set_cover(old, CoverKind::Boxart, &cached).unwrap();
    let id = keeper_of(&fx, &lib);
    lib.record_played(id, 5).unwrap();
    mark(&fx.db, &hero_path(&fx));
    scan(&mut lib, &fx.roms);
    assert_eq!(lib.games().unwrap().len(), 1);
    assert_eq!(
        lib.cover(id, CoverKind::Boxart).unwrap().unwrap().state,
        cached
    );
}

#[test]
fn the_cover_of_the_new_row_is_kept_over_the_old_one() {
    let (fx, mut lib) = interrupted_move();
    let old = old_hero(&fx, &lib);
    let cached = |p: &str| CoverState::Cached {
        path: PathBuf::from(p),
        source: "libretro".to_owned(),
    };
    lib.set_cover(old, CoverKind::Boxart, &cached("a/old.png"))
        .unwrap();
    let id = keeper_of(&fx, &lib);
    lib.set_cover(id, CoverKind::Boxart, &cached("a/new.png"))
        .unwrap();
    mark(&fx.db, &hero_path(&fx));
    scan(&mut lib, &fx.roms);
    assert_eq!(
        lib.cover(id, CoverKind::Boxart).unwrap().unwrap().state,
        cached("a/new.png")
    );
}

#[test]
fn ambiguous_twins_merge_nothing() {
    let (fx, mut lib) = interrupted_move();
    // A second vanished copy of the same content: the stayer has two possible twins.
    let copy = fx.roms.join("Hero2.sfc");
    Connection::open(&fx.db)
        .unwrap()
        .execute(
            "INSERT INTO game (path, entry, system, size, file_size, mtime_unix, crc32, sha1, title, identification,
                               pinned, is_dir)
             SELECT ?2, entry, system, size, file_size, mtime_unix, crc32, sha1, title, identification, pinned, is_dir
             FROM game WHERE path = ?1",
            [
                fx.roms.join("Hero.sfc").to_string_lossy().as_ref(),
                copy.to_string_lossy().as_ref(),
            ],
        )
        .unwrap();
    let id = keeper_of(&fx, &lib);
    lib.record_played(id, 7).unwrap();
    mark(&fx.db, &hero_path(&fx));
    let report = scan(&mut lib, &fx.roms);
    assert_eq!(report.removed, 2);
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 1);
    assert!(!games[0].favorite, "nothing was carried over");
    assert_eq!(games[0].play_count, 1);
}

#[test]
fn a_row_updated_by_this_scan_is_not_a_leftover_candidate() {
    // vrac/Hero.sfc was never marked: only its mtime changes, so this scan updates (and marks) it itself.
    let (fx, mut lib) = interrupted_move();
    let file = fs::File::options()
        .write(true)
        .open(hero_path(&fx))
        .unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(file);
    let report = scan(&mut lib, &fx.roms);
    assert_eq!(report.updated, 1);
    assert_eq!((report.moved, report.removed), (0, 1));
    assert!(lib.games().unwrap().iter().all(|g| !g.favorite));
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn an_updated_row_is_marked_while_the_scan_runs() {
    let (fx, mut lib) = interrupted_move();
    let file = fs::File::options()
        .write(true)
        .open(hero_path(&fx))
        .unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(file);
    sql(
        &fx.db,
        "CREATE TABLE written (path TEXT, mark INTEGER);
         CREATE TRIGGER w AFTER UPDATE OF scan_pending ON game BEGIN
             INSERT INTO written VALUES (NEW.path, NEW.scan_pending);
         END;",
    );
    scan(&mut lib, &fx.roms);
    let conn = Connection::open(&fx.db).unwrap();
    let first: Option<i64> = conn
        .query_row("SELECT mark FROM written ORDER BY rowid LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(first, Some(1));
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn a_scan_that_fails_keeps_its_marks_and_the_next_one_is_normal() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    fs::create_dir_all(&roms).unwrap();
    for i in 0..140usize {
        fs::write(roms.join(format!("f{i:03}.sfc")), vec![i as u8; 64 + i]).unwrap();
    }
    let db = tmp.path().join("library.db");
    let (mut lib, _) = Library::open(&db).unwrap();
    // The second batch cannot be written: the first one (128 rows) is already committed and marked.
    sql(
        &db,
        "CREATE TRIGGER boom BEFORE INSERT ON game WHEN (SELECT COUNT(*) FROM game) >= 128 BEGIN
             SELECT RAISE(ABORT, 'boom');
         END;",
    );
    let opts = ScanOptions {
        roots: vec![roms.clone()],
        ..Default::default()
    };
    let r = lib.scan(&opts, &AtomicBool::new(false), &mut |_| {});
    assert!(matches!(r, Err(LibraryError::Database(_))), "{r:?}");
    assert_eq!(marks(&db), 128);
    sql(&db, "DROP TRIGGER boom;");
    let report = scan(&mut lib, &roms);
    assert!(!report.cautious);
    assert_eq!((report.added, report.removed), (12, 0));
    assert_eq!(lib.games().unwrap().len(), 140);
    assert_eq!(marks(&db), 0);
}

#[test]
fn a_7_6_file_with_marked_rows_is_cautious_once_and_lifts_both() {
    let (fx, mut lib) = interrupted_move();
    mark(&fx.db, &hero_path(&fx));
    fs::write(marker(&fx.db), b"left by a 7.6").unwrap();
    fs::remove_file(hero_path(&fx)).unwrap();
    fs::write(fx.roms.join("Hero.sfc"), noise(4096)).unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!(report.removed, 0, "cautious: nothing purged");
    assert_eq!(marks(&fx.db), 0);
    assert!(!marker(&fx.db).exists());
    let report = scan(&mut lib, &fx.roms);
    assert!(!report.cautious);
    assert_eq!(
        report.removed, 1,
        "the vanished vrac row goes at the next scan"
    );
}

#[test]
fn a_7_6_file_with_a_marked_pristine_row_merges_the_move() {
    let (fx, mut lib) = interrupted_move();
    mark(&fx.db, &hero_path(&fx));
    fs::write(marker(&fx.db), b"left by a 7.6").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!((report.moved, report.removed), (1, 0));
    let games = lib.games().unwrap();
    assert!(games.len() == 1 && games[0].favorite);
    assert_eq!(marks(&fx.db), 0);
    assert!(!marker(&fx.db).exists());
}

#[test]
fn a_cached_cover_of_the_vanished_twin_replaces_a_missing_one_of_the_new_row() {
    let (fx, mut lib) = interrupted_move();
    let old = old_hero(&fx, &lib);
    let cached = CoverState::Cached {
        path: PathBuf::from("a/old.png"),
        source: "libretro".to_owned(),
    };
    lib.set_cover(old, CoverKind::Boxart, &cached).unwrap();
    let id = keeper_of(&fx, &lib);
    lib.set_cover(
        id,
        CoverKind::Boxart,
        &CoverState::Missing { checked_unix: 1 },
    )
    .unwrap();
    mark(&fx.db, &hero_path(&fx));
    scan(&mut lib, &fx.roms);
    assert_eq!(lib.games().unwrap().len(), 1);
    assert_eq!(
        lib.cover(id, CoverKind::Boxart).unwrap().unwrap().state,
        cached
    );
}

#[test]
fn a_missing_cover_of_the_vanished_twin_never_replaces_a_cached_one() {
    let (fx, mut lib) = interrupted_move();
    let old = old_hero(&fx, &lib);
    lib.set_cover(
        old,
        CoverKind::Boxart,
        &CoverState::Missing { checked_unix: 1 },
    )
    .unwrap();
    let id = keeper_of(&fx, &lib);
    let cached = CoverState::Cached {
        path: PathBuf::from("a/new.png"),
        source: "libretro".to_owned(),
    };
    lib.set_cover(id, CoverKind::Boxart, &cached).unwrap();
    mark(&fx.db, &hero_path(&fx));
    scan(&mut lib, &fx.roms);
    assert_eq!(
        lib.cover(id, CoverKind::Boxart).unwrap().unwrap().state,
        cached
    );
}

/// A killed scan left the new side of a move marked and carrying data (a play); the old row (the favourite) is
/// still stored and its file is gone.
fn killed_with_data() -> (Fx, Library, emu_library::GameId) {
    let (fx, mut lib) = interrupted_move();
    let id = keeper_of(&fx, &lib);
    lib.record_played(id, 2_000).unwrap();
    mark(&fx.db, &hero_path(&fx));
    (fx, lib, id)
}

fn assert_data_merged(fx: &Fx, lib: &Library, id: emu_library::GameId) {
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 1, "no row lost");
    let kept = &games[0];
    assert_eq!(kept.id, id);
    assert!(kept.favorite, "the favourite of the vanished twin survives");
    assert_eq!(kept.play_count, 1);
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn a_cancelled_scan_keeps_the_mark_of_a_row_that_has_data_so_the_next_scan_merges() {
    let (fx, mut lib, id) = killed_with_data();
    fs::write(fx.roms.join("New.sfc"), noise(1024)).unwrap();
    fs::write(fx.roms.join("New2.sfc"), noise(1500)).unwrap();
    let cancel = AtomicBool::new(false);
    let opts = ScanOptions {
        roots: vec![fx.roms.clone()],
        ..Default::default()
    };
    let r = lib.scan(&opts, &cancel, &mut |_| {
        cancel.store(true, Ordering::Relaxed)
    });
    assert!(matches!(r, Err(LibraryError::Cancelled)));
    assert_eq!(marks(&fx.db), 1, "the row with data keeps its mark");
    let paths: Vec<_> = lib.games().unwrap().into_iter().map(|g| g.path).collect();
    assert!(
        paths.contains(&fx.roms.join("Hero.sfc")),
        "nothing purged by a cancelled scan"
    );
    let report = scan(&mut lib, &fx.roms);
    assert_eq!((report.moved, report.removed), (1, 0));
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 3, "the two new files and the merged game");
    let kept = lib.game(id).unwrap().expect("row kept");
    assert!(kept.favorite);
    assert_eq!(kept.play_count, 1);
    assert_eq!(marks(&fx.db), 0);
}

#[test]
fn a_cancelled_scan_still_lifts_the_marks_of_pristine_rows() {
    let (fx, mut lib, _) = killed_with_data();
    fs::write(fx.roms.join("Plain.sfc"), noise(1024)).unwrap();
    fs::write(fx.roms.join("Plain2.sfc"), noise(1500)).unwrap();
    let cancel = AtomicBool::new(false);
    let opts = ScanOptions {
        roots: vec![fx.roms.clone()],
        ..Default::default()
    };
    let r = lib.scan(&opts, &cancel, &mut |_| {
        cancel.store(true, Ordering::Relaxed)
    });
    assert!(matches!(r, Err(LibraryError::Cancelled)));
    // Only the row with data stays marked; whatever the cancelled scan itself stored is lifted.
    assert_eq!(marks(&fx.db), 1);
}

#[test]
fn a_cautious_scan_keeps_the_mark_of_a_row_that_has_data_so_the_next_scan_merges() {
    let (fx, mut lib, id) = killed_with_data();
    fs::write(marker(&fx.db), b"left by a 7.6").unwrap();
    let report = scan(&mut lib, &fx.roms);
    assert!(report.cautious);
    assert_eq!(report.removed, 0);
    assert_eq!(lib.games().unwrap().len(), 2, "cautious: nothing purged");
    assert_eq!(marks(&fx.db), 1, "the row with data keeps its mark");
    let report = scan(&mut lib, &fx.roms);
    assert!(!report.cautious);
    assert_eq!((report.moved, report.removed), (1, 0));
    assert_data_merged(&fx, &lib, id);
}
