//! Integration tests of `Library::apply_tidy` through the public API: generated trees, real files, a real SQLite
//! database. What needs injected system faults (every primitive of a unit failing, `CrossesDevices`, journal write
//! errors) is covered by the unit tests of `src/tidy/exec/tests.rs` and is not repeated here.

mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use common::*;
use emu_library::{
    CoverKind, CoverState, Library, LibraryError, TidyBlock, TidyOptions, TidyRefusal, TidyStatus,
    last_tidy,
};

fn two_roms(fx: &Fx) {
    write(&fx.roms.join("Alpha.sfc"), &noise(40 * 1024, 1));
    write(&fx.roms.join("Beta.nes"), &noise(24 * 1024, 2));
    write(&fx.roms.join("Gamma.gba"), &noise(8 * 1024, 3));
}

// ------------------------------------------------------------------------------------------------ identity

#[test]
fn applying_keeps_ids_favorites_counters_and_covers() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let before = library.games().unwrap();
    assert_eq!(before.len(), 3);
    let alpha = before.iter().find(|g| g.title == "Alpha").unwrap().id;
    library.set_favorite(alpha, true).unwrap();
    library.record_played(alpha, 1_700_000_000).unwrap();
    library.record_played(alpha, 1_700_000_100).unwrap();
    let cover = CoverState::Cached {
        path: PathBuf::from("snes/alpha.png"),
        source: "libretro".to_owned(),
    };
    library.set_cover(alpha, CoverKind::Boxart, &cover).unwrap();
    let before = rows(&library, &fx.roms.canonicalize().unwrap());

    let plan = fx.plan(&library);
    let ids = sure_ids(&plan);
    assert_eq!(ids.len(), 3, "{:#?}", plan.units);
    let report = apply(&mut library, &fx, &plan, &ids);
    assert!(
        report.left.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    assert!(!report.cancelled);

    let root = fx.roms.canonicalize().unwrap();
    assert!(root.join("snes/Alpha.sfc").is_file());
    assert!(root.join("nes/Beta.nes").is_file());
    assert!(!root.join("Alpha.sfc").exists());
    assert_eq!(report.moved.len(), 3);
    for (from, to) in &report.moved {
        assert!(
            from.starts_with(&root) && to.starts_with(&root),
            "{from:?} {to:?}"
        );
    }

    let after = rows(&library, &root);
    assert_eq!(after.len(), 3);
    let by_id = |rows: &[Row], id| rows.iter().find(|r| r.0 == id).cloned().unwrap();
    let a_before = by_id(&before, alpha);
    let a_after = by_id(&after, alpha);
    assert_eq!(a_after.1, "snes/Alpha.sfc");
    assert!(a_after.2, "favorite kept");
    assert_eq!(a_after.3, a_before.3, "play count kept");
    assert_eq!(a_after.3, 2);
    assert_eq!(
        library
            .cover(alpha, CoverKind::Boxart)
            .unwrap()
            .unwrap()
            .state,
        cover
    );
    assert_rows_exist(&library);

    // The next scan finds nothing new: same rows, same ids.
    fx.scan(&mut library);
    assert_eq!(rows(&library, &root), after);

    // The journal is closed and offered for undo.
    let info = last_tidy(&fx.options()).unwrap().unwrap();
    assert!(info.complete && !info.undone && info.can_undo());
    assert_eq!(info.moved, 3);
    assert_eq!(report.journal.as_deref(), Some(info.path.as_path()));
}

#[test]
fn a_multi_disk_group_keeps_its_media_list() {
    let fx = Fx::new();
    for n in 1..=2 {
        write(
            &fx.roms.join(format!("Saga (Disk {n} of 2).st")),
            &noise(4096, n),
        );
    }
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let root = fx.roms.canonicalize().unwrap();
    let before = rows(&library, &root);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].4.len(), 2, "{before:?}");

    let plan = fx.plan(&library);
    let report = apply(&mut library, &fx, &plan, &sure_ids(&plan));
    assert!(
        report.left.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    let after = rows(&library, &root);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].0, before[0].0, "same id");
    assert!(after[0].1.starts_with("atarist/"), "{after:?}");
    assert_eq!(after[0].4.len(), 2);
    assert!(
        after[0].4.iter().all(|m| m.starts_with("atarist/")),
        "{after:?}"
    );
    assert_rows_exist(&library);
}

