//! Execution, undo, recovery: real files under a temporary folder, faults injected after every system call
//! (FV0 15). Invariant checked everywhere: no file lost nor overwritten, every unit entirely on one side, no
//! assembly folder left, the database rows on existing files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::*;
use crate::tidy::journal::{Journal, JournalWriter, Op, State, header};
use crate::tidy::sys::RealSys;
use crate::tidy::sys::faulty::{Faulty, Mode};
use crate::tidy::tests::{cd, noise, psx_track, write};
use crate::{Library, ScanOptions, TidyOptions, TidyPlan};

fn rel(s: &str) -> PathBuf {
    s.split('/').collect()
}

struct Fx {
    tmp: tempfile::TempDir,
    roms: PathBuf,
    journals: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let roms = tmp.path().join("roms");
        fs::create_dir(&roms).unwrap();
        let journals = tmp.path().join("rangement");
        Fx {
            tmp,
            roms,
            journals,
        }
    }

    fn options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.journals).unwrap()
    }

    fn plan(&self, library: &Library) -> TidyPlan {
        library
            .plan_tidy(&self.options(), &AtomicBool::new(false), &mut |_| {})
            .unwrap()
    }

    fn scan(&self, library: &mut Library) {
        let options = ScanOptions {
            roots: vec![self.roms.clone()],
            ..Default::default()
        };
        library
            .scan(&options, &AtomicBool::new(false), &mut |_| {})
            .unwrap();
    }

    fn journals(&self) -> Vec<PathBuf> {
        journal::list(&self.journals)
            .unwrap()
            .into_iter()
            .map(|l| l.path)
            .collect()
    }

    fn only_journal(&self) -> Journal {
        let all = self.journals();
        assert_eq!(all.len(), 1, "{all:?}");
        journal::read(&all[0]).unwrap().unwrap()
    }

    fn other_root() -> &'static Path {
        Path::new(if cfg!(windows) {
            r"C:\elsewhere"
        } else {
            "/elsewhere"
        })
    }
}

/// Every regular file under `root` with its content.
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

fn has_part_folder(root: &Path) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap() {
            let e = e.unwrap();
            if e.file_name().to_string_lossy().ends_with(".emu-rust-part") {
                return true;
            }
            if e.file_type().unwrap().is_dir() {
                stack.push(e.path());
            }
        }
    }
    false
}

/// The test tree: flat file, file in a folder emptied by the run, disk group, assembled CD (`roms/psx`
/// absent), CD folder moved whole, DOS game folder, a stray file.
fn tree(roms: &Path) {
    write(&roms.join("Hero.sfc"), &noise(4096, 1));
    write(&roms.join("sub/Hero2.sfc"), &noise(4096, 9));
    write(&roms.join("Quest (Disk 1 of 2).st"), &noise(900, 2));
    write(&roms.join("Quest (Disk 2 of 2).st"), &noise(900, 3));
    deep_cd(roms);
    cd(
        &roms.join("cds/Other (Europe)"),
        "Other (Europe)",
        1,
        psx_track,
    );
    write(&roms.join("Doom/DOOM.EXE"), &noise(500, 4));
    write(&roms.join("Doom/DOOM.WAD"), &noise(700, 5));
}

/// A `.cue` and its tracks next to a file that is not part of it (assembled, not moved whole).
fn deep_cd(roms: &Path) {
    cd(&roms.join("vrac"), "Deep (Europe)", 1, psx_track);
    write(&roms.join("vrac/readme.txt"), b"x");
}

/// Units that may be selected (sure, suggested or resolved).
fn selectable(plan: &TidyPlan) -> Vec<u32> {
    plan.units
        .iter()
        .filter(|u| {
            matches!(u.status, TidyStatus::Sure(_) | TidyStatus::Suggested(_))
                || (u.status == TidyStatus::Ambiguous && u.system.is_some())
        })
        .map(|u| u.id)
        .collect()
}

/// `(before, after)` file maps of every selectable unit.
type Sides = Vec<(BTreeMap<PathBuf, Vec<u8>>, BTreeMap<PathBuf, Vec<u8>>)>;

fn sides(plan: &TidyPlan, initial: &BTreeMap<PathBuf, Vec<u8>>) -> Sides {
    let chosen = selectable(plan);
    plan.units
        .iter()
        .filter(|u| chosen.contains(&u.id))
        .map(|u| {
            let before = u
                .moves
                .iter()
                .map(|m| (m.from.clone(), initial[&m.from].clone()))
                .collect();
            let after = u
                .moves
                .iter()
                .map(|m| (m.to.clone(), initial[&m.from].clone()))
                .collect();
            (before, after)
        })
        .collect()
}

