//! The "scan in progress" marker (7.6): a scan killed between two batches leaves it, and the next scan then
//! merges moved files but never purges, so a favourite is not lost with the old row of a file moved by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use emu_library::{CoverKind, CoverState, Library, LibraryError, ScanOptions, ScanReport};

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
fn the_marker_exists_during_a_scan_and_goes_with_a_normal_end() {
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
    assert!(seen);
    assert!(!marker(&fx.db).exists());
}

#[test]
fn a_clean_cancellation_removes_the_marker() {
    let (fx, mut lib) = interrupted_move();
    let opts = ScanOptions {
        roots: vec![fx.roms.clone()],
        ..Default::default()
    };
    let r = lib.scan(&opts, &AtomicBool::new(true), &mut |_| {});
    assert!(matches!(r, Err(LibraryError::Cancelled)));
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
