//! A base written by the 7.4 (schema v7) or by the PC 7.1/7.2 line (schema v6) holds `megacd` rows with
//! `needs_32x` false or never computed; the 7.5 name rules are re-evaluated at the next scan without rewriting the
//! file (design v7.5 §2): the new forms mark the row, a bare `(32X)` and ordinary discs do not.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use emu_library::{Game, Library, ScanOptions};
use rusqlite::Connection;

fn scan(lib: &mut Library, root: &Path) {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    let report = lib
        .scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
}

fn game(lib: &Library, file: &str) -> Game {
    lib.games()
        .unwrap()
        .into_iter()
        .find(|g| g.path.file_name().is_some_and(|n| n == file))
        .unwrap_or_else(|| panic!("{file} not listed"))
}

const TAGGED: [&str; 3] = [
    "Night Trap (CD 32X).chd",
    "Corpse Killer (Mega-CD 32X).chd",
    "Fahrenheit (sega cd 32x).chd",
];
const PLAIN: [&str; 2] = ["Sonic CD (USA).chd", "Bare 32X Demo (32X).chd"];

fn fill(root: &Path) {
    fs::create_dir_all(root.join("megacd")).unwrap();
    for f in TAGGED.iter().chain(&PLAIN) {
        fs::write(root.join("megacd").join(f), b"not read").unwrap();
    }
}

fn reset(db: &Path, value: &str, version: i64) {
    let conn = Connection::open(db).unwrap();
    conn.execute(&format!("UPDATE game SET needs_32x = {value}"), [])
        .unwrap();
    // Back to the layout of `version` (7 or 6): v8 added `scan_pending`.
    conn.execute_batch("ALTER TABLE game DROP COLUMN scan_pending;")
        .unwrap();
    conn.pragma_update(None, "user_version", version).unwrap();
}

fn check(db: &Path, root: &Path) {
    let (mut lib, _) = Library::open(db).unwrap();
    scan(&mut lib, root);
    for f in TAGGED {
        assert!(game(&lib, f).needs_32x, "{f}");
    }
    for f in PLAIN {
        assert!(!game(&lib, f).needs_32x, "{f}");
    }
}

#[test]
fn a_7_4_base_with_false_flags_is_remarked_by_the_name_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("roms");
    let db = dir.path().join("library.sqlite");
    fill(&root);
    {
        let (mut lib, _) = Library::open(&db).unwrap();
        scan(&mut lib, &root);
    }
    reset(&db, "0", 7);
    check(&db, &root);
}

#[test]
fn a_v6_base_with_never_computed_flags_is_remarked_by_the_name_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("roms");
    let db = dir.path().join("library.sqlite");
    fill(&root);
    {
        let (mut lib, _) = Library::open(&db).unwrap();
        scan(&mut lib, &root);
    }
    reset(&db, "NULL", 6);
    check(&db, &root);
    let version: i64 = Connection::open(&db)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 8);
}
