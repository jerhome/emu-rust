// Real cores and mock core: Linux only (`.so`, `cc` for the mock core).
#![cfg(target_os = "linux")]
//! Real 7.1 tests of the ROM tidying (ADR 0023 §4, design `docs/design/v7.1-finition-interface.md` §5, task FQ3):
//! a « loose » `roms/` folder made of the free data already fetched (240p Test Suite Mega Drive and PlayStation
//! `.cue` + `.bin`, GBA `sram.gba`, NES / SMS / GG / GBC / SNES homebrews, plus one generated ROM for the mock core
//! whose SRAM is deterministic) is played, saved, tidied, played again; then the tidying is undone.
//!
//! All `#[ignore]`: `scripts/fetch-test-assets.sh --with-7.0` (the GBA, 32X… data come with `--with-6.0`), then
//! `cargo test -p emu-ui --test tidy_real -- --ignored --test-threads=1 --nocapture`.
//! The last test downloads `info.zip` from the libretro buildbot (cached in `target/test-output/`): the only
//! network access, in an `#[ignore]` test (CLAUDE.md « terminé »).
//!
//! Measured limit of ADR 0023 (« états sauvegardés qui mémorisent le chemin du média »): what happens to the
//! state of a CD game after tidying is printed (`--nocapture`) and asserted by `cd_state_after_tidying_is_measured`.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};

use emu_library::{Library, ScanOptions, TidyOptions, TidyPlan, TidyReport, TidyStatus, last_tidy};
use emu_runtime::{
    CollectSink, Emulator, EmulatorOptions, JoypadButton, NullInput, ScriptedInput, StateOrigin,
    core_id_from_path,
};
use emu_ui::assets::cores::CorePreferences;
use emu_ui::{DataDirs, GameContent, content_spec, finish_pending_identities, resolve_game};

/// One libretro core per process at a time (ADR 0008).
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn asset(rel: &str) -> PathBuf {
    let p = root().join("tests").join(rel);
    assert!(
        p.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh --with-7.0 --with-6.0",
        p.display()
    );
    p
}

