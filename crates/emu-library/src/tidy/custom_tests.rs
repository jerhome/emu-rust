//! The tidy with custom systems (ADR 0025 §12, design 8.0 §4 « Rangement »): suggested units, reserved
//! folders, official outcomes unchanged. Catalogs come from `Catalog::load` and go to `plan_tidy_in` /
//! `plan::run_in`: nothing here publishes a catalog.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::tests::{noise, write};
use super::*;
use crate::catalog::Catalog;
use crate::custom::{CustomSystemDraft, DefinitionOrigin, Retirement, retire_custom, write_custom};
use crate::{DatSource, Library};

fn leak(catalog: Catalog) -> &'static Catalog {
    Box::leak(Box::new(catalog))
}

fn draft(id: &str, database: &str, extensions: &[&str]) -> CustomSystemDraft {
    CustomSystemDraft {
        id: id.into(),
        name: format!("Custom {id}"),
        database: Some(database.into()),
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

/// A `systems/` folder: five active custom systems (some extensions shared with official systems, `shr` shared
/// by two of them), `gone` retired, `broken.toml` invalid.
fn systems_dir(root: &Path) -> PathBuf {
    let dir = root.join("systems");
    for d in [
        draft("dmoone", "Demo - One", &["dmo", "bin"]),
        draft("dmotwo", "Demo - Two", &["gb", "md", "dm2"]),
        draft("shra", "Demo - Shra", &["shr"]),
        draft("shrb", "Demo - Shrb", &["shr"]),
        draft("dmofour", "Demo - Four", &["st", "adf", "hdf"]),
        draft("gone", "Demo - Gone", &["gon"]),
    ] {
        write_custom(&dir, &d, &Catalog::load(&dir).catalog).unwrap();
    }
    retire_custom(&dir, "gone", Retirement::Removed, 1_700_000_000).unwrap();
    fs::write(dir.join("broken.toml"), "not toml [").unwrap();
    dir
}

struct Fx {
    tmp: tempfile::TempDir,
    roms: PathBuf,
    cat: &'static Catalog,
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    // Canonical like `TidyOptions` roots (`/var` is a link to `/private/var` on macOS).
    let roms = fs::canonicalize(tmp.path()).unwrap().join("roms");
    fs::create_dir(&roms).unwrap();
    let cat = leak(Catalog::load(&systems_dir(tmp.path())).catalog);
    assert_eq!(cat.customs().count(), 5);
    Fx { tmp, roms, cat }
}

fn official() -> &'static Catalog {
    leak(Catalog::official())
}

impl Fx {
    fn options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.tmp.path().join("rangement")).unwrap()
    }

    fn plan_in(&self, lib: &Library, cat: &'static Catalog) -> TidyPlan {
        lib.plan_tidy_in(cat, &self.options(), &AtomicBool::new(false), &mut |_| {})
            .unwrap()
    }
}

fn unit<'p>(plan: &'p TidyPlan, primary: &str) -> &'p TidyUnit {
    let wanted: PathBuf = primary.split('/').collect();
    plan.units
        .iter()
        .zip(&plan.meta)
        .find(|(_, m)| m.files.first().is_some_and(|f| f.path == wanted))
        .map(|(u, _)| u)
        .unwrap_or_else(|| panic!("no unit for {primary}: {:#?}", plan.units))
}

fn insert_game(lib: &Library, key: &Path, system: &str) {
    lib.conn
        .execute(
            "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification)
             VALUES (?1, ?2, 1, 1, 0, 'T', 'unknown')",
            rusqlite::params![key.to_str().unwrap(), system],
        )
        .unwrap();
}

#[test]
fn a_file_of_an_extension_proper_to_one_custom_system_is_only_suggested() {
    let fx = fx();
    write(&fx.roms.join("Game.dmo"), &noise(2048, 1));
    let lib = Library::open_in_memory().unwrap();
    let plan = fx.plan_in(&lib, fx.cat);
    let u = unit(&plan, "Game.dmo");
    assert_eq!(u.status, TidyStatus::Suggested(TidyReason::Extension));
    assert_eq!(u.system.as_deref(), Some("dmoone"));
    assert!(u.candidates.is_empty());
    let moves: Vec<PathBuf> = u.moves.iter().map(|m| m.to.clone()).collect();
    assert_eq!(moves, [PathBuf::from("dmoone").join("Game.dmo")]);
    // Without the custom system the file is not a game at all.
    let plain = fx.plan_in(&lib, official());
    assert!(plain.units.is_empty());
    assert_eq!(plain.ignored, 1);
}

