//! The launch gate with a real custom system (design 8.0 §0.12, ADR 0025 §5 to §7): a definition on disk is
//! published as the process-wide catalog, then games of that system are launched through the library screen.
//! Acceptance of an unexamined core, « non testé » warning, dry run then direct launch, « Ça marche chez moi »,
//! games that cannot be tried, official systems untouched. The child of the dry run is a shell script (Unix);
//! everything else runs everywhere. Every test shares the one published catalog and has its own data folder.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
#[cfg(unix)]
use std::time::Duration;

use emu_ui::library_db::{
    Catalog, CustomSystemDraft, DatSource, DefinitionOrigin, systems, write_custom,
};
use emu_ui::test_support::{Answer, DialogKind, GameToLaunch, LibraryHarness};
use emu_ui::{Config, DataDirs};

const CORE: &str = "acme_core";
const SYSTEM_KEY: &str = "Acme - Fictional Console";

/// One test at a time: the catalog is shared and cores are loaded one per process.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Publishes `acme` once; the folder of its definition lives as long as the process.
fn publish_acme() {
    static DEFINITIONS: OnceLock<tempfile::TempDir> = OnceLock::new();
    DEFINITIONS.get_or_init(|| {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::under(tmp.path());
        let draft = CustomSystemDraft {
            id: "acme".into(),
            name: "Acme Fictional Console".into(),
            database: Some(SYSTEM_KEY.into()),
            extensions: vec!["fic".into()],
            dats: vec![DatSource::NoIntro],
            screenscraper: None,
            igdb: None,
            cores: vec![CORE.into()],
            ignored_firmware: Vec::new(),
            origin: DefinitionOrigin {
                core: CORE.into(),
                info_name: None,
                created_by: "8.0.0".into(),
                created_unix: 1_764_028_800,
            },
        };
        write_custom(&dirs.custom_systems, &draft, &Catalog::official()).unwrap();
        let load = emu_ui::publish_catalog(&dirs);
        assert!(load.problems.is_empty(), "{:?}", load.problems);
        tmp
    });
    assert!(
        systems().iter().any(|s| s.id == "acme" && s.is_custom()),
        "the definition is published"
    );
}

struct Fixture {
    _tmp: tempfile::TempDir,
    dirs: DataDirs,
    core: PathBuf,
    game: PathBuf,
}

fn fixture() -> Fixture {
    publish_acme();
    let tmp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::under(tmp.path());
    dirs.create_all().unwrap();
    let core = tmp.path().join("acme_core_libretro.bin");
    std::fs::write(&core, b"core bytes").unwrap();
    let folder = tmp.path().join("roms").join("acme");
    std::fs::create_dir_all(&folder).unwrap();
    let game = folder.join("Game.fic");
    std::fs::write(&game, b"rom").unwrap();
    Fixture {
        _tmp: tmp,
        dirs,
        core,
        game,
    }
}

fn config(accepted: bool, works_for_me: bool) -> Config {
    let mut config = Config::default();
    if accepted {
        config
            .custom_systems
            .content_accepted
            .insert(CORE.to_owned());
    }
    if works_for_me {
        config
            .custom_systems
            .works_for_me
            .insert(format!("{CORE}@acme"));
    }
    config
}

fn to_launch<'a>(f: &'a Fixture, title: &'a str, file: &'a Path) -> GameToLaunch<'a> {
    GameToLaunch {
        system_key: SYSTEM_KEY,
        title,
        core: CORE,
        core_path: &f.core,
        file,
    }
}

#[cfg(unix)]
fn shell(script: &str) -> emu_ui::DryRunCommand {
    emu_ui::DryRunCommand {
        program: PathBuf::from("sh"),
        args: ["-c", script, "child"]
            .map(std::ffi::OsString::from)
            .to_vec(),
        env: Vec::new(),
    }
}

/// `$1` is the request file: the result goes next to it.
#[cfg(unix)]
const CHILD_RAN: &str = r#"d="$(dirname "$1")"; echo 'outcome = "ran"' > "$d/result.toml""#;

#[test]
fn an_unexamined_core_is_accepted_first_then_the_game_is_warned_about() {
    let _guard = lock();
    let f = fixture();
    let mut h = LibraryHarness::open(f.dirs.clone(), config(false, false));
    let game = f.game.clone();
    assert!(h.launch(&to_launch(&f, "Game", &game)).is_empty());
    assert_eq!(h.dialog(), DialogKind::Accept);
    // « Annuler »: nothing is remembered, nothing starts.
    assert!(h.answer(Answer::Cancel).is_empty());
    assert!(h.config().custom_systems.content_accepted.is_empty());
    // Again, then « Accepter et continuer »: remembered, and the warning follows (no launch yet).
    h.launch(&to_launch(&f, "Game", &game));
    assert_eq!(h.dialog(), DialogKind::Accept);
    assert!(h.answer(Answer::Primary).is_empty());
    assert!(h.config().custom_systems.content_accepted.contains(CORE));
    assert_eq!(h.dialog(), DialogKind::Untested { can_try: true });
    assert!(h.answer(Answer::Cancel).is_empty());
    assert_eq!(h.dialog(), DialogKind::None);
}

#[cfg(unix)]
#[test]
fn a_dry_run_that_passes_launches_the_game_and_the_next_launch_is_direct() {
    let _guard = lock();
    let f = fixture();
    let mut h = LibraryHarness::open(f.dirs.clone(), config(true, false));
    h.set_dry_run_command(shell(CHILD_RAN));
    let game = f.game.clone();
    assert!(h.launch(&to_launch(&f, "Game", &game)).is_empty());
    assert_eq!(h.dialog(), DialogKind::Untested { can_try: true });
    h.answer(Answer::Primary);
    let launched = h.wait_try(Duration::from_secs(20));
    assert_eq!(launched.len(), 1, "{launched:?}");
    assert_eq!(launched[0].core, CORE);
    assert!(
        f.dirs.cache_dir().join("dry-runs.toml").is_file(),
        "the pass is remembered"
    );
    // Same game, same core file: no dialog, straight through.
    let again = h.launch(&to_launch(&f, "Game", &game));
    assert_eq!(again.len(), 1);
    assert_eq!(h.dialog(), DialogKind::None);
    // Another game of the same system is tried too.
    let other = f.game.with_file_name("Other.fic");
    std::fs::write(&other, b"rom 2").unwrap();
    assert!(h.launch(&to_launch(&f, "Other", &other)).is_empty());
    assert_eq!(h.dialog(), DialogKind::Untested { can_try: true });
}

#[test]
fn it_works_for_me_skips_the_warning_and_is_remembered_on_disk() {
    let _guard = lock();
    let f = fixture();
    let mut h = LibraryHarness::open(f.dirs.clone(), config(true, false));
    let game = f.game.clone();
    h.launch(&to_launch(&f, "Game", &game));
    assert_eq!(h.dialog(), DialogKind::Untested { can_try: true });
    let launched = h.answer(Answer::Secondary);
    assert_eq!(launched.len(), 1, "{launched:?}");
    assert!(
        h.config()
            .custom_systems
            .works_for_me
            .contains(&format!("{CORE}@acme"))
    );
    let saved = Config::load(&f.dirs.config_file).unwrap();
    assert!(
        saved
            .custom_systems
            .works_for_me
            .contains(&format!("{CORE}@acme"))
    );
    // A game of the system never asks again.
    let other = f.game.with_file_name("Other.fic");
    std::fs::write(&other, b"rom 2").unwrap();
    assert_eq!(h.launch(&to_launch(&f, "Other", &other)).len(), 1);
    assert_eq!(h.dialog(), DialogKind::None);
}

#[test]
fn it_works_for_me_never_skips_the_acceptance_of_the_core() {
    let _guard = lock();
    let f = fixture();
    let mut h = LibraryHarness::open(f.dirs.clone(), config(false, true));
    let game = f.game.clone();
    assert!(h.launch(&to_launch(&f, "Game", &game)).is_empty());
    assert_eq!(h.dialog(), DialogKind::Accept);
}

#[test]
fn a_game_in_an_archive_is_not_tried_and_the_warning_says_why() {
    let _guard = lock();
    let f = fixture();
    let mut h = LibraryHarness::open(f.dirs.clone(), config(true, false));
    let archive = f.game.with_file_name("Game.zip");
    std::fs::write(&archive, b"PK").unwrap();
    assert!(h.launch(&to_launch(&f, "Zipped", &archive)).is_empty());
    assert_eq!(h.dialog(), DialogKind::Untested { can_try: false });
    let launched = h.answer(Answer::Primary);
    assert_eq!(launched.len(), 1, "{launched:?}");
    assert!(
        launched[0].warnings.iter().any(|w| w.contains("archive")),
        "{:?}",
        launched[0].warnings
    );
}

#[cfg(unix)]
#[test]
fn a_dry_run_that_fails_offers_another_core_or_the_launch_anyway() {
    let _guard = lock();
    let f = fixture();
    let mut h = LibraryHarness::open(f.dirs.clone(), config(true, false));
    h.set_dry_run_command(shell("exit 3"));
    let game = f.game.clone();
    h.launch(&to_launch(&f, "Game", &game));
    h.answer(Answer::Primary);
    let launched = h.wait_try(Duration::from_secs(20));
    assert!(launched.is_empty(), "a failed try never launches by itself");
    assert_eq!(h.dialog(), DialogKind::Failed);
    assert!(
        !f.dirs.cache_dir().join("dry-runs.toml").exists(),
        "a failure is not remembered"
    );
    let launched = h.answer(Answer::Secondary);
    assert_eq!(launched.len(), 1, "{launched:?}");
}

#[test]
fn an_official_system_passes_the_gate_untouched() {
    let _guard = lock();
    let f = fixture();
    let mut h = LibraryHarness::open(f.dirs.clone(), config(false, false));
    let rom = f.game.with_file_name("Official.gb");
    std::fs::write(&rom, b"rom").unwrap();
    let launched = h.launch(&GameToLaunch {
        system_key: "Nintendo - Game Boy",
        title: "Official",
        core: "gambatte",
        core_path: &f.core,
        file: &rom,
    });
    assert_eq!(launched.len(), 1);
    assert_eq!(h.dialog(), DialogKind::None);
}
