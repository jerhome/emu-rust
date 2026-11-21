//! A row stored by a pre-7.0 scan for a secondary track (`Game (Track 2).bin`) is purged by the 7.0 rescan.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use emu_library::{Library, ScanOptions};

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

#[test]
#[ignore = "needs scripts/fetch-test-assets.sh --with-7.0"]
fn old_track_row_is_removed_at_rescan() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let db = tmp.path().join("lib.db");
    // Real free PlayStation disc (240p Test Suite, fetched by scripts/fetch-test-assets.sh --with-7.0), renamed.
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/roms/psx/240pTestSuitePS1-EMU.bin");
    let Ok(track1) = fs::read(&src) else {
        eprintln!("skipped: {} absent", src.display());
        return;
    };
    write(&root.join("Jeu (Track 1).bin"), &track1);
    write(&root.join("Jeu (Track 2).bin"), &[3u8; 2352 * 4]);
    write(
        &root.join("Jeu.cue"),
        b"FILE \"Jeu (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\nFILE \"Jeu (Track 2).bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n",
    );
    let opts = ScanOptions {
        roots: vec![root.clone()],
        ..Default::default()
    };
    let (mut lib, _) = Library::open(&db).unwrap();
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let cue_rows = lib.games().unwrap();
    assert_eq!(cue_rows.len(), 1, "{cue_rows:?}");
    drop(lib);

    // Simulate the 6.0 database: the secondary track was stored as a game of its own.
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "INSERT INTO game (path, entry, system, size, file_size, mtime_unix, title, identification) \
         SELECT ?1, NULL, system, 9408, 9408, mtime_unix, 'Jeu (Track 2)', identification FROM game LIMIT 1",
        [root.join("Jeu (Track 2).bin").to_string_lossy().as_ref()],
    )
    .unwrap();
    drop(conn);

    let (mut lib, _) = Library::open(&db).unwrap();
    assert_eq!(lib.games().unwrap().len(), 2);
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 1, "{games:?}");
    assert_eq!(games[0].path.file_name().unwrap(), "Jeu.cue");
}
