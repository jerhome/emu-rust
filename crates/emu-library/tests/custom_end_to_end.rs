//! 8.0 custom systems, end to end through the public API only (design 8.0 section 4, lot PQ2): real definitions
//! written by `write_custom` and published with `install_catalog`, then scan, tidy with reserved folders, the switch
//! with move (run, crash, resume, undo) and the v7 -> v8 migration under a catalog with custom systems.
//!
//! The catalog is process-wide: every test takes `LOCK`, publishes its own catalog and puts the official one back.
//! Nothing here touches the network. Tests that need a downloaded homebrew are `#[ignore]` (ADR 0007).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use common::{never, noise, write};
use emu_library::{
    Catalog, CustomSystemDraft, DatSource, DefinitionOrigin, KeysChoice, Library, LibraryError,
    Retirement, ScanOptions, ScanReport, SwitchChoices, SwitchDirection, SwitchKind, SwitchMove,
    TidyOptions, TidyRefusal, cartridge_extensions, install_catalog, open_switch, proposed_id,
    restore_custom, retire_custom, retired_customs, write_custom,
};

static LOCK: Mutex<()> = Mutex::new(());

const SNES_DATABASE: &str = "Nintendo - Super Nintendo Entertainment System";

/// A temporary data folder (`roms/`, `systems/`, `rangement/`) holding the process-wide catalog for its lifetime.
struct Env {
    _guard: MutexGuard<'static, ()>,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    roms: PathBuf,
    systems: PathBuf,
    journals: PathBuf,
}

impl Env {
    fn new() -> Self {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        install_catalog(Catalog::official());
        let tmp = tempfile::tempdir().unwrap();
        // Canonical: macOS temp dirs sit behind the /var -> /private/var link.
        let root = fs::canonicalize(tmp.path()).unwrap();
        let (roms, systems, journals) = (
            root.join("roms"),
            root.join("systems"),
            root.join("rangement"),
        );
        for dir in [&roms, &systems] {
            fs::create_dir_all(dir).unwrap();
        }
        Env {
            _guard: guard,
            _tmp: tmp,
            root,
            roms,
            systems,
            journals,
        }
    }

    /// Reads `systems/` and publishes the result, as the application does at start-up and after an edit.
    fn publish(&self) -> &'static Catalog {
        install_catalog(Catalog::load(&self.systems).catalog)
    }

    fn options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.journals)
            .unwrap()
            .with_systems_dir(&self.systems)
    }

    fn scan(&self, lib: &mut Library) -> ScanReport {
        scan_roots(lib, std::slice::from_ref(&self.roms))
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        install_catalog(Catalog::official());
    }
}

fn scan_roots(lib: &mut Library, roots: &[PathBuf]) -> ScanReport {
    let options = ScanOptions {
        roots: roots.to_vec(),
        ..ScanOptions::default()
    };
    lib.scan(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn draft(id: &str, database: Option<&str>, extensions: &[&str]) -> CustomSystemDraft {
    CustomSystemDraft {
        id: id.into(),
        name: format!("System {id}"),
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
            created_unix: 1_764_028_800,
        },
    }
}

/// Writes the definition through the public path (validation, atomic write), against the published catalog.
fn add(env: &Env, d: &CustomSystemDraft) {
    let catalog = Catalog::load(&env.systems).catalog;
    write_custom(&env.systems, d, &catalog).unwrap();
}

/// `relative path -> system id` of every visible game.
fn systems_by_path(lib: &Library, root: &Path) -> BTreeMap<String, String> {
    lib.games()
        .unwrap()
        .iter()
        .map(|g| {
            let rel: Vec<String> = g
                .path
                .strip_prefix(root)
                .unwrap_or_else(|_| panic!("{:?} not under {root:?}", g.path))
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            (rel.join("/"), g.system.clone())
        })
        .collect()
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                let rel: Vec<String> = p
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.insert(rel.join("/"), fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

// ------------------------------------------------------------------------------------------- real definitions

/// `supported_extensions` of the real `.info` files (libretro-super `dist/info`, read on 2026-11-27):
/// stella `a26|bin`, mednafen_pce_fast `pce|cue|ccd|chd|toc|m3u`, flycast `chd|cdi|elf|bin|cue|gdi|lst|zip|rar|dat|7z|m3u`.
fn list(text: &str) -> Vec<String> {
    text.split('|').map(str::to_owned).collect()
}

#[test]
fn the_real_info_extensions_give_the_documented_cartridge_lists() {
    // Stella: `bin` is the only disc-like extension of the core, so it is kept for the cartridges.
    assert_eq!(cartridge_extensions(&list("a26|bin")), ["a26", "bin"]);
    // Beetle PCE FAST loses its disc and playlist extensions.
    assert_eq!(
        cartridge_extensions(&list("pce|cue|ccd|chd|toc|m3u")),
        ["pce"]
    );
    // Flycast: archives (`zip`, `7z`; `rar` is not one of the 8.0 archive extensions) and playlists go, `bin` goes
    // with the disc formats of its core.
    let flycast = cartridge_extensions(&list("chd|cdi|elf|bin|cue|gdi|lst|zip|rar|dat|7z|m3u"));
    for gone in ["chd", "cdi", "gdi", "cue", "zip", "7z", "m3u", "bin"] {
        assert!(
            !flycast.iter().any(|e| e == gone),
            "{gone} kept: {flycast:?}"
        );
    }
}

#[test]
fn definitions_of_the_two_real_cores_are_accepted_published_and_proposed_with_their_table_ids() {
    let env = Env::new();
    let a2600 = proposed_id(Some("Atari - 2600"), "Atari 2600", &|_| false);
    let pce = proposed_id(
        Some("NEC - PC Engine - TurboGrafx 16"),
        "PC Engine",
        &|_| false,
    );
    assert_ne!(a2600, pce);
    add(&env, &draft(&a2600, Some("Atari - 2600"), &["a26", "bin"]));
    add(
        &env,
        &draft(&pce, Some("NEC - PC Engine - TurboGrafx 16"), &["pce"]),
    );
    let loaded = Catalog::load(&env.systems);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    let catalog = env.publish();
    for id in [&a2600, &pce] {
        assert!(catalog.system(id).is_some_and(|s| s.is_custom()), "{id}");
    }
    // `bin` stays the Mega Drive extension outside the folder of the custom system (folder-only extension).
    let md = catalog.for_extension("bin");
    assert!(
        md.iter().any(|s| s.id == "megadrive"),
        "the official `bin` is untouched: {md:?}"
    );
    // The names offered by the table are the ones scan and tidy use as folders.
    assert!(!catalog.reserved_folders().contains(&a2600));
}

// ------------------------------------------------------------------------------------------------- the scan

/// A tree of official games at the root and in their folders, plus loose files of shared extensions.
fn official_tree(roms: &Path) {
    write(&roms.join("snes").join("Alpha.sfc"), &noise(3000, 1));
    write(&roms.join("nes").join("Beta.nes"), &noise(2500, 2));
    write(&roms.join("Gamma.gba"), &noise(2200, 3));
    write(&roms.join("gb").join("Delta.gb"), &noise(2100, 4));
    write(&roms.join("megadrive").join("Echo.md"), &noise(4000, 5));
    // Shared extension at the root: no folder, no rule.
    write(&roms.join("Loose.bin"), &noise(2300, 6));
    write(&roms.join("Notes.txt"), b"not a game");
}

fn report_shape(r: &ScanReport) -> (u64, u64, u64, u64, u64, usize, usize, bool) {
    (
        r.added,
        r.updated,
        r.removed,
        r.unchanged,
        r.unclassified,
        r.unclassified_paths.len(),
        r.errors.len(),
        r.cautious,
    )
}

#[test]
fn five_custom_systems_change_neither_the_attribution_nor_the_report_of_official_files() {
    let env = Env::new();
    official_tree(&env.roms);

    let mut plain = Library::open_in_memory().unwrap();
    let before = env.scan(&mut plain);
    let rows_before = systems_by_path(&plain, &env.roms);
    assert_eq!(rows_before.len(), 6, "{rows_before:?}");

    let a2600 = proposed_id(Some("Atari - 2600"), "Atari 2600", &|_| false);
    add(&env, &draft(&a2600, Some("Atari - 2600"), &["a26", "bin"]));
    add(&env, &draft("zzone", Some("Zz - One"), &["zza"]));
    add(&env, &draft("zztwo", None, &["zzb", "zzc"]));
    add(&env, &draft("zzthree", Some("Zz - Three"), &["zzd"]));
    add(&env, &draft("zzfour", None, &["zze"]));
    let catalog = env.publish();
    assert_eq!(catalog.customs().count(), 5);

    let mut custom = Library::open_in_memory().unwrap();
    let after = env.scan(&mut custom);
    assert_eq!(
        report_shape(&after),
        report_shape(&before),
        "the report does not change"
    );
    assert_eq!(systems_by_path(&custom, &env.roms), rows_before);

    // Now the games of the custom systems: attributed by their folder, official rows untouched.
    write(&env.roms.join(&a2600).join("Berta.bin"), &noise(4096, 7));
    write(&env.roms.join(&a2600).join("Other.a26"), &noise(2048, 8));
    write(&env.roms.join("zzone").join("One.zza"), &noise(1500, 9));
    write(&env.roms.join("zztwo").join("Two.zzc"), &noise(1600, 10));
    // Its own extension outside its folder: no rule attributes it (folder-only), nothing is invented.
    write(&env.roms.join("Stray.zzd"), &noise(1700, 11));
    let third = env.scan(&mut custom);
    assert_eq!(third.added, 4, "{third:?}");
    let rows = systems_by_path(&custom, &env.roms);
    assert_eq!(rows[&format!("{a2600}/Berta.bin")], a2600);
    assert_eq!(rows[&format!("{a2600}/Other.a26")], a2600);
    assert_eq!(rows["zzone/One.zza"], "zzone");
    assert_eq!(rows["zztwo/Two.zzc"], "zztwo");
    for (path, system) in &rows_before {
        assert_eq!(&rows[path], system, "{path} kept its system");
    }
    // The loose `.bin` at the root is still the Mega Drive's, not the custom system's.
    assert_eq!(rows["Loose.bin"], "megadrive");
    assert!(!rows.contains_key("Stray.zzd"), "{rows:?}");
}

#[test]
fn a_system_without_a_libretro_name_has_no_database_name_to_ask_anything_for() {
    let env = Env::new();
    add(&env, &draft("zznodb", None, &["zzn"]));
    add(&env, &draft("zzdb", Some("Zz - Named"), &["zzm"]));
    let catalog = env.publish();
    let nodb = catalog.system("zznodb").unwrap();
    // The caller of every network job (DAT, thumbnails) keys on this name: none, no request.
    assert_eq!(nodb.libretro_name(), None);
    assert_eq!(nodb.core_key(), "zznodb");
    assert_eq!(
        catalog.system("zzdb").unwrap().libretro_name(),
        Some("Zz - Named")
    );

    write(&env.roms.join("zznodb").join("A.zzn"), &noise(1000, 1));
    let mut lib = Library::open_in_memory().unwrap();
    assert_eq!(env.scan(&mut lib).added, 1);
    // No DAT row is created by a scan, whatever the name.
    assert!(lib.dat_infos("zznodb").unwrap().is_empty());
}

#[test]
#[ignore = "needs scripts/fetch-test-assets.sh --with-8.0 (Berta and Butterflies, MIT)"]
fn a_real_atari_2600_homebrew_is_a_game_of_the_custom_system_under_its_folder() {
    let rom = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("roms")
        .join("a2600")
        .join("berta-and-butterflies.v1.00.ntsc.en.bin");
    let bytes = fs::read(&rom).expect("run scripts/fetch-test-assets.sh --with-8.0 first");
    let env = Env::new();
    let id = proposed_id(Some("Atari - 2600"), "Atari 2600", &|_| false);
    add(&env, &draft(&id, Some("Atari - 2600"), &["a26", "bin"]));
    env.publish();
    write(
        &env.roms.join(&id).join("Berta and Butterflies.bin"),
        &bytes,
    );
    let mut lib = Library::open_in_memory().unwrap();
    let report = env.scan(&mut lib);
    assert_eq!(report.added, 1, "{report:?}");
    let games = lib.games().unwrap();
    assert_eq!(games[0].system, id);
    assert_eq!(games[0].size, 4096);
    assert!(games[0].crc32.is_some() && games[0].sha1.is_some());
}

// ------------------------------------------------------------------------------- tidy: reserved folders

#[test]
fn a_retired_system_keeps_its_folder_out_of_the_tidy_and_restoring_gives_every_game_back() {
    let env = Env::new();
    add(&env, &draft("zzfic", Some("Zz - Fictional"), &["fic"]));
    env.publish();
    let own = env.roms.join("zzfic");
    write(&own.join("Game.fic"), &noise(3000, 1));
    // Files the official systems would take, sitting in the folder of the system.
    write(&own.join("Found.sfc"), &noise(2600, 2));
    write(&own.join("Tool.bin"), &noise(2400, 3));
    write(&env.roms.join("Loose.sfc"), &noise(2800, 4));

    let mut lib = Library::open_in_memory().unwrap();
    env.scan(&mut lib);
    let by_path = systems_by_path(&lib, &env.roms);
    assert_eq!(by_path["zzfic/Game.fic"], "zzfic");
    lib.set_favorite(
        lib.games()
            .unwrap()
            .iter()
            .find(|g| g.title == "Game")
            .unwrap()
            .id,
        true,
    )
    .unwrap();
    let untouched = tree(&own);

    // « Supprimer »: the definition goes to `.retired/`, the games stay in the database but are hidden.
    retire_custom(&env.systems, "zzfic", Retirement::Removed, 1_764_028_900).unwrap();
    let after = env.publish();
    assert!(after.system("zzfic").is_none());
    assert!(after.reserved_folders().iter().any(|f| f == "zzfic"));
    assert!(
        lib.games().unwrap().iter().all(|g| g.title != "Game"),
        "hidden"
    );

    let plan = lib
        .plan_tidy(&env.options(), &never(), &mut |_| {})
        .unwrap();
    assert!(
        plan.reserved_folders.iter().any(|f| f == "zzfic"),
        "{:?}",
        plan.reserved_folders
    );
    for unit in &plan.units {
        for m in &unit.moves {
            let p = format!("{m:?}");
            assert!(
                !p.contains("zzfic"),
                "a unit takes from a reserved folder: {p}"
            );
        }
    }
    let ids: Vec<u32> = plan.units.iter().map(|u| u.id).collect();
    let report = lib
        .apply_tidy(&env.options(), &plan, &ids, &never(), &mut |_| {})
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(tree(&own), untouched, "nothing taken, nothing put");
    assert!(own.is_dir());
    assert!(
        env.roms.join("snes").join("Loose.sfc").is_file(),
        "the rest is tidied"
    );

    // « Restaurer »: the same file is back in `systems/`, the favourite game is visible with its row.
    let retired = retired_customs(&env.systems);
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].why, Retirement::Removed);
    restore_custom(
        &env.systems,
        &retired[0].file,
        &Catalog::load(&env.systems).catalog,
    )
    .unwrap();
    let back = env.publish();
    assert!(back.system("zzfic").is_some());
    let game = lib
        .games()
        .unwrap()
        .into_iter()
        .find(|g| g.title == "Game")
        .expect("visible again");
    assert!(game.favorite, "the row and its favourite were kept");
    assert_eq!(game.system, "zzfic");
    env.scan(&mut lib);
    assert_eq!(tree(&own), untouched);
}

// -------------------------------------------------------------------------------------- the switch, end to end

fn mysnes_definition(env: &Env) {
    let text = format!(
        "format = 1\nrender = \"software\"\n\n[system]\nid = \"mysnes\"\nname = \"Sys mysnes\"\n\
         database = \"{SNES_DATABASE}\"\nextensions = [\"sfc\", \"smc\", \"xyz\"]\ndat = []\n\n\
         [cores]\nlist = [\"demo_core\"]\nignored_firmware = []\n\n\
         [origin]\ncore = \"demo_core\"\ncreated_by = \"8.0.0\"\ncreated_unix = 1\n"
    );
    fs::write(env.systems.join("mysnes.toml"), text).unwrap();
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
        prior_preference: Some(vec!["snes9x".into()]),
        prior_profile: None,
    }
}

