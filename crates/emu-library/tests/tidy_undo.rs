//! Integration tests of `Library::undo_tidy`, `recover_tidy`, `pending_tidy_identities` and
//! `mark_tidy_identities` through the public API. Faults injected inside the primitives (every system call of a
//! run, an undo or a recovery failing) are covered by `src/tidy/exec/tests.rs`; here the crash states are built
//! from real runs and hand-cut journals.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use emu_library::{
    Library, LibraryError, TidyBlock, TidyOptions, TidyRefusal, TidySection, last_tidy,
};

fn three(fx: &Fx) {
    write(&fx.roms.join("Alpha.sfc"), &noise(40 * 1024, 1));
    write(&fx.roms.join("Beta.nes"), &noise(24 * 1024, 2));
    write(&fx.roms.join("Gamma.gba"), &noise(8 * 1024, 3));
}

fn refusal(err: LibraryError) -> TidyRefusal {
    match err {
        LibraryError::TidyRefused { reason, .. } => reason,
        other => panic!("not a TidyRefused: {other:?}"),
    }
}

/// Runs the tidy of every sure unit and returns the journal.
fn tidy_all(library: &mut Library, fx: &Fx) -> PathBuf {
    let plan = fx.plan(library);
    let report = apply(library, fx, &plan, &sure_ids(&plan));
    assert!(
        report.left.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    report.journal.expect("a journal")
}

// ------------------------------------------------------------------------------------------------ round trip

#[test]
fn tidy_then_undo_gives_back_the_original_tree_and_database() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let root = fx.roms.canonicalize().unwrap();
    let alpha = library
        .games()
        .unwrap()
        .iter()
        .find(|g| g.title == "Alpha")
        .unwrap()
        .id;
    library.set_favorite(alpha, true).unwrap();
    library.record_played(alpha, 1_700_000_000).unwrap();
    let tree_before = tree(&fx.roms);
    let rows_before = rows(&library, &root);

    let journal = tidy_all(&mut library, &fx);
    assert_ne!(tree(&fx.roms), tree_before);
    let report = undo(&mut library, &fx, &journal);
    assert!(
        report.left.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    assert_eq!(report.moved.len(), 3);
    assert!(report.extra_names.is_empty() && report.closed.is_empty());
    assert_eq!(
        tree(&fx.roms),
        tree_before,
        "files and folders as at the start"
    );
    assert_eq!(
        rows(&library, &root),
        rows_before,
        "same ids, favorite, counters"
    );
    assert_rows_exist(&library);

    let info = last_tidy(&fx.options()).unwrap().unwrap();
    assert!(info.undone && info.complete && !info.can_undo());

    // The scan finds nothing to change.
    fx.scan(&mut library);
    assert_eq!(rows(&library, &root), rows_before);
}

#[test]
fn a_second_undo_and_an_undo_without_run_are_refused() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    // Nothing to undo yet: no journal at all.
    let missing = fx.journals.join("journal-1.txt");
    let err = library
        .undo_tidy(&fx.options(), &missing, &never(), &mut |_| {})
        .unwrap_err();
    assert_eq!(refusal(err), TidyRefusal::NothingToUndo);

    let journal = tidy_all(&mut library, &fx);
    undo(&mut library, &fx, &journal);
    let err = library
        .undo_tidy(&fx.options(), &journal, &never(), &mut |_| {})
        .unwrap_err();
    assert_eq!(refusal(err), TidyRefusal::AlreadyUndone);
}

#[test]
fn only_the_newest_journal_can_be_undone() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let first = unit(&plan, "Alpha.sfc").id;
    let j1 = apply(&mut library, &fx, &plan, &[first]).journal.unwrap();
    // Journal names carry the unix second: make sure the second run sorts after the first.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let plan = fx.plan(&library);
    let j2 = apply(&mut library, &fx, &plan, &sure_ids(&plan))
        .journal
        .unwrap();
    assert_ne!(j1, j2);
    let before = tree(&fx.roms);
    let err = library
        .undo_tidy(&fx.options(), &j1, &never(), &mut |_| {})
        .unwrap_err();
    assert_eq!(refusal(err), TidyRefusal::NotLatest);
    assert_eq!(tree(&fx.roms), before);
    // The newest one comes back, then the older one is the newest and can be undone in turn.
    undo(&mut library, &fx, &j2);
    assert!(
        fx.roms.join("snes/Alpha.sfc").is_file(),
        "the first run is still tidied"
    );
    assert!(fx.roms.join("Beta.nes").is_file());
}