fn core(name: &str) -> PathBuf {
    asset(&format!("cores/{name}_libretro.so"))
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Every file under `dir` (relative path with `/`) to its hash.
fn tree(dir: &Path) -> BTreeMap<String, u64> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, u64>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fnv(&fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// Registries and locks of the runtime are bookkeeping, not saves.
fn saves_tree(dir: &Path) -> BTreeMap<String, u64> {
    tree(dir)
        .into_iter()
        .filter(|(k, _)| !k.contains(".emu-rust-games") && !k.ends_with(".lock"))
        .collect()
}

/// One game of the loose tree: which core plays it and where it starts.
struct Title {
    label: &'static str,
    core: PathBuf,
    /// Relative to `roms/`, before tidying.
    loose: &'static str,
}

struct Rig {
    _tmp: tempfile::TempDir,
    dirs: DataDirs,
    roms: PathBuf,
    titles: Vec<Title>,
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(from, to).unwrap();
}

impl Rig {
    /// A data folder with a loose `roms/`: files at the root (and one in a sub-folder), no system folder.
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let dirs = DataDirs::under(&base.join("data"));
        let roms = dirs.root.join("roms");
        fs::create_dir_all(&roms).unwrap();
        for sub in [&dirs.system, &dirs.saves, &dirs.states, &dirs.assets] {
            fs::create_dir_all(sub).unwrap();
        }
        let r = |rel: &str| asset(&format!("roms/{rel}"));
        // Real homebrews (free licences), all loose at the root.
        copy(&r("240pSuite-1.21.bin"), &roms.join("240pSuite-1.21.bin"));
        copy(&r("gba/sram.gba"), &roms.join("sram.gba"));
        copy(
            &r("psx/240pTestSuitePS1-EMU.cue"),
            &roms.join("240pTestSuitePS1-EMU.cue"),
        );
        copy(
            &r("psx/240pTestSuitePS1-EMU.bin"),
            &roms.join("240pTestSuitePS1-EMU.bin"),
        );
        copy(&r("240pee-0.23.nes"), &roms.join("240pee-0.23.nes"));
        copy(
            &r("sub_rescue-0.3.sms"),
            &roms.join("homebrew/sub_rescue.sms"),
        );
        copy(&r("waternet-1.0.gg"), &roms.join("waternet-1.0.gg"));
        copy(
            &r("240pSuite-SNES-1.02.sfc"),
            &roms.join("240pSuite-SNES-1.02.sfc"),
        );
        copy(&r("gb240p-0.23.gb"), &roms.join("gb240p-0.23.gb"));
        // Generated: the mock core writes a frame counter to its SRAM (deterministic `.srm`).
        fs::write(
            roms.join("Mock Game.md"),
            b"mock game content for the tidy test",
        )
        .unwrap();
        let titles = vec![
            Title {
                label: "Mega Drive 240p",
                core: core("genesis_plus_gx"),
                loose: "240pSuite-1.21.bin",
            },
            Title {
                label: "GBA sram.gba",
                core: core("mgba"),
                loose: "sram.gba",
            },
            Title {
                label: "PS1 240p (cue)",
                core: core("pcsx_rearmed"),
                loose: "240pTestSuitePS1-EMU.cue",
            },
            Title {
                label: "Mock Game",
                core: mock_core::build(mock_core::Variant::Default),
                loose: "Mock Game.md",
            },
        ];
        Rig {
            _tmp: tmp,
            dirs,
            roms,
            titles,
        }
    }

    fn emu_options(&self) -> EmulatorOptions {
        EmulatorOptions {
            system_dir: self.dirs.system.clone(),
            save_dir: self.dirs.saves.clone(),
            state_dir: self.dirs.states.clone(),
            assets_dir: self.dirs.assets.clone(),
            core_variables: Vec::new(),
            restore_added_disks: true,
            core_save_scope: emu_runtime::SaveScope::Core,
            fastforward_override: false,
            variable_state_size: false,
            port_devices: Vec::new(),
            cache_dir: self.dirs.root.join("cache"),
            provide_vfs: false,
        }
    }

    /// The UI launch path (`content_spec` → `resolve_game` → `load_content_as`), as `App::build` does.
    fn launch(&self, core: &Path, rom: &Path) -> (emu_ui::ResolvedGame, Emulator) {
        let id = core_id_from_path(core);
        let spec = content_spec(
            &id,
            &GameContent::Files(vec![rom.to_path_buf()]),
            &CorePreferences::builtin(),
        );
        let resolved = resolve_game(&id, spec, &self.dirs);
        let emu = Emulator::load_content_as(
            core,
            resolved.spec.clone(),
            resolved.identity.clone(),
            self.emu_options(),
        )
        .unwrap_or_else(|e| panic!("load {}: {e}", rom.display()));
        (resolved, emu)
    }

    fn library(&self) -> Library {
        Library::open(&self.dirs.library_db).unwrap().0
    }

    fn scan(&self, library: &mut Library) {
        let options = ScanOptions {
            roots: vec![self.roms.clone()],
            ..Default::default()
        };
        library.scan(&options, &never(), &mut |_| {}).unwrap();
    }

    fn tidy_options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.dirs.tidy_journals).unwrap()
    }

    fn plan(&self, library: &Library) -> TidyPlan {
        library
            .plan_tidy(&self.tidy_options(), &never(), &mut |_| {})
            .unwrap()
    }

    /// Plays `frames` frames holding Start (the mock core writes SRAM), flushes SRAM, saves state 0 and returns
    /// the hash of the frame that follows the save.
    fn play_and_save(&self, core: &Path, rom: &Path, frames: u64) -> u64 {
        let (_, mut emu) = self.launch(core, rom);
        let mut input = ScriptedInput::new().press(0, JoypadButton::Start, 0..frames);
        let mut sink = CollectSink::default();
        for _ in 0..frames {
            emu.run_frame(&mut input, &mut sink).unwrap();
        }
        emu.flush_sram().unwrap();
        emu.save_state(0).unwrap();
        emu.run_frame(&mut NullInput, &mut sink).unwrap();
        let h = fnv(emu.frame().pixels());
        drop(emu);
        h
    }
}

fn play_all(rig: &Rig) -> BTreeMap<&'static str, u64> {
    let mut after_save = BTreeMap::new();
    for t in &rig.titles {
        let rom = rig.roms.join(t.loose);
        let frames = if t.label.starts_with("PS1") { 90 } else { 40 };
        after_save.insert(t.label, rig.play_and_save(&t.core, &rom, frames));
    }
    after_save
}

