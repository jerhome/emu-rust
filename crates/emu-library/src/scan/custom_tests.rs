//! Scan with custom systems: non-regression of the official systems, games under a custom folder, rows of
//! a system absent from the snapshot. Catalogs come from `Catalog::load` and go to `run_in`: nothing here publishes
//! a catalog (ADR 0025 §1).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;
use crate::custom::{CustomSystemDraft, DefinitionOrigin, write_custom};
use crate::{DatSource, Library};

fn leak(catalog: Catalog) -> &'static Catalog {
    Box::leak(Box::new(catalog))
}

fn official() -> &'static Catalog {
    leak(Catalog::official())
}

fn draft(id: &str, database: Option<&str>, extensions: &[&str]) -> CustomSystemDraft {
    CustomSystemDraft {
        id: id.into(),
        name: format!("Custom {id}"),
        database: database.map(str::to_owned),
        extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        dats: vec![DatSource::NoIntro],
        screenscraper: None,
        igdb: None,
        cores: vec!["demo_core".into()],
        ignored_firmware: Vec::new(),
        origin: DefinitionOrigin {
            core: "demo_core".into(),
            info_name: None,
            created_by: "8.0.0".into(),
            created_unix: 1,
        },
    }
}

/// Five custom systems; four of them reuse extensions of official systems (they only count under their folder).
fn five_customs(dir: &Path) -> &'static Catalog {
    let drafts = [
        draft("dmoone", Some("Demo - One"), &["dmo", "bin"]),
        draft("dmotwo", Some("Demo - Two"), &["gb", "sfc", "md"]),
        draft("dmothree", None, &["nes", "gba", "gg", "sms"]),
        draft("dmofour", Some("Demo - Four"), &["st", "adf", "pce"]),
        draft(
            "dmofive",
            Some("Demo - Five"),
            &["a26", "rom", "bin", "gen"],
        ),
    ];
    for d in &drafts {
        let catalog = Catalog::load(dir).catalog;
        write_custom(dir, d, &catalog).unwrap();
    }
    let loaded = Catalog::load(dir);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    assert_eq!(loaded.catalog.customs().count(), 5);
    leak(loaded.catalog)
}

fn blob(seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..4096)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

fn put(path: &Path, seed: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, blob(seed)).unwrap();
}

fn zip_of(path: &Path, entries: &[(&str, Vec<u8>)]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        w.start_file(*name, options).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
}

fn scan_in(lib: &mut Library, cat: &'static Catalog, root: &Path) -> ScanReport {
    let options = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..ScanOptions::default()
    };
    run_in(
        cat,
        &mut lib.conn,
        &options,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap()
}

/// `path relative to root -> (system, identification, pinned)` of every row, hidden ones included.
fn rows(lib: &Library, root: &Path) -> BTreeMap<String, (String, String, bool)> {
    let mut stmt = lib
        .conn
        .prepare("SELECT path, system, identification, pinned FROM game")
        .unwrap();
    stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            (
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)? != 0,
            ),
        ))
    })
    .unwrap()
    .map(|r| {
        let (path, v) = r.unwrap();
        let rel = Path::new(&path)
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        (rel, v)
    })
    .collect()
}

fn roms(tmp: &tempfile::TempDir) -> PathBuf {
    fs::canonicalize(tmp.path()).unwrap().join("roms")
}

/// For every extension of every official system: a file under the folder of the system and one under a folder
/// that names no system.
fn official_tree(roms: &Path) {
    let mut seed = 1;
    for system in Catalog::official().systems() {
        for ext in &system.extensions {
            seed += 1;
            put(&roms.join(&system.id).join(format!("f.{ext}")), seed);
            seed += 1;
            put(&roms.join("misc").join(format!("g.{ext}")), seed);
        }
    }
}

