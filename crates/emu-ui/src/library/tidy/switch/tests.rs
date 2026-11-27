//! The switch on a real data folder: `run_job` with `Job::Switch` (worker side), the end of a switch at start-up,
//! the settings it changes. The process-wide catalog stays the official one: the definition files exist on the disk
//! only, which is all `plan_switch`, `apply_switch` and `complete_switch` read.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use emu_library::{Catalog, KeysChoice, Library, SwitchChoices, SwitchMove, TidyOptions};

use super::*;
use crate::config::DataDirs;
use crate::keyboard::KeyMap;
use crate::library::tidy::exec::{Job, RunKind, SwitchRun, run_job};

fn snes_database() -> String {
    emu_library::system("snes")
        .and_then(|s| s.libretro_name().map(str::to_owned))
        .expect("snes has a database name")
}

fn definition(id: &str, database: &str, core: &str) -> String {
    format!(
        "format = 1\nrender = \"software\"\n[system]\nid = \"{id}\"\nname = \"Mine\"\ndatabase = \"{database}\"\n\
         extensions = [\"sfc\"]\n[cores]\nlist = [\"{core}\"]\n\
         [origin]\ncore = \"{core}\"\ncreated_by = \"t\"\ncreated_unix = 1\n"
    )
}

struct Data {
    _dir: tempfile::TempDir,
    dirs: DataDirs,
}

fn data(id: &str, core: &str) -> Data {
    let dir = tempfile::tempdir().unwrap();
    // Canonical like the roms root the library works with (`/var` is `/private/var` on macOS).
    let root = dir.path().canonicalize().unwrap();
    let dirs = DataDirs::under(&root);
    std::fs::create_dir_all(dirs.root.join("roms")).unwrap();
    std::fs::create_dir_all(&dirs.custom_systems).unwrap();
    std::fs::write(
        dirs.custom_systems.join(format!("{id}.toml")),
        definition(id, &snes_database(), core),
    )
    .unwrap();
    Library::open(&dirs.library_db).unwrap();
    let mut config = Config::default();
    let mut keys = KeyMap::default();
    keys.bind(
        winit::keyboard::KeyCode::KeyQ,
        0,
        emu_runtime::JoypadButton::A,
    );
    config.set_keymap(&format!("{core}@{id}"), keys);
    config.save(&dirs.config_file).unwrap();
    Data { _dir: dir, dirs }
}

fn official_snes() -> emu_library::SystemInfo {
    Catalog::official().system("snes").unwrap().clone()
}

fn choices() -> SwitchChoices {
    SwitchChoices {
        keys: KeysChoice::Copy,
        prior_preference: None,
        prior_profile: None,
    }
}

fn run(dirs: &DataDirs, job: Job) -> exec::Outcome {
    run_job(dirs, job, &AtomicBool::new(false), &mut |_| {})
}