/// Where each unit went: `(original relative, current relative)` for the titles, from the report.
fn moved_rel(rig: &Rig, report: &TidyReport) -> BTreeMap<String, String> {
    let root = fs::canonicalize(&rig.roms).unwrap();
    report
        .moved
        .iter()
        .map(|(from, to)| {
            let rel = |p: &Path| {
                p.strip_prefix(&root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            };
            (rel(from), rel(to))
        })
        .collect()
}

fn apply_sure(rig: &Rig, library: &mut Library) -> (TidyPlan, TidyReport) {
    let plan = rig.plan(library);
    let ids: Vec<u32> = plan
        .units
        .iter()
        .filter(|u| matches!(u.status, TidyStatus::Sure(_)) && u.system.is_some())
        .map(|u| u.id)
        .collect();
    assert!(!ids.is_empty(), "{:#?}", plan.units);
    let report = library
        .apply_tidy(&rig.tidy_options(), &plan, &ids, &never(), &mut |_| {})
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    (plan, report)
}

fn undo_last(rig: &Rig, library: &mut Library) -> TidyReport {
    let info = last_tidy(&rig.tidy_options()).unwrap().expect("a journal");
    assert!(info.can_undo(), "{info:?}");
    let report = library
        .undo_tidy(&rig.tidy_options(), &info.path, &never(), &mut |_| {})
        .unwrap();
    assert!(
        report.errors.is_empty() && report.left.is_empty(),
        "{report:?}"
    );
    report
}

/// Checks every title against what it was before tidying: same save name, same `.srm` and state bytes on disk,
/// state loads and says it is the same content. Returns `(loaded_ok, origin, same_next_frame)` per title.
fn relaunch_all(
    rig: &Rig,
    paths: &BTreeMap<&'static str, PathBuf>,
    names: &BTreeMap<&'static str, String>,
    saves_before: &BTreeMap<String, u64>,
    after_save: &BTreeMap<&'static str, u64>,
) -> BTreeMap<&'static str, (bool, Option<StateOrigin>, bool)> {
    let mut out = BTreeMap::new();
    for t in &rig.titles {
        let rom = &paths[t.label];
        assert!(rom.exists(), "{}: {} missing", t.label, rom.display());
        let (resolved, mut emu) = rig.launch(&t.core, rom);
        assert_eq!(
            resolved.identity.save_name, names[t.label],
            "{}: the save name must not change",
            t.label
        );
        assert_eq!(resolved.identity.notice, None, "{}: no banner", t.label);
        let origin = emu.state_origin(0);
        let load = emu.load_state(0);
        if let Err(e) = &load {
            println!("  {}: load_state error: {e}", t.label);
        }
        let loaded = load.is_ok();
        let mut sink = CollectSink::default();
        let same_frame = loaded && {
            emu.run_frame(&mut NullInput, &mut sink).unwrap();
            fnv(emu.frame().pixels()) == after_save[t.label]
        };
        drop(emu);
        out.insert(t.label, (loaded, origin, same_frame));
    }
    // Relaunching must not have rewritten anything (an unchanged SRAM is not written again).
    assert_eq!(&saves_tree(&rig.dirs.saves), saves_before, "saves changed");
    out
}

struct Baseline {
    names: BTreeMap<&'static str, String>,
    after_save: BTreeMap<&'static str, u64>,
    saves: BTreeMap<String, u64>,
    states: BTreeMap<String, u64>,
}

fn baseline(rig: &Rig) -> Baseline {
    let after_save = play_all(rig);
    let mut names = BTreeMap::new();
    for t in &rig.titles {
        let (resolved, emu) = rig.launch(&t.core, &rig.roms.join(t.loose));
        names.insert(t.label, resolved.identity.save_name.clone());
        drop(emu);
    }
    let saves = saves_tree(&rig.dirs.saves);
    let states = tree(&rig.dirs.states);
    assert!(
        saves.keys().any(|k| k.ends_with(".srm")),
        "at least one SRAM expected: {saves:?}"
    );
    println!("saves: {saves:?}");
    for srm in ["sram.srm", "Mock Game.srm"] {
        assert!(
            saves.keys().any(|k| k.ends_with(srm)),
            "{srm} expected: {saves:?}"
        );
    }
    assert_eq!(states.len(), rig.titles.len(), "{states:?}");
    Baseline {
        names,
        after_save,
        saves,
        states,
    }
}