#[test]
fn an_extension_shared_by_two_custom_systems_is_never_proposed() {
    let fx = fx();
    write(&fx.roms.join("Shared.shr"), &noise(2048, 2));
    let lib = Library::open_in_memory().unwrap();
    let plan = fx.plan_in(&lib, fx.cat);
    let u = unit(&plan, "Shared.shr");
    assert_eq!(u.status, TidyStatus::Unknown);
    assert!(u.moves.is_empty() && u.system.is_none() && u.candidates.is_empty());
}

/// One loose file per extension of the official systems, a pair of ST disks and a CD.
fn official_files(roms: &Path) {
    let mut seed = 10;
    let mut done = std::collections::BTreeSet::new();
    for system in Catalog::official().systems() {
        for ext in &system.extensions {
            if ext == "m3u" || !done.insert(ext.clone()) {
                continue;
            }
            seed += 1;
            write(&roms.join(format!("t_{ext}.{ext}")), &noise(3000, seed));
        }
    }
    write(&roms.join("Disk (Disk 1 of 2).st"), &noise(3000, 900));
    write(&roms.join("Disk (Disk 2 of 2).st"), &noise(3000, 901));
    write(&roms.join("Hero.gb"), &noise(3000, 902));
}

#[test]
fn custom_systems_change_no_outcome_of_the_official_systems() {
    let fx = fx();
    official_files(&fx.roms);
    let lib = Library::open_in_memory().unwrap();
    let with = fx.plan_in(&lib, fx.cat);
    let without = fx.plan_in(&lib, official());
    assert!(without.units.len() > 20, "{} units", without.units.len());
    assert_eq!(with.units, without.units);
    assert_eq!(
        (with.already_tidy, with.ignored, &with.ignored_paths),
        (
            without.already_tidy,
            without.ignored,
            &without.ignored_paths
        )
    );
}

#[test]
fn a_folder_only_official_extension_stays_sure_when_a_custom_system_claims_it_too() {
    let fx = fx();
    // `hdf` is Amiga-only outside its folder in the official catalog; dmofour claims it under its folder.
    write(&fx.roms.join("Hard.hdf"), &noise(3000, 3));
    let lib = Library::open_in_memory().unwrap();
    let with = fx.plan_in(&lib, fx.cat);
    let without = fx.plan_in(&lib, official());
    assert_eq!(unit(&with, "Hard.hdf"), unit(&without, "Hard.hdf"));
    assert_eq!(
        unit(&with, "Hard.hdf").status,
        TidyStatus::Sure(TidyReason::Extension)
    );
    assert_eq!(unit(&with, "Hard.hdf").system.as_deref(), Some("amiga"));
}

#[test]
fn a_known_game_keeps_its_system_and_is_never_offered_a_custom_one() {
    let fx = fx();
    write(&fx.roms.join("Known.dmo"), &noise(2048, 4));
    write(&fx.roms.join("Known.gb"), &noise(2048, 5));
    write(&fx.roms.join("Ghost.gb"), &noise(2048, 6));
    let lib = Library::open_in_memory().unwrap();
    let key = |n: &str| fx.options().scan_root().join(n);
    insert_game(&lib, &key("Known.dmo"), "dmoone");
    insert_game(&lib, &key("Known.gb"), "gb");
    insert_game(&lib, &key("Ghost.gb"), "ghost");
    let plan = fx.plan_in(&lib, fx.cat);
    let custom = unit(&plan, "Known.dmo");
    assert_eq!(custom.status, TidyStatus::Sure(TidyReason::Library));
    assert_eq!(custom.system.as_deref(), Some("dmoone"));
    assert!(custom.candidates.is_empty());
    // `gb` is also an extension of the custom `dmotwo`: not a choice (only `gbc`, as without it).
    let official_game = unit(&plan, "Known.gb");
    assert_eq!(official_game.candidates, ["gbc"]);
    assert_eq!(official_game.system.as_deref(), Some("gb"));
    // A row whose system is absent from the catalog stays where it is: nothing is put into `roms/ghost/`.
    let ghost = unit(&plan, "Ghost.gb");
    assert!(ghost.moves.is_empty(), "{ghost:#?}");
    assert_eq!(ghost.status, TidyStatus::Unknown);
    assert!(ghost.known_game.is_some());
}