#[test]
fn maniacd_disks_are_grouped_into_one_c64_game() {
    let fx = Fx::new();
    write(&fx.roms.join("MANIACD0.D64"), &noise(2048, 11));
    write(&fx.roms.join("MANIACD1.D64"), &noise(2048, 12));
    let mut library = Library::open_in_memory().unwrap();
    fx.scan(&mut library);
    let root = fx.roms.canonicalize().unwrap();
    let before = rows(&library, &root);
    assert_eq!(before.len(), 1, "one tile for the two disks: {before:?}");
    assert_eq!(before[0].4.len(), 2);

    let plan = fx.plan(&library);
    let u = unit(&plan, "MANIACD");
    assert_eq!(u.system.as_deref(), Some("c64"), "{u:?}");
    assert_eq!(u.moves.len(), 2);
    let ids = vec![u.id];
    let report = apply(&mut library, &fx, &plan, &ids);
    assert!(
        report.left.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    assert!(root.join("c64/MANIACD0.D64").is_file());
    assert!(root.join("c64/MANIACD1.D64").is_file());
    assert!(!root.join("MANIACD0.D64").exists() && !root.join("MANIACD1.D64").exists());

    let after = rows(&library, &root);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0].0, before[0].0, "same id");
    assert!(after[0].1.starts_with("c64/MANIACD"), "{after:?}");
    assert_eq!(after[0].4.len(), 2);
    assert!(
        after[0].4.iter().all(|m| m.starts_with("c64/")),
        "{after:?}"
    );
    assert_rows_exist(&library);

    // The rescan keeps one game, the same row.
    fx.scan(&mut library);
    assert_eq!(rows(&library, &root), after);
}

#[test]
fn the_generated_tree_is_tidied_with_every_sure_unit() {
    let Some(g) = generate() else { return };
    let mut library = Library::open_in_memory().unwrap();
    scan(&mut library, &g.roms);
    let root = g.roms.canonicalize().unwrap();
    let before_files = contents(&root);
    let plan = library
        .plan_tidy(&g.options(), &never(), &mut |_| {})
        .unwrap();
    let ids = sure_ids(&plan);
    assert!(ids.len() > 10, "the generated tree has many sure units");
    let report = library
        .apply_tidy(&g.options(), &plan, &ids, &never(), &mut |_| {})
        .unwrap();
    assert!(report.errors.is_empty(), "{:#?}", report.errors);
    assert!(report.left.is_empty(), "{:#?}", report.left);
    assert!(report.extra_names.is_empty());
    assert!(report.closed.is_empty(), "apply never fills `closed`");
    assert_eq!(
        contents(&root),
        before_files,
        "no file lost, none duplicated"
    );
    assert_rows_exist(&library);
    assert!(
        !tree(&root).keys().any(|k| k.contains("emu-rust-part")),
        "no assembly folder left"
    );
    // A second plan has nothing sure left to move.
    let again = library
        .plan_tidy(&g.options(), &never(), &mut |_| {})
        .unwrap();
    assert!(sure_ids(&again).is_empty(), "{:#?}", sure_ids(&again));
    // The scan after the tidy keeps every row.
    // The only row that goes is the symbolic link whose target was moved (it now dangles; its unit stays blocked).
    let mut expected = rows(&library, &root);
    expected.retain(|r| r.1 != "Link to SNES.sfc");
    scan(&mut library, &g.roms);
    assert_eq!(rows(&library, &root), expected);
}

// ------------------------------------------------------------------------------------------------ selection

#[test]
fn only_the_selected_units_move() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let nes = unit(&plan, "Beta.nes").id;
    let report = apply(&mut library, &fx, &plan, &[nes]);
    assert_eq!(report.moved.len(), 1);
    assert!(fx.roms.join("nes/Beta.nes").is_file());
    assert!(fx.roms.join("Alpha.sfc").is_file());
    assert!(fx.roms.join("Gamma.gba").is_file());
}

#[test]
fn nothing_selected_writes_nothing() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let before = tree(&fx.roms);
    let report = apply(&mut library, &fx, &plan, &[]);
    assert_eq!(report.journal, None);
    assert!(report.moved.is_empty());
    assert_eq!(tree(&fx.roms), before);
    assert!(fx.journal_files().is_empty());
}

#[test]
fn a_blocked_unit_selected_by_mistake_stays_and_is_reported() {
    let fx = Fx::new();
    write(&fx.roms.join("Alpha.sfc"), &noise(4096, 1));
    write(&fx.roms.join("snes/Alpha.sfc"), &noise(4096, 9)); // a different file already there
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let u = unit(&plan, "Alpha.sfc");
    assert!(
        matches!(u.status, TidyStatus::Blocked(TidyBlock::Conflict { .. })),
        "{u:?}"
    );
    let before = tree(&fx.roms);
    let report = apply(&mut library, &fx, &plan, &[u.id]);
    assert_eq!(report.left.len(), 1);
    assert!(matches!(report.left[0].1, TidyBlock::Conflict { .. }));
    assert_eq!(tree(&fx.roms), before);
}