/// FV0 15: no file lost nor overwritten, every unit entirely on one side, nothing else changed.
fn check_invariant(
    roms: &Path,
    initial: &BTreeMap<PathBuf, Vec<u8>>,
    sides: &Sides,
    context: &str,
) {
    let now = snapshot(roms);
    let mut expected: BTreeMap<PathBuf, Vec<u8>> = initial.clone();
    for (before, after) in sides {
        let at_source = before.iter().all(|(p, c)| now.get(p) == Some(c));
        let at_dest = after.iter().all(|(p, c)| now.get(p) == Some(c));
        assert!(
            at_source ^ at_dest,
            "{context}: unit split or lost: {before:?}\n{:?}",
            now.keys().collect::<Vec<_>>()
        );
        if at_dest {
            for p in before.keys() {
                expected.remove(p);
            }
            expected.extend(after.clone());
        }
    }
    assert_eq!(
        now.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>(),
        "{context}: extra or missing files"
    );
    assert_eq!(now, expected, "{context}: content changed");
    assert!(!has_part_folder(roms), "{context}: assembly folder left");
}

fn db_paths_exist(library: &Library, context: &str) {
    for sql in ["SELECT path FROM game", "SELECT path FROM game_media"] {
        let mut stmt = library.conn.prepare(sql).unwrap();
        let paths: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for p in paths {
            assert!(
                fs::symlink_metadata(&p).is_ok(),
                "{context}: row on a missing file {p}"
            );
        }
    }
}

fn game_ids(library: &Library) -> BTreeMap<i64, String> {
    let mut stmt = library.conn.prepare("SELECT id, path FROM game").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn apply_with(
    sys: &dyn Sys,
    library: &mut Library,
    options: &TidyOptions,
    plan: &TidyPlan,
) -> Result<TidyReport, LibraryError> {
    let selected = selectable(plan);
    Exec::new(sys, &mut library.conn, options).apply(
        plan,
        &selected,
        &AtomicBool::new(false),
        &mut |_| {},
    )
}

fn recover_with(
    sys: &dyn Sys,
    library: &mut Library,
    options: &TidyOptions,
) -> Result<Option<TidyReport>, LibraryError> {
    Exec::new(sys, &mut library.conn, options).recover()
}

fn undo_with(
    sys: &dyn Sys,
    library: &mut Library,
    options: &TidyOptions,
    journal: &Path,
) -> Result<TidyReport, LibraryError> {
    Exec::new(sys, &mut library.conn, options).undo(journal, &AtomicBool::new(false), &mut |_| {})
}

/// Fresh tree, scanned library, plan, initial snapshot.
fn setup() -> (Fx, Library, TidyPlan, BTreeMap<PathBuf, Vec<u8>>) {
    let fx = Fx::new();
    tree(&fx.roms);
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let plan = fx.plan(&library);
    let initial = snapshot(&fx.roms);
    (fx, library, plan, initial)
}

fn calls_of(run: impl FnOnce(&Faulty)) -> u64 {
    let counter = Faulty::new(u64::MAX, Mode::Once);
    run(&counter);
    counter.calls()
}

fn last_tidy_can_undo(options: &TidyOptions) -> bool {
    crate::last_tidy(options)
        .unwrap()
        .is_some_and(|i| i.can_undo())
}

#[test]
fn a_full_run_moves_every_selected_unit_and_keeps_the_rows() {
    let (fx, mut library, plan, initial) = setup();
    let options = fx.options();
    assert_eq!(selectable(&plan).len(), 6, "{:#?}", plan.units);
    let ids_before = game_ids(&library);
    let report = apply_with(&RealSys, &mut library, &options, &plan).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.left.is_empty(), "{:?}", report.left);
    assert!(!report.cancelled);
    let units = sides(&plan, &initial);
    check_invariant(&fx.roms, &initial, &units, "full run");
    for (_, after) in &units {
        assert!(after.keys().all(|p| fx.roms.join(p).is_file()));
    }
    // Emptied folders are gone, the stray file stayed.
    assert!(!fx.roms.join("sub").exists());
    assert!(!fx.roms.join("cds").exists());
    assert!(!fx.roms.join("Doom").exists());
    assert!(fx.roms.join("vrac/readme.txt").is_file());
    assert!(
        fx.roms
            .join("psx/Deep (Europe)/Deep (Europe) (Track 1).bin")
            .is_file()
    );
    let j = fx.only_journal();
    assert_eq!(j.state, State::Ended);
    assert!(j.forward.iter().all(|b| b.committed));
    // `roms/psx` was absent: exactly one block wrote its `D+`, the one that created it (RV1 I4).
    let psx_lines: usize = j
        .forward
        .iter()
        .map(|b| {
            b.body
                .iter()
                .filter(|op| **op == Op::DirCreated(rel("psx")))
                .count()
        })
        .sum();
    assert_eq!(psx_lines, 1);
    assert_eq!(report.dirs_removed, 2, "sub, cds (Doom is moved whole)");
    // Net list = report, absolute under the canonical root.
    assert_eq!(report.moved.len(), j.net_moves().len());
    assert!(
        report
            .moved
            .iter()
            .all(|(a, b)| a.starts_with(options.roms_root()) && b.starts_with(options.roms_root()))
    );
    assert_eq!(report.journal.as_deref(), Some(fx.journals()[0].as_path()));
    // Rows kept their ids and follow the files.
    let ids_after = game_ids(&library);
    assert_eq!(
        ids_before.keys().collect::<Vec<_>>(),
        ids_after.keys().collect::<Vec<_>>()
    );
    assert_ne!(ids_before, ids_after);
    db_paths_exist(&library, "full run");
    // Recovery has nothing to do; identities are due once.
    assert_eq!(
        recover_with(&RealSys, &mut library, &options).unwrap(),
        None
    );
    let pending = library.pending_tidy_identities(&options).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].1, report.moved);
    library
        .mark_tidy_identities(&options, &pending[0].0)
        .unwrap();
    library
        .mark_tidy_identities(&options, &pending[0].0)
        .unwrap();
    assert!(
        library
            .pending_tidy_identities(&options)
            .unwrap()
            .is_empty()
    );
    assert_eq!(fx.only_journal().state, State::EndedMarked);
    assert!(last_tidy_can_undo(&options));
}