#[test]
fn a_custom_system_is_never_a_choice_for_an_ambiguous_unit() {
    let fx = fx();
    // `.bin` of an official system and of a custom one: only the official candidates, as without it.
    write(&fx.roms.join("Raw.bin"), &noise(2048, 7));
    let lib = Library::open_in_memory().unwrap();
    let mut plan = fx.plan_in(&lib, fx.cat);
    let u = unit(&plan, "Raw.bin").clone();
    assert!(!u.candidates.iter().any(|c| c.starts_with("dmo")), "{u:?}");
    assert_eq!(
        u.candidates,
        unit(&fx.plan_in(&lib, official()), "Raw.bin").candidates
    );
    if u.status == TidyStatus::Ambiguous {
        assert!(plan.choose_system(u.id, "dmoone").is_err());
    }
}

#[test]
fn folders_of_retired_broken_and_unknown_systems_are_left_alone() {
    let fx = fx();
    write(&fx.roms.join("gone").join("Game.gon"), &noise(2048, 8));
    write(&fx.roms.join("gone").join("disc.bin"), &noise(2048, 9));
    write(&fx.roms.join("broken").join("Game.dmo"), &noise(2048, 10));
    write(&fx.roms.join("orphan").join("Game.gb"), &noise(2048, 11));
    write(&fx.roms.join("dmoone").join("Mine.dmo"), &noise(2048, 12));
    write(&fx.roms.join("gb").join("Pocket.gb"), &noise(2048, 13));
    write(&fx.roms.join("loose.gb"), &noise(2048, 14));
    let lib = Library::open_in_memory().unwrap();
    // `orphan` is an id of the library that the catalog does not have.
    insert_game(
        &lib,
        &fx.options().scan_root().join("elsewhere").join("x.gb"),
        "orphan",
    );
    let plan = fx.plan_in(&lib, fx.cat);
    assert_eq!(plan.reserved_folders, ["broken", "gone", "orphan"]);
    // Nothing is taken from them; only the loose file is a unit; the active folders count as already tidy.
    assert_eq!(plan.units.len(), 1, "{:#?}", plan.units);
    assert_eq!(plan.units[0].title, "loose");
    assert_eq!(plan.already_tidy, 2);
    assert_eq!(plan.ignored, 0);

    // Control: without the retired and broken definitions nothing is reserved and their files are units.
    let plain = fx.plan_in(&lib, official());
    assert_eq!(plain.reserved_folders, ["orphan"]);
    assert!(plain.units.len() > 1);
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_named_like_a_reserved_folder_is_left_alone_too() {
    let fx = fx();
    write(
        &fx.tmp.path().join("elsewhere").join("x.gb"),
        &noise(2048, 15),
    );
    std::os::unix::fs::symlink(fx.tmp.path().join("elsewhere"), fx.roms.join("gone")).unwrap();
    let lib = Library::open_in_memory().unwrap();
    let plan = fx.plan_in(&lib, fx.cat);
    assert!(plan.units.is_empty(), "{:#?}", plan.units);
    assert_eq!(plan.reserved_folders, ["gone"]);
}

#[test]
fn the_folder_of_an_active_id_is_never_reserved() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    fs::create_dir_all(dir.join(".retired")).unwrap();
    fs::write(dir.join("megadrive.toml"), "not toml [").unwrap();
    fs::write(
        dir.join(".retired")
            .join("megadrive-1700000001.replaced.toml"),
        "x",
    )
    .unwrap();
    let cat = leak(Catalog::load(&dir).catalog);
    let roms = tmp.path().join("roms");
    write(&roms.join("megadrive").join("Game.md"), &noise(2048, 16));
    let options = TidyOptions::new(&roms, &tmp.path().join("rangement")).unwrap();
    let lib = Library::open_in_memory().unwrap();
    let plan = lib
        .plan_tidy_in(cat, &options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(plan.reserved_folders.is_empty());
    assert_eq!(plan.already_tidy, 1);
}

#[test]
fn the_source_of_a_switch_is_exempt_from_the_reservation() {
    let fx = fx();
    write(&fx.roms.join("gone").join("Game.gon"), &noise(2048, 17));
    write(&fx.roms.join("gone").join("Raw.bin"), &noise(2048, 18));
    let lib = Library::open_in_memory().unwrap();
    let plan = plan::run_in(
        &lib.conn,
        fx.cat,
        &fx.options(),
        &AtomicBool::new(false),
        &mut |_| {},
        Some("gone"),
    )
    .unwrap();
    assert!(plan.reserved_folders.is_empty());
    assert!(
        plan.units.iter().any(|u| u.title == "Raw"),
        "{:#?}",
        plan.units
    );
}

#[test]
fn system_folders_are_created_for_active_systems_only() {
    let fx = fx();
    fs::create_dir(fx.roms.join("shra")).unwrap();
    let created = create_system_folders_in(fx.cat, &fx.roms).unwrap();
    let names: Vec<String> = created
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for custom in ["dmoone", "dmotwo", "shrb", "dmofour"] {
        assert!(names.iter().any(|n| n == custom), "{custom} in {names:?}");
        assert!(fx.roms.join(custom).is_dir());
    }
    // Already there: left alone and not reported. Retired and broken: never recreated.
    assert!(!names.iter().any(|n| n == "shra"));
    assert!(!fx.roms.join("gone").exists());
    assert!(!fx.roms.join("broken").exists());
    assert!(fx.roms.join("snes").is_dir());
}

#[test]
fn a_plan_is_only_equal_to_a_plan_of_the_same_catalog() {
    let fx = fx();
    write(&fx.roms.join("Hero.gb"), &noise(2048, 19));
    let lib = Library::open_in_memory().unwrap();
    let a = fx.plan_in(&lib, fx.cat);
    assert_eq!(a, fx.plan_in(&lib, fx.cat));
    let other = fx.plan_in(&lib, official());
    assert_ne!(a, other, "the snapshot is part of the identity of a plan");
}

#[test]
fn a_reserved_folder_written_in_another_case_is_reserved_too() {
    let fx = fx();
    write(&fx.roms.join("GONE").join("Game.gon"), &noise(2048, 20));
    write(&fx.roms.join("Gone").join("Disc.bin"), &noise(2048, 21));
    write(&fx.roms.join("Broken").join("Game.dmo"), &noise(2048, 22));
    let lib = Library::open_in_memory().unwrap();
    let plan = fx.plan_in(&lib, fx.cat);
    assert_eq!(plan.reserved_folders, ["broken", "gone"]);
    assert!(plan.units.is_empty(), "{:#?}", plan.units);
    assert_eq!(plan.ignored, 0);
}

#[test]
fn restoring_a_retired_system_finds_all_its_games_again() {
    let fx = fx();
    let games = ["A.gon", "B.gon", "C.gon"];
    for (i, name) in games.iter().enumerate() {
        write(
            &fx.roms.join("gone").join(name),
            &noise(2048, 30 + i as u32),
        );
    }
    let systems = fx.tmp.path().join("systems");
    let mut lib = Library::open_in_memory().unwrap();
    // Reserved while retired: nothing is taken, nothing is put there.
    let plan = fx.plan_in(&lib, fx.cat);
    assert_eq!(plan.reserved_folders, ["gone"]);
    assert!(plan.units.is_empty(), "{:#?}", plan.units);

    let retired = systems.join(".retired").join("gone-1700000000.toml");
    crate::custom::restore_custom(&systems, &retired, &Catalog::load(&systems).catalog).unwrap();
    let back = leak(Catalog::load(&systems).catalog);
    assert!(back.system("gone").is_some());
    let plan = fx.plan_in(&lib, back);
    assert!(plan.reserved_folders.is_empty());
    assert!(plan.units.is_empty(), "{:#?}", plan.units);

    let options = crate::ScanOptions {
        roots: vec![fx.roms.clone()],
        ..crate::ScanOptions::default()
    };
    crate::scan::run_in(
        back,
        &mut lib.conn,
        &options,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    let found: i64 = lib
        .conn
        .query_row("SELECT COUNT(*) FROM game WHERE system = 'gone'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(found, games.len() as i64);
}