#[test]
fn five_custom_systems_change_neither_the_attribution_nor_the_report_of_official_files() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    official_tree(&roms);
    let systems = tempfile::tempdir().unwrap();
    let with = five_customs(systems.path());

    let mut plain = Library::open_in_memory().unwrap();
    let plain_report = scan_in(&mut plain, official(), &roms);
    let mut custom = Library::open_in_memory().unwrap();
    let custom_report = scan_in(&mut custom, with, &roms);

    let plain_rows = rows(&plain, &roms);
    assert!(plain_rows.len() > 30, "{} rows", plain_rows.len());
    assert_eq!(rows(&custom, &roms), plain_rows);
    assert_eq!(custom_report, plain_report);
    assert!(
        plain_report.unclassified > 0,
        "the tree must exercise the unclassified list"
    );
}

#[test]
fn a_game_under_the_folder_of_a_custom_system_belongs_to_it() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    put(&roms.join("dmoone").join("Game.dmo"), 1);
    zip_of(
        &roms.join("dmoone").join("Packed.zip"),
        &[("inner.dmo", blob(2))],
    );
    // `.bin` is an extension of the Mega Drive too: the folder decides.
    put(&roms.join("dmoone").join("Raw.bin"), 3);
    put(&roms.join("megadrive").join("Raw.bin"), 4);
    put(&roms.join("dmotwo").join("Hero.gb"), 5);
    put(&roms.join("gb").join("Hero.gb"), 6);
    put(&roms.join("misc").join("Loose.bin"), 7);
    let systems = tempfile::tempdir().unwrap();
    let with = five_customs(systems.path());
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan_in(&mut lib, with, &roms);
    let rows = rows(&lib, &roms);
    let system = |p: &str| {
        rows.get(p)
            .unwrap_or_else(|| panic!("{p}: {rows:?}"))
            .0
            .as_str()
    };
    assert_eq!(system("dmoone/Game.dmo"), "dmoone");
    assert_eq!(system("dmoone/Packed.zip"), "dmoone");
    assert_eq!(system("dmoone/Raw.bin"), "dmoone");
    assert_eq!(system("megadrive/Raw.bin"), "megadrive");
    assert_eq!(system("dmotwo/Hero.gb"), "dmotwo");
    assert_eq!(system("gb/Hero.gb"), "gb");
    // Outside any folder the official rule is unchanged (same as without customs, see the test above).
    assert_eq!(system("misc/Loose.bin"), "megadrive");
    assert!(rows["dmoone/Game.dmo"].2, "pinned by its folder");
    assert_eq!((report.unclassified, report.errors.len()), (0, 0));
}

#[test]
fn an_extension_proper_to_a_custom_system_is_unclassified_outside_its_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    put(&roms.join("misc").join("Loose.dmo"), 1);
    put(&roms.join("dmoone").join("Game.dmo"), 2);
    let systems = tempfile::tempdir().unwrap();
    let with = five_customs(systems.path());
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan_in(&mut lib, with, &roms);
    assert_eq!(report.unclassified, 1);
    assert_eq!(
        report.unclassified_paths,
        [roms.join("misc").join("Loose.dmo")]
    );
    assert_eq!(
        rows(&lib, &roms).keys().collect::<Vec<_>>(),
        ["dmoone/Game.dmo"]
    );
    // Without the custom system the file is not even a candidate: the report is the 7.x one.
    let mut plain = Library::open_in_memory().unwrap();
    assert_eq!(scan_in(&mut plain, official(), &roms).unclassified, 0);
}

