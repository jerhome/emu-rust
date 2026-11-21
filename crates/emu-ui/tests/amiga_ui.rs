// The mock core only builds on Unix.
#![cfg(unix)]
//! 3.0 integration of the UI layer (design v3.0 §2, §6) without any Kickstart: the launch path of `App::build`
//! (`content_spec` → `resolve_game` → firmware check → `Emulator::load_content_as`) on the mock core named like
//! puae, the BIOS import on generated files, and the ROM folder button in a container (child process, so the
//! environment is never changed in this process).

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{ContentSpec, Emulator, EmulatorOptions, NullSink, ScriptedInput};
use emu_ui::assets::cores::CorePreferences;
use emu_ui::{
    BiosIdentified, Config, DataDirs, FolderOutcome, GameContent, ROM_KEY_NAME, content_spec,
    firmware_catalogue, identify_bios, import_bios, prepare_launch_firmware, resolve_game,
    unverified_item,
};
use mock_core::Variant;

/// One libretro core per process (ADR 0008).
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    dirs: DataDirs,
    /// The mock core under the file name of puae (`core_id_from_path` → `puae`).
    core: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let dirs = DataDirs::under(&root.join("data"));
        dirs.create_all().unwrap();
        let core = root.join("cores").join(format!(
            "puae_libretro.{}",
            if cfg!(target_os = "macos") {
                "dylib"
            } else {
                "so"
            }
        ));
        fs::create_dir_all(core.parent().unwrap()).unwrap();
        fs::copy(mock_core::build(Variant::Default), &core).unwrap();
        Self {
            _tmp: tmp,
            root,
            dirs,
            core,
        }
    }

    fn game(&self) -> PathBuf {
        // `.mock` is an extension of the mock core; the name has no model tag: A500, `kick34005.A500`.
        let path = self.root.join("roms/amiga/Point and Click.mock");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![0x42u8; 4096]).unwrap();
        path
    }

    fn options(&self, core_variables: Vec<(String, String)>) -> EmulatorOptions {
        EmulatorOptions {
            system_dir: self.dirs.system.clone(),
            save_dir: self.dirs.saves.clone(),
            state_dir: self.dirs.states.clone(),
            assets_dir: self.dirs.assets.clone(),
            core_variables,
            restore_added_disks: true,
            cache_dir: self.dirs.root.join("cache"),
            provide_vfs: false,
        }
    }
}

fn aros() -> (String, String) {
    ("puae_kickstart".to_owned(), "aros".to_owned())
}

/// Empty system folder: the game starts with AROS (computed default, never written), the warning is given on
/// the first launch only and remembered in the game's options; a single disk goes through a working copy.
#[test]
fn missing_kickstart_falls_back_to_aros_and_warns_once() {
    let _guard = lock();
    let env = Env::new();
    let game = env.game();
    let config = Config::default();
    let content = GameContent::Files(vec![game.clone()]);
    for launch in 0..2 {
        let spec = content_spec("puae", &content, &CorePreferences::builtin());
        assert!(
            matches!(&spec, ContentSpec::Media { .. }),
            "a single puae disk is never ContentSpec::File: {spec:?}"
        );
        let resolved = resolve_game("puae", spec, &env.dirs);
        let setup =
            prepare_launch_firmware("puae", &content, &env.dirs, &config, &resolved.options_path);
        assert_eq!(setup.computed, vec![aros()], "launch {launch}");
        if launch == 0 {
            let notice = setup.notice.expect("warning on the first launch");
            assert!(notice.contains("kick34005.A500"), "{notice}");
            assert!(setup.acknowledged_now);
            let saved = fs::read_to_string(&resolved.options_path).unwrap();
            assert!(saved.contains("aros_acknowledged = true"), "{saved}");
        } else {
            assert_eq!(setup.notice, None, "warned once per game");
            assert!(!setup.acknowledged_now);
        }
        let mut emu = Emulator::load_content_as(
            &env.core,
            resolved.spec,
            resolved.identity,
            env.options(setup.computed),
        )
        .expect("load");
        assert!(emu.work_dir().is_some(), "working copy");
        let mut input = ScriptedInput::new();
        let mut sink = NullSink;
        for _ in 0..5 {
            emu.run_frame(&mut input, &mut sink).unwrap();
        }
    }
    // The original disk is never written.
    assert_eq!(fs::read(&game).unwrap(), vec![0x42u8; 4096]);
}