// ------------------------------------------------------------------------------------------------ races

#[test]
fn a_destination_taken_after_the_plan_is_never_overwritten() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    // A different file appears at the destination of Alpha, a folder at the destination of Beta.
    let intruder = noise(100, 77);
    write(&fx.roms.join("snes/Alpha.sfc"), &intruder);
    fs::create_dir_all(fx.roms.join("nes/Beta.nes")).unwrap();
    let report = apply(&mut library, &fx, &plan, &sure_ids(&plan));
    assert_eq!(fs::read(fx.roms.join("snes/Alpha.sfc")).unwrap(), intruder);
    assert!(fx.roms.join("Alpha.sfc").is_file(), "source kept");
    assert!(fx.roms.join("nes/Beta.nes").is_dir());
    assert!(fx.roms.join("Beta.nes").is_file(), "source kept");
    assert!(
        fx.roms.join("gba/Gamma.gba").is_file(),
        "the free unit moved"
    );
    assert_eq!(report.left.len(), 2, "{report:?}");
    assert_eq!(report.moved.len(), 1);
}

#[test]
fn a_file_changed_after_the_plan_stays() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    // Another size: caught whatever the clock grain.
    fs::write(fx.roms.join("Alpha.sfc"), noise(41 * 1024, 5)).unwrap();
    fs::remove_file(fx.roms.join("Beta.nes")).unwrap();
    let report = apply(&mut library, &fx, &plan, &sure_ids(&plan));
    assert!(
        fx.roms.join("Alpha.sfc").is_file(),
        "changed file untouched"
    );
    assert!(!fx.roms.join("snes/Alpha.sfc").exists());
    assert!(!fx.roms.join("nes/Beta.nes").exists(), "nothing invented");
    assert!(fx.roms.join("gba/Gamma.gba").is_file());
    assert!(
        report
            .left
            .iter()
            .any(|(_, b)| matches!(b, TidyBlock::ChangedSincePlan)),
        "{report:?}"
    );
}

#[test]
fn a_plan_for_another_root_is_refused() {
    let fx = Fx::new();
    two_roms(&fx);
    let other = Fx::new();
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let before = tree(&other.roms);
    let err = library
        .apply_tidy(
            &other.options(),
            &plan,
            &sure_ids(&plan),
            &never(),
            &mut |_| {},
        )
        .unwrap_err();
    assert!(matches!(err, LibraryError::Io { .. }), "{err:?}");
    assert_eq!(tree(&other.roms), before);
    assert!(fx.roms.join("Alpha.sfc").is_file());
}

// ------------------------------------------------------------------------------------------------ cancellation

#[test]
fn cancelling_before_the_start_moves_nothing() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let before = tree(&fx.roms);
    let report = library
        .apply_tidy(
            &fx.options(),
            &plan,
            &sure_ids(&plan),
            &AtomicBool::new(true),
            &mut |_| {},
        )
        .unwrap();
    assert!(report.cancelled);
    assert!(report.moved.is_empty());
    assert_eq!(tree(&fx.roms), before);
    assert!(fx.journal_files().is_empty(), "no journal for no unit");
}

#[test]
fn cancelling_after_the_first_unit_keeps_it_and_closes_the_journal() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let cancel = AtomicBool::new(false);
    let mut calls = 0;
    let report = library
        .apply_tidy(&fx.options(), &plan, &sure_ids(&plan), &cancel, &mut |_| {
            calls += 1;
            // Raised during the first unit's progress: that unit finishes, the second one is not started.
            if calls == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.moved.len(), 1, "{report:?}");
    let moved_files = files(&fx.roms).keys().filter(|k| k.contains('/')).count();
    assert_eq!(moved_files, 1, "exactly one unit moved");
    let info = last_tidy(&fx.options()).unwrap().unwrap();
    assert!(info.complete, "the journal is closed (E)");
    assert_rows_exist(&library);
}

// ------------------------------------------------------------------------------------------------ lock, journal state

#[test]
fn a_second_run_while_one_is_in_progress_is_busy() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let mut other = Library::open_in_memory().unwrap();
    let options = fx.options();
    let other_plan = fx.plan(&other);
    let mut seen_busy = false;
    let report = library
        .apply_tidy(&options, &plan, &sure_ids(&plan), &never(), &mut |_| {
            if !seen_busy {
                let err = other
                    .apply_tidy(&options, &other_plan, &[], &never(), &mut |_| {})
                    .unwrap_err();
                assert!(matches!(err, LibraryError::TidyBusy), "{err:?}");
                let err = other.recover_tidy(&options).unwrap_err();
                assert!(matches!(err, LibraryError::TidyBusy), "{err:?}");
                seen_busy = true;
            }
        })
        .unwrap();
    assert!(seen_busy);
    assert_eq!(report.moved.len(), 3, "the first run was not disturbed");
}