#[test]
fn rows_of_a_system_absent_from_the_snapshot_are_never_modified_or_purged() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    put(&roms.join("dmoone").join("Game.dmo"), 1);
    put(&roms.join("dmoone").join("Raw.bin"), 2);
    put(&roms.join("dmotwo").join("Hero.gb"), 3);
    put(&roms.join("gb").join("Plain.gb"), 4);
    let systems = tempfile::tempdir().unwrap();
    let with = five_customs(systems.path());
    let mut lib = Library::open_in_memory().unwrap();
    scan_in(&mut lib, with, &roms);
    let before = rows(&lib, &roms);
    assert_eq!(before.len(), 4);
    let hero_id: i64 = lib
        .conn
        .query_row("SELECT id FROM game WHERE path LIKE '%Hero.gb'", [], |r| {
            r.get(0)
        })
        .unwrap();
    lib.set_favorite(crate::GameId(hero_id), true).unwrap();

    // The customs are gone from the snapshot (retired, in error, folder unreadable): their rows stay as they are,
    // even for files that are still on disk with an official extension.
    let report = scan_in(&mut lib, official(), &roms);
    assert_eq!((report.removed, report.added, report.updated), (0, 0, 0));
    assert_eq!(rows(&lib, &roms), before);
    assert_eq!(
        lib.game(crate::GameId(hero_id)).unwrap().unwrap().system,
        "dmotwo"
    );
    assert!(lib.game(crate::GameId(hero_id)).unwrap().unwrap().favorite);

    // Their files disappear: still no purge while the system is absent.
    fs::remove_file(roms.join("dmoone").join("Game.dmo")).unwrap();
    fs::remove_file(roms.join("dmotwo").join("Hero.gb")).unwrap();
    let report = scan_in(&mut lib, official(), &roms);
    assert_eq!(report.removed, 0);
    assert_eq!(rows(&lib, &roms), before);

    // Hidden while absent, back with the system.
    let shown = |lib: &Library, cat: &Catalog| -> Vec<String> {
        let mut v: Vec<String> = lib
            .games_in(cat)
            .unwrap()
            .into_iter()
            .map(|g| g.system)
            .collect();
        v.sort();
        v
    };
    assert_eq!(shown(&lib, official()), ["gb"]);
    assert_eq!(shown(&lib, with), ["dmoone", "dmoone", "dmotwo", "gb"]);

    // With the systems back, the vanished files are purged as usual.
    let report = scan_in(&mut lib, with, &roms);
    assert_eq!(report.removed, 2);
    assert_eq!(rows(&lib, &roms).len(), 2);
}

#[test]
fn a_system_recreated_with_the_same_id_finds_its_rows_again() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    put(&roms.join("dmoone").join("Game.dmo"), 1);
    let systems = tempfile::tempdir().unwrap();
    let with = five_customs(systems.path());
    let mut lib = Library::open_in_memory().unwrap();
    scan_in(&mut lib, with, &roms);
    let id: i64 = lib
        .conn
        .query_row("SELECT id FROM game", [], |r| r.get(0))
        .unwrap();
    lib.set_favorite(crate::GameId(id), true).unwrap();

    crate::custom::retire_custom(
        systems.path(),
        "dmoone",
        crate::custom::Retirement::Removed,
        5,
    )
    .unwrap();
    let without = leak(Catalog::load(systems.path()).catalog);
    assert!(without.system("dmoone").is_none());
    assert!(lib.games_in(without).unwrap().is_empty());
    let report = scan_in(&mut lib, without, &roms);
    assert_eq!((report.removed, report.added), (0, 0));

    // Restored: the same row, with its favourite.
    let retired = systems.path().join(".retired").join("dmoone-5.toml");
    crate::custom::restore_custom(
        systems.path(),
        &retired,
        &Catalog::load(systems.path()).catalog,
    )
    .unwrap();
    let back = leak(Catalog::load(systems.path()).catalog);
    let games = lib.games_in(back).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!((games[0].id.0, games[0].favorite), (id, true));
}

#[test]
fn hidden_rows_do_not_ask_for_covers() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    put(&roms.join("gb").join("Plain.gb"), 1);
    put(&roms.join("dmoone").join("Game.dmo"), 2);
    let systems = tempfile::tempdir().unwrap();
    let with = five_customs(systems.path());
    let mut lib = Library::open_in_memory().unwrap();
    scan_in(&mut lib, with, &roms);
    // The published catalog is the official one here: the custom row is hidden, so no cover is looked for.
    let wanted = lib
        .games_needing_cover(crate::CoverKind::Boxart, i64::MAX)
        .unwrap();
    let systems_of: Vec<String> = wanted
        .iter()
        .map(|id| lib.game(*id).unwrap().unwrap().system)
        .collect();
    assert_eq!(systems_of, ["gb"]);
}