/// « Importer un BIOS… » on generated files: an unknown file is refused unless imported unverified under an
/// expected name; the import is then seen by the launch check (wrong hash: no more AROS fallback, a warning).
#[test]
fn bios_import_end_to_end_on_fake_files() {
    let _guard = lock();
    let env = Env::new();
    let fake = env.root.join("downloads/kick13.rom");
    fs::create_dir_all(fake.parent().unwrap()).unwrap();
    fs::write(&fake, b"generated, not a Kickstart").unwrap();
    let catalogue = firmware_catalogue(None);
    let identified = identify_bios(&env.dirs.system, &fake, &catalogue).unwrap();
    assert!(matches!(
        identified,
        BiosIdentified::Unknown { rom_key: false, .. }
    ));
    assert!(unverified_item(&identified, "free-name.bin").is_none());
    let item = unverified_item(&identified, "kick34005.A500").expect("expected name");
    let report = import_bios(&env.dirs.system, &fake, &[item], None);
    assert_eq!((report.installed, report.failed), (1, 0), "{report:?}");
    assert_eq!(
        fs::read(env.dirs.system.join("kick34005.A500")).unwrap(),
        b"generated, not a Kickstart"
    );
    // Importing again under the same name is refused (another content would need « Remplacer »): same file.
    let again = unverified_item(&identified, "kick34005.A500").unwrap();
    let report = import_bios(&env.dirs.system, &fake, &[again], None);
    assert_eq!(report.failed, 0, "same content: nothing to do ({report:?})");

    let game = env.game();
    let content = GameContent::Files(vec![game]);
    let spec = content_spec("puae", &content, &CorePreferences::builtin());
    let resolved = resolve_game("puae", spec, &env.dirs);
    let setup = prepare_launch_firmware(
        "puae",
        &content,
        &env.dirs,
        &Config::default(),
        &resolved.options_path,
    );
    assert!(setup.computed.is_empty(), "a file is there: no AROS");
    assert!(setup.notice.unwrap().contains("kick34005.A500"));

    // rom.key of Amiga Forever: only under its fixed name.
    let key = env.root.join("downloads/rom.key");
    fs::write(&key, b"key").unwrap();
    let identified = identify_bios(&env.dirs.system, &key, &catalogue).unwrap();
    assert_eq!(identified.unverified_names(), [ROM_KEY_NAME.to_owned()]);
    let report = import_bios(
        &env.dirs.system,
        &key,
        &[unverified_item(&identified, ROM_KEY_NAME).unwrap()],
        None,
    );
    assert_eq!(report.installed, 1);
    assert!(env.dirs.system.join("rom.key").is_file());
}

const CHILD: &str = "EMU_UI_TEST_FOLDER_CHILD";

fn run_child(test: &str, hint: &str) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1"])
        .env(CHILD, "1")
        .env("EMU_RUST_CONTAINER", "1")
        .env("EMU_HOST_ROMS_DIR", hint)
        .status()
        .unwrap();
    assert!(status.success(), "child test failed");
}

/// Docker (`EMU_RUST_CONTAINER=1`): the folder is shown with the host path, `opener` is never called.
#[test]
fn rom_folder_in_a_container_is_shown_with_the_host_path() {
    if std::env::var_os(CHILD).is_some() {
        assert_eq!(
            emu_ui::open_folder(Path::new("/data/roms")),
            FolderOutcome::ShowPath {
                path: PathBuf::from("/data/roms"),
                host_hint: Some("C:\\Users\\moi\\roms".to_owned()),
            }
        );
        return;
    }
    run_child(
        "rom_folder_in_a_container_is_shown_with_the_host_path",
        " C:\\Users\\moi\\roms ",
    );
}

/// An empty `EMU_HOST_ROMS_DIR` means « not mounted ».
#[test]
fn rom_folder_in_a_container_without_host_path() {
    if std::env::var_os(CHILD).is_some() {
        assert_eq!(
            emu_ui::open_folder(Path::new("/data/roms")),
            FolderOutcome::ShowPath {
                path: PathBuf::from("/data/roms"),
                host_hint: None,
            }
        );
        return;
    }
    run_child("rom_folder_in_a_container_without_host_path", "");
}