fn paths_after(rig: &Rig, moved: &BTreeMap<String, String>) -> BTreeMap<&'static str, PathBuf> {
    rig.titles
        .iter()
        .map(|t| {
            let rel = moved.get(t.loose).map_or(t.loose, String::as_str);
            (t.label, rig.roms.join(rel))
        })
        .collect()
}

#[test]
#[ignore = "real cores: scripts/fetch-test-assets.sh --with-7.0 --with-6.0"]
fn tidy_then_relaunch_finds_srm_and_states_and_undo_restores_everything() {
    let _guard = lock();
    let rig = Rig::new();
    let base = baseline(&rig);
    // Control: the same relaunch without any tidying. What a core cannot do by itself (mgba rejects its own
    // state on `sram.gba`) must not be blamed on the tidying: after tidying every title must behave like this.
    let loose: BTreeMap<&'static str, PathBuf> = rig
        .titles
        .iter()
        .map(|t| (t.label, rig.roms.join(t.loose)))
        .collect();
    let control = relaunch_all(&rig, &loose, &base.names, &base.saves, &base.after_save);
    println!("control, before tidying: (state loaded, origin, same next frame)");
    for (label, r) in &control {
        println!("  {label}: {r:?}");
    }
    let mut library = rig.library();
    rig.scan(&mut library);
    let tree_before = tree(&rig.roms);
    let games_before = library.games().unwrap();
    assert!(games_before.len() >= 6, "{games_before:#?}");
    let ids_before: BTreeMap<String, _> = games_before
        .iter()
        .map(|g| {
            (
                g.path
                    .strip_prefix(&rig.roms)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                g.id,
            )
        })
        .collect();

    // Tidy (journal, rows, then identities of the registry, as the UI does right after the run).
    let (_plan, report) = apply_sure(&rig, &mut library);
    let moved = moved_rel(&rig, &report);
    println!("moved by the tidying:");
    for (a, b) in &moved {
        println!("  {a} -> {b}");
    }
    for t in &rig.titles {
        assert!(
            moved.contains_key(t.loose),
            "{} was not tidied: {moved:?}",
            t.label
        );
    }
    finish_pending_identities(&rig.dirs);
    assert!(
        rig.dirs.tidy_journals.is_dir(),
        "journal folder expected after a run"
    );
    let paths = paths_after(&rig, &moved);
    for t in &rig.titles {
        assert!(
            !rig.roms.join(t.loose).exists(),
            "{} still at its old place",
            t.label
        );
    }
    // No save, state or option file was moved or renamed.
    assert_eq!(saves_tree(&rig.dirs.saves), base.saves);
    assert_eq!(tree(&rig.dirs.states), base.states);

    // The library keeps its rows (same ids, here favorites are not touched: ids prove the rows were moved).
    let games_after = library.games().unwrap();
    assert_eq!(games_after.len(), games_before.len());
    for g in &games_after {
        assert!(g.path.exists(), "row points at a missing path {:?}", g.path);
    }
    for (rel, id) in &ids_before {
        if let Some(new) = moved.get(rel) {
            let g = games_after.iter().find(|g| g.id == *id);
            assert!(
                g.is_some_and(
                    |g| g.path == rig.roms.join(new) || g.path.starts_with(rig.roms.join(new))
                ),
                "{rel}: id {id:?} must follow to {new}"
            );
        }
    }

    // Relaunch from the new paths: same save name, `.srm` and states found.
    let results = relaunch_all(&rig, &paths, &base.names, &base.saves, &base.after_save);
    println!("after tidying: (state loaded, origin, same next frame)");
    for (label, r) in &results {
        println!("  {label}: {r:?}");
    }
    assert_eq!(
        results, control,
        "tidying must not change what a relaunch finds"
    );
    assert!(
        control["Mega Drive 240p"].0 && control["Mock Game"].0 && control["PS1 240p (cue)"].0,
        "{control:?}"
    );

    // The scan after tidying changes nothing (same rows, same ids).
    rig.scan(&mut library);
    assert_eq!(library.games().unwrap().len(), games_before.len());

    // Undo: the tree and the rows come back, the saves are untouched, the old paths launch again.
    let undo = undo_last(&rig, &mut library);
    assert!(!undo.moved.is_empty());
    finish_pending_identities(&rig.dirs);
    assert_eq!(
        tree(&rig.roms),
        tree_before,
        "roms/ must be identical after undo"
    );
    let home: BTreeMap<&'static str, PathBuf> = rig
        .titles
        .iter()
        .map(|t| (t.label, rig.roms.join(t.loose)))
        .collect();
    let games_undone = library.games().unwrap();
    assert_eq!(games_undone.len(), games_before.len());
    for (rel, id) in &ids_before {
        assert!(
            games_undone
                .iter()
                .any(|g| g.id == *id && g.path == rig.roms.join(rel)),
            "{rel}: row {id:?} must be back"
        );
    }
    let back = relaunch_all(&rig, &home, &base.names, &base.saves, &base.after_save);
    println!("after undo: (state loaded, origin, same next frame)");
    for (label, r) in &back {
        println!("  {label}: {r:?}");
    }
    assert_eq!(
        back, control,
        "after undo, a relaunch finds what it found before"
    );
    assert_eq!(saves_tree(&rig.dirs.saves), base.saves);
    assert_eq!(tree(&rig.dirs.states), base.states);
}

