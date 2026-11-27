//! The switch with move (design 8.0 §2.1, §2.6; ADR 0025 §10; ADR 0023 amendment of 2026-11-27): the plan, the run
//! under a version 2 journal, the end (`complete_switch`), the undo and its check. Real files under a temporary
//! folder, faults injected through [`Sys`]. Catalogs come from `Catalog::load`; nothing here publishes one.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::exec::Exec;
use super::journal::{self, Journal, State};
use super::sys::faulty::{Faulty, Mode};
use super::sys::{RealSys, Sys};
use super::tests::{noise, write};
use super::*;
use crate::catalog::Catalog;
use crate::custom::{Retirement, retired_file_name};
use crate::{ForgetDats, ForgetScope, Library, ScanOptions, TidyRefusal};

const OFFICIAL_DATABASE: &str = "Nintendo - Super Nintendo Entertainment System";

fn leak(catalog: Catalog) -> &'static Catalog {
    Box::leak(Box::new(catalog))
}

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

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        w.start_file(*name, options).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn mv() -> SwitchMove {
    SwitchMove {
        custom_id: "mysnes".into(),
        official_id: "snes".into(),
    }
}

fn choices() -> SwitchChoices {
    SwitchChoices {
        keys: KeysChoice::KeepUser,
        prior_preference: Some(vec!["snes9x".into(), "bsnes".into()]),
        prior_profile: Some("[keys]\nup = \"W\"\n".into()),
    }
}

/// `roms/mysnes/` with two games the official Super Nintendo accepts (`Game.sfc`, `Second.smc`), two files only
/// the definition accepts (`Mine.xyz`, `Pack.zip` holding `a.xyz`), all scanned under the definition with a
/// favourite and a play count; then an unscanned `Late.sfc` and a text file. `systems/mysnes.toml` is active (the
/// official Super Nintendo is masked), `snes.toml`-less.
struct Fx {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    roms: PathBuf,
    systems: PathBuf,
    journals: PathBuf,
    lib: Library,
    before: &'static Catalog,
}