fn retired_files(dirs: &DataDirs) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dirs.custom_systems.join(".retired"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn options(dirs: &DataDirs) -> TidyOptions {
    TidyOptions::new(&dirs.root.join("roms"), &dirs.tidy_journals)
        .unwrap()
        .with_systems_dir(&dirs.custom_systems)
}

#[test]
fn a_switch_in_place_moves_nothing_and_retires_the_definition_after_keys_and_core() {
    let d = data("snes", "bsnes");
    let out = run(
        &d.dirs,
        Job::Switch {
            switch: SwitchMove {
                custom_id: "snes".into(),
                official_id: "snes".into(),
            },
            official: Box::new(official_snes()),
            choices: choices(),
            run: SwitchRun::Direct { rows_move: false },
        },
    );
    assert_eq!(out.kind, RunKind::Switch);
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    assert!(out.switched.is_some(), "the switch is over");
    assert!(!d.dirs.custom_systems.join("snes.toml").exists());
    let retired = retired_files(&d.dirs);
    assert_eq!(retired.len(), 1, "{retired:?}");
    assert!(retired[0].starts_with("snes-") && retired[0].ends_with(".replaced.toml"));
    let config = Config::load(&d.dirs.config_file).unwrap();
    assert!(
        config.input.keyboard.contains_key("bsnes"),
        "the official profile of the core was created from the user's"
    );
    assert_eq!(
        config.cores.preferred.get("snes").map(String::as_str),
        Some("bsnes"),
        "the user's core stays first"
    );
}

#[test]
fn nothing_is_retired_when_the_settings_cannot_be_written() {
    let d = data("snes", "bsnes");
    std::fs::write(&d.dirs.config_file, "this is [not toml").unwrap();
    let out = run(
        &d.dirs,
        Job::Switch {
            switch: SwitchMove {
                custom_id: "snes".into(),
                official_id: "snes".into(),
            },
            official: Box::new(official_snes()),
            choices: choices(),
            run: SwitchRun::Direct { rows_move: false },
        },
    );
    assert!(out.switched.is_none());
    assert_eq!(out.messages.len(), 1, "{:?}", out.messages);
    assert!(d.dirs.custom_systems.join("snes.toml").is_file());
    assert!(retired_files(&d.dirs).is_empty());
}

fn plan_with_moves(d: &Data, custom: &str) -> emu_library::TidyPlan {
    let library = Library::open_existing(&d.dirs.library_db).unwrap();
    let after = Catalog::load_without(&d.dirs.custom_systems, custom).catalog;
    library
        .plan_switch(
            &options(&d.dirs),
            &after,
            &SwitchMove {
                custom_id: custom.into(),
                official_id: "snes".into(),
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap()
}

fn write_game(dirs: &DataDirs, rel: &Path) {
    let path = dirs.root.join("roms").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, [7u8; 1024]).unwrap();
}

fn switch_job(plan: emu_library::TidyPlan) -> Job {
    Job::Switch {
        switch: SwitchMove {
            custom_id: "mysnes".into(),
            official_id: "snes".into(),
        },
        official: Box::new(official_snes()),
        choices: choices(),
        run: SwitchRun::WithMoves(Box::new(plan)),
    }
}

#[test]
fn a_switch_with_moves_runs_to_its_end_and_its_undo_brings_everything_back() {
    let d = data("mysnes", "bsnes");
    write_game(&d.dirs, Path::new("mysnes/Quest.sfc"));
    let plan = plan_with_moves(&d, "mysnes");
    assert_eq!(
        mode(emu_library::SwitchKind::NeedsMove, Some(&plan)),
        SwitchMode::WithMoves
    );

    let out = run(&d.dirs, switch_job(plan));
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    let done = out.switched.as_ref().expect("the end of the switch ran");
    assert_eq!(done.direction, SwitchDirection::Forward);
    assert!(d.dirs.root.join("roms/snes/Quest.sfc").is_file());
    assert!(!d.dirs.root.join("roms/mysnes/Quest.sfc").exists());
    assert!(!d.dirs.custom_systems.join("mysnes.toml").exists());
    assert_eq!(retired_files(&d.dirs).len(), 1);
    let config = Config::load(&d.dirs.config_file).unwrap();
    assert!(config.input.keyboard.contains_key("bsnes"));
    assert_eq!(out.last.undo_refusal, None, "the undo is possible");
    let journal = out
        .last
        .undo_target()
        .expect("the undo is offered")
        .to_path_buf();
    let text = std::fs::read_to_string(&journal).unwrap();
    assert!(text.starts_with("emu-rust tidy 2\n"), "journal version 2");

    let out = run(&d.dirs, Job::Undo { journal });
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    let undone = out.switched.as_ref().expect("the end of the undo ran");
    assert_eq!(undone.direction, SwitchDirection::Undo);
    assert!(d.dirs.root.join("roms/mysnes/Quest.sfc").is_file());
    assert!(d.dirs.custom_systems.join("mysnes.toml").is_file());
    let config = Config::load(&d.dirs.config_file).unwrap();
    assert!(
        !config.input.keyboard.contains_key("bsnes"),
        "the copied official profile is gone"
    );
    assert!(!config.cores.preferred.contains_key("snes"));
    assert!(config.input.keyboard.contains_key("bsnes@mysnes"));
}

#[test]
fn a_cut_between_the_last_unit_and_the_end_is_finished_at_the_next_start_without_a_question() {
    let d = data("mysnes", "bsnes");
    write_game(&d.dirs, Path::new("mysnes/Quest.sfc"));
    let plan = plan_with_moves(&d, "mysnes");
    // The run stops after `E`: the library applied the units, nobody called `complete_switch`.
    let mut library = Library::open_existing(&d.dirs.library_db).unwrap();
    library
        .apply_switch(
            &options(&d.dirs),
            &plan,
            &SwitchChoices {
                keys: KeysChoice::KeepUser,
                prior_preference: None,
                prior_profile: None,
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert!(
        d.dirs.custom_systems.join("mysnes.toml").is_file(),
        "not ended"
    );

    let out = run(&d.dirs, Job::Recovery);
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    assert!(out.switched.is_some());
    assert!(!d.dirs.custom_systems.join("mysnes.toml").exists());
    let config = Config::load(&d.dirs.config_file).unwrap();
    assert!(
        config.input.keyboard.contains_key("bsnes"),
        "keys from the header"
    );
    // Replayed once more: nothing is due any more.
    let out = run(&d.dirs, Job::Recovery);
    assert!(out.switched.is_none() && out.messages.is_empty());
}

#[test]
fn a_blocked_unit_makes_the_switch_not_proposed_and_a_missing_unit_means_no_move() {
    let d = data("mysnes", "bsnes");
    let empty = plan_with_moves(&d, "mysnes");
    assert_eq!(
        mode(emu_library::SwitchKind::NeedsMove, Some(&empty)),
        SwitchMode::NoUnits
    );
    // A file already there under the official folder: the unit is a conflict.
    write_game(&d.dirs, Path::new("mysnes/Quest.sfc"));
    write_game(&d.dirs, Path::new("snes/Quest.sfc"));
    std::fs::write(d.dirs.root.join("roms/snes/Quest.sfc"), [9u8; 500]).unwrap();
    let blocked = plan_with_moves(&d, "mysnes");
    assert_eq!(
        mode(emu_library::SwitchKind::NeedsMove, Some(&blocked)),
        SwitchMode::Blocked
    );
    assert_eq!(
        mode(emu_library::SwitchKind::InPlace, None),
        SwitchMode::InPlace
    );
    assert_eq!(
        mode(emu_library::SwitchKind::Uncertain, None),
        SwitchMode::InPlace
    );
}

#[test]
fn the_cores_of_a_definition_come_from_the_active_file_then_the_retired_one() {
    let d = data("mysnes", "bsnes");
    assert_eq!(
        definition_cores(&d.dirs.custom_systems, "mysnes", None),
        ["bsnes"]
    );
    assert!(definition_cores(&d.dirs.custom_systems, "nothing", None).is_empty());
    let file = emu_library::retire_custom(
        &d.dirs.custom_systems,
        "mysnes",
        emu_library::Retirement::Replaced,
        5,
    )
    .unwrap();
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    assert!(definition_cores(&d.dirs.custom_systems, "mysnes", None).is_empty());
    assert_eq!(
        definition_cores(&d.dirs.custom_systems, "mysnes", Some(&name)),
        ["bsnes"]
    );
}

fn cut_after_the_units(d: &Data) -> emu_library::PendingSwitch {
    write_game(&d.dirs, Path::new("mysnes/Quest.sfc"));
    let plan = plan_with_moves(d, "mysnes");
    let mut library = Library::open_existing(&d.dirs.library_db).unwrap();
    library
        .apply_switch(
            &options(&d.dirs),
            &plan,
            &choices(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    library
        .pending_switch(&options(&d.dirs))
        .unwrap()
        .expect("the end is due")
}

#[test]
fn a_taken_place_blocks_the_end_and_removing_the_system_lets_it_finish() {
    let d = data("mysnes", "bsnes");
    let pending = cut_after_the_units(&d);
    // Something else already sits under the name the definition is to be retired to.
    let retired = d.dirs.custom_systems.join(".retired");
    std::fs::create_dir_all(&retired).unwrap();
    std::fs::write(retired.join(&pending.retired_name), "another file").unwrap();
    assert_eq!(end_block(&d.dirs, &pending), EndBlock::PlaceTaken);

    let out = run(&d.dirs, Job::Recovery);
    assert!(out.switched.is_none());
    assert_eq!(out.messages.len(), 1, "{:?}", out.messages);
    assert!(out.messages[0].contains("retentée"), "{}", out.messages[0]);
    assert!(out.last.info.as_ref().is_some_and(|i| i.end_due));
    assert!(
        d.dirs.custom_systems.join("mysnes.toml").is_file(),
        "nothing was deleted"
    );
    let library = Library::open_existing(&d.dirs.library_db).unwrap();
    let due = read_end_due(&library, &options(&d.dirs), &d.dirs).expect("the banner has its data");
    assert_eq!(
        (due.switch.custom_id.as_str(), due.block),
        ("mysnes", EndBlock::PlaceTaken)
    );

    // « Retirer ce système et terminer la bascule »: retired as removed, then the end runs again.
    emu_library::retire_custom(
        &d.dirs.custom_systems,
        "mysnes",
        emu_library::Retirement::Removed,
        9,
    )
    .unwrap();
    let out = run(&d.dirs, Job::Recovery);
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    assert!(out.switched.is_some(), "the switch is over");
    assert_eq!(
        read_end_due(&library, &options(&d.dirs), &d.dirs),
        None,
        "no banner any more"
    );
    // The removed definition is listed with the retired ones, restorable.
    assert!(
        emu_library::retired_customs(&d.dirs.custom_systems)
            .iter()
            .any(|r| r.id == "mysnes" && r.why == emu_library::Retirement::Removed)
    );
}

#[test]
fn a_missing_retired_file_is_named_and_nothing_else_is_done() {
    let d = data("mysnes", "bsnes");
    let pending = cut_after_the_units(&d);
    // Forward end with neither the definition nor its retired copy: only the user can put the file back.
    std::fs::remove_file(d.dirs.custom_systems.join("mysnes.toml")).unwrap();
    assert_eq!(end_block(&d.dirs, &pending), EndBlock::RetiredMissing);
    let line = text::switch_end_banner("mysnes", EndBlock::RetiredMissing, &pending.retired_name);
    assert!(
        line.contains(&format!("systems/.retired/{}", pending.retired_name)),
        "{line}"
    );
}

#[test]
fn frozen_names_are_shown_relative_to_the_roms_root() {
    let items = vec![
        r"\\?\C:\Users\me\emu\roms\mine\Hack.xyz".to_owned(),
        "C:/Users/me/emu/roms/mine/sub/Demo.xyz".to_owned(),
        "roms/mine/Already.xyz".to_owned(),
    ];
    let shown = relative_names(&items, &[Path::new(r"\\?\C:\Users\me\emu\roms")]);
    assert_eq!(
        shown,
        [
            "roms/mine/Hack.xyz",
            "roms/mine/sub/Demo.xyz",
            "roms/mine/Already.xyz"
        ]
    );
    assert!(shown.iter().all(|s| !s.contains("Users")));
}

#[test]
fn settings_failure_reasons_never_hold_the_config_path() {
    use crate::config::ConfigError;
    let path = std::path::PathBuf::from("/home/someone/secret/config.toml");
    let errors = [
        ConfigError::Io {
            path: path.clone(),
            source: std::io::Error::other("refusé"),
        },
        ConfigError::Locked { path: path.clone() },
        ConfigError::TooLarge { path },
    ];
    for e in &errors {
        let reason = config_reason(e);
        assert!(
            !reason.contains("someone") && !reason.contains("config.toml"),
            "{reason}"
        );
    }
}

#[test]
fn ignored_files_are_listed_relative_to_the_roms_folder() {
    let shown = ignored_names(&[
        PathBuf::from("mine").join("Notes.txt"),
        PathBuf::from(r"mine\sub\a.xyz"),
    ]);
    assert_eq!(shown, ["roms/mine/Notes.txt", "roms/mine/sub/a.xyz"]);
}

fn direct_job(custom: &str, rows_move: bool) -> Job {
    Job::Switch {
        switch: SwitchMove {
            custom_id: custom.into(),
            official_id: "snes".into(),
        },
        official: Box::new(official_snes()),
        choices: choices(),
        run: SwitchRun::Direct { rows_move },
    }
}

#[test]
fn a_switch_without_units_cut_before_the_retirement_is_run_again_to_the_same_result() {
    let d = data("mysnes", "bsnes");
    let out = run(&d.dirs, direct_job("mysnes", true));
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    // The cut: the keys, the core and the rows are done, the definition is still active.
    let retired = d.dirs.custom_systems.join(".retired");
    let file = std::fs::read_dir(&retired)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    std::fs::copy(file.path(), d.dirs.custom_systems.join("mysnes.toml")).unwrap();
    std::fs::remove_file(file.path()).unwrap();
    let again = run(&d.dirs, direct_job("mysnes", true));
    assert!(again.messages.is_empty(), "{:?}", again.messages);
    assert!(again.switched.is_some());
    assert!(!d.dirs.custom_systems.join("mysnes.toml").exists());
    assert_eq!(retired_files(&d.dirs).len(), 1);
    let config = Config::load(&d.dirs.config_file).unwrap();
    assert!(config.input.keyboard.contains_key("bsnes"));
}

#[test]
fn the_undo_is_refused_in_advance_and_says_why() {
    let d = data("mysnes", "bsnes");
    write_game(&d.dirs, Path::new("mysnes/Quest.sfc"));
    let plan = plan_with_moves(&d, "mysnes");
    let out = run(&d.dirs, switch_job(plan));
    assert!(out.switched.is_some());
    let library = Library::open_existing(&d.dirs.library_db).unwrap();
    let read = || exec::read_last(&options(&d.dirs), Some(&library), &d.dirs.custom_systems);
    assert_eq!(read().undo_refusal, None);
    // The definition is recreated under the same id: delete it first.
    std::fs::write(
        d.dirs.custom_systems.join("mysnes.toml"),
        definition("mysnes", &snes_database(), "bsnes"),
    )
    .unwrap();
    let last = read();
    let reason = last.undo_refusal.clone().expect("refused");
    assert!(
        reason.contains("supprimez d'abord le système (mysnes)"),
        "{reason}"
    );
    assert!(last.undo_target().is_none(), "no undo button");
}

#[test]
fn the_preview_asks_about_the_keys_only_when_the_two_profiles_differ_and_keeps_the_users_by_default()
 {
    let d = data("snes", "bsnes");
    let proposal = |kind| emu_library::SwitchProposal {
        custom_id: "snes".into(),
        official: official_snes(),
        kind,
    };
    let analysis = |kind| SwitchAnalysis {
        proposal: proposal(kind),
        plan: None,
        frozen: Vec::new(),
        ignored: Vec::new(),
        last: exec::LastTidy::default(),
    };
    let mut config = Config::default();
    let quiet = SwitchPreview::new(analysis(emu_library::SwitchKind::InPlace), &config, &d.dirs);
    assert!(!quiet.conflict);
    assert_eq!(quiet.keys, KeysChoice::Copy);
    assert_eq!(quiet.mode, SwitchMode::InPlace);
    let official = official_snes();
    let official_key = crate::profile_key_for(
        &official,
        crate::PadStyle::for_system(&official, "bsnes"),
        "bsnes",
    );
    let keys = |code| KeyMap::new(vec![(code, 0, emu_runtime::JoypadButton::A)]);
    config
        .input
        .keyboard
        .insert("bsnes@snes".into(), keys(winit::keyboard::KeyCode::KeyA));
    config
        .input
        .keyboard
        .insert(official_key, keys(winit::keyboard::KeyCode::KeyB));
    let asked = SwitchPreview::new(
        analysis(emu_library::SwitchKind::Uncertain),
        &config,
        &d.dirs,
    );
    assert!(asked.conflict, "the profiles differ: the question is asked");
    assert_eq!(
        asked.keys,
        KeysChoice::KeepUser,
        "the user's keys by default"
    );
    assert!(
        text::switch_intro("Super Nintendo", emu_library::SwitchKind::Uncertain)
            .contains("si c'est la même machine")
    );
    assert!(
        text::switch_intro("Super Nintendo", emu_library::SwitchKind::InPlace)
            .contains("aucun fichier n'est déplacé")
    );
}

#[test]
fn frozen_names_handle_unc_and_never_show_an_absolute_path() {
    let items = vec![r"\\?\UNC\nas\share\emu\roms\mine\Hack.xyz".to_owned()];
    let unc = Path::new(r"\\nas\share\emu\roms");
    assert_eq!(relative_names(&items, &[unc]), ["roms/mine/Hack.xyz"]);
    let other = Path::new("/elsewhere/roms");
    // The canonical root does not match, the plain one does.
    assert_eq!(
        relative_names(&items, &[other, unc]),
        ["roms/mine/Hack.xyz"]
    );
    // No root matches: the file name alone.
    let shown = relative_names(&items, &[other]);
    assert_eq!(shown, ["Hack.xyz"]);
    let shown = relative_names(&["/home/me/emu/roms/mine/Old.xyz".to_owned()], &[]);
    assert_eq!(shown, ["Old.xyz"]);
}

#[cfg(windows)]
#[test]
fn frozen_names_ignore_the_case_of_the_windows_root() {
    let items = vec![r"c:\users\me\emu\roms\mine\Hack.xyz".to_owned()];
    let shown = relative_names(&items, &[Path::new(r"\\?\C:\Users\ME\emu\roms")]);
    assert_eq!(shown, ["roms/mine/Hack.xyz"]);
}