#[test]
#[ignore = "real cores: scripts/fetch-test-assets.sh --with-7.0"]
fn cd_state_after_tidying_is_measured() {
    // ADR 0023 limit: a state may memorize the media path. Measure it on pcsx_rearmed with the 240p Test Suite
    // PS1 (cue + bin): state saved from the loose files, then loaded after the CD went to roms/<system>/<name>/.
    let _guard = lock();
    let rig = Rig::new();
    let t = rig
        .titles
        .iter()
        .find(|t| t.label.starts_with("PS1"))
        .unwrap();
    let h = rig.play_and_save(&t.core, &rig.roms.join(t.loose), 90);
    let state_file = rig
        .dirs
        .states
        .join(core_id_from_path(&t.core))
        .join("240pTestSuitePS1-EMU.state0");
    let state_bytes = fs::read(&state_file).unwrap();
    // Does the state embed the media path at all? (the loose absolute path, or its file name)
    let needle_abs = rig.roms.join(t.loose).to_string_lossy().into_owned();
    let contains = |hay: &[u8], needle: &str| {
        !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    };
    println!("state size: {} bytes", state_bytes.len());
    println!(
        "state embeds the absolute path of the cue: {}",
        contains(&state_bytes, &needle_abs)
    );
    println!(
        "state embeds the cue file name: {}",
        contains(&state_bytes, "240pTestSuitePS1-EMU.cue")
    );
    println!(
        "state embeds the bin file name: {}",
        contains(&state_bytes, "240pTestSuitePS1-EMU.bin")
    );

    let mut library = rig.library();
    rig.scan(&mut library);
    let (_plan, report) = apply_sure(&rig, &mut library);
    let moved = moved_rel(&rig, &report);
    finish_pending_identities(&rig.dirs);
    let new_cue = moved
        .get("240pTestSuitePS1-EMU.cue")
        .or_else(|| moved.get("240pTestSuitePS1-EMU"))
        .unwrap_or_else(|| panic!("the CD was not tidied: {moved:?}"))
        .clone();
    println!("CD moved: {moved:?}");
    let cue = if rig.roms.join(&new_cue).is_dir() {
        rig.roms.join(&new_cue).join("240pTestSuitePS1-EMU.cue")
    } else {
        rig.roms.join(&new_cue)
    };
    assert!(cue.is_file(), "{}", cue.display());

    let (resolved, mut emu) = rig.launch(&t.core, &cue);
    println!("save name after tidying: {:?}", resolved.identity.save_name);
    let origin = emu.state_origin(0);
    let loaded = emu.load_state(0);
    println!("state origin after tidying: {origin:?}");
    println!("load_state after tidying: {loaded:?}");
    let mut sink = CollectSink::default();
    let mut same = false;
    if loaded.is_ok() {
        emu.run_frame(&mut NullInput, &mut sink).unwrap();
        same = fnv(emu.frame().pixels()) == h;
        println!("next frame after load identical to the original run: {same}");
    }
    drop(emu);
    assert_eq!(resolved.identity.save_name, "240pTestSuitePS1-EMU");
    assert_eq!(
        fs::read(&state_file).unwrap(),
        state_bytes,
        "state file untouched"
    );
    assert!(
        loaded.is_ok(),
        "state of a CD must reload after tidying: {loaded:?}"
    );
    assert_eq!(origin, Some(StateOrigin::Same));
    println!("RESULT cd_state_reload_after_tidy = ok (same_frame = {same})");
}