#[test]
fn a_run_is_refused_while_a_journal_is_in_progress() {
    let fx = Fx::new();
    two_roms(&fx);
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let first = unit(&plan, "Alpha.sfc").id;
    let report = apply(&mut library, &fx, &plan, &[first]);
    let journal = report.journal.unwrap();
    // Cut the closing `E`: the run looks interrupted.
    let text = fs::read_to_string(&journal).unwrap();
    let cut = text.strip_suffix("E\n").expect("closed by E");
    fs::write(&journal, cut).unwrap();
    let plan = fx.plan(&library);
    let before = tree(&fx.roms);
    let err = library
        .apply_tidy(
            &fx.options(),
            &plan,
            &sure_ids(&plan),
            &never(),
            &mut |_| {},
        )
        .unwrap_err();
    match err {
        LibraryError::TidyRefused { reason, .. } => assert_eq!(reason, TidyRefusal::Interrupted),
        other => panic!("{other:?}"),
    }
    assert_eq!(tree(&fx.roms), before);
    // The recovery closes it and a new run is possible again.
    let closed = library.recover_tidy(&fx.options()).unwrap().unwrap();
    assert_eq!(closed.closed.len(), 1);
    let plan = fx.plan(&library);
    let report = apply(&mut library, &fx, &plan, &sure_ids(&plan));
    assert_eq!(report.moved.len(), 2);
}

#[test]
fn an_unreadable_journal_blocks_the_run_and_touches_nothing() {
    let fx = Fx::new();
    two_roms(&fx);
    fs::create_dir_all(&fx.journals).unwrap();
    fs::write(
        fx.journals.join("journal-100.txt"),
        b"not a journal\nat all\n",
    )
    .unwrap();
    let mut library = Library::open_in_memory().unwrap();
    let plan = fx.plan(&library);
    let before = tree(&fx.roms);
    let err = library
        .apply_tidy(
            &fx.options(),
            &plan,
            &sure_ids(&plan),
            &never(),
            &mut |_| {},
        )
        .unwrap_err();
    assert!(matches!(err, LibraryError::Journal { .. }), "{err:?}");
    assert_eq!(tree(&fx.roms), before);
    // Recovery refuses too (nothing was closed), and says why with the same error.
    let err = library.recover_tidy(&fx.options()).unwrap_err();
    assert!(matches!(err, LibraryError::Journal { .. }), "{err:?}");
    // Once the file is moved away the run goes through.
    fs::remove_file(fx.journals.join("journal-100.txt")).unwrap();
    let report = apply(&mut library, &fx, &plan, &sure_ids(&plan));
    assert_eq!(report.moved.len(), 3);
}

#[test]
fn the_journal_names_the_canonical_root_and_a_link_root_gives_canonical_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real-roms");
    fs::create_dir(&real).unwrap();
    write(&real.join("Alpha.sfc"), &noise(4096, 1));
    let link = tmp.path().join("link-roms");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(&real, &link).is_err() {
        eprintln!("no symlink privilege: skipped");
        return;
    }
    let journals = tmp.path().join("rangement");
    let options = TidyOptions::new(&link, &journals).unwrap();
    let canonical = real.canonicalize().unwrap();
    assert_eq!(options.roms_root(), canonical);
    let mut library = Library::open_in_memory().unwrap();
    scan(&mut library, &link);
    let plan = library.plan_tidy(&options, &never(), &mut |_| {}).unwrap();
    let report = library
        .apply_tidy(&options, &plan, &sure_ids(&plan), &never(), &mut |_| {})
        .unwrap();
    assert_eq!(report.moved.len(), 1);
    for (from, to) in &report.moved {
        assert!(
            from.starts_with(&canonical) && to.starts_with(&canonical),
            "{from:?} {to:?}"
        );
    }
    let journal = fs::read_to_string(report.journal.unwrap()).unwrap();
    let root_line = journal.lines().nth(1).unwrap();
    assert!(root_line.starts_with("R "), "{root_line}");
    assert!(
        !root_line.contains("link-roms"),
        "the journal holds the canonical root: {root_line}"
    );
    // The row (spelled through the link by the scan) follows the file.
    assert!(real.join("snes/Alpha.sfc").is_file());
    let games = library.games().unwrap();
    assert_eq!(games.len(), 1);
    assert!(
        games[0].path.ends_with("snes/Alpha.sfc"),
        "{:?}",
        games[0].path
    );
    assert!(games[0].path.exists());
}

// ------------------------------------------------------------------------------------------------ rights (Unix)

#[cfg(unix)]
mod rights {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;

    fn chmod(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// Restores the write bit so the temporary folder can be removed even when an assertion fails.
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            chmod(&self.0, 0o755);
        }
    }

    #[test]
    fn a_destination_folder_without_write_right_leaves_the_unit_whole() {
        let fx = Fx::new();
        write(&fx.roms.join("Alpha.sfc"), &noise(4096, 1));
        fs::create_dir(fx.roms.join("snes")).unwrap();
        if ignores_permissions(fx.tmp.path()) {
            eprintln!("permissions are not enforced here: skipped");
            return;
        }
        let mut library = Library::open_in_memory().unwrap();
        let plan = fx.plan(&library);
        let id = unit(&plan, "Alpha.sfc").id;
        chmod(&fx.roms.join("snes"), 0o555);
        let _restore = Restore(fx.roms.join("snes"));
        let report = apply(&mut library, &fx, &plan, &[id]);
        assert!(report.moved.is_empty());
        assert_eq!(report.left.len(), 1, "{report:?}");
        assert!(
            matches!(report.left[0].1, TidyBlock::NotWritable { .. }),
            "{:?}",
            report.left
        );
        assert!(fx.roms.join("Alpha.sfc").is_file());
        assert!(report.extra_names.is_empty());
        assert_eq!(fs::read_dir(fx.roms.join("snes")).unwrap().count(), 0);
    }

    #[test]
    fn a_source_folder_that_refuses_the_removal_undoes_the_unit() {
        let fx = Fx::new();
        let data = noise(4096, 1);
        write(&fx.roms.join("Alpha.sfc"), &data);
        if ignores_permissions(fx.tmp.path()) {
            eprintln!("permissions are not enforced here: skipped");
            return;
        }
        let mut library = Library::open_in_memory().unwrap();
        let plan = fx.plan(&library);
        let id = unit(&plan, "Alpha.sfc").id;
        // roms/ cannot lose a name any more (the link goes in roms/snes, created by the run: so create it first).
        fs::create_dir(fx.roms.join("snes")).unwrap();
        chmod(&fx.roms, 0o555);
        let _restore = Restore(fx.roms.clone());
        let report = apply(&mut library, &fx, &plan, &[id]);
        chmod(&fx.roms, 0o755);
        // The data is intact at the source; any second name is accounted for by extra_names, never lost.
        assert_eq!(fs::read(fx.roms.join("Alpha.sfc")).unwrap(), data);
        let second = fx.roms.join("snes/Alpha.sfc");
        if second.exists() {
            assert!(
                report
                    .extra_names
                    .iter()
                    .any(|e| e.remove == second.canonicalize().unwrap()
                        || e.remove == fx.roms.canonicalize().unwrap().join("snes/Alpha.sfc")),
                "a leftover second name must be listed: {report:?}"
            );
        }
        assert!(report.moved.is_empty(), "{report:?}");
        assert_eq!(report.left.len(), 1, "{report:?}");
        assert!(
            matches!(report.left[0].1, TidyBlock::NotWritable { .. }),
            "{:?}",
            report.left
        );
    }
}

// ------------------------------------------------------------------------------------------------ rights (Windows)

#[cfg(windows)]
mod windows_rights {
    use super::*;

    #[test]
    fn a_read_only_source_is_blocked_before_any_link_and_keeps_its_attribute() {
        let fx = Fx::new();
        let source = fx.roms.join("Alpha.sfc");
        write(&source, &noise(4096, 1));
        let mut perms = fs::metadata(&source).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&source, perms).unwrap();
        let mut library = Library::open_in_memory().unwrap();
        let plan = fx.plan(&library);
        let u = unit(&plan, "Alpha.sfc");
        assert!(
            matches!(
                u.status,
                TidyStatus::Blocked(TidyBlock::ReadOnlyFile { .. })
            ),
            "{:?}",
            u.status
        );
        let report = apply(&mut library, &fx, &plan, &[u.id]);
        assert!(report.moved.is_empty());
        assert!(source.is_file());
        assert!(!fx.roms.join("snes/Alpha.sfc").exists());
        assert!(fs::metadata(&source).unwrap().permissions().readonly());
        let mut perms = fs::metadata(&source).unwrap().permissions();
        // Windows only: clears the read-only attribute; the Unix world-writable warning does not apply.
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&source, perms).unwrap();
    }
}
