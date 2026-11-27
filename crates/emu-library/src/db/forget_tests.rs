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
    lib.forget_system("ghost", ForgetDats::Remove, ForgetScope::All)
        .unwrap();
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
    lib.forget_system("ghost", ForgetDats::Keep, ForgetScope::All)
        .unwrap();
    assert_eq!(count_where(&lib, "game", "system = 'ghost'"), 0);
    assert_eq!(count_where(&lib, "dat_info", "system = 'ghost'"), 1);
    assert_eq!(count_where(&lib, "dat_entry", "system = 'ghost'"), 1);
    assert_eq!(count(&lib, "cover"), 0);
}

#[test]
fn forgetting_an_unknown_system_changes_nothing() {
    let mut lib = seeded();
    lib.forget_system("nothere", ForgetDats::Remove, ForgetScope::All)
        .unwrap();
    assert_eq!(
        (
            count(&lib, "game"),
            count(&lib, "dat_info"),
            count(&lib, "cover")
        ),
        (4, 3, 1)
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Scopes of « Oublier ses jeux » and of the switch end (ADR 0025 §9, §10)

fn definition_text(id: &str, database: Option<&str>, extensions: &[&str]) -> String {
    let database = database.map_or(String::new(), |d| format!("database = \"{d}\"\n"));
    let list: Vec<String> = extensions.iter().map(|e| format!("\"{e}\"")).collect();
    format!(
        "format = 1\nrender = \"software\"\n\n[system]\nid = \"{id}\"\nname = \"Sys {id}\"\n{database}\
         extensions = [{}]\ndat = []\n\n[cores]\nlist = [\"demo_core\"]\nignored_firmware = []\n\n\
         [origin]\ncore = \"demo_core\"\ncreated_by = \"8.0.0\"\ncreated_unix = 1\n",
        list.join(", ")
    )
}

/// `snes.toml` taking the id of the official Super Nintendo with another database: an uncertain conflict.
fn catalog_with(
    id: &str,
    database: Option<&str>,
    extensions: &[&str],
) -> (tempfile::TempDir, Catalog) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{id}.toml")),
        definition_text(id, database, extensions),
    )
    .unwrap();
    let loaded = Catalog::load(&dir);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    (tmp, loaded.catalog)
}

fn row(lib: &Library, path: &str, system: &str, entry: Option<&str>) -> i64 {
    lib.conn
        .execute(
            "INSERT INTO game (path, entry, system, size, file_size, mtime_unix, title, identification,
                               favorite, play_count)
             VALUES (?1, ?2, ?3, 10, 10, 1, 'T', 'unknown', 1, 7)",
            params![path, entry, system],
        )
        .unwrap();
    lib.conn.last_insert_rowid()
}

fn systems_of(lib: &Library, ids: &[i64]) -> Vec<Option<String>> {
    ids.iter()
        .map(|id| {
            lib.conn
                .query_row("SELECT system FROM game WHERE id = ?1", [id], |r| r.get(0))
                .ok()
        })
        .collect()
}

#[test]
fn claimed_by_forgets_only_the_rows_the_definition_claims_and_the_official_does_not_accept() {
    let (_tmp, cat) = catalog_with("snes", Some("Demo - Not A Snes"), &["xyz", "sfc", "zzz"]);
    assert!(cat.is_conflicted("snes"));
    let mut lib = Library::open_in_memory().unwrap();
    let xyz = row(&lib, "r/Mine.xyz", "snes", None);
    let packed = row(&lib, "r/Pack.zip", "snes", Some("inside.xyz"));
    let shared = row(&lib, "r/Shared.sfc", "snes", None);
    let unknown_entry = row(&lib, "r/Odd.zip", "snes", None);
    let other = row(&lib, "r/Other.xyz", "gb", None);
    dat(&lib, "snes");
    lib.forget_system("snes", ForgetDats::Keep, ForgetScope::ClaimedBy(&cat))
        .unwrap();
    assert_eq!(
        systems_of(&lib, &[xyz, packed, shared, unknown_entry, other]),
        [
            None,
            None,
            Some("snes".into()),
            Some("snes".into()),
            Some("gb".into())
        ]
    );
    // The favourite and play count of the kept rows are untouched, the DAT rows are the official's.
    assert_eq!(
        count_where(&lib, "game", "favorite = 1 AND play_count = 7"),
        3
    );
    assert_eq!(count_where(&lib, "dat_info", "system = 'snes'"), 1);
}

/// A custom definition on the id of the official system, with the database of another one: the rows of disc files
/// (`.cue`, `.chd`) that the official accepts and the definition does not claim stay, with favourite and history.
#[test]
fn claimed_by_keeps_the_disc_rows_the_official_accepts_with_favourite_and_history() {
    let (_tmp, cat) = catalog_with("megacd", Some("Demo - Not A Mega CD"), &["xyz"]);
    assert!(cat.is_conflicted("megacd"));
    let mut lib = Library::open_in_memory().unwrap();
    let cue = row(&lib, "r/Game.cue", "megacd", None);
    let chd = row(&lib, "r/Game2.chd", "megacd", None);
    let mine = row(&lib, "r/Mine.xyz", "megacd", None);
    lib.conn
        .execute("UPDATE game SET last_played_unix = 99", [])
        .unwrap();
    lib.forget_system("megacd", ForgetDats::Keep, ForgetScope::ClaimedBy(&cat))
        .unwrap();
    assert_eq!(
        systems_of(&lib, &[cue, chd, mine]),
        [Some("megacd".into()), Some("megacd".into()), None]
    );
    assert_eq!(
        count_where(
            &lib,
            "game",
            "favorite = 1 AND play_count = 7 AND last_played_unix = 99"
        ),
        2
    );
}