#[test]
#[ignore = "downloads info.zip (libretro buildbot) once into target/test-output/"]
fn optional_firmware_matches_the_installed_info_files() {
    use emu_assets::cores::{
        CoreId, EnhancementKind, ExtractLimits, FirmwareState, KnownFirmware,
        firmware_enhancements, parse_info, read_info_bundle,
    };
    let zip_path = root().join("target/test-output/info.zip");
    if !zip_path.is_file() {
        fs::create_dir_all(zip_path.parent().unwrap()).unwrap();
        let ok = Command::new("curl")
            .args(["-sS", "--fail", "--max-time", "120", "-o"])
            .arg(&zip_path)
            .arg("https://buildbot.libretro.com/assets/frontend/info.zip")
            .status()
            .expect("curl")
            .success();
        assert!(ok, "info.zip download failed");
    }
    let bundle =
        read_info_bundle(&fs::read(&zip_path).unwrap(), ExtractLimits::INFO_BUNDLE).unwrap();
    assert!(bundle.len() > 100, "{} .info files", bundle.len());
    let infos: BTreeMap<CoreId, _> = bundle
        .into_iter()
        .filter_map(|(id, text)| parse_info(id.clone(), &text).ok().map(|i| (id, i)))
        .collect();
    let prefs = CorePreferences::builtin();
    let known = KnownFirmware::builtin();
    let sys_dir = tempfile::tempdir().unwrap();
    let mut checked = 0;
    for (system, cores) in prefs.systems() {
        for core in cores {
            let optional = prefs.optional_firmware(core, system);
            if optional.is_empty() {
                continue;
            }
            let info = infos
                .get(core)
                .unwrap_or_else(|| panic!("no .info for core {core}"));
            for o in optional {
                let listed = info.firmware.iter().any(|s| s.path == o.file);
                println!(
                    "{core} / {system}: {} listed_by_info={listed} outside_info={}",
                    o.file, o.outside_info
                );
                assert!(
                    listed || o.outside_info,
                    "{core}: {} not listed by the installed .info and not outside_info",
                    o.file
                );
                if o.outside_info {
                    assert!(
                        !listed,
                        "{core}: {} is now listed by the .info, outside_info could go",
                        o.file
                    );
                }
                checked += 1;
            }
            // Nothing in the system folder: the entries are proposed as Missing, never as present.
            let found =
                firmware_enhancements(&prefs, core, system, Some(info), sys_dir.path(), &known)
                    .unwrap();
            for o in optional {
                let e = found
                    .iter()
                    .find(|e| e.file == o.file && e.kind == EnhancementKind::Optional)
                    .unwrap_or_else(|| panic!("{core}: {} not proposed", o.file));
                assert_eq!(e.state, FirmwareState::Missing, "{core}: {}", o.file);
            }
            // Without the .info the inert entries disappear (names are never trusted alone).
            let blind =
                firmware_enhancements(&prefs, core, system, None, sys_dir.path(), &known).unwrap();
            for o in optional {
                let present = blind
                    .iter()
                    .any(|e| e.file == o.file && e.kind == EnhancementKind::Optional);
                assert_eq!(present, o.outside_info, "{core}: {} without .info", o.file);
            }
        }
    }
    assert!(checked >= 7, "{checked} optional files checked");
    // The three fallback tables keep working with the real .info (Amiga, PlayStation, 3DO).
    for (core, system) in [
        ("pcsx_rearmed", "Sony - PlayStation"),
        ("opera", "The 3DO Company - 3DO"),
        ("puae", "Commodore - Amiga"),
    ] {
        let id = CoreId::new(core).unwrap();
        let found =
            firmware_enhancements(&prefs, &id, system, infos.get(&id), sys_dir.path(), &known)
                .unwrap();
        assert!(
            found.iter().any(|e| e.kind == EnhancementKind::Fallback),
            "{core}: fallback entries expected, got {found:?}"
        );
    }
}