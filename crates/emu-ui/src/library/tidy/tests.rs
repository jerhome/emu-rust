//! Preview logic on real plans (`Library::plan_tidy` on a generated tree, in-memory database).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use emu_library::{Library, TidyOptions, TidyPlan, TidyStatus};

use super::*;

fn write(path: &Path, data: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

fn noise(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

struct Tree {
    _dir: tempfile::TempDir,
    roms: std::path::PathBuf,
    journals: std::path::PathBuf,
}

fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let roms = dir.path().join("roms");
    write(&roms.join("Advance.gba"), &noise(4096, 1));
    write(&roms.join("Vagrant.chd"), &noise(3000, 2));
    write(&roms.join("Other.chd"), &noise(3000, 3));
    write(&roms.join("Third.chd"), &noise(3000, 4));
    write(&roms.join("Pocket.gb"), &noise(100, 5));
    write(&roms.join("notes.txt"), b"hello");
    write(&roms.join("scph5502.bin"), &noise(512, 6));
    write(
        &roms.join("Broken.cue"),
        b"FILE \"Broken (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
    );
    write(&roms.join("snes/Already.sfc"), &noise(1024, 7));
    let journals = dir.path().join("rangement");
    Tree {
        _dir: dir,
        roms,
        journals,
    }
}

fn plan(t: &Tree, names: &[&str]) -> TidyPlan {
    let library = Library::open_in_memory().unwrap();
    let options = TidyOptions::new(&t.roms, &t.journals)
        .unwrap()
        .with_firmware_names(names);
    library
        .plan_tidy(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn unit<'p>(plan: &'p TidyPlan, name: &str) -> &'p emu_library::TidyUnit {
    plan.units
        .iter()
        .find(|u| {
            u.title.contains(name.split('.').next().unwrap_or(name))
                || u.moves
                    .iter()
                    .any(|m| m.from.to_string_lossy().contains(name))
        })
        .unwrap_or_else(|| panic!("no unit {name}: {:#?}", plan.units))
}

fn preview(t: &Tree) -> Preview {
    Preview::new(Analysis {
        plan: plan(t, &["scph5502.bin"]),
        bios: Vec::new(),
    })
}

#[test]
fn refusals_in_priority_order() {
    assert_eq!(refusal(Busy::default()), None);
    assert_eq!(
        refusal(Busy {
            scan: true,
            ..Busy::default()
        }),
        Some(text::REFUSED_SCAN)
    );
    assert_eq!(
        refusal(Busy {
            download: true,
            ..Busy::default()
        }),
        Some(text::REFUSED_DOWNLOAD)
    );
    assert_eq!(
        refusal(Busy {
            game: true,
            scan: true,
            ..Busy::default()
        }),
        Some(text::REFUSED_GAME)
    );
    assert_eq!(
        refusal(Busy {
            no_db: true,
            ..Busy::default()
        }),
        Some(text::REFUSED_NO_DB)
    );
}

#[test]
fn sure_is_checked_suggested_and_ambiguous_are_not() {
    let t = tree();
    let p = preview(&t);
    let gba = unit(&p.plan, "Advance.gba");
    assert!(matches!(gba.status, TidyStatus::Sure(_)), "{gba:?}");
    assert_eq!(row_kind(gba), RowKind::Sure);
    assert!(p.checked.contains(&gba.id));
    let chd = unit(&p.plan, "Vagrant.chd");
    assert_eq!(row_kind(chd), RowKind::Choose);
    assert!(!selectable(chd));
    assert!(!p.checked.contains(&chd.id));
    let cue = unit(&p.plan, "Broken");
    assert_eq!(row_kind(cue), RowKind::Left);
    assert!(!selectable(cue));
    // Every checked unit can move; nothing else is checked.
    for u in &p.plan.units {
        assert_eq!(
            p.checked.contains(&u.id),
            row_kind(u) == RowKind::Sure && selectable(u),
            "{u:?}"
        );
    }
    // BIOS names and non-games stay out of the units; `snes/` is already tidy.
    assert!(p.plan.units.iter().all(|u| !u.title.contains("scph5502")));
    assert!(
        p.plan
            .ignored_paths
            .iter()
            .any(|q| q.ends_with("scph5502.bin"))
    );
    assert_eq!(p.plan.already_tidy, 1);
}

#[test]
fn groups_by_destination_then_choose_then_left() {
    let t = tree();
    let p = preview(&t);
    let g = groups(&p.plan);
    let keys: Vec<&Group> = g.iter().map(|(k, _)| k).collect();
    assert!(
        matches!(keys[0], Group::System(_, id) if id == "gba"),
        "{keys:?}"
    );
    assert!(keys.contains(&&Group::Choose));
    assert_eq!(keys.last(), Some(&&Group::Left));
    let total: usize = g.iter().map(|(_, v)| v.len()).sum();
    assert_eq!(total, p.plan.units.len());
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(sorted, keys);
}

#[test]
fn bulk_choice_by_extension_resolves_and_checks() {
    let t = tree();
    let mut p = preview(&t);
    let bulk = bulk_choices(&p.plan);
    // A single ambiguous `.gb` is chosen on its row: no bulk line for it.
    assert_eq!(bulk.len(), 1, "{bulk:?}");
    assert_eq!(bulk[0].extension, "chd");
    assert_eq!(bulk[0].count, 3);
    assert!(bulk[0].systems.iter().any(|s| s == "psx"));

    let n = p.choose_for_extension("chd", "psx");
    assert_eq!(n, 3);
    for name in ["Vagrant.chd", "Other.chd", "Third.chd"] {
        let u = unit(&p.plan, name);
        assert_eq!(row_kind(u), RowKind::Resolved);
        assert_eq!(u.system.as_deref(), Some("psx"));
        assert!(p.checked.contains(&u.id), "a user choice is checked");
    }
    assert!(bulk_choices(&p.plan).is_empty());
    assert!(p.note.as_deref().unwrap().contains("PlayStation"));
    let g = groups(&p.plan);
    assert!(
        g.iter()
            .any(|(k, v)| matches!(k, Group::System(_, id) if id == "psx") && v.len() == 3)
    );
}

#[test]
fn row_choice_then_change_and_uncheck() {
    let t = tree();
    let mut p = preview(&t);
    let id = unit(&p.plan, "Pocket.gb").id;
    assert!(p.choose(id, "snes").is_err(), "not a candidate");
    p.choose(id, "gb").unwrap();
    assert!(p.checked.contains(&id));
    let t1 = totals(&p.plan, &p.checked);
    p.set_checked(id, false);
    assert!(!p.checked.contains(&id));
    let t2 = totals(&p.plan, &p.checked);
    assert_eq!(t1.units, t2.units + 1);
    assert_eq!(t1.bytes, t2.bytes + 100);
    p.choose(id, "gbc").unwrap();
    let u = unit(&p.plan, "Pocket.gb");
    assert_eq!(u.moves[0].to, Path::new("gbc").join("Pocket.gb"));
    // A unit that cannot move is never checked.
    let cue = unit(&p.plan, "Broken").id;
    p.set_checked(cue, true);
    assert!(!p.checked.contains(&cue));
}

#[test]
fn totals_count_files_bytes_and_folders() {
    let t = tree();
    let mut p = preview(&t);
    p.choose_for_extension("chd", "psx");
    let tt = totals(&p.plan, &p.checked);
    assert_eq!(tt.units, 4); // gba + 3 chd
    assert_eq!(tt.files, 4);
    assert_eq!(tt.bytes, 4096 + 3 * 3000);
    assert_eq!(tt.dirs, 2); // gba/, psx/
    assert_eq!(totals(&p.plan, &BTreeSet::new()), Totals::default());
}

#[test]
fn bios_candidates_and_initial_unchecked_bios_unit() {
    let t = tree();
    // Named like a BIOS: ignored by the plan, hashed by the UI.
    let plan_named = plan(&t, &["scph5502.bin"]);
    let c = bios_candidates(&plan_named, &["SCPH5502.BIN".to_owned()]);
    assert!(
        c.iter()
            .any(|(p, u)| p.ends_with("scph5502.bin") && u.is_none()),
        "{c:?}"
    );
    // Without the name, the small `.bin` is a unit: candidate too, and left unchecked when it is a known BIOS.
    let plan_plain = plan(&t, &[]);
    let c = bios_candidates(&plan_plain, &[]);
    let (_, unit_id) = c
        .iter()
        .find(|(p, _)| p.ends_with("scph5502.bin"))
        .expect("small .bin unit");
    let unit_id = unit_id.expect("a unit");
    let bios: BTreeSet<u32> = [unit_id].into();
    assert!(!initial_checked(&plan_plain, &bios).contains(&unit_id));
}

#[test]
fn firmware_names_cover_tables_and_optional_files() {
    let catalogue = crate::bios_import::firmware_catalogue(None);
    let prefs = CorePreferences::builtin();
    let names = firmware_names(&catalogue, &prefs);
    for n in [
        "scph5502.bin",
        "panafz10.bin",
        "gba_bios.bin",
        "bios_CD_E.bin",
    ] {
        assert!(names.iter().any(|x| x == n), "{n} missing");
    }
}

#[test]
fn stage_machine_poll_and_close() {
    let mut ui = TidyUi::default();
    assert!(!ui.busy());
    assert!(!ui.poll());
    let t = tree();
    let roms = t.roms.clone();
    let journals = t.journals.clone();
    let task: PlanTask = Task::spawn("tidy-test", Arc::new(|| {}), move |_, progress| {
        progress(ScanProgress::default());
        let library = Library::open_in_memory().map_err(|e| e.to_string())?;
        let options = TidyOptions::new(&roms, &journals).map_err(|e| e.to_string())?;
        let plan = library
            .plan_tidy(&options, &AtomicBool::new(false), &mut |_| {})
            .map_err(|e| e.to_string())?;
        Ok(Analysis {
            plan,
            bios: Vec::new(),
        })
    });
    ui.stage = Stage::Analysing {
        task,
        progress: None,
    };
    assert!(ui.busy());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while ui.busy() && std::time::Instant::now() < deadline {
        ui.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(matches!(ui.stage, Stage::Preview(_)));
    assert!(ui.focus_first);
    ui.close();
    assert!(matches!(ui.stage, Stage::Idle));
}

#[test]
fn texts_explain_every_block_in_french() {
    use emu_library::TidyBlock::*;
    let p = std::path::PathBuf::from("psx/Game.chd");
    for b in [
        Conflict {
            existing: p.clone(),
        },
        Duplicate { existing: p },
        MissingReference {
            name: "a.bin".into(),
        },
        OutsideReference {
            name: "../a.bin".into(),
        },
        SharedTrack {
            name: "t.bin".into(),
        },
        UngroupedTracks,
        Symlink,
        DiscInArchive,
        DestinationNotPlainDir { dir: "psx".into() },
        OtherVolume,
        NoHardLinks,
        NonUtf8Path,
        PathTooLong,
        FolderNamedLikeSystem { name: "gb".into() },
        ChangedSincePlan,
    ] {
        assert!(!text::block(&b).is_empty());
    }
    assert!(text::system_change("gb", "gbc").contains("sauvegardes"));
    // Untrusted file names lose their control characters.
    assert!(!text::path(Path::new("a\u{1b}[31mb")).contains('\u{1b}'));
}

/// Choices run in a worker (FV0 I6), one at a time and in order; a refused one is reported and changes nothing.
#[test]
fn choices_are_applied_in_a_worker_in_order() {
    let t = tree();
    let mut p = preview(&t);
    let id = unit(&p.plan, "Pocket.gb").id;
    let notify: Notify = Arc::new(|| {});
    p.request(
        Choice::Unit {
            unit: id,
            system: "snes".to_owned(),
        },
        Arc::clone(&notify),
    );
    p.request(
        Choice::Unit {
            unit: id,
            system: "gbc".to_owned(),
        },
        Arc::clone(&notify),
    );
    p.request(
        Choice::Extension {
            extension: "chd".to_owned(),
            system: "psx".to_owned(),
        },
        Arc::clone(&notify),
    );
    assert_eq!(p.updating(), 3);
    let mut results = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while p.updating() > 0 && std::time::Instant::now() < deadline {
        if let Some(r) = p.poll_choice(Arc::clone(&notify)) {
            results.push(r);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(results.len(), 3, "{results:?}");
    assert!(results[0].is_err(), "snes is not a candidate");
    assert!(results[1].is_ok() && results[2].is_ok());
    let u = unit(&p.plan, "Pocket.gb");
    assert_eq!(u.moves[0].to, Path::new("gbc").join("Pocket.gb"));
    assert!(p.checked.contains(&id));
    assert_eq!(unit(&p.plan, "Vagrant.chd").system.as_deref(), Some("psx"));
    assert!(p.note.is_some());
}

/// Leaving the preview drops the task: a choice not started yet computes nothing.
#[test]
fn a_cancelled_choice_does_nothing() {
    let t = tree();
    let mut p = preview(&t);
    let id = unit(&p.plan, "Pocket.gb").id;
    let choice = Choice::Unit {
        unit: id,
        system: "gb".to_owned(),
    };
    let (plan, outcome) = choice_job(&AtomicBool::new(true), p.plan.clone(), &choice);
    assert!(matches!(outcome, ChoiceOutcome::Cancelled));
    assert_eq!(plan, p.plan);
    assert!(p.apply(&choice, plan, outcome).is_err());
    assert!(unit(&p.plan, "Pocket.gb").system.is_none());
}