#[test]
fn an_existing_system_folder_gets_no_dir_line() {
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    fs::create_dir(fx.roms.join("snes")).unwrap();
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    apply_with(&RealSys, &mut library, &fx.options(), &plan).unwrap();
    let j = fx.only_journal();
    assert!(
        !j.forward[0]
            .body
            .iter()
            .any(|op| matches!(op, Op::DirCreated(_)))
    );
}

/// FV0 15 and RV1 I2: a fault at every system call of the run, then the recovery. In `Crash` mode the in-run
/// undo cannot run either: the run fails without closing, the journal stays in progress.
#[test]
fn fault_at_every_call_of_the_run_then_recovery() {
    let total = calls_of(|sys| {
        let (fx, mut library, plan, _) = setup();
        apply_with(sys, &mut library, &fx.options(), &plan).unwrap();
    });
    assert!(total > 40, "{total}");
    for at in 0..=total {
        for mode in [Mode::Crash, Mode::Once] {
            let (fx, mut library, plan, initial) = setup();
            let options = fx.options();
            let units = sides(&plan, &initial);
            let context = format!("{mode:?} at {at}");
            let result = apply_with(&Faulty::new(at, mode), &mut library, &options, &plan);
            if mode == Mode::Crash && at < total {
                assert!(result.is_err(), "{context}: a crash cannot end well");
                for path in fx.journals() {
                    if let Some(j) = journal::read(&path).unwrap() {
                        assert_eq!(j.state, State::Running, "{context}: closed after a crash");
                    }
                }
            }
            recover_with(&RealSys, &mut library, &options)
                .unwrap_or_else(|e| panic!("{context}: {e}"));
            check_invariant(&fx.roms, &initial, &units, &context);
            db_paths_exist(&library, &context);
            for path in fx.journals() {
                let j = journal::read(&path).unwrap().unwrap();
                assert!(!j.state.in_progress(), "{context}");
            }
        }
    }
}

/// A crash during the recovery itself (after each of its calls), then a second recovery (idempotent).
#[test]
fn crash_during_the_recovery_then_recovery_again() {
    let total = calls_of(|sys| {
        let (fx, mut library, plan, _) = setup();
        apply_with(sys, &mut library, &fx.options(), &plan).unwrap();
    });
    let crashed = |at: u64| {
        let (fx, mut library, plan, initial) = setup();
        let _ = apply_with(
            &Faulty::new(at, Mode::Crash),
            &mut library,
            &fx.options(),
            &plan,
        );
        (fx, library, plan, initial)
    };
    for at in (0..total).step_by(3) {
        let recovery_calls = calls_of(|sys| {
            let (fx, mut library, _, _) = crashed(at);
            recover_with(sys, &mut library, &fx.options()).unwrap();
        });
        for again in 0..recovery_calls {
            let (fx, mut library, plan, initial) = crashed(at);
            let options = fx.options();
            let units = sides(&plan, &initial);
            let context = format!("run crash at {at}, recovery crash at {again}");
            let failed = recover_with(&Faulty::new(again, Mode::Crash), &mut library, &options);
            assert!(failed.is_err(), "{context}");
            for path in fx.journals() {
                if let Ok(Some(j)) = journal::read(&path) {
                    // RV1 I2: no closing line after a failed recovery (an empty file may have been removed).
                    assert!(
                        j.state.in_progress(),
                        "{context}: closed after a failed recovery"
                    );
                }
            }
            recover_with(&RealSys, &mut library, &options)
                .unwrap_or_else(|e| panic!("{context}: {e}"));
            check_invariant(&fx.roms, &initial, &units, &context);
            db_paths_exist(&library, &context);
        }
    }
}

/// Undo of a full run with a fault at every call of the undo, then the recovery (state 4 → 5).
#[test]
fn fault_at_every_call_of_the_undo_then_recovery() {
    let prepared = || {
        let (fx, mut library, plan, initial) = setup();
        apply_with(&RealSys, &mut library, &fx.options(), &plan).unwrap();
        let journal = fx.journals()[0].clone();
        (fx, library, plan, initial, journal)
    };
    let total = calls_of(|sys| {
        let (fx, mut library, _, _, journal) = prepared();
        let report = undo_with(sys, &mut library, &fx.options(), &journal).unwrap();
        assert!(
            report.left.is_empty() && report.errors.is_empty(),
            "{report:?}"
        );
    });
    for at in 0..=total {
        for mode in [Mode::Crash, Mode::Once] {
            let (fx, mut library, plan, initial, journal) = prepared();
            let options = fx.options();
            let units = sides(&plan, &initial);
            let context = format!("undo {mode:?} at {at}");
            let result = undo_with(&Faulty::new(at, mode), &mut library, &options, &journal);
            if mode == Mode::Crash && at < total {
                assert!(result.is_err(), "{context}");
            }
            recover_with(&RealSys, &mut library, &options)
                .unwrap_or_else(|e| panic!("{context}: {e}"));
            check_invariant(&fx.roms, &initial, &units, &context);
            db_paths_exist(&library, &context);
            let j = journal::read(&journal).unwrap().unwrap();
            assert!(!j.state.in_progress(), "{context}");
            if j.state == State::Undone {
                let now = snapshot(&fx.roms);
                for (_, current) in j.net_moves() {
                    assert!(
                        now.keys().any(|p| p.starts_with(&current)),
                        "{context}: net list points to a missing place {current:?}"
                    );
                }
            }
        }
    }
}

/// A crash during the recovery of an interrupted undo, then the recovery again.
#[test]
fn crash_during_the_recovery_of_an_undo() {
    let interrupted = |at: u64| {
        let (fx, mut library, plan, initial) = setup();
        apply_with(&RealSys, &mut library, &fx.options(), &plan).unwrap();
        let journal = fx.journals()[0].clone();
        let _ = undo_with(
            &Faulty::new(at, Mode::Crash),
            &mut library,
            &fx.options(),
            &journal,
        );
        (fx, library, plan, initial)
    };
    for at in [5u64, 9, 14, 20] {
        let recovery_calls = calls_of(|sys| {
            let (fx, mut library, _, _) = interrupted(at);
            recover_with(sys, &mut library, &fx.options()).unwrap();
        });
        for again in 0..recovery_calls {
            let (fx, mut library, plan, initial) = interrupted(at);
            let options = fx.options();
            let context = format!("undo crash at {at}, recovery crash at {again}");
            assert!(
                recover_with(&Faulty::new(again, Mode::Crash), &mut library, &options).is_err(),
                "{context}"
            );
            recover_with(&RealSys, &mut library, &options)
                .unwrap_or_else(|e| panic!("{context}: {e}"));
            check_invariant(&fx.roms, &initial, &sides(&plan, &initial), &context);
            db_paths_exist(&library, &context);
        }
    }
}

#[test]
fn undo_brings_everything_back_with_the_same_rows() {
    let (fx, mut library, plan, initial) = setup();
    let options = fx.options();
    let ids_before = game_ids(&library);
    apply_with(&RealSys, &mut library, &options, &plan).unwrap();
    let journal = fx.journals()[0].clone();
    let report = library
        .undo_tidy(&options, &journal, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(
        report.left.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    assert_eq!(snapshot(&fx.roms), initial);
    assert!(fx.roms.join("sub").is_dir() && fx.roms.join("Doom").is_dir());
    for created in ["psx", "snes", "atarist", "dos"] {
        assert!(
            !fx.roms.join(created).exists(),
            "{created}: created by the run, removed"
        );
    }
    assert_eq!(game_ids(&library), ids_before);
    let j = journal::read(&journal).unwrap().unwrap();
    assert_eq!(j.state, State::Undone);
    // The net list of the undone journal points to where the files are now.
    assert_eq!(report.moved.len(), j.net_moves().len());
    for (stale, current) in &report.moved {
        assert!(current.exists() && !stale.exists(), "{current:?}");
    }
    assert_eq!(
        library.pending_tidy_identities(&options).unwrap()[0].1,
        report.moved
    );
    // Only once.
    assert!(matches!(
        undo_with(&RealSys, &mut library, &options, &journal),
        Err(LibraryError::Journal { .. })
    ));
    assert!(!last_tidy_can_undo(&options));
}

#[test]
fn undo_leaves_a_changed_unit_and_a_taken_place_entire() {
    let (fx, mut library, plan, initial) = setup();
    let options = fx.options();
    apply_with(&RealSys, &mut library, &options, &plan).unwrap();
    // After the run: one disk of the group changes (size), the original place of the flat file is taken.
    fs::write(fx.roms.join("atarist/Quest (Disk 2 of 2).st"), b"changed").unwrap();
    fs::write(fx.roms.join("Hero.sfc"), b"new file").unwrap();
    let journal = fx.journals()[0].clone();
    let report = undo_with(&RealSys, &mut library, &options, &journal).unwrap();
    assert_eq!(report.left.len(), 2, "{report:?}");
    assert!(
        report
            .left
            .iter()
            .any(|(_, b)| *b == TidyBlock::ChangedSincePlan)
    );
    assert!(
        report
            .left
            .iter()
            .any(|(_, b)| matches!(b, TidyBlock::Conflict { .. }))
    );
    // Both stay entirely tidied, nothing overwritten.
    assert!(fx.roms.join("atarist/Quest (Disk 1 of 2).st").is_file());
    assert!(!fx.roms.join("Quest (Disk 1 of 2).st").exists());
    assert_eq!(fs::read(fx.roms.join("Hero.sfc")).unwrap(), b"new file");
    assert_eq!(
        fs::read(fx.roms.join("snes/Hero.sfc")).unwrap(),
        initial[&rel("Hero.sfc")]
    );
    // The others came back.
    assert!(fx.roms.join("Doom/DOOM.EXE").is_file());
    assert_eq!(
        journal::read(&journal).unwrap().unwrap().state,
        State::Undone
    );
}

#[cfg(unix)]
#[test]
fn undo_refuses_a_file_with_another_hard_link() {
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let options = fx.options();
    apply_with(&RealSys, &mut library, &options, &plan).unwrap();
    fs::hard_link(
        fx.roms.join("snes/Hero.sfc"),
        fx.tmp.path().join("elsewhere"),
    )
    .unwrap();
    let report = undo_with(&RealSys, &mut library, &options, &fx.journals()[0]).unwrap();
    assert_eq!(report.left.len(), 1);
    assert!(fx.roms.join("snes/Hero.sfc").is_file());
}

#[test]
fn undo_only_the_newest_journal_of_this_root() {
    let (fx, mut library, plan, _) = setup();
    let options = fx.options();
    apply_with(&RealSys, &mut library, &options, &plan).unwrap();
    let first = fx.journals()[0].clone();
    let newer = fx.journals.join("journal-99999999999.txt");
    // A newer journal: the older one cannot be undone.
    fs::write(
        &newer,
        format!("{}E\n", header(options.roms_root()).unwrap()),
    )
    .unwrap();
    assert!(matches!(
        undo_with(&RealSys, &mut library, &options, &first),
        Err(LibraryError::Journal { .. })
    ));
    // A newer unreadable one blocks too (never skipped).
    fs::write(&newer, b"emu-rust tidy 1\nR /x\nZ\n").unwrap();
    assert!(undo_with(&RealSys, &mut library, &options, &first).is_err());
    // A newer one of another root blocks (TidyRootMismatch).
    fs::write(&newer, format!("{}E\n", header(Fx::other_root()).unwrap())).unwrap();
    assert!(matches!(
        undo_with(&RealSys, &mut library, &options, &first),
        Err(LibraryError::TidyRootMismatch { .. })
    ));
    // An empty newer file (state 0) is ignored.
    fs::write(&newer, b"").unwrap();
    undo_with(&RealSys, &mut library, &options, &first).unwrap();
}

/// FV0 3: a journal of another root is never replayed nor completed, not even by a closing line.
#[test]
fn another_root_is_refused_before_anything() {
    let fx = Fx::new();
    let options = fx.options();
    fs::create_dir_all(&fx.journals).unwrap();
    let path = fx.journals.join("journal-5.txt");
    let text = format!(
        "{}B 0\nM 0 1 2 a.sfc snes/a.sfc\nm 0\n",
        header(Fx::other_root()).unwrap()
    );
    fs::write(&path, &text).unwrap();
    let mut library = Library::open_in_memory().unwrap();
    assert!(matches!(
        library.recover_tidy(&options),
        Err(LibraryError::TidyRootMismatch { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    // Closed journal of another root: the identity marker is refused too.
    let closed = format!("{}E\n", header(Fx::other_root()).unwrap());
    fs::write(&path, &closed).unwrap();
    assert!(matches!(
        library.mark_tidy_identities(&options, &path),
        Err(LibraryError::TidyRootMismatch { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), closed);
    assert!(
        library
            .pending_tidy_identities(&options)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_unreadable_journal_is_never_replayed_and_blocks() {
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    let options = fx.options();
    fs::create_dir_all(&fx.journals).unwrap();
    let path = fx.journals.join("journal-5.txt");
    // `M` outside a block: the whole journal is refused.
    let text = format!(
        "{}M 0 1 2 Hero.sfc snes/Hero.sfc\n",
        header(options.roms_root()).unwrap()
    );
    fs::write(&path, &text).unwrap();
    let mut library = Library::open_in_memory().unwrap();
    assert!(matches!(
        library.recover_tidy(&options),
        Err(LibraryError::Journal { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    let plan = fx.plan(&library);
    assert!(apply_with(&RealSys, &mut library, &options, &plan).is_err());
    assert!(fx.roms.join("Hero.sfc").is_file());
    assert!(
        library
            .pending_tidy_identities(&options)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_empty_journal_means_nothing_moved_and_is_removed_by_the_recovery() {
    let fx = Fx::new();
    let options = fx.options();
    fs::create_dir_all(&fx.journals).unwrap();
    let path = fx.journals.join("journal-5.txt");
    fs::write(&path, b"emu-rust tidy 1\nR /hal").unwrap();
    let mut library = Library::open_in_memory().unwrap();
    assert_eq!(crate::last_tidy(&options).unwrap(), None);
    assert_eq!(library.recover_tidy(&options).unwrap(), None);
    assert!(!path.exists());
}

#[test]
fn a_run_that_starts_nothing_has_no_journal() {
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    write(&fx.roms.join("snes/Hero.sfc"), b"taken since the plan");
    let all: Vec<u32> = plan.units.iter().map(|u| u.id).collect();
    let report = library
        .apply_tidy(
            &fx.options(),
            &plan,
            &all,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(report.journal, None);
    assert_eq!(report.left.len(), 1);
    assert!(fx.journals().is_empty());
}

#[test]
fn a_unit_changed_since_the_plan_is_left() {
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    write(&fx.roms.join("Other.sfc"), &noise(4096, 2));
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    fs::write(fx.roms.join("Hero.sfc"), b"changed").unwrap();
    let report = apply_with(&RealSys, &mut library, &fx.options(), &plan).unwrap();
    assert_eq!(report.left.len(), 1);
    assert_eq!(report.left[0].1, TidyBlock::ChangedSincePlan);
    assert!(fx.roms.join("Hero.sfc").is_file());
    assert!(fx.roms.join("snes/Other.sfc").is_file());
    // A destination taken since the plan, in another case: left, never overwritten.
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    let plan = fx.plan(&library);
    write(&fx.roms.join("snes/HERO.SFC"), b"other");
    let report = apply_with(&RealSys, &mut library, &fx.options(), &plan).unwrap();
    assert!(matches!(report.left[0].1, TidyBlock::Conflict { .. }));
    assert_eq!(fs::read(fx.roms.join("snes/HERO.SFC")).unwrap(), b"other");
    assert!(fx.roms.join("Hero.sfc").is_file());
}

#[test]
fn cancellation_stops_between_units_and_closes() {
    let (fx, mut library, plan, initial) = setup();
    let options = fx.options();
    let selected = selectable(&plan);
    let cancel = AtomicBool::new(false);
    let mut seen = 0;
    let report = library
        .apply_tidy(&options, &plan, &selected, &cancel, &mut |_| {
            seen += 1;
            if seen == 2 {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .unwrap();
    assert!(report.cancelled);
    check_invariant(&fx.roms, &initial, &sides(&plan, &initial), "cancel");
    let j = fx.only_journal();
    assert_eq!(j.state, State::Ended);
    assert_eq!(j.forward.len(), 2);
}

#[test]
fn the_lock_refuses_a_second_run() {
    let fx = Fx::new();
    let options = fx.options();
    let _held = lock(&fx.journals).unwrap();
    let mut library = Library::open_in_memory().unwrap();
    assert!(matches!(
        library.recover_tidy(&options),
        Err(LibraryError::TidyBusy)
    ));
    let plan = fx.plan(&library);
    assert!(matches!(
        apply_with(&RealSys, &mut library, &options, &plan),
        Err(LibraryError::TidyBusy)
    ));
}

/// FV0 12: beyond the 20 newest, only readable journals ending with `I` are removed.
#[test]
fn the_purge_keeps_twenty_and_never_an_unfinished_journal() {
    let fx = Fx::new();
    let options = fx.options();
    fs::create_dir_all(&fx.journals).unwrap();
    let head = header(options.roms_root()).unwrap();
    for unix in 1..=25 {
        let body = match unix {
            1 => "E\n",       // identities due: kept
            2 => "B 0\n",     // in progress: kept
            3 => "garbage\n", // unreadable: kept
            _ => "E\nI\n",
        };
        fs::write(
            fx.journals.join(format!("journal-{unix}.txt")),
            format!("{head}{body}"),
        )
        .unwrap();
    }
    let newest = fx.journals.join("journal-26.txt");
    fs::write(&newest, format!("{head}E\n")).unwrap();
    let mut library = Library::open_in_memory().unwrap();
    library.mark_tidy_identities(&options, &newest).unwrap();
    let left: Vec<String> = fx
        .journals()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for kept in 1..=3 {
        assert!(left.contains(&format!("journal-{kept}.txt")), "{left:?}");
    }
    for gone in 4..=6 {
        assert!(!left.contains(&format!("journal-{gone}.txt")), "{left:?}");
    }
    assert_eq!(left.len(), 23);
    // `mark` refuses an unfinished journal.
    assert!(matches!(
        library.mark_tidy_identities(&options, &fx.journals.join("journal-2.txt")),
        Err(LibraryError::Journal { .. })
    ));
}

/// FV0 I5: the inverse rewrite of the recovery never takes the original row for a stale one.
#[test]
fn recovery_keeps_the_original_row_when_the_database_was_not_rewritten() {
    let fx = Fx::new();
    write(&fx.roms.join("a.sfc"), &noise(4096, 1));
    write(&fx.roms.join("snes/a.sfc"), &noise(4096, 2));
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let rows = game_ids(&library);
    assert_eq!(rows.len(), 2);
    // A stale row stays at `snes/a.sfc` (its file is gone); the run moved `a.sfc` there and stopped before the
    // database transaction.
    fs::remove_file(fx.roms.join("snes/a.sfc")).unwrap();
    let meta = fs::metadata(fx.roms.join("a.sfc")).unwrap();
    fs::rename(fx.roms.join("a.sfc"), fx.roms.join("snes/a.sfc")).unwrap();
    let options = fx.options();
    fs::create_dir_all(&fx.journals).unwrap();
    let mut w = JournalWriter::create(&RealSys, &fx.journals, options.roms_root(), 5).unwrap();
    w.append(
        &RealSys,
        &[
            Op::Begin(0),
            Op::Move {
                n: 0,
                size: meta.len(),
                mtime: crate::scan::mtime_of(&meta),
                from: rel("a.sfc"),
                to: rel("snes/a.sfc"),
            },
            Op::MoveDone(0),
        ],
    )
    .unwrap();
    library.recover_tidy(&options).unwrap().unwrap();
    assert!(fx.roms.join("a.sfc").is_file());
    let (id, path) = rows.iter().find(|(_, p)| !p.contains("snes")).unwrap();
    let after = game_ids(&library);
    assert_eq!(after.get(id), Some(path), "{after:?}");
}

/// RV1 I1: two equal copies at the source and the destination, one link each: never removed under Unix (the
/// destination must have a second link); with a second link, the duplicate name goes.
#[cfg(unix)]
#[test]
fn recovery_removes_a_destination_only_when_it_is_a_second_link() {
    let fx = Fx::new();
    let data = noise(4096, 1);
    write(&fx.roms.join("a.sfc"), &data);
    write(&fx.roms.join("snes/a.sfc"), &data);
    let options = fx.options();
    fs::create_dir_all(&fx.journals).unwrap();
    let journal_at = |unix: i64| {
        let mut w =
            JournalWriter::create(&RealSys, &fx.journals, options.roms_root(), unix).unwrap();
        w.append(
            &RealSys,
            &[
                Op::Begin(0),
                Op::Move {
                    n: 0,
                    size: 4096,
                    mtime: 0,
                    from: rel("a.sfc"),
                    to: rel("snes/a.sfc"),
                },
            ],
        )
        .unwrap();
    };
    journal_at(5);
    let mut library = Library::open_in_memory().unwrap();
    let report = library.recover_tidy(&options).unwrap().unwrap();
    assert!(fx.roms.join("snes/a.sfc").is_file(), "single link kept");
    assert_eq!(report.errors.len(), 1, "{report:?}");
    fs::remove_file(fx.roms.join("snes/a.sfc")).unwrap();
    fs::hard_link(fx.roms.join("a.sfc"), fx.roms.join("snes/a.sfc")).unwrap();
    journal_at(6);
    library.recover_tidy(&options).unwrap().unwrap();
    assert!(!fx.roms.join("snes/a.sfc").exists(), "second link removed");
    assert_eq!(fs::read(fx.roms.join("a.sfc")).unwrap(), data);
}

/// RV1 I1: a case-only move is blocked by the plan (and refused by the journal grammar).
#[test]
fn a_case_only_move_is_never_run() {
    let fx = Fx::new();
    write(&fx.roms.join("SNES/Case.sfc"), &noise(300, 6));
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    assert!(matches!(plan.units[0].status, TidyStatus::Blocked(_)));
    let all: Vec<u32> = plan.units.iter().map(|u| u.id).collect();
    let report = library
        .apply_tidy(
            &fx.options(),
            &plan,
            &all,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(report.journal, None);
    assert!(fx.roms.join("SNES/Case.sfc").is_file());
}

/// RV1 I3: an assembly cut anywhere is cleaned up: created folders deepest first at both
/// places, then `S`, then `<D>`; no assembly folder left, the unit whole.
#[test]
fn an_interrupted_assembly_is_cleaned_up() {
    let build = || {
        let fx = Fx::new();
        deep_cd(&fx.roms);
        let library = Library::open_in_memory().unwrap();
        let plan = fx.plan(&library);
        (fx, library, plan)
    };
    let (probe_fx, _, plan) = build();
    let initial = snapshot(&probe_fx.roms);
    let units = sides(&plan, &initial);
    assert!(plan.assembly_dir(0).is_some());
    let total = calls_of(|sys| {
        let (fx, mut library, plan) = build();
        apply_with(sys, &mut library, &fx.options(), &plan).unwrap();
    });
    for at in 0..total {
        let (fx, mut library, plan) = build();
        let _ = apply_with(
            &Faulty::new(at, Mode::Crash),
            &mut library,
            &fx.options(),
            &plan,
        );
        recover_with(&RealSys, &mut library, &fx.options()).unwrap();
        let context = format!("assembly crash at {at}");
        check_invariant(&fx.roms, &initial, &units, &context);
        let done = fx.roms.join("psx/Deep (Europe)/Deep (Europe).cue").exists();
        assert!(
            done || !fx.roms.join("psx/Deep (Europe)").exists(),
            "{context}"
        );
    }
}

/// FV0 4: the recovery never follows a link (here `roms/snes` replaced by a link after the crash).
#[cfg(unix)]
#[test]
fn recovery_never_follows_a_link() {
    let fx = Fx::new();
    let outside = fx.tmp.path().join("outside");
    write(&outside.join("a.sfc"), &noise(4096, 1));
    std::os::unix::fs::symlink(&outside, fx.roms.join("snes")).unwrap();
    let options = fx.options();
    fs::create_dir_all(&fx.journals).unwrap();
    let mut w = JournalWriter::create(&RealSys, &fx.journals, options.roms_root(), 5).unwrap();
    w.append(
        &RealSys,
        &[
            Op::Begin(0),
            Op::Move {
                n: 0,
                size: 4096,
                mtime: 0,
                from: rel("a.sfc"),
                to: rel("snes/a.sfc"),
            },
            Op::MoveDone(0),
        ],
    )
    .unwrap();
    let mut library = Library::open_in_memory().unwrap();
    let report = library.recover_tidy(&options).unwrap().unwrap();
    assert!(
        !fx.roms.join("a.sfc").exists(),
        "nothing pulled through the link"
    );
    assert!(outside.join("a.sfc").is_file());
    assert_eq!(report.errors.len(), 1);
}

/// A system folder replaced by a link after the plan: the execution refuses it.
#[cfg(unix)]
#[test]
fn a_linked_system_folder_is_never_used() {
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let outside = fx.tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, fx.roms.join("snes")).unwrap();
    let report = apply_with(&RealSys, &mut library, &fx.options(), &plan).unwrap();
    assert!(matches!(
        report.left[0].1,
        TidyBlock::DestinationNotPlainDir { .. }
    ));
    assert!(fx.roms.join("Hero.sfc").is_file());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}

/// A plan made for another roms root is refused before anything.
#[test]
fn a_plan_of_another_root_is_refused() {
    let fx = Fx::new();
    write(&fx.roms.join("Hero.sfc"), &noise(4096, 1));
    let other = fx.tmp.path().join("other");
    fs::create_dir(&other).unwrap();
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let options = TidyOptions::new(&other, &fx.journals).unwrap();
    assert!(apply_with(&RealSys, &mut library, &options, &plan).is_err());
    assert!(fx.roms.join("Hero.sfc").is_file());
    assert!(fx.journals().is_empty());
}