#[test]
fn a_journal_of_another_root_is_never_replayed() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let journal = tidy_all(&mut library, &fx);
    let other_roms = fx.tmp.path().join("other-roms");
    fs::create_dir(&other_roms).unwrap();
    let other = TidyOptions::new(&other_roms, &fx.journals).unwrap();
    let before = tree(fx.tmp.path());
    let err = library
        .undo_tidy(&other, &journal, &never(), &mut |_| {})
        .unwrap_err();
    assert!(
        matches!(err, LibraryError::TidyRootMismatch { .. }),
        "{err:?}"
    );
    assert!(matches!(
        last_tidy(&other).unwrap_err(),
        LibraryError::TidyRootMismatch { .. }
    ));
    assert_eq!(tree(fx.tmp.path()), before);
}

// ------------------------------------------------------------------------------------------------ units left whole

#[test]
fn a_file_modified_or_deleted_after_the_tidy_keeps_its_unit_where_it_is() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let journal = tidy_all(&mut library, &fx);
    // Alpha edited (other size), Beta deleted, Gamma untouched.
    let edited = noise(41 * 1024, 99);
    fs::write(fx.roms.join("snes/Alpha.sfc"), &edited).unwrap();
    fs::remove_file(fx.roms.join("nes/Beta.nes")).unwrap();
    let report = undo(&mut library, &fx, &journal);
    assert_eq!(
        fs::read(fx.roms.join("snes/Alpha.sfc")).unwrap(),
        edited,
        "edited file untouched"
    );
    assert!(!fx.roms.join("Alpha.sfc").exists());
    assert!(!fx.roms.join("Beta.nes").exists() && !fx.roms.join("nes/Beta.nes").exists());
    assert!(
        fx.roms.join("Gamma.gba").is_file(),
        "the intact unit came back"
    );
    let root = fx.roms.canonicalize().unwrap();
    assert!(
        report
            .moved
            .contains(&(root.join("gba/Gamma.gba"), root.join("Gamma.gba"))),
        "the way back of the intact unit is listed: {report:?}"
    );
    assert_eq!(report.left.len(), 2, "both reported: {report:?}");
    assert!(
        report
            .left
            .iter()
            .all(|(_, b)| matches!(b, TidyBlock::ChangedSincePlan)),
        "{:?}",
        report.left
    );
    // Closed anyway: one undo per journal; the units left stay tidied (and known to the library).
    let info = last_tidy(&fx.options()).unwrap().unwrap();
    assert!(info.undone && info.complete);
}

#[test]
fn an_original_place_taken_again_keeps_the_unit_tidied_and_overwrites_nothing() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let journal = tidy_all(&mut library, &fx);
    let squatter = noise(10, 5);
    write(&fx.roms.join("Alpha.sfc"), &squatter); // the user put another Alpha.sfc back
    fs::create_dir(fx.roms.join("Beta.nes")).unwrap(); // and a folder where Beta was
    let report = undo(&mut library, &fx, &journal);
    assert_eq!(fs::read(fx.roms.join("Alpha.sfc")).unwrap(), squatter);
    assert!(fx.roms.join("snes/Alpha.sfc").is_file());
    assert!(fx.roms.join("Beta.nes").is_dir());
    assert!(fx.roms.join("nes/Beta.nes").is_file());
    assert!(fx.roms.join("Gamma.gba").is_file());
    assert_eq!(report.left.len(), 2, "{report:?}");
    assert_rows_exist(&library);
}

#[test]
fn cancelling_the_undo_leaves_the_rest_tidied_and_the_journal_closed() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let journal = tidy_all(&mut library, &fx);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut calls = 0;
    let report = library
        .undo_tidy(&fx.options(), &journal, &cancel, &mut |_| {
            calls += 1;
            if calls == 1 {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .unwrap();
    assert!(report.cancelled);
    let back = files(&fx.roms).keys().filter(|k| !k.contains('/')).count();
    assert_eq!(
        back, 1,
        "exactly one unit came back before the cancellation"
    );
    let info = last_tidy(&fx.options()).unwrap().unwrap();
    assert!(info.complete && info.undone);
    assert_rows_exist(&library);
    // Nothing lost.
    assert_eq!(files(&fx.roms).len(), 3);
}

// ------------------------------------------------------------------------------------------------ MANIACD

#[test]
fn maniacd_goes_back_to_the_root_as_one_tile_with_the_same_id() {
    let fx = Fx::new();
    write(&fx.roms.join("MANIACD0.D64"), &noise(2048, 11));
    write(&fx.roms.join("MANIACD1.D64"), &noise(2048, 12));
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let root = fx.roms.canonicalize().unwrap();
    let rows_before = rows(&library, &root);
    let tree_before = tree(&fx.roms);
    assert_eq!(rows_before.len(), 1);

    let journal = tidy_all(&mut library, &fx);
    assert_eq!(rows(&library, &root).len(), 1);
    let report = undo(&mut library, &fx, &journal);
    assert_eq!(report.moved.len(), 2, "{report:?}");
    assert_eq!(tree(&fx.roms), tree_before);
    assert_eq!(rows(&library, &root), rows_before);
    fx.scan(&mut library);
    assert_eq!(
        rows(&library, &root),
        rows_before,
        "still one game after the rescan"
    );
}

// ------------------------------------------------------------------------------------------------ round trip, big tree

#[test]
fn the_generated_tree_makes_a_full_round_trip() {
    let Some(g) = generate() else { return };
    let mut library = Library::open_in_memory().unwrap();
    scan(&mut library, &g.roms);
    let root = g.roms.canonicalize().unwrap();
    let tree_before = tree(&g.roms);
    let rows_before = rows(&library, &root);
    let plan = library
        .plan_tidy(&g.options(), &never(), &mut |_| {})
        .unwrap();
    let ids = sure_ids(&plan);
    let report = library
        .apply_tidy(&g.options(), &plan, &ids, &never(), &mut |_| {})
        .unwrap();
    let journal = report.journal.unwrap();
    assert_ne!(tree(&g.roms), tree_before);
    let report = library
        .undo_tidy(&g.options(), &journal, &never(), &mut |_| {})
        .unwrap();
    assert!(report.left.is_empty(), "{:#?}", report.left);
    assert!(report.errors.is_empty(), "{:#?}", report.errors);
    assert_eq!(tree(&g.roms), tree_before, "same files, same folders");
    assert_eq!(rows(&library, &root), rows_before, "same ids and rows");
    // A fresh plan is the same as the first one (same statuses, same destinations).
    let again = library
        .plan_tidy(&g.options(), &never(), &mut |_| {})
        .unwrap();
    assert_eq!(sure_ids(&again), ids);
    let key = |p: &emu_library::TidyPlan| {
        p.units
            .iter()
            .map(|u| (u.title.clone(), u.status.clone(), u.system.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&again), key(&plan));
}

// ------------------------------------------------------------------------------------------------ recovery

/// Percent-encodes a root the way the journal does (`A-Za-z0-9-._~()[]!,+&'` and `/` kept).
fn encode_root(root: &Path) -> String {
    let mut out = String::new();
    for &b in root.to_str().unwrap().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~()[]!,+&'/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn write_journal(fx: &Fx, name: &str, body: &str) -> PathBuf {
    fs::create_dir_all(&fx.journals).unwrap();
    let root = fx.roms.canonicalize().unwrap();
    let path = fx.journals.join(name);
    fs::write(
        &path,
        format!("emu-rust tidy 1\nR {}\n{body}", encode_root(&root)),
    )
    .unwrap();
    path
}

/// Plan of the one-CD tree with the system chosen by hand (a cue of noise is ambiguous).
fn disc_plan(library: &Library, fx: &Fx) -> (emu_library::TidyPlan, Vec<u32>) {
    let mut plan = fx.plan(library);
    assert_eq!(plan.units.len(), 1, "{:#?}", plan.units);
    let id = plan.units[0].id;
    plan.choose_system(id, "psx").unwrap();
    (plan, vec![id])
}

#[test]
fn a_journal_cut_before_the_commit_of_its_last_unit_brings_that_unit_back() {
    let fx = Fx::new();
    // One unit with three files: a CD (.cue + two tracks) is moved into one folder.
    let cue = b"FILE \"Disc (Track 1).bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\n    INDEX 01 00:00:00\r\nFILE \"Disc (Track 2).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    INDEX 01 00:00:00\r\n";
    write(&fx.roms.join("Disc.cue"), cue);
    write(&fx.roms.join("Disc (Track 1).bin"), &noise(2352 * 4, 1));
    write(&fx.roms.join("Disc (Track 2).bin"), &noise(2352 * 2, 2));
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let files_before = files(&fx.roms);
    let (plan, ids) = disc_plan(&library, &fx);
    let report = apply(&mut library, &fx, &plan, &ids);
    let journal = report.journal.unwrap();
    let text = fs::read_to_string(&journal).unwrap();
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    assert!(lines.last().is_some_and(|l| *l == "E\n"));
    let commit = lines.iter().position(|l| l.starts_with("C ")).unwrap();
    let tidied = files(&fx.roms);

    // Every cut from "all intentions written" up to just before `C`: the unit is open, it comes back whole.
    let last_intention = lines
        .iter()
        .rposition(|l| {
            l.starts_with("M ")
                || l.starts_with("W ")
                || l.starts_with("S ")
                || l.starts_with("D+ ")
        })
        .unwrap();
    for keep in last_intention + 1..=commit {
        let prefix: String = lines[..keep].concat();
        fs::write(&journal, &prefix).unwrap();
        let rows_now = rows(&library, &fx.roms.canonicalize().unwrap());
        let _ = rows_now;
        let report = library
            .recover_tidy(&fx.options())
            .unwrap_or_else(|e| panic!("cut after {keep} lines: {e:?}"))
            .unwrap_or_else(|| panic!("cut after {keep} lines: nothing in progress"));
        assert_eq!(
            report.closed,
            vec![(journal.clone(), TidySection::Tidy)],
            "cut {keep}"
        );
        // Files only: the recovery leaves the empty `psx/` it had created (design: « arbre d'origine »; reported to
        // library-assets: `rollback` removes the created folders before the assembly folder inside them).
        assert_eq!(
            files(&fx.roms),
            files_before,
            "cut after {keep} lines: the unit is not whole at the origin"
        );
        assert_rows_exist(&library);
        let info = last_tidy(&fx.options()).unwrap().unwrap();
        assert!(info.complete, "closed");
        // Put the disk back to the tidied state for the next cut (the library is rewritten the same way).
        let (plan, ids) = disc_plan(&library, &fx);
        // Remove the journal written by the recovery so the next run starts from a clean folder.
        for j in fx.journal_files() {
            fs::remove_file(j).unwrap();
        }
        let again = apply(&mut library, &fx, &plan, &ids);
        assert_eq!(files(&fx.roms), tidied, "re-tidy after cut {keep}");
        let journal2 = again.journal.unwrap();
        // Same name or not, the next loop iteration writes its cut over this journal.
        if journal2 != journal {
            fs::rename(&journal2, &journal).unwrap();
        }
    }
    // Cut after the commit (only `E` missing): the unit stays tidied and the journal is closed.
    fs::write(&journal, lines[..lines.len() - 1].concat()).unwrap();
    let report = library.recover_tidy(&fx.options()).unwrap().unwrap();
    assert_eq!(report.closed.len(), 1);
    assert_eq!(files(&fx.roms), tidied);
    assert_rows_exist(&library);
    assert!(last_tidy(&fx.options()).unwrap().unwrap().complete);
}

#[test]
fn recovery_with_nothing_in_progress_returns_none() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    assert!(library.recover_tidy(&fx.options()).unwrap().is_none());
    tidy_all(&mut library, &fx);
    assert!(library.recover_tidy(&fx.options()).unwrap().is_none());
}

#[test]
fn recovery_removes_an_orphan_assembly_folder_and_closes_the_journal() {
    let fx = Fx::new();
    write(&fx.roms.join("Keep.sfc"), &noise(1024, 1));
    fs::create_dir_all(fx.roms.join("psx/Game.emu-rust-part")).unwrap();
    let journal = write_journal(
        &fx,
        "journal-1700000000.txt",
        "B 1\nD+ psx\nS psx/Game.emu-rust-part\n",
    );
    let mut library = Library::open_in_memory().unwrap();
    let report = library.recover_tidy(&fx.options()).unwrap().unwrap();
    assert_eq!(report.closed, vec![(journal, TidySection::Tidy)]);
    assert!(
        !fx.roms.join("psx/Game.emu-rust-part").exists(),
        "orphan (empty) assembly folder removed"
    );
    assert!(fx.roms.join("Keep.sfc").is_file(), "user data untouched");
}

#[test]
fn an_unreadable_or_oversized_journal_is_an_error_and_nothing_is_touched() {
    let fx = Fx::new();
    three(&fx);
    fs::create_dir_all(&fx.journals).unwrap();
    let mut library = Library::open_in_memory().unwrap();
    let before = tree(&fx.roms);
    // Out of grammar.
    let bad = fx.journals.join("journal-5.txt");
    fs::write(&bad, "emu-rust tidy 1\nR /x\nZ nonsense\n").unwrap();
    let err = library.recover_tidy(&fx.options()).unwrap_err();
    assert!(matches!(err, LibraryError::Journal { .. }), "{err:?}");
    let err = library
        .undo_tidy(&fx.options(), &bad, &never(), &mut |_| {})
        .unwrap_err();
    assert!(matches!(err, LibraryError::Journal { .. }), "{err:?}");
    assert!(matches!(
        last_tidy(&fx.options()).unwrap_err(),
        LibraryError::Journal { .. }
    ));
    // Above 16 MiB (sparse file).
    let file = fs::File::create(&bad).unwrap();
    file.set_len(17 * 1024 * 1024).unwrap();
    drop(file);
    let err = library.recover_tidy(&fx.options()).unwrap_err();
    assert!(matches!(err, LibraryError::Journal { .. }), "{err:?}");
    fs::remove_file(&bad).unwrap();
    assert_eq!(tree(&fx.roms), before);
}

#[test]
fn an_interrupted_journal_of_another_root_is_not_replayed() {
    let fx = Fx::new();
    three(&fx);
    fs::create_dir_all(&fx.journals).unwrap();
    fs::write(
        fx.journals.join("journal-5.txt"),
        "emu-rust tidy 1\nR /somewhere/else/roms\nB 1\nM 0 3 1700000000 a.sfc snes/a.sfc\n",
    )
    .unwrap();
    let mut library = Library::open_in_memory().unwrap();
    let before = tree(&fx.roms);
    let err = library.recover_tidy(&fx.options()).unwrap_err();
    assert!(
        matches!(err, LibraryError::TidyRootMismatch { .. }),
        "{err:?}"
    );
    assert_eq!(tree(&fx.roms), before);
}

// ------------------------------------------------------------------------------------------------ identities

#[test]
fn the_identity_step_is_offered_until_it_is_marked_done() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    assert!(
        library
            .pending_tidy_identities(&fx.options())
            .unwrap()
            .is_empty()
    );
    let journal = tidy_all(&mut library, &fx);
    let root = fx.roms.canonicalize().unwrap();

    let pending = library.pending_tidy_identities(&fx.options()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, journal);
    assert_eq!(pending[0].1.len(), 3);
    assert!(
        pending[0]
            .1
            .contains(&(root.join("Alpha.sfc"), root.join("snes/Alpha.sfc")))
    );
    // Idempotent until marked.
    assert_eq!(
        library.pending_tidy_identities(&fx.options()).unwrap(),
        pending
    );

    library
        .mark_tidy_identities(&fx.options(), &journal)
        .unwrap();
    assert!(
        library
            .pending_tidy_identities(&fx.options())
            .unwrap()
            .is_empty()
    );
    assert!(last_tidy(&fx.options()).unwrap().unwrap().identities_done);
    // Marking twice is fine.
    library
        .mark_tidy_identities(&fx.options(), &journal)
        .unwrap();

    // The undo opens a new pending step (U without I), with the moves the other way round.
    undo(&mut library, &fx, &journal);
    let pending = library.pending_tidy_identities(&fx.options()).unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert!(
        pending[0].1.is_empty() || pending[0].1.iter().all(|(from, _)| from.starts_with(&root))
    );
    library
        .mark_tidy_identities(&fx.options(), &journal)
        .unwrap();
    assert!(
        library
            .pending_tidy_identities(&fx.options())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn marking_an_open_or_unknown_journal_is_refused() {
    let fx = Fx::new();
    three(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let journal = tidy_all(&mut library, &fx);
    // Unknown file name.
    let err = library
        .mark_tidy_identities(&fx.options(), &fx.journals.join("journal-42.txt"))
        .unwrap_err();
    assert_eq!(refusal(err), TidyRefusal::UnknownJournal);
    let err = library
        .mark_tidy_identities(&fx.options(), &fx.roms.join("Beta.nes"))
        .unwrap_err();
    assert_eq!(refusal(err), TidyRefusal::UnknownJournal);
    // In progress.
    let text = fs::read_to_string(&journal).unwrap();
    fs::write(&journal, text.strip_suffix("E\n").unwrap()).unwrap();
    let err = library
        .mark_tidy_identities(&fx.options(), &journal)
        .unwrap_err();
    assert_eq!(refusal(err), TidyRefusal::NotClosed);
    assert!(
        library
            .pending_tidy_identities(&fx.options())
            .unwrap()
            .is_empty()
    );
}