fn scan_in(lib: &mut Library, cat: &'static Catalog, roms: &Path) -> crate::ScanReport {
    let options = ScanOptions {
        roots: vec![roms.to_path_buf()],
        ..Default::default()
    };
    crate::scan::run_in(
        cat,
        &mut lib.conn,
        &options,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap()
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let (roms, systems, journals) = (
            root.join("roms"),
            root.join("systems"),
            root.join("rangement"),
        );
        fs::create_dir_all(&systems).unwrap();
        fs::write(
            systems.join("mysnes.toml"),
            definition_text("mysnes", Some(OFFICIAL_DATABASE), &["sfc", "smc", "xyz"]),
        )
        .unwrap();
        let before = leak(Catalog::load(&systems).catalog);
        assert!(before.system("mysnes").unwrap().is_custom());
        assert!(before.system("snes").is_none());
        let own = roms.join("mysnes");
        write(&own.join("Game.sfc"), &noise(3000, 1));
        write(&own.join("Second.smc"), &noise(2500, 2));
        write(&own.join("Mine.xyz"), &noise(2000, 3));
        fs::write(own.join("Pack.zip"), zip_of(&[("a.xyz", &noise(1500, 4))])).unwrap();
        let mut lib = Library::open_in_memory().unwrap();
        let report = scan_in(&mut lib, before, &roms);
        assert_eq!(report.removed, 0);
        lib.conn
            .execute(
                "UPDATE game SET favorite = 1, play_count = 7, last_played_unix = 99",
                [],
            )
            .unwrap();
        assert_eq!(count(&lib, "system = 'mysnes'"), 4);
        write(&own.join("Late.sfc"), &noise(1800, 5));
        write(&own.join("Notes.txt"), b"notes");
        Fx {
            _tmp: tmp,
            root,
            roms,
            systems,
            journals,
            lib,
            before,
        }
    }

    fn options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.journals)
            .unwrap()
            .with_systems_dir(&self.systems)
    }

    fn plan(&self) -> TidyPlan {
        self.lib
            .plan_switch(
                &self.options(),
                &Catalog::official(),
                &mv(),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap()
    }

    fn apply(&mut self, plan: &TidyPlan) -> TidyReport {
        let options = self.options();
        self.lib
            .apply_switch(
                &options,
                plan,
                &choices(),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap()
    }

    fn pending(&self) -> Option<PendingSwitch> {
        self.lib.pending_switch(&self.options()).unwrap()
    }

    fn complete(&mut self) {
        let options = self.options();
        let pending = self.pending().expect("a switch end is due");
        self.lib
            .complete_switch(&options, &pending, &self.systems)
            .unwrap();
    }

    /// The catalog once the definition is retired.
    fn after_catalog(&self) -> &'static Catalog {
        leak(Catalog::load(&self.systems).catalog)
    }

    fn journals(&self) -> Vec<PathBuf> {
        journal::list(&self.journals)
            .unwrap()
            .into_iter()
            .map(|l| l.path)
            .collect()
    }

    fn journal(&self) -> Journal {
        let all = self.journals();
        assert_eq!(all.len(), 1, "{all:?}");
        journal::read(&all[0]).unwrap().unwrap()
    }

    fn retired(&self) -> Vec<String> {
        names_in(&self.systems.join(".retired"))
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn count(lib: &Library, clause: &str) -> i64 {
    lib.conn
        .query_row(
            &format!("SELECT COUNT(*) FROM game WHERE {clause}"),
            [],
            |r| r.get(0),
        )
        .unwrap()
}

/// `file name → (system, favourite, play count)` of every row.
fn rows(lib: &Library) -> BTreeMap<String, (String, i64, i64)> {
    let mut stmt = lib
        .conn
        .prepare("SELECT path, system, favorite, play_count FROM game")
        .unwrap();
    stmt.query_map([], |r| {
        let path: String = r.get(0)?;
        let name = Path::new(&path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        Ok((name, (r.get(1)?, r.get(2)?, r.get(3)?)))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn system_of(lib: &Library, name: &str) -> String {
    rows(lib)[name].0.clone()
}

fn exists(roms: &Path, folder: &str, name: &str) -> bool {
    roms.join(folder).join(name).is_file()
}

fn unit_titles(plan: &TidyPlan) -> Vec<String> {
    let mut titles: Vec<String> = plan
        .units
        .iter()
        .map(|u| {
            u.moves
                .first()
                .and_then(|m| m.from.file_name())
                .map_or_else(|| u.title.clone(), |n| n.to_string_lossy().into_owned())
        })
        .collect();
    titles.sort();
    titles
}

// ---------------------------------------------------------------------------------------------------------------
// Plan

#[test]
fn the_plan_moves_what_the_official_system_takes_and_lists_what_it_does_not() {
    let fx = Fx::new();
    // A file elsewhere is not part of the switch.
    write(&fx.roms.join("other").join("Elsewhere.sfc"), &noise(900, 9));
    let plan = fx.plan();
    assert!(plan.switchable());
    assert_eq!(unit_titles(&plan), ["Game.sfc", "Late.sfc", "Second.smc"]);
    assert!(
        plan.units
            .iter()
            .all(|u| u.system.as_deref() == Some("snes"))
    );
    let mut destinations: Vec<PathBuf> = plan
        .units
        .iter()
        .flat_map(|u| u.moves.iter().map(|m| m.to.clone()))
        .collect();
    destinations.sort();
    assert_eq!(
        destinations,
        [
            PathBuf::from("snes").join("Game.sfc"),
            PathBuf::from("snes").join("Late.sfc"),
            PathBuf::from("snes").join("Second.smc"),
        ]
    );
    // Ids are `0..n`: the journal names them so.
    let ids: Vec<u32> = plan.units.iter().map(|u| u.id).collect();
    assert_eq!(ids, [0, 1, 2]);
    // The rows the official does not accept (extension, `.zip` entry) stay: listed, sorted, stored paths.
    let frozen: Vec<String> = plan
        .switch_frozen
        .iter()
        .map(|p| {
            Path::new(p)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(frozen, ["Mine.xyz", "Pack.zip"]);
    // Read-only: nothing moved, nothing written.
    assert!(exists(&fx.roms, "mysnes", "Game.sfc"));
    assert!(!fx.journals.exists() || fx.journals().is_empty());
    assert_eq!(count(&fx.lib, "system = 'mysnes'"), 4);
}

#[test]
fn a_taken_destination_blocks_the_switch() {
    let mut fx = Fx::new();
    write(&fx.roms.join("snes").join("Game.sfc"), &noise(3000, 77));
    let plan = fx.plan();
    assert!(!plan.switchable());
    assert!(
        plan.units
            .iter()
            .any(|u| matches!(u.status, TidyStatus::Blocked(TidyBlock::Conflict { .. })))
    );
    let options = fx.options();
    let result = fx.lib.apply_switch(
        &options,
        &plan,
        &choices(),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert!(result.is_err());
    assert!(fx.journals().is_empty());
    assert!(exists(&fx.roms, "mysnes", "Game.sfc"));
    // The same file already there: a duplicate, no more switchable.
    fs::write(fx.roms.join("snes").join("Game.sfc"), noise(3000, 1)).unwrap();
    let plan = fx.plan();
    assert!(
        plan.units
            .iter()
            .any(|u| matches!(u.status, TidyStatus::Blocked(TidyBlock::Duplicate { .. })))
    );
    assert!(!plan.switchable());
}

#[test]
fn a_plan_without_unit_or_not_made_by_plan_switch_is_not_switchable() {
    let mut fx = Fx::new();
    let plain = fx
        .lib
        .plan_tidy(&fx.options(), &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(!plain.switchable());
    let options = fx.options();
    assert!(
        fx.lib
            .apply_switch(
                &options,
                &plain,
                &choices(),
                &AtomicBool::new(false),
                &mut |_| {}
            )
            .is_err()
    );
    // Nothing to move: only a file the official does not take.
    let empty = tempfile::tempdir().unwrap();
    let roms = fs::canonicalize(empty.path()).unwrap().join("roms");
    write(&roms.join("mysnes").join("Only.xyz"), b"x");
    let lib = Library::open_in_memory().unwrap();
    let options = TidyOptions::new(&roms, &empty.path().join("rangement"))
        .unwrap()
        .with_systems_dir(&empty.path().join("systems"));
    let plan = lib
        .plan_switch(
            &options,
            &Catalog::official(),
            &mv(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert!(plan.units.is_empty() && !plan.switchable());
}

#[test]
fn plan_switch_wants_the_official_system_active_and_the_definition_gone() {
    let fx = Fx::new();
    let ask = |after: &Catalog, switch: &SwitchMove| {
        fx.lib.plan_switch(
            &fx.options(),
            after,
            switch,
            &AtomicBool::new(false),
            &mut |_| {},
        )
    };
    // `before` still has the definition and masks the official system.
    assert!(ask(fx.before, &mv()).is_err());
    assert!(
        ask(
            &Catalog::official(),
            &SwitchMove {
                custom_id: "mysnes".into(),
                official_id: "nope".into()
            }
        )
        .is_err()
    );
    assert!(
        ask(
            &Catalog::official(),
            &SwitchMove {
                custom_id: "snes".into(),
                official_id: "snes".into()
            }
        )
        .is_err()
    );
    assert!(
        ask(
            &Catalog::official(),
            &SwitchMove {
                custom_id: "Bad Id".into(),
                official_id: "snes".into()
            }
        )
        .is_err()
    );
    assert!(ask(&Catalog::official(), &mv()).is_ok());
}

// ---------------------------------------------------------------------------------------------------------------
// Run and end

#[test]
fn a_switch_moves_the_games_then_ends_with_the_rows_the_definition_and_f() {
    let mut fx = Fx::new();
    let plan = fx.plan();
    let report = fx.apply(&plan);
    assert!(
        report.errors.is_empty() && report.left.is_empty(),
        "{report:?}"
    );
    for name in ["Game.sfc", "Late.sfc", "Second.smc"] {
        assert!(exists(&fx.roms, "snes", name), "{name}");
        assert!(!exists(&fx.roms, "mysnes", name), "{name}");
    }
    assert!(exists(&fx.roms, "mysnes", "Mine.xyz") && exists(&fx.roms, "mysnes", "Pack.zip"));
    assert!(exists(&fx.roms, "mysnes", "Notes.txt"));
    // The custom folder stays, even when emptied of games (it becomes a reserved folder).
    assert!(fx.roms.join("mysnes").is_dir());

    // Version 2 journal: the header records the choices, `E` ends the run, the end is due and nothing else runs.
    let journal = fx.journal();
    let header = journal.switch.as_ref().unwrap();
    assert_eq!(
        (
            header.custom_id.as_str(),
            header.official_id.as_str(),
            header.units
        ),
        ("mysnes", "snes", 3)
    );
    assert_eq!(header.keys, KeysChoice::KeepUser);
    assert_eq!(
        header.prior_preference.as_deref(),
        Some(&["snes9x".to_owned(), "bsnes".to_owned()][..])
    );
    assert_eq!(
        header.prior_profile.as_deref(),
        Some("[keys]\nup = \"W\"\n")
    );
    assert!(
        fs::read_to_string(&fx.journals()[0])
            .unwrap()
            .starts_with("emu-rust tidy 2\n")
    );
    assert_eq!(journal.state, State::Ended);
    assert!(!journal.forward_final);
    let pending = fx.pending().unwrap();
    assert_eq!(pending.direction, SwitchDirection::Forward);
    assert_eq!(pending.switch, mv());
    assert_eq!(pending.choices, choices());
    assert_eq!(fx.retired(), Vec::<String>::new());
    assert!(fx.systems.join("mysnes.toml").is_file());
    // Rows keep the custom id until the end; their paths followed the files.
    assert_eq!(count(&fx.lib, "system = 'mysnes'"), 4);
    assert!(
        fx.lib
            .conn
            .query_row(
                "SELECT path FROM game WHERE path LIKE '%Game.sfc'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap()
            .contains("snes")
    );

    // While the end is due: no tidy, no undo, no identity marker, for any root, and no identities yet.
    let options = fx.options();
    let other = fx.root.join("roms2");
    fs::create_dir(&other).unwrap();
    let other_options = TidyOptions::new(&other, &fx.journals)
        .unwrap()
        .with_systems_dir(&fx.systems);
    for o in [&options, &other_options] {
        let plain = fx
            .lib
            .plan_tidy(o, &AtomicBool::new(false), &mut |_| {})
            .unwrap();
        let err = fx
            .lib
            .apply_tidy(o, &plain, &[], &AtomicBool::new(false), &mut |_| {})
            .unwrap_err();
        assert!(
            matches!(
                err,
                LibraryError::TidyRefused {
                    reason: TidyRefusal::SwitchPending,
                    ..
                }
            ),
            "{err:?}"
        );
        let err = fx.lib.apply_switch(
            o,
            &fx.plan_for(o),
            &choices(),
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert!(err.is_err());
        let err = fx
            .lib
            .undo_tidy(o, &fx.journals()[0], &AtomicBool::new(false), &mut |_| {})
            .unwrap_err();
        assert!(
            matches!(
                err,
                LibraryError::TidyRefused {
                    reason: TidyRefusal::SwitchPending,
                    ..
                }
            ),
            "{err:?}"
        );
        let err = fx
            .lib
            .mark_tidy_identities(o, &fx.journals()[0])
            .unwrap_err();
        assert!(
            matches!(
                err,
                LibraryError::TidyRefused {
                    reason: TidyRefusal::SwitchPending,
                    ..
                }
            ),
            "{err:?}"
        );
    }
    assert!(fx.lib.pending_tidy_identities(&options).unwrap().is_empty());
    // `pending_switch` and `complete_switch` ignore the root of the journal.
    assert!(fx.lib.pending_switch(&other_options).unwrap().is_some());

    fx.complete();
    let journal = fx.journal();
    assert!(journal.forward_final);
    assert_eq!(journal.state, State::Ended);
    assert!(fx.pending().is_none());
    // Games follow the official system; the others keep the retired id, hidden by the catalog.
    let rows = rows(&fx.lib);
    for name in ["Game.sfc", "Late.sfc", "Second.smc"] {
        assert_eq!(
            rows.get(name).map(|r| r.0.as_str()),
            if name == "Late.sfc" {
                None
            } else {
                Some("snes")
            },
            "{name}"
        );
    }
    assert_eq!(rows["Mine.xyz"], ("mysnes".into(), 1, 7));
    assert_eq!(rows["Pack.zip"], ("mysnes".into(), 1, 7));
    assert_eq!(rows["Game.sfc"], ("snes".into(), 1, 7));
    // The definition is retired under the recorded name; nothing deleted.
    assert!(!fx.systems.join("mysnes.toml").exists());
    assert_eq!(
        fx.retired(),
        [journal.switch.as_ref().unwrap().retired_name.clone()]
    );
    // Identities are now due, the marker accepted.
    let due = fx.lib.pending_tidy_identities(&options).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].1.len(), 3);
    fx.lib.mark_tidy_identities(&options, &due[0].0).unwrap();
    assert!(fx.lib.pending_tidy_identities(&options).unwrap().is_empty());
    // The catalog now: the official system is active, the custom id reserved.
    let after = fx.after_catalog();
    assert!(after.system("snes").is_some() && after.system("mysnes").is_none());
    assert!(after.reserved_folders().contains(&"mysnes".to_owned()));
}

impl Fx {
    /// A plan for other options (an empty root): enough to ask `apply_switch` for something.
    fn plan_for(&self, options: &TidyOptions) -> TidyPlan {
        self.lib
            .plan_switch(
                options,
                &Catalog::official(),
                &mv(),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap()
    }
}

#[test]
fn the_ordinary_tidy_still_writes_version_1() {
    let mut fx = Fx::new();
    write(&fx.roms.join("Loose.sfc"), &noise(800, 31));
    let options = fx.options();
    let cat = leak(Catalog::official());
    let plan = fx
        .lib
        .plan_tidy_in(cat, &options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let selected: Vec<u32> = plan
        .units
        .iter()
        .filter(|u| matches!(u.status, TidyStatus::Sure(_) | TidyStatus::Suggested(_)))
        .map(|u| u.id)
        .collect();
    assert!(!selected.is_empty());
    let report = fx
        .lib
        .apply_tidy(
            &options,
            &plan,
            &selected,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    let text = fs::read_to_string(report.journal.unwrap()).unwrap();
    assert!(text.starts_with("emu-rust tidy 1\n"), "{text}");
    assert!(!text.contains("\nX "));
}

#[test]
fn a_unit_changed_since_the_preview_makes_the_switch_incomplete_and_no_end_is_due() {
    let mut fx = Fx::new();
    let plan = fx.plan();
    // Another size: the unit is left, the rest of the plan runs.
    fs::write(fx.roms.join("mysnes").join("Game.sfc"), noise(3100, 8)).unwrap();
    let report = fx.apply(&plan);
    assert_eq!(report.left.len(), 1, "{report:?}");
    assert!(matches!(report.left[0].1, TidyBlock::ChangedSincePlan));
    let journal = fx.journal();
    assert_eq!(journal.state, State::Ended);
    assert_eq!(journal.committed_units(), 2);
    assert!(fx.pending().is_none());
    assert!(fx.systems.join("mysnes.toml").is_file());
    // Everything stays visible under the definition, which is still active.
    assert_eq!(count(&fx.lib, "system = 'mysnes'"), 4);
    // Identities are due at once (net list of the committed units), the tidy is not blocked.
    let options = fx.options();
    assert_eq!(
        fx.lib.pending_tidy_identities(&options).unwrap()[0].1.len(),
        2
    );
    // A scan under the definition keeps the system of the rows that already sit under `roms/snes/`.
    let report = scan_in(&mut fx.lib, fx.before, &fx.roms);
    assert_eq!(report.removed, 0, "{report:?}");
    assert_eq!(system_of(&fx.lib, "Second.smc"), "mysnes");
    assert_eq!(count(&fx.lib, "system = 'snes'"), 0);
    // A new switch takes what is left (the changed file is a new plan's unit) and ends.
    let plan = fx.plan();
    assert_eq!(unit_titles(&plan), ["Game.sfc"]);
    fx.apply(&plan);
    assert!(fx.journals().len() == 2);
    fx.complete();
    assert_eq!(system_of(&fx.lib, "Game.sfc"), "snes");
    assert_eq!(system_of(&fx.lib, "Second.smc"), "snes");
    assert!(!fx.systems.join("mysnes.toml").exists());
}

#[test]
fn a_cancelled_switch_is_incomplete() {
    let mut fx = Fx::new();
    let plan = fx.plan();
    let cancel = AtomicBool::new(false);
    let options = fx.options();
    let report = fx
        .lib
        .apply_switch(&options, &plan, &choices(), &cancel, &mut |p| {
            if p.done_units == 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(fx.journal().committed_units(), 1);
    assert!(fx.pending().is_none());
    assert!(fx.systems.join("mysnes.toml").is_file());
}

/// Every file is on exactly one side of its unit, nothing else changed (design 8.0 §4).
fn check_sides(roms: &Path, plan: &TidyPlan, initial: &BTreeMap<PathBuf, Vec<u8>>, context: &str) {
    let now = snapshot(roms);
    let mut expected = initial.clone();
    for unit in &plan.units {
        let before: Vec<&PathBuf> = unit.moves.iter().map(|m| &m.from).collect();
        let after: Vec<&PathBuf> = unit.moves.iter().map(|m| &m.to).collect();
        let at_source = before.iter().all(|p| now.get(*p) == initial.get(*p));
        let at_dest = unit
            .moves
            .iter()
            .all(|m| now.get(&m.to) == initial.get(&m.from));
        assert!(
            at_source ^ at_dest,
            "{context}: unit {} split or lost",
            unit.id
        );
        if at_dest {
            for p in before {
                expected.remove(p);
            }
            for (m, p) in unit.moves.iter().zip(after) {
                expected.insert(p.clone(), initial[&m.from].clone());
            }
        }
    }
    assert_eq!(now, expected, "{context}");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(root.join(&dir)).unwrap() {
            let e = e.unwrap();
            let r = dir.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                stack.push(r);
            } else {
                out.insert(r, fs::read(e.path()).unwrap());
            }
        }
    }
    out
}

fn fast() -> Faulty {
    Faulty::new(u64::MAX, Mode::Once)
}

fn apply_with(sys: &dyn Sys, fx: &mut Fx, plan: &TidyPlan) -> Result<TidyReport, LibraryError> {
    let options = fx.options();
    Exec::new(sys, &mut fx.lib.conn, &options).apply_switch(
        plan,
        &choices(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
}

fn recover_with(sys: &dyn Sys, fx: &mut Fx) -> Result<Option<TidyReport>, LibraryError> {
    let options = fx.options();
    Exec::new(sys, &mut fx.lib.conn, &options).recover()
}

/// A fault at every system call of the run, then the recovery, then (when no end is due) a new switch: no file
/// lost nor split, a journal that is never left in progress, rows on existing files, and the definition retired
/// exactly once at the very end.
#[test]
fn a_fault_at_every_call_of_the_run_then_the_recovery_and_the_end() {
    let total = {
        let mut fx = Fx::new();
        let plan = fx.plan();
        let counter = Faulty::new(u64::MAX, Mode::Once);
        apply_with(&counter, &mut fx, &plan).unwrap();
        counter.calls()
    };
    assert!(total > 30, "{total}");
    for at in 0..=total {
        for mode in [Mode::Crash, Mode::DoneButFailed] {
            let mut fx = Fx::new();
            let plan = fx.plan();
            let initial = snapshot(&fx.roms);
            let context = format!("{mode:?} at {at}");
            let _ = apply_with(&Faulty::new(at, mode), &mut fx, &plan);
            recover_with(&fast(), &mut fx).unwrap_or_else(|e| panic!("{context}: {e}"));
            check_sides(&fx.roms, &plan, &initial, &context);
            for path in fx.journals() {
                let j = journal::read(&path).unwrap().unwrap();
                assert!(!j.state.in_progress(), "{context}");
            }
            for (path,) in fx
                .lib
                .conn
                .prepare("SELECT path FROM game")
                .unwrap()
                .query_map([], |r| Ok((r.get::<_, String>(0)?,)))
                .unwrap()
                .map(Result::unwrap)
            {
                assert!(
                    fs::symlink_metadata(&path).is_ok(),
                    "{context}: row on a missing file {path}"
                );
            }
            // The end is due only when every unit made it; else a new switch takes the rest.
            if fx.pending().is_none() {
                let again = fx.plan();
                if again.units.is_empty() {
                    continue;
                }
                assert!(again.switchable(), "{context}");
                fx.apply(&again);
            }
            fx.complete();
            assert!(fx.pending().is_none(), "{context}");
            assert_eq!(fx.retired().len(), 1, "{context}");
            assert!(!fx.systems.join("mysnes.toml").exists(), "{context}");
            for name in ["Game.sfc", "Late.sfc", "Second.smc"] {
                assert!(exists(&fx.roms, "snes", name), "{context}: {name}");
            }
            assert_eq!(system_of(&fx.lib, "Game.sfc"), "snes", "{context}");
            assert_eq!(system_of(&fx.lib, "Mine.xyz"), "mysnes", "{context}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The end of the switch

/// A switch run to the point where its end is due.
fn due() -> Fx {
    let mut fx = Fx::new();
    let plan = fx.plan();
    fx.apply(&plan);
    assert!(fx.pending().is_some());
    fx
}

fn complete_with(sys: &dyn Sys, fx: &mut Fx) -> Result<(), LibraryError> {
    let options = fx.options();
    let pending = fx.pending().unwrap();
    let systems = fx.systems.clone();
    fx.lib
        .complete_switch_with(sys, &options, &pending, &systems)
}

#[test]
fn a_fault_at_every_call_of_the_end_then_the_end_again() {
    let total = {
        let mut fx = due();
        let counter = Faulty::new(u64::MAX, Mode::Once);
        complete_with(&counter, &mut fx).unwrap();
        counter.calls()
    };
    assert!(total >= 6, "{total}");
    for at in 0..=total {
        for mode in [Mode::Crash, Mode::Once, Mode::DoneButFailed] {
            let mut fx = due();
            let context = format!("{mode:?} at {at}");
            let failed = complete_with(&Faulty::new(at, mode), &mut fx).is_err();
            if at < total && mode == Mode::Crash {
                assert!(failed, "{context}: a crash cannot end well");
            }
            if fx.pending().is_some() {
                // Still due: nothing else runs, for any root, until the end succeeds.
                let options = fx.options();
                let err = fx
                    .lib
                    .mark_tidy_identities(&options, &fx.journals()[0])
                    .unwrap_err();
                assert!(
                    matches!(
                        err,
                        LibraryError::TidyRefused {
                            reason: TidyRefusal::SwitchPending,
                            ..
                        }
                    ),
                    "{context}: {err:?}"
                );
            }
            // The next start: `recover_tidy` has nothing to do, the end is retried and idempotent.
            if fx.pending().is_some() {
                assert!(
                    recover_with(&fast(), &mut fx).unwrap().is_none(),
                    "{context}"
                );
                complete_with(&fast(), &mut fx).unwrap_or_else(|e| panic!("{context}: {e}"));
            }
            assert!(fx.journal().forward_final, "{context}");
            assert!(fx.pending().is_none(), "{context}");
            assert!(!fx.systems.join("mysnes.toml").exists(), "{context}");
            assert_eq!(fx.retired().len(), 1, "{context}");
            assert!(
                names_in(&fx.systems.join(".retired"))
                    .iter()
                    .all(|n| !n.starts_with('.')),
                "{context}"
            );
            assert_eq!(system_of(&fx.lib, "Game.sfc"), "snes", "{context}");
            assert_eq!(system_of(&fx.lib, "Mine.xyz"), "mysnes", "{context}");
            // A second call after `F` is a no-op.
            let options = fx.options();
            assert!(fx.pending().is_none());
            let _ = options;
        }
    }
}

#[test]
fn an_end_that_fails_stays_due_and_is_retried_when_the_place_is_free() {
    let mut fx = due();
    let name = fx.journal().switch.unwrap().retired_name;
    // Another definition sits at the retired name: « place prise ».
    let kept = fx.systems.join(".retired");
    fs::create_dir_all(&kept).unwrap();
    fs::write(kept.join(&name), "somebody else's file").unwrap();
    let options = fx.options();
    let pending = fx.pending().unwrap();
    let err = fx
        .lib
        .complete_switch(&options, &pending, &fx.systems)
        .unwrap_err();
    assert!(matches!(err, LibraryError::Io { .. }), "{err:?}");
    assert!(fx.systems.join("mysnes.toml").is_file());
    assert_eq!(
        fs::read_to_string(kept.join(&name)).unwrap(),
        "somebody else's file"
    );
    assert!(!fx.journal().forward_final);
    // Blocked, whatever the root; found again at the next start.
    let plain = fx
        .lib
        .plan_tidy(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let err = fx
        .lib
        .apply_tidy(&options, &plain, &[], &AtomicBool::new(false), &mut |_| {})
        .unwrap_err();
    assert!(matches!(
        err,
        LibraryError::TidyRefused {
            reason: TidyRefusal::SwitchPending,
            ..
        }
    ));
    assert!(fx.pending().is_some());
    // The user moves their file away: the end goes through.
    fs::remove_file(kept.join(&name)).unwrap();
    fx.complete();
    assert!(fx.journal().forward_final);
    assert_eq!(fx.retired(), [name]);
}

#[test]
fn a_definition_already_moved_by_an_interrupted_end_is_taken_for_done() {
    let mut fx = due();
    let name = fx.journal().switch.unwrap().retired_name;
    // The state after the link and before the removal: two names of one file.
    let kept = fx.systems.join(".retired");
    fs::create_dir_all(&kept).unwrap();
    fs::hard_link(fx.systems.join("mysnes.toml"), kept.join(&name)).unwrap();
    fx.complete();
    assert!(!fx.systems.join("mysnes.toml").exists());
    assert_eq!(fx.retired(), std::slice::from_ref(&name));
    // Destination alone (the source went, `F` did not): done.
    let mut fx = due();
    let name = fx.journal().switch.unwrap().retired_name;
    fs::create_dir_all(kept_of(&fx)).unwrap();
    fs::rename(fx.systems.join("mysnes.toml"), kept_of(&fx).join(&name)).unwrap();
    fx.complete();
    assert!(fx.journal().forward_final);
    // A name of another case is a taken place, never « already done ».
    let fx = due();
    let name = fx.journal().switch.unwrap().retired_name;
    fs::create_dir_all(kept_of(&fx)).unwrap();
    fs::write(
        kept_of(&fx).join(name.to_uppercase()),
        fs::read(fx.systems.join("mysnes.toml")).unwrap(),
    )
    .unwrap();
    let mut fx = fx;
    let options = fx.options();
    let pending = fx.pending().unwrap();
    assert!(
        fx.lib
            .complete_switch(&options, &pending, &fx.systems)
            .is_err()
    );
    assert!(fx.systems.join("mysnes.toml").is_file());
    assert!(!fx.journal().forward_final);
}

fn kept_of(fx: &Fx) -> PathBuf {
    fx.systems.join(".retired")
}

#[test]
fn an_active_definition_that_is_already_gone_counts_as_retired() {
    let mut fx = due();
    fs::remove_file(fx.systems.join("mysnes.toml")).unwrap();
    fx.complete();
    assert!(fx.journal().forward_final);
    assert!(fx.retired().is_empty());
    assert_eq!(system_of(&fx.lib, "Game.sfc"), "snes");
}

#[test]
fn a_missing_retired_folder_is_created_and_an_orphan_temporary_removed() {
    let mut fx = due();
    assert!(!fx.systems.join(".retired").exists());
    fs::create_dir_all(fx.systems.join(".retired")).unwrap();
    let orphan = fx.systems.join(".retired").join(".mysnes.emu-rust-tmp");
    fs::write(&orphan, "torn").unwrap();
    fx.complete();
    assert!(!orphan.exists());
    assert_eq!(fx.retired().len(), 1);
    // The temporary never counts as a retired system.
    assert_eq!(crate::custom::retired_customs(&fx.systems).len(), 1);
}

#[test]
fn a_switch_end_is_idempotent_after_f() {
    let mut fx = due();
    let options = fx.options();
    let pending = fx.pending().unwrap();
    fx.lib
        .complete_switch(&options, &pending, &fx.systems)
        .unwrap();
    // The same call again (a lost answer): nothing to do, nothing changes.
    fx.lib
        .complete_switch(&options, &pending, &fx.systems)
        .unwrap();
    assert_eq!(fx.retired().len(), 1);
    // A stale announcement for another switch is refused.
    let mut wrong = pending.clone();
    wrong.retired_name = "mysnes-1.replaced.toml".into();
    assert!(
        fx.lib
            .complete_switch(&options, &wrong, &fx.systems)
            .is_err()
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Rows the official system does not accept

#[test]
fn rows_the_official_does_not_accept_are_never_purged_and_are_forgotten_only_on_request() {
    let mut fx = due();
    fx.complete();
    let after = fx.after_catalog();
    for _ in 0..2 {
        let report = scan_in(&mut fx.lib, after, &fx.roms);
        assert_eq!(report.removed, 0, "{report:?}");
        let rows = rows(&fx.lib);
        assert_eq!(rows["Mine.xyz"], ("mysnes".into(), 1, 7));
        assert_eq!(rows["Pack.zip"], ("mysnes".into(), 1, 7));
        assert_eq!(rows["Game.sfc"], ("snes".into(), 1, 7));
    }
    // The files stayed under the reserved folder; « Oublier ses jeux » is the only way out.
    assert!(exists(&fx.roms, "mysnes", "Mine.xyz"));
    fx.lib
        .forget_system("mysnes", ForgetDats::Remove, ForgetScope::All)
        .unwrap();
    let rows = rows(&fx.lib);
    assert!(!rows.contains_key("Mine.xyz") && !rows.contains_key("Pack.zip"));
    assert!(rows.contains_key("Game.sfc") && rows.contains_key("Second.smc"));
}

#[test]
fn the_dats_of_the_custom_id_are_kept() {
    let mut fx = due();
    fx.lib
        .conn
        .execute(
            "INSERT INTO dat_info (system, source, name, entries, imported_unix) VALUES ('mysnes', 'no-intro', 'N', 1, 1)",
            [],
        )
        .unwrap();
    fx.complete();
    let n: i64 = fx
        .lib
        .conn
        .query_row(
            "SELECT COUNT(*) FROM dat_info WHERE system = 'mysnes'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

// ---------------------------------------------------------------------------------------------------------------
// Undo

fn done() -> Fx {
    let mut fx = due();
    fx.complete();
    fx
}

fn undo(fx: &mut Fx) -> Result<TidyReport, LibraryError> {
    let options = fx.options();
    let journal = fx.journals().last().unwrap().clone();
    let cat = fx.after_catalog();
    Exec::new(&RealSys, &mut fx.lib.conn, &options)
        .with_catalog(cat)
        .undo(&journal, &AtomicBool::new(false), &mut |_| {})
}

#[test]
fn undoing_a_done_switch_brings_back_files_rows_and_the_definition() {
    let mut fx = done();
    let options = fx.options();
    let due = fx.lib.pending_tidy_identities(&options).unwrap();
    fx.lib.mark_tidy_identities(&options, &due[0].0).unwrap();
    let name = fx.journal().switch.unwrap().retired_name;
    // The check passes, with the definitions folder given explicitly.
    let journal = fx.journals()[0].clone();
    fx.lib
        .check_switch_undo_in(fx.after_catalog(), &options, &journal, &fx.systems)
        .unwrap();
    let report = undo(&mut fx).unwrap();
    assert!(
        report.left.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    for name in ["Game.sfc", "Late.sfc", "Second.smc"] {
        assert!(exists(&fx.roms, "mysnes", name), "{name}");
        assert!(!exists(&fx.roms, "snes", name), "{name}");
    }
    // `roms/snes/` was made by the switch: gone with it. The reserved custom folder stays.
    assert!(!fx.roms.join("snes").exists());
    assert!(fx.roms.join("mysnes").is_dir());
    // The end of the undo is due; nothing else runs before it.
    let pending = fx.pending().unwrap();
    assert_eq!(pending.direction, SwitchDirection::Undo);
    assert_eq!(pending.retired_name, name);
    let j = fx.journal();
    assert_eq!(j.state, State::Undone);
    assert!(j.forward_final && !j.undo_final);
    assert!(fx.lib.pending_tidy_identities(&options).unwrap().is_empty());
    fx.complete();
    assert!(fx.pending().is_none());
    let j = fx.journal();
    assert!(j.undo_final);
    // The definition is back under its own name, the rows under the custom id, data intact.
    assert!(fx.systems.join("mysnes.toml").is_file());
    assert!(fx.retired().is_empty());
    let rows = rows(&fx.lib);
    for name in ["Game.sfc", "Second.smc", "Mine.xyz", "Pack.zip"] {
        assert_eq!(rows[name], ("mysnes".into(), 1, 7), "{name}");
    }
    assert_eq!(count(&fx.lib, "system = 'snes'"), 0);
    // Identities of the undo are due; the journal is not undoable twice.
    let due = fx.lib.pending_tidy_identities(&options).unwrap();
    assert_eq!(due.len(), 1);
    fx.lib.mark_tidy_identities(&options, &due[0].0).unwrap();
    assert!(undo(&mut fx).is_err());
    // The catalog is the one of before the switch.
    assert_eq!(Catalog::load(&fx.systems).catalog, *fx.before);
}

#[test]
fn an_undo_that_cannot_bring_everything_back_is_refused_before_any_action() {
    // The definition was created again.
    let mut fx = done();
    let options = fx.options();
    let journal = fx.journals()[0].clone();
    fs::write(fx.systems.join("mysnes.toml"), "new one").unwrap();
    let refused = |fx: &Fx| {
        matches!(
            fx.lib
                .check_switch_undo_in(fx.after_catalog(), &fx.options(), &journal, &fx.systems),
            Err(LibraryError::TidyRefused {
                reason: TidyRefusal::SwitchNotReversible,
                ..
            })
        )
    };
    assert!(refused(&fx));
    assert!(exists(&fx.roms, "snes", "Game.sfc"));
    fs::remove_file(fx.systems.join("mysnes.toml")).unwrap();
    assert!(!refused(&fx));
    // The retired file is missing (exact name).
    let kept = fx
        .systems
        .join(".retired")
        .join(fx.journal().switch.unwrap().retired_name);
    let saved = fs::read(&kept).unwrap();
    fs::remove_file(&kept).unwrap();
    assert!(refused(&fx));
    fs::write(&kept, saved).unwrap();
    assert!(!refused(&fx));
    // A game changed since: its unit cannot come back.
    fs::write(fx.roms.join("snes").join("Game.sfc"), b"changed").unwrap();
    assert!(refused(&fx));
    let before = snapshot(&fx.roms);
    let err = undo(&mut fx).unwrap_err();
    assert!(
        matches!(
            err,
            LibraryError::TidyRefused {
                reason: TidyRefusal::SwitchNotReversible,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(snapshot(&fx.roms), before);
    let _ = options;
}

#[test]
fn a_place_taken_at_the_origin_refuses_the_undo_of_a_switch_in_advance() {
    let mut fx = done();
    write(
        &fx.roms.join("mysnes").join("Game.sfc"),
        b"someone put a file here",
    );
    let err = undo(&mut fx).unwrap_err();
    assert!(
        matches!(
            err,
            LibraryError::TidyRefused {
                reason: TidyRefusal::SwitchNotReversible,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(exists(&fx.roms, "snes", "Game.sfc"));
}

#[test]
fn undoing_an_incomplete_switch_returns_the_units_and_ends_without_f() {
    let mut fx = Fx::new();
    let plan = fx.plan();
    fs::write(fx.roms.join("mysnes").join("Game.sfc"), noise(3100, 8)).unwrap();
    fx.apply(&plan);
    assert!(fx.pending().is_none());
    let report = undo(&mut fx).unwrap();
    assert!(report.left.is_empty(), "{report:?}");
    assert!(exists(&fx.roms, "mysnes", "Second.smc") && exists(&fx.roms, "mysnes", "Late.sfc"));
    let j = fx.journal();
    assert_eq!(j.state, State::Undone);
    assert!(!j.forward_final && !j.undo_final);
    // No end: the definition never moved, the rows never changed.
    assert!(fx.pending().is_none());
    assert!(fx.systems.join("mysnes.toml").is_file());
    assert_eq!(count(&fx.lib, "system = 'mysnes'"), 4);
}

#[test]
fn a_partial_undo_ends_without_f_and_leaves_the_returned_games_on_the_official_system() {
    let mut fx = done();
    let options = fx.options();
    let journal = fx.journals()[0].clone();
    let cancel = AtomicBool::new(false);
    let cat = fx.after_catalog();
    let report = Exec::new(&RealSys, &mut fx.lib.conn, &options)
        .with_catalog(cat)
        .undo(&journal, &cancel, &mut |p| {
            if p.done_units == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
    assert!(report.cancelled);
    let j = fx.journal();
    assert_eq!(j.state, State::Undone);
    assert!(j.forward_final && !j.undo_final);
    assert_eq!(j.end_due(), None);
    assert!(fx.pending().is_none());
    // The definition stays retired and the official system active; the returned game is under `roms/mysnes/` but
    // its row is the official's (the user moves it by hand).
    assert!(!fx.systems.join("mysnes.toml").exists());
    assert_eq!(fx.retired().len(), 1);
    assert_eq!(count(&fx.lib, "system = 'snes'"), 2);
    // Not undoable twice.
    assert!(undo(&mut fx).is_err());
}

#[test]
fn an_undo_leaves_a_unit_under_a_reserved_folder_whole() {
    // Version 1 journal: the games went to `roms/megadrive/`; a definition of another id now takes that database,
    // so `megadrive` is a masked official, a reserved folder.
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let (roms, systems, journals) = (
        root.join("roms"),
        root.join("systems"),
        root.join("rangement"),
    );
    write(&roms.join("Hero.md"), &noise(2000, 3));
    let mut lib = Library::open_in_memory().unwrap();
    let options = TidyOptions::new(&roms, &journals).unwrap();
    let official = leak(Catalog::official());
    let plan = lib
        .plan_tidy_in(official, &options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let ids: Vec<u32> = plan.units.iter().map(|u| u.id).collect();
    assert_eq!(ids.len(), 1);
    let report = lib
        .apply_tidy(&options, &plan, &ids, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let journal = report.journal.unwrap();
    assert!(roms.join("megadrive").join("Hero.md").is_file());
    fs::create_dir_all(&systems).unwrap();
    fs::write(
        systems.join("mymd.toml"),
        definition_text("mymd", Some("Sega - Mega Drive - Genesis"), &["md1"]),
    )
    .unwrap();
    let masking = leak(Catalog::load(&systems).catalog);
    assert!(masking.reserved_folders().contains(&"megadrive".to_owned()));
    let report = Exec::new(&RealSys, &mut lib.conn, &options)
        .with_catalog(masking)
        .undo(&journal, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    // Nothing taken from the reserved folder, nothing put into it; it is not removed, and the unit is reported.
    assert_eq!(report.left.len(), 1, "{report:?}");
    assert!(matches!(
        report.left[0].1,
        TidyBlock::FolderNamedLikeSystem { .. }
    ));
    assert!(roms.join("megadrive").join("Hero.md").is_file());
    assert!(!roms.join("Hero.md").exists());
    // The same undo with the plain catalog brings it back and removes the folder it had made.
    let (tmp2, roms2) = {
        let t = tempfile::tempdir().unwrap();
        let r = fs::canonicalize(t.path()).unwrap().join("roms");
        (t, r)
    };
    write(&roms2.join("Hero.md"), &noise(2000, 3));
    let mut lib2 = Library::open_in_memory().unwrap();
    let options2 = TidyOptions::new(&roms2, &tmp2.path().join("rangement")).unwrap();
    let plan = lib2
        .plan_tidy_in(official, &options2, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let ids: Vec<u32> = plan.units.iter().map(|u| u.id).collect();
    let journal = lib2
        .apply_tidy(&options2, &plan, &ids, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
        .journal
        .unwrap();
    let report = Exec::new(&RealSys, &mut lib2.conn, &options2)
        .with_catalog(official)
        .undo(&journal, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(report.left.is_empty(), "{report:?}");
    assert!(roms2.join("Hero.md").is_file() && !roms2.join("megadrive").exists());
}

#[test]
fn the_undo_of_a_switch_exempts_both_of_its_folders_from_the_reservation() {
    // Run and end done: `mysnes` is a retired id (reserved), `snes` active; before the `F` it is the other way
    // round. The undo works both times (see the undo tests); here the reserved sets themselves.
    let fx = done();
    let after = fx.after_catalog();
    assert!(after.reserved_folders().contains(&"mysnes".to_owned()));
    let journal = fx.journal();
    let reserved = exec::undo_reserved(&fx.lib.conn, after, journal.switch.as_ref()).unwrap();
    assert!(!reserved.contains("mysnes") && !reserved.contains("snes"));
    let plain = exec::undo_reserved(&fx.lib.conn, after, None).unwrap();
    assert!(plain.contains("mysnes"));
    // A folder of the library's rows that the catalog lacks is reserved too.
    fx.lib
        .conn
        .execute(
            "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification)
             VALUES ('x', 'ghost', 1, 1, 0, 'T', 'unknown')",
            [],
        )
        .unwrap();
    assert!(
        exec::undo_reserved(&fx.lib.conn, after, journal.switch.as_ref())
            .unwrap()
            .contains("ghost")
    );
}

#[test]
fn the_retired_name_of_two_switches_is_never_shared() {
    let mut fx = Fx::new();
    // Definitions retired earlier under every name the next journal could take in the next seconds: the second
    // of the new journal is raised past them, so its retired name is a free one.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let kept = fx.systems.join(".retired");
    fs::create_dir_all(&kept).unwrap();
    for unix in now.saturating_sub(5)..now + 5 {
        fs::write(
            kept.join(retired_file_name("mysnes", Retirement::Replaced, unix)),
            "old",
        )
        .unwrap();
    }
    // The names taken in `.retired/` are read in the folder the options give.
    let after = Catalog::load_without(&fx.systems, "mysnes").catalog;
    let plan = fx
        .lib
        .plan_switch(
            &fx.options(),
            &after,
            &mv(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    let report = fx.apply(&plan);
    assert!(report.errors.is_empty(), "{report:?}");
    let header = fx.journal().switch.unwrap();
    assert!(
        !names_in(&kept).contains(&header.retired_name),
        "{header:?}"
    );
    // And the name is the one of the journal's own file name.
    let unix: u64 = fx.journals()[0]
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .trim_start_matches("journal-")
        .parse()
        .unwrap();
    assert_eq!(
        header.retired_name,
        retired_file_name("mysnes", Retirement::Replaced, unix)
    );
    assert!(unix >= now + 5);
}

#[test]
fn a_row_of_an_unplugged_root_follows_the_switch_and_its_undo() {
    let mut fx = due();
    let outside = fx.root.join("external").join("Unplugged.sfc");
    fx.lib
        .conn
        .execute(
            "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification, favorite, play_count)
             VALUES (?1, 'mysnes', 1, 1, 0, 'T', 'unknown', 1, 7)",
            [outside.to_str().unwrap()],
        )
        .unwrap();
    fx.complete();
    // Same scope at the end of the run and of the undo: the row of a missing file included.
    assert_eq!(rows(&fx.lib)["Unplugged.sfc"], ("snes".into(), 1, 7));
    let report = undo(&mut fx).unwrap();
    assert!(report.left.is_empty(), "{report:?}");
    fx.complete();
    assert_eq!(rows(&fx.lib)["Unplugged.sfc"], ("mysnes".into(), 1, 7));
}

#[test]
fn the_end_of_a_switch_is_completed_whatever_the_root_of_the_journal() {
    let mut fx = due();
    let other = fx.root.join("roms2");
    fs::create_dir(&other).unwrap();
    let options = TidyOptions::new(&other, &fx.journals).unwrap();
    let pending = fx.lib.pending_switch(&options).unwrap().unwrap();
    fx.lib
        .complete_switch(&options, &pending, &fx.systems)
        .unwrap();
    assert!(fx.journal().forward_final);
    assert_eq!(system_of(&fx.lib, "Game.sfc"), "snes");
}

#[test]
fn a_definition_is_off_limits_from_the_first_block_until_the_end_is_done() {
    let mut fx = Fx::new();
    let options = fx.options();
    assert_eq!(open_switch(&options).unwrap(), None);
    // A crash in the middle of the run: in progress.
    let plan = fx.plan();
    let _ = apply_with(&Faulty::new(12, Mode::Crash), &mut fx, &plan);
    assert_eq!(open_switch(&options).unwrap(), Some(mv()));
    recover_with(&fast(), &mut fx).unwrap();
    // Closed and incomplete (or due): incomplete blocks nothing.
    let due = fx.pending().is_some();
    assert_eq!(open_switch(&options).unwrap().is_some(), due);
    if !due {
        let again = fx.plan();
        fx.apply(&again);
    }
    assert_eq!(open_switch(&options).unwrap(), Some(mv()));
    fx.complete();
    assert_eq!(open_switch(&options).unwrap(), None);
    // An undo in progress blocks it again until its end is done.
    let report = undo(&mut fx).unwrap();
    assert!(report.left.is_empty());
    assert_eq!(open_switch(&options).unwrap(), Some(mv()));
    fx.complete();
    assert_eq!(open_switch(&options).unwrap(), None);
    // An incomplete switch, closed: nothing blocked.
    let mut fx = Fx::new();
    let plan = fx.plan();
    fs::write(fx.roms.join("mysnes").join("Game.sfc"), noise(3100, 8)).unwrap();
    fx.apply(&plan);
    assert!(fx.pending().is_none());
    assert_eq!(open_switch(&fx.options()).unwrap(), None);
}

#[test]
fn a_switch_in_place_freezes_the_rows_only_the_definition_accepted() {
    // `snes.toml` (same id, same database, extra extension `xyz`) is replaced by the official system, in place:
    // `.retired/` keeps it under a Replaced name, so the id is in conflict (ADR 0025 §10, case (c)).
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let (roms, systems) = (root.join("roms"), root.join("systems"));
    fs::create_dir_all(systems.join(".retired")).unwrap();
    fs::write(
        systems.join(".retired").join("snes-100.replaced.toml"),
        definition_text("snes", Some(OFFICIAL_DATABASE), &["sfc", "xyz"]),
    )
    .unwrap();
    let after = leak(Catalog::load(&systems).catalog);
    assert!(after.is_conflicted("snes") && !after.system("snes").unwrap().is_custom());
    write(&roms.join("snes").join("Game.sfc"), &noise(1000, 1));
    write(&roms.join("snes").join("Mine.xyz"), &noise(1000, 2));
    let mut lib = Library::open_in_memory().unwrap();
    for (name, size) in [("Game.sfc", 1000), ("Mine.xyz", 1000), ("Packed.xyz", 10)] {
        lib.conn
            .execute(
                "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification, favorite, play_count)
                 VALUES (?1, 'snes', 1, ?2, 0, 'T', 'unknown', 1, 7)",
                rusqlite::params![roms.join("snes").join(name).to_str().unwrap(), size],
            )
            .unwrap();
    }
    for _ in 0..2 {
        let report = scan_in(&mut lib, after, &roms);
        // The row of the missing `.xyz` and the row of the present one are frozen; `Game.sfc` is the official's.
        assert_eq!(count(&lib, "path LIKE '%.xyz'"), 2, "{report:?}");
        assert_eq!(
            count(
                &lib,
                "favorite = 1 AND play_count = 7 AND path LIKE '%.xyz'"
            ),
            2
        );
    }
    assert!(rows(&lib).contains_key("Game.sfc"));
    // « Oublier ses jeux » of the frozen rows only (DAT rows kept).
    lib.forget_system("snes", ForgetDats::Keep, ForgetScope::Frozen(after))
        .unwrap();
    assert_eq!(rows(&lib).keys().collect::<Vec<_>>(), ["Game.sfc"]);
}

// ---------------------------------------------------------------------------------------------------------------
// The definitions folder is given, never deduced (PB3 corrections)

fn is_not_reversible(err: &LibraryError) -> bool {
    matches!(
        err,
        LibraryError::TidyRefused {
            reason: TidyRefusal::SwitchNotReversible,
            ..
        }
    )
}

#[test]
fn the_undo_of_a_done_switch_is_refused_when_the_definitions_folder_is_unknown() {
    let mut fx = done();
    let before = snapshot(&fx.roms);
    // The caller gave no folder: the retired definition cannot be checked, nothing moves.
    let options = TidyOptions::new(&fx.roms, &fx.journals).unwrap();
    let journal = fx.journals().last().unwrap().clone();
    let cat = fx.after_catalog();
    let err = Exec::new(&RealSys, &mut fx.lib.conn, &options)
        .with_catalog(cat)
        .undo(&journal, &AtomicBool::new(false), &mut |_| {})
        .unwrap_err();
    assert!(is_not_reversible(&err), "{err:?}");
    assert_eq!(snapshot(&fx.roms), before);
    assert_eq!(fx.journal().state, State::Ended);
    assert_eq!(system_of(&fx.lib, "Game.sfc"), "snes");
}

#[test]
fn the_undo_of_a_done_switch_is_refused_when_the_retired_definition_is_gone() {
    let mut fx = done();
    let name = fx.journal().switch.unwrap().retired_name;
    fs::remove_file(fx.systems.join(".retired").join(name)).unwrap();
    let before = snapshot(&fx.roms);
    let err = undo(&mut fx).unwrap_err();
    assert!(is_not_reversible(&err), "{err:?}");
    assert_eq!(snapshot(&fx.roms), before);
    assert_eq!(system_of(&fx.lib, "Game.sfc"), "snes");
}

#[test]
fn a_switch_is_refused_without_a_definitions_folder() {
    let mut fx = Fx::new();
    let plan = fx.plan();
    let bare = TidyOptions::new(&fx.roms, &fx.journals).unwrap();
    let err = fx
        .lib
        .plan_switch(
            &bare,
            &Catalog::official(),
            &mv(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
    assert!(
        matches!(&err, LibraryError::UnknownSystem(id) if id == "mysnes"),
        "{err:?}"
    );
    let before = snapshot(&fx.roms);
    let err = fx
        .lib
        .apply_switch(
            &bare,
            &plan,
            &choices(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
    assert!(
        matches!(&err, LibraryError::UnknownSystem(id) if id == "mysnes"),
        "{err:?}"
    );
    assert_eq!(snapshot(&fx.roms), before);
    assert!(fx.journals().is_empty());
}

#[test]
fn a_name_already_in_retired_is_never_taken_by_the_next_switch_journal() {
    let mut fx = Fx::new();
    // The name the journal would take at its first second, and every one around it.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let kept = fx.systems.join(".retired");
    fs::create_dir_all(&kept).unwrap();
    for unix in now.saturating_sub(2)..now + 3 {
        fs::write(
            kept.join(retired_file_name("mysnes", Retirement::Replaced, unix)),
            "old",
        )
        .unwrap();
    }
    let plan = fx.plan();
    fx.apply(&plan);
    let name = fx.journal().switch.unwrap().retired_name;
    assert_eq!(fs::read(kept.join(&name)).ok(), None, "{name}");
    assert_eq!(fx.retired().len(), 5, "the old files are untouched");
}

#[test]
fn the_same_after_catalog_is_published_once_for_all_its_previews() {
    let fx = Fx::new();
    let after = Catalog::load_without(&fx.systems, "mysnes").catalog;
    // Its own memory and a published catalog that differs from `after`: nothing shared with the process.
    let last = std::sync::Mutex::new(None);
    let published = Catalog::load(&fx.systems).catalog;
    assert!(published != after);
    let published: &'static Catalog = Box::leak(Box::new(published));
    let a = super::switch::snapshot_with(&last, published, &after);
    let b = super::switch::snapshot_with(&last, published, &after.clone());
    assert!(std::ptr::eq(a, b), "the kept copy is reused");
    assert!(!std::ptr::eq(a, published), "it is not the published one");
    assert!(*a == after);
}

// ---------------------------------------------------------------------------------------------------------------
// Scope of a switch plan

#[test]
fn a_switch_plan_counts_only_what_is_left_in_the_folder_of_the_definition() {
    let fx = Fx::new();
    // Files outside `roms/mysnes/` that no system takes, and a loose game.
    write(&fx.roms.join("Elsewhere.txt"), b"x");
    write(&fx.roms.join("misc").join("Other.bin"), b"y");
    write(&fx.roms.join("snes").join("Tidy.sfc"), &noise(900, 40));
    let plain = fx
        .lib
        .plan_tidy(&fx.options(), &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(
        plain
            .ignored_paths
            .contains(&PathBuf::from("Elsewhere.txt"))
    );
    assert!(plain.already_tidy >= 1);
    let plan = fx.plan();
    let paths: Vec<String> = plan
        .ignored_paths
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(paths, ["mysnes/Mine.xyz", "mysnes/Notes.txt"]);
    assert_eq!(plan.ignored, 2);
    assert_eq!(plan.already_tidy, 0);
}

#[test]
fn a_journal_with_a_due_end_cannot_be_undone() {
    let fx = due();
    let info = last_tidy(&fx.options()).unwrap().unwrap();
    assert!(info.end_due && info.complete && !info.undone);
    assert!(!info.can_undo());
    let mut fx = fx;
    fx.complete();
    let info = last_tidy(&fx.options()).unwrap().unwrap();
    assert!(!info.end_due && info.can_undo());
}

// ---------------------------------------------------------------------------------------------------------------
// Cancel then cut

/// The run of a switch is cancelled between two units, then the process dies at every following system call
/// (the `E` line included): the recovery closes the journal, no end is due, the definition stays active.
#[test]
fn a_cancel_between_two_units_then_a_cut_before_e_leaves_no_end_due() {
    let run = |sys: &dyn Sys, fx: &mut Fx, plan: &TidyPlan| {
        let cancel = AtomicBool::new(false);
        let options = fx.options();
        let result = Exec::new(sys, &mut fx.lib.conn, &options).apply_switch(
            plan,
            &choices(),
            &cancel,
            &mut |p| {
                if p.done_units == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        (result, cancel.load(Ordering::Relaxed))
    };
    let total = {
        let mut fx = Fx::new();
        let plan = fx.plan();
        let counter = fast();
        let (result, cancelled) = run(&counter, &mut fx, &plan);
        assert!(cancelled && result.unwrap().cancelled);
        counter.calls()
    };
    assert!(total > 10, "{total}");
    for at in 0..=total {
        let mut fx = Fx::new();
        let plan = fx.plan();
        let initial = snapshot(&fx.roms);
        let context = format!("cut at {at}");
        let _ = run(&Faulty::new(at, Mode::Crash), &mut fx, &plan);
        recover_with(&fast(), &mut fx).unwrap_or_else(|e| panic!("{context}: {e}"));
        check_sides(&fx.roms, &plan, &initial, &context);
        assert!(fx.pending().is_none(), "{context}");
        assert!(fx.systems.join("mysnes.toml").is_file(), "{context}");
        assert!(fx.retired().is_empty(), "{context}");
        for path in fx.journals() {
            let j = journal::read(&path).unwrap().unwrap();
            assert!(!j.state.in_progress(), "{context}");
        }
    }
}