/// Text of a definition file written by hand (`write_custom` refuses an id of an official system).
fn definition_text(id: &str, database: Option<&str>, ext: &str) -> String {
    let database = database.map_or(String::new(), |d| format!("database = \"{d}\"\n"));
    format!(
        "format = 1\nrender = \"software\"\n\n[system]\nid = \"{id}\"\nname = \"Sys {id}\"\n{database}\
         extensions = [\"{ext}\"]\ndat = []\n\n[cores]\nlist = [\"demo_core\"]\nignored_firmware = []\n\n\
         [origin]\ncore = \"demo_core\"\ncreated_by = \"8.0.0\"\ncreated_unix = 1\n"
    )
}

/// A row of `system` at `path` with a favourite and a play history (what a purge would lose).
fn seed_row(lib: &Library, path: &Path, system: &str, entry: Option<&str>) -> i64 {
    lib.conn
        .execute(
            "INSERT INTO game (path, entry, system, size, file_size, mtime_unix, title, identification,
                               favorite, play_count, last_played_unix)
             VALUES (?1, ?2, ?3, 10, 10, 1, 'T', 'unknown', 1, 7, 99)",
            params![path.to_string_lossy(), entry, system],
        )
        .unwrap();
    lib.conn.last_insert_rowid()
}

fn row_data(lib: &Library, id: i64) -> Option<(String, i64, i64)> {
    lib.conn
        .query_row(
            "SELECT system, favorite, play_count FROM game WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()
}

#[test]
fn a_definition_taking_an_official_id_never_costs_the_rows_of_that_official() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    let systems = tmp.path().join("systems");
    fs::create_dir_all(&systems).unwrap();
    // psx.toml: id of an official system, other database, an extension of its own: an uncertain conflict.
    fs::write(
        systems.join("psx.toml"),
        definition_text("psx", Some("Demo - Not A PlayStation"), "xyz"),
    )
    .unwrap();
    let cat = leak(Catalog::load(&systems).catalog);
    assert!(cat.system("psx").unwrap().is_custom());
    assert!(cat.is_conflicted("psx"));

    let mut lib = Library::open_in_memory().unwrap();
    put(&roms.join("psx").join("Game.cue"), 1);
    put(&roms.join("psx").join("Mine.xyz"), 2);
    let cue = seed_row(&lib, &roms.join("psx").join("Game.cue"), "psx", None);
    let chd = seed_row(&lib, &roms.join("psx").join("Gone.chd"), "psx", None);
    let zip = seed_row(
        &lib,
        &roms.join("psx").join("Packed.zip"),
        "psx",
        Some("track.iso"),
    );
    let folder = seed_row(&lib, &roms.join("psx").join("Hybrid"), "psx", None);
    lib.conn
        .execute("UPDATE game SET is_dir = 1 WHERE id = ?1", [folder])
        .unwrap();
    for _ in 0..2 {
        let report = scan_in(&mut lib, cat, &roms);
        assert_eq!(report.removed, 0, "{report:?}");
        for id in [cue, chd, zip, folder] {
            assert_eq!(row_data(&lib, id), Some(("psx".into(), 1, 7)), "row {id}");
        }
    }
    // The extension the definition does accept is scanned as usual, under the definition.
    assert_eq!(rows(&lib, &roms)["psx/Mine.xyz"].0, "psx");
}