/// `roms/mysnes/` with two games the official Super Nintendo takes and one only the definition takes, scanned
/// into a file-backed database (favourite and play count set), the definition published.
fn switch_fixture(env: &Env) -> (Library, PathBuf) {
    mysnes_definition(env);
    env.publish();
    let own = env.roms.join("mysnes");
    write(&own.join("Game.sfc"), &noise(3000, 1));
    write(&own.join("Second.smc"), &noise(2500, 2));
    write(&own.join("Mine.xyz"), &noise(2000, 3));
    let db = env.root.join("library.sqlite");
    let (mut lib, _) = Library::open(&db).unwrap();
    assert_eq!(env.scan(&mut lib).added, 3);
    for g in lib.games().unwrap() {
        lib.set_favorite(g.id, true).unwrap();
        lib.record_played(g.id, 1_800_000_000).unwrap();
    }
    (lib, db)
}

fn after_catalog(env: &Env) -> Catalog {
    Catalog::load_without(&env.systems, "mysnes").catalog
}

fn journal_files(env: &Env) -> Vec<PathBuf> {
    // `journal-<unix>.txt` only: the folder also holds the lock file.
    let mut v: Vec<PathBuf> = fs::read_dir(&env.journals)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("journal-") && n.ends_with(".txt"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn run_switch(env: &Env, lib: &mut Library) -> emu_library::TidyReport {
    let options = env.options();
    let plan = lib
        .plan_switch(&options, &after_catalog(env), &mv(), &never(), &mut |_| {})
        .unwrap();
    assert!(plan.units.len() >= 2, "{:?}", plan.units);
    lib.apply_switch(&options, &plan, &choices(), &never(), &mut |_| {})
        .unwrap()
}

#[test]
fn the_definition_of_the_official_database_under_another_id_proposes_a_switch_with_move() {
    let env = Env::new();
    mysnes_definition(&env);
    let loaded = Catalog::load(&env.systems);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    let proposal = loaded
        .pending_switches
        .iter()
        .find(|p| p.custom_id == "mysnes")
        .expect("a proposal");
    assert_eq!(proposal.kind, SwitchKind::NeedsMove);
    assert_eq!(proposal.official.id, "snes");
}

#[test]
fn a_switch_runs_is_cut_before_its_end_resumes_after_a_restart_then_is_undone_and_redone() {
    let env = Env::new();
    let (mut lib, db) = switch_fixture(&env);
    let options = env.options();
    assert!(open_switch(&options).unwrap().is_none());

    let report = run_switch(&env, &mut lib);
    assert!(
        report.errors.is_empty() && report.left.is_empty(),
        "{report:?}"
    );
    let journals = journal_files(&env);
    assert_eq!(journals.len(), 1);
    assert!(
        fs::read_to_string(&journals[0])
            .unwrap()
            .starts_with("emu-rust tidy 2\n"),
        "the switch writes the version 2 journal"
    );
    for name in ["Game.sfc", "Second.smc"] {
        assert!(env.roms.join("snes").join(name).is_file(), "{name}");
        assert!(!env.roms.join("mysnes").join(name).exists(), "{name}");
    }
    assert!(
        env.roms.join("mysnes").join("Mine.xyz").is_file(),
        "not taken by the official system"
    );
    // The end is due: the definition is still active, the definitions are not editable (open switch).
    assert_eq!(open_switch(&options).unwrap(), Some(mv()));
    assert!(env.systems.join("mysnes.toml").is_file());

    // The application is killed here. A new process opens the same database and the same journals.
    drop(lib);
    let (mut lib, _) = Library::open(&db).unwrap();
    assert!(
        lib.recover_tidy(&options).unwrap().is_none(),
        "nothing in progress, only an end due"
    );
    // While the end is due, no other tidy for any root.
    let plain = lib.plan_tidy(&options, &never(), &mut |_| {}).unwrap();
    let ids: Vec<u32> = plain.units.iter().map(|u| u.id).collect();
    match lib.apply_tidy(&options, &plain, &ids, &never(), &mut |_| {}) {
        Err(LibraryError::TidyRefused { reason, .. }) => {
            assert_eq!(reason, TidyRefusal::SwitchPending);
        }
        other => panic!("expected SwitchPending, got {other:?}"),
    }
    let pending = lib.pending_switch(&options).unwrap().expect("end due");
    assert_eq!(pending.direction, SwitchDirection::Forward);
    assert_eq!(pending.switch, mv());
    assert_eq!(pending.choices, choices());
    lib.complete_switch(&options, &pending, &env.systems)
        .unwrap();
    assert!(lib.pending_switch(&options).unwrap().is_none());
    assert!(open_switch(&options).unwrap().is_none());
    // Idempotent: a second call after `F` changes nothing.
    lib.complete_switch(&options, &pending, &env.systems)
        .unwrap();
    assert!(!env.systems.join("mysnes.toml").exists());
    let retired = retired_customs(&env.systems);
    assert_eq!(retired.len(), 1, "{retired:?}");
    assert_eq!(retired[0].why, Retirement::Replaced);

    let published = env.publish();
    assert!(published.system("mysnes").is_none());
    assert!(published.system("snes").is_some_and(|s| !s.is_custom()));
    let rows = systems_by_path(&lib, &env.roms);
    assert_eq!(rows["snes/Game.sfc"], "snes");
    assert_eq!(rows["snes/Second.smc"], "snes");
    // The game only the definition took is gelé: not shown, not lost.
    assert!(!rows.contains_key("mysnes/Mine.xyz"));
    for g in lib.games().unwrap() {
        assert!(
            g.favorite && g.play_count == 1,
            "{}: favourite and history follow",
            g.title
        );
    }

    // Undo: files, rows and the definition come back.
    let journal = journal_files(&env)[0].clone();
    lib.check_switch_undo(&options, &journal, &env.systems)
        .unwrap();
    let undone = lib
        .undo_tidy(&options, &journal, &never(), &mut |_| {})
        .unwrap();
    assert!(
        undone.errors.is_empty() && undone.left.is_empty(),
        "{undone:?}"
    );
    let pending = lib.pending_switch(&options).unwrap().expect("undo end due");
    assert_eq!(pending.direction, SwitchDirection::Undo);
    lib.complete_switch(&options, &pending, &env.systems)
        .unwrap();
    assert!(env.systems.join("mysnes.toml").is_file());
    assert!(retired_customs(&env.systems).is_empty());
    env.publish();
    let rows = systems_by_path(&lib, &env.roms);
    assert_eq!(rows["mysnes/Game.sfc"], "mysnes");
    assert_eq!(rows["mysnes/Second.smc"], "mysnes");
    assert_eq!(rows["mysnes/Mine.xyz"], "mysnes");
    assert!(
        !env.roms.join("snes").exists(),
        "the folder made by the switch goes with it"
    );
    for g in lib.games().unwrap() {
        assert!(g.favorite && g.play_count == 1, "{}", g.title);
    }

    // The same switch can be run again after an undo (the retired names never collide).
    let again = run_switch(&env, &mut lib);
    assert!(
        again.errors.is_empty() && again.left.is_empty(),
        "{again:?}"
    );
    let pending = lib
        .pending_switch(&options)
        .unwrap()
        .expect("end due again");
    lib.complete_switch(&options, &pending, &env.systems)
        .unwrap();
    assert_eq!(retired_customs(&env.systems).len(), 1);
}

#[test]
fn a_journal_cut_before_its_end_line_is_recovered_then_the_switch_is_completed() {
    let env = Env::new();
    let (mut lib, db) = switch_fixture(&env);
    let options = env.options();
    let report = run_switch(&env, &mut lib);
    assert!(report.errors.is_empty(), "{report:?}");
    let journal = journal_files(&env).remove(0);
    let text = fs::read_to_string(&journal).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // The last line is `E`: the cut drops it, as a power failure between the last unit and `E` would.
    assert_eq!(lines.last().map(|l| l.trim()), Some("E"), "{lines:?}");
    let cut = format!("{}\n", lines[..lines.len() - 1].join("\n"));
    fs::write(&journal, cut).unwrap();
    drop(lib);

    let (mut lib, _) = Library::open(&db).unwrap();
    // In progress: the definitions stay locked and no end is due yet.
    assert_eq!(open_switch(&options).unwrap(), Some(mv()));
    assert!(lib.pending_switch(&options).unwrap().is_none());
    let recovered = lib
        .recover_tidy(&options)
        .unwrap()
        .expect("a journal to recover");
    assert!(recovered.errors.is_empty(), "{recovered:?}");
    // Every unit was validated before the cut: recovery keeps them where they are, closes the run, and the end of
    // the switch is due (as after an `E`).
    assert_eq!(recovered.moved.len(), 2, "{recovered:?}");
    let pending = lib.pending_switch(&options).unwrap().expect("end due");
    assert_eq!(pending.direction, SwitchDirection::Forward);
    lib.complete_switch(&options, &pending, &env.systems)
        .unwrap();
    assert!(lib.pending_switch(&options).unwrap().is_none());
    assert!(open_switch(&options).unwrap().is_none());
    env.publish();
    let rows = systems_by_path(&lib, &env.roms);
    assert_eq!(
        rows,
        BTreeMap::from([
            ("snes/Game.sfc".to_owned(), "snes".to_owned()),
            ("snes/Second.smc".to_owned(), "snes".to_owned()),
        ]),
        "the game only the definition took is hidden, not lost"
    );
    for g in lib.games().unwrap() {
        assert!(g.path.exists(), "{:?}", g.path);
    }
}

#[test]
fn a_cancelled_switch_is_incomplete_the_definition_stays_active_and_no_game_is_hidden() {
    let env = Env::new();
    let (mut lib, _) = switch_fixture(&env);
    let options = env.options();
    let plan = lib
        .plan_switch(&options, &after_catalog(&env), &mv(), &never(), &mut |_| {})
        .unwrap();
    let cancel = AtomicBool::new(false);
    let report = lib
        .apply_switch(&options, &plan, &choices(), &cancel, &mut |_| {
            cancel.store(true, Ordering::SeqCst);
        })
        .unwrap();
    assert!(report.cancelled, "{report:?}");
    assert!(
        lib.pending_switch(&options).unwrap().is_none(),
        "no end is due after a cancellation"
    );
    assert!(env.systems.join("mysnes.toml").is_file());
    assert!(retired_customs(&env.systems).is_empty());
    env.publish();
    let games = lib.games().unwrap();
    assert_eq!(games.len(), 3, "no game hidden");
    for g in &games {
        assert_eq!(g.system, "mysnes");
        assert!(g.path.exists());
    }
}

// --------------------------------------------------------------------------------------------- migration

#[test]
fn a_v7_base_opened_by_8_0_and_scanned_with_custom_systems_loses_nothing() {
    use rusqlite::Connection;
    let env = Env::new();
    official_tree(&env.roms);
    let db = env.root.join("library.sqlite");
    let favourite_path;
    {
        let (mut lib, _) = Library::open(&db).unwrap();
        env.scan(&mut lib);
        let g = lib
            .games()
            .unwrap()
            .into_iter()
            .find(|g| g.title == "Alpha")
            .unwrap();
        lib.set_favorite(g.id, true).unwrap();
        lib.record_played(g.id, 1_800_000_000).unwrap();
        favourite_path = g.path;
    }
    // Back to the v7 layout (the only change of v8 is `game.scan_pending`).
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("ALTER TABLE game DROP COLUMN scan_pending;")
            .unwrap();
        conn.pragma_update(None, "user_version", 7).unwrap();
    }
    add(&env, &draft("zzone", Some("Zz - One"), &["zza"]));
    add(&env, &draft("zztwo", None, &["zzb"]));
    env.publish();

    let (mut lib, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, emu_library::OpenOutcome::Opened);
    let before = systems_by_path(&lib, &env.roms);
    assert_eq!(before.len(), 6);
    write(&env.roms.join("zzone").join("One.zza"), &noise(1500, 9));
    let report = env.scan(&mut lib);
    assert_eq!(
        (report.added, report.removed, report.unchanged),
        (1, 0, 6),
        "{report:?}"
    );
    assert!(!report.cautious);
    let after = systems_by_path(&lib, &env.roms);
    for (path, system) in &before {
        assert_eq!(&after[path], system);
    }
    let alpha = lib
        .games()
        .unwrap()
        .into_iter()
        .find(|g| g.path == favourite_path)
        .unwrap();
    assert!(alpha.favorite && alpha.play_count == 1);
}