#[test]
fn claimed_by_is_refused_outside_an_uncertain_conflict() {
    let mut lib = Library::open_in_memory().unwrap();
    row(&lib, "r/Mine.xyz", "snes", None);
    // Official catalog: no custom definition.
    let official = Catalog::official();
    assert!(
        lib.forget_system("snes", ForgetDats::Keep, ForgetScope::ClaimedBy(&official))
            .is_err()
    );
    // A custom system that masks nothing of that id.
    let (_tmp, cat) = catalog_with("dmo", Some("Demo - Dmo"), &["dmo"]);
    assert!(
        lib.forget_system("dmo", ForgetDats::Keep, ForgetScope::ClaimedBy(&cat))
            .is_err()
    );
    assert_eq!(count(&lib, "game"), 1);
}

#[test]
fn frozen_forgets_only_the_rows_the_active_official_does_not_accept() {
    // Switch accepted in place: `.retired/` keeps a Replaced definition of the id, the official is active again.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    std::fs::create_dir_all(dir.join(".retired")).unwrap();
    std::fs::write(
        dir.join(".retired").join("snes-100.replaced.toml"),
        definition_text(
            "snes",
            Some("Nintendo - Super Nintendo Entertainment System"),
            &["sfc", "xyz"],
        ),
    )
    .unwrap();
    let cat = Catalog::load(&dir).catalog;
    assert!(cat.is_conflicted("snes"));
    let mut lib = Library::open_in_memory().unwrap();
    let kept = row(&lib, "r/Game.sfc", "snes", None);
    let kept_zip = row(&lib, "r/Game.zip", "snes", Some("g.smc"));
    let xyz = row(&lib, "r/Mine.xyz", "snes", None);
    let zxyz = row(&lib, "r/Pack.zip", "snes", Some("a.xyz"));
    let no_entry = row(&lib, "r/Odd.zip", "snes", None);
    dat(&lib, "snes");
    lib.forget_system("snes", ForgetDats::Keep, ForgetScope::Frozen(&cat))
        .unwrap();
    assert_eq!(
        systems_of(&lib, &[kept, kept_zip, xyz, zxyz, no_entry]),
        [Some("snes".into()), Some("snes".into()), None, None, None]
    );
    assert_eq!(count_where(&lib, "dat_info", "system = 'snes'"), 1);
    // Not in conflict: nothing is frozen, nothing goes.
    let plain = Catalog::official();
    lib.forget_system("snes", ForgetDats::Keep, ForgetScope::Frozen(&plain))
        .unwrap();
    assert_eq!(count_where(&lib, "game", "system = 'snes'"), 2);
}

#[test]
fn reassign_all_moves_every_row_and_is_idempotent() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = row(&lib, "r/a.xyz", "mine", None);
    let b = row(&lib, "r/b.sfc", "mine", None);
    let c = row(&lib, "r/c.sfc", "other", None);
    assert_eq!(
        lib.reassign_system("mine", "snes", ReassignScope::All)
            .unwrap(),
        2
    );
    assert_eq!(
        systems_of(&lib, &[a, b, c]),
        [
            Some("snes".into()),
            Some("snes".into()),
            Some("other".into())
        ]
    );
    assert_eq!(
        lib.reassign_system("mine", "snes", ReassignScope::All)
            .unwrap(),
        0
    );
    // Data stay on the rows.
    assert_eq!(
        count_where(&lib, "game", "favorite = 1 AND play_count = 7"),
        3
    );
}

#[test]
fn reassign_accepted_by_leaves_the_rows_the_official_does_not_accept() {
    let official = Catalog::official();
    let snes = official.system("snes").unwrap();
    let mut lib = Library::open_in_memory().unwrap();
    let sfc = row(&lib, "r/a.sfc", "mysnes", None);
    let zipped = row(&lib, "r/b.zip", "mysnes", Some("x.smc"));
    let xyz = row(&lib, "r/c.xyz", "mysnes", None);
    let zxyz = row(&lib, "r/d.zip", "mysnes", Some("y.xyz"));
    let n = lib
        .reassign_system("mysnes", "snes", ReassignScope::AcceptedBy(snes))
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(
        systems_of(&lib, &[sfc, zipped, xyz, zxyz]),
        [
            Some("snes".into()),
            Some("snes".into()),
            Some("mysnes".into()),
            Some("mysnes".into())
        ]
    );
    assert_eq!(
        lib.switch_frozen_rows("mysnes", snes).unwrap(),
        ["r/c.xyz", "r/d.zip"]
    );
    assert!(lib.switch_frozen_rows("snes", snes).unwrap().is_empty());
}