#[test]
fn a_definition_in_error_with_an_official_id_keeps_the_rows_of_the_definition() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    let systems = tmp.path().join("systems");
    fs::create_dir_all(&systems).unwrap();
    let valid = definition_text(
        "snes",
        Some("Nintendo - Super Nintendo Entertainment System"),
        "sf1",
    );
    fs::write(systems.join("snes.toml"), &valid).unwrap();
    let working = leak(Catalog::load(&systems).catalog);
    assert!(working.system("snes").unwrap().is_custom());

    let mut lib = Library::open_in_memory().unwrap();
    put(&roms.join("snes").join("Hack.sf1"), 1);
    scan_in(&mut lib, working, &roms);
    let hack: i64 = lib
        .conn
        .query_row("SELECT id FROM game", [], |r| r.get(0))
        .unwrap();
    lib.conn
        .execute(
            "UPDATE game SET favorite = 1, play_count = 7 WHERE id = ?1",
            [hack],
        )
        .unwrap();
    // A row of the official extension whose file is gone is purged as usual: only the other half is frozen.
    let plain = seed_row(&lib, &roms.join("snes").join("Gone.sfc"), "snes", None);

    // The file becomes invalid: the official snes is active again and does not accept `sf1`.
    fs::write(systems.join("snes.toml"), "not toml [").unwrap();
    let loaded = Catalog::load(&systems);
    assert_eq!(loaded.problems.len(), 1);
    let broken = leak(loaded.catalog);
    assert!(!broken.system("snes").unwrap().is_custom());
    for _ in 0..2 {
        scan_in(&mut lib, broken, &roms);
        assert_eq!(row_data(&lib, hack), Some(("snes".into(), 1, 7)));
    }
    assert_eq!(row_data(&lib, plain), None);
}

#[test]
fn a_folder_of_an_absent_system_is_reported_like_an_unknown_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    put(&roms.join("ghost").join("New.gb"), 1);
    put(&roms.join("misc").join("New.gb"), 1);
    // An extension no system has is not a candidate: not in the report either way.
    put(&roms.join("ghost").join("Game.ghs"), 2);
    put(&roms.join("misc").join("Game.ghs"), 2);
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan_in(&mut lib, official(), &roms);
    let mut kept: Vec<String> = rows(&lib, &roms).into_keys().collect();
    kept.sort();
    // Same outcome under `roms/ghost/` (no such system) as under `roms/misc/`.
    assert_eq!(kept, ["ghost/New.gb", "misc/New.gb"], "{report:?}");
    assert_eq!(report.unclassified, 0);
}

/// Cached boxart recorded for `id`, with a path that need not exist (only the record is looked at).
fn cache_cover(lib: &mut Library, id: i64, roms: &Path) {
    lib.set_cover(
        crate::GameId(id),
        crate::CoverKind::Boxart,
        &crate::CoverState::Cached {
            path: roms.join("cover.png"),
            source: "test".into(),
        },
    )
    .unwrap();
}

fn cover_kept(lib: &Library, id: i64) -> bool {
    matches!(
        lib.cover(crate::GameId(id), crate::CoverKind::Boxart)
            .unwrap()
            .map(|r| r.state),
        Some(crate::CoverState::Cached { .. })
    )
}

fn visible(lib: &Library, cat: &'static Catalog) -> Vec<i64> {
    let mut ids: Vec<i64> = lib.games_in(cat).unwrap().iter().map(|g| g.id.0).collect();
    ids.sort_unstable();
    ids
}

#[test]
fn removing_the_conflicting_definition_by_hand_shows_the_rows_again_under_the_official() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    let systems = tmp.path().join("systems");
    fs::create_dir_all(&systems).unwrap();
    fs::write(
        systems.join("psx.toml"),
        definition_text("psx", Some("Demo - Not A PlayStation"), "xyz"),
    )
    .unwrap();
    let uncertain = leak(Catalog::load(&systems).catalog);
    assert!(uncertain.is_conflicted("psx"));

    let mut lib = Library::open_in_memory().unwrap();
    put(&roms.join("psx").join("Game.cue"), 1);
    put(&roms.join("psx").join("Game.chd"), 2);
    let cue = seed_row(&lib, &roms.join("psx").join("Game.cue"), "psx", None);
    let chd = seed_row(&lib, &roms.join("psx").join("Game.chd"), "psx", None);
    cache_cover(&mut lib, cue, &roms);
    scan_in(&mut lib, uncertain, &roms);
    // Frozen under the definition: nothing lost, cover included.
    assert_eq!(row_data(&lib, cue), Some(("psx".into(), 1, 7)));
    assert_eq!(row_data(&lib, chd), Some(("psx".into(), 1, 7)));
    assert!(cover_kept(&lib, cue));

    fs::remove_file(systems.join("psx.toml")).unwrap();
    let loaded = Catalog::load(&systems);
    assert!(loaded.problems.is_empty());
    let official = leak(loaded.catalog);
    assert!(!official.is_conflicted("psx"));
    let report = scan_in(&mut lib, official, &roms);
    assert_eq!(report.removed, 0, "{report:?}");
    for id in [cue, chd] {
        assert_eq!(row_data(&lib, id), Some(("psx".into(), 1, 7)), "row {id}");
        assert!(visible(&lib, official).contains(&id), "row {id} visible");
    }
    assert!(cover_kept(&lib, cue));
}

