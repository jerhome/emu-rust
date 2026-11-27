//! « Oublier ses jeux » (ADR 0025 §9): the only way rows of a system are removed on request.

use super::*;

fn count(lib: &Library, table: &str) -> i64 {
    lib.conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn count_where(lib: &Library, table: &str, clause: &str) -> i64 {
    lib.conn
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {clause}"),
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn game(lib: &Library, path: &str, system: &str) -> i64 {
    lib.conn
        .execute(
            "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification, favorite)
             VALUES (?1, ?2, 10, 10, 1, 'T', 'unknown', 1)",
            params![path, system],
        )
        .unwrap();
    lib.conn.last_insert_rowid()
}

fn dat(lib: &Library, system: &str) {
    lib.conn
        .execute(
            "INSERT INTO dat_info (system, source, name, entries, imported_unix) VALUES (?1, 'no-intro', 'N', 1, 1)",
            [system],
        )
        .unwrap();
    lib.conn
        .execute(
            "INSERT INTO dat_entry (system, source, game, rom_name, size, crc32) VALUES (?1, 'no-intro', 'G', 'r', 1, 1)",
            [system],
        )
        .unwrap();
}

/// Systems \`ghost\` (two games, covers, disks, DAT), \`gh\` and \`gb\` (one game each, DAT).
fn seeded() -> Library {
    let lib = Library::open_in_memory().unwrap();
    let g1 = game(&lib, "r-a.x", "ghost");
    game(&lib, "r-b.x", "ghost");
    game(&lib, "r-c.x", "gh");
    game(&lib, "r-d.gb", "gb");
    lib.conn
        .execute(
            "INSERT INTO cover (game, kind, state, path, source, checked_unix) VALUES (?1, 'boxart', 'cached', 'a/b.png', 'libretro', 1)",
            [g1],
        )
        .unwrap();
    lib.conn
        .execute(
            "INSERT INTO game_media (game_id, idx, path) VALUES (?1, 0, 'r-disk0.x')",
            [g1],
        )
        .unwrap();
    for s in ["ghost", "gh", "gb"] {
        dat(&lib, s);
    }
    lib
}

#[test]
fn forgetting_a_system_removes_its_games_covers_disks_and_dats() {
    let mut lib = seeded();
    lib.forget_system("ghost", ForgetDats::Remove).unwrap();
    assert_eq!(count_where(&lib, "game", "system = 'ghost'"), 0);
    assert_eq!(count(&lib, "game"), 2);
    assert_eq!(count(&lib, "cover"), 0);
    assert_eq!(count(&lib, "game_media"), 0);
    assert_eq!(count_where(&lib, "dat_info", "system = 'ghost'"), 0);
    assert_eq!(count_where(&lib, "dat_entry", "system = 'ghost'"), 0);
    // Exact id only: \`gh\` and \`gb\` keep everything.
    assert_eq!(count(&lib, "dat_info"), 2);
    assert_eq!(count(&lib, "dat_entry"), 2);
    assert_eq!(count_where(&lib, "game", "system IN ('gh', 'gb')"), 2);
}

#[test]
fn keeping_the_dats_removes_the_games_only() {
    // An uncertain conflict: the DAT rows are the official's (ADR 0025 §9).
    let mut lib = seeded();
    lib.forget_system("ghost", ForgetDats::Keep).unwrap();
    assert_eq!(count_where(&lib, "game", "system = 'ghost'"), 0);
    assert_eq!(count_where(&lib, "dat_info", "system = 'ghost'"), 1);
    assert_eq!(count_where(&lib, "dat_entry", "system = 'ghost'"), 1);
    assert_eq!(count(&lib, "cover"), 0);
}

#[test]
fn forgetting_an_unknown_system_changes_nothing() {
    let mut lib = seeded();
    lib.forget_system("nothere", ForgetDats::Remove).unwrap();
    assert_eq!(
        (
            count(&lib, "game"),
            count(&lib, "dat_info"),
            count(&lib, "cover")
        ),
        (4, 3, 1)
    );
}