#[test]
fn a_repaired_definition_accepts_its_frozen_rows_again() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    let systems = tmp.path().join("systems");
    fs::create_dir_all(&systems).unwrap();
    let valid = definition_text(
        "snes",
        Some("Nintendo - Super Nintendo Entertainment System"),
        "sf1",
    );
    fs::write(systems.join("snes.toml"), &valid).unwrap();
    let working = leak(Catalog::load(&systems).catalog);
    let mut lib = Library::open_in_memory().unwrap();
    put(&roms.join("snes").join("Hack.sf1"), 1);
    scan_in(&mut lib, working, &roms);
    let hack: i64 = lib
        .conn
        .query_row("SELECT id FROM game", [], |r| r.get(0))
        .unwrap();
    lib.conn
        .execute(
            "UPDATE game SET favorite = 1, play_count = 7 WHERE id = ?1",
            [hack],
        )
        .unwrap();
    cache_cover(&mut lib, hack, &roms);

    fs::write(systems.join("snes.toml"), "not toml [").unwrap();
    let broken = leak(Catalog::load(&systems).catalog);
    scan_in(&mut lib, broken, &roms);
    assert_eq!(row_data(&lib, hack), Some(("snes".into(), 1, 7)));
    assert!(cover_kept(&lib, hack));

    fs::write(systems.join("snes.toml"), &valid).unwrap();
    let repaired = leak(Catalog::load(&systems).catalog);
    assert!(repaired.system("snes").unwrap().is_custom());
    scan_in(&mut lib, repaired, &roms);
    assert_eq!(row_data(&lib, hack), Some(("snes".into(), 1, 7)));
    assert!(visible(&lib, repaired).contains(&hack), "accepted again");
    assert!(cover_kept(&lib, hack));
}

#[test]
fn a_double_conflict_freezes_the_rows_of_the_extension_the_official_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = roms(&tmp);
    let systems = tmp.path().join("systems");
    fs::create_dir_all(&systems).unwrap();
    // Id of snes, database of the Mega Drive: DuplicateDatabase, no official masked.
    fs::write(
        systems.join("snes.toml"),
        definition_text("snes", Some("Sega - Mega Drive - Genesis"), "xyz"),
    )
    .unwrap();
    let loaded = Catalog::load(&systems);
    assert_eq!(loaded.problems.len(), 1);
    assert_eq!(
        loaded.problems[0].error,
        crate::DefinitionError::DuplicateDatabase
    );
    let cat = leak(loaded.catalog);
    assert!(cat.is_conflicted("snes"));
    assert!(!cat.system("snes").unwrap().is_custom());
    assert!(cat.system("megadrive").is_some());

    let mut lib = Library::open_in_memory().unwrap();
    // One `.xyz` row whose file is gone, one whose file is present, one official row whose file is gone.
    put(&roms.join("snes").join("Here.xyz"), 1);
    let gone = seed_row(&lib, &roms.join("snes").join("Gone.xyz"), "snes", None);
    let here = seed_row(&lib, &roms.join("snes").join("Here.xyz"), "snes", None);
    let plain = seed_row(&lib, &roms.join("snes").join("Gone.sfc"), "snes", None);
    cache_cover(&mut lib, gone, &roms);
    for _ in 0..2 {
        scan_in(&mut lib, cat, &roms);
        for id in [gone, here] {
            assert_eq!(row_data(&lib, id), Some(("snes".into(), 1, 7)), "row {id}");
        }
        assert!(cover_kept(&lib, gone));
    }
    // The row of the official extension is purged as usual.
    assert_eq!(row_data(&lib, plain), None);
}
