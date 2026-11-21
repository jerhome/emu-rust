// The mock core only builds on Unix.
#![cfg(unix)]
//! 6.0 (design §2.1, §5 « UI »): when `retro_load_game` fails, the error the core logged reaches the UI
//! (`UiError::core_error`) and names the same BIOS as the check made before the launch. Mock core named like
//! genesis_plus_gx, `MOCK_LOAD_ERROR` given to a child process (the environment of this process is never changed).

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use emu_runtime::{ContentSpec, Emulator, EmulatorOptions, SaveScope};
use emu_ui::assets::cores::CorePreferences;
use emu_ui::{Config, DataDirs, GameContent, UiError, content_spec, resolve_game};
use mock_core::Variant;

const CHILD: &str = "EMU_UI_TEST_CORE_ERROR_CHILD";

fn run_child(test: &str, root: &Path, vars: &[(&str, &str)]) {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", test, "--test-threads=1"])
        .env(CHILD, root);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    assert!(cmd.status().unwrap().success(), "child test failed");
}

fn options(dirs: &DataDirs) -> EmulatorOptions {
    EmulatorOptions {
        system_dir: dirs.system.clone(),
        save_dir: dirs.saves.clone(),
        state_dir: dirs.states.clone(),
        assets_dir: dirs.assets.clone(),
        core_variables: Vec::new(),
        cache_dir: dirs.cache_dir(),
        provide_vfs: false,
        restore_added_disks: true,
        core_save_scope: SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
    }
}

#[test]
fn refused_disc_shows_the_core_error_naming_the_checked_bios() {
    let Some(root) = std::env::var_os(CHILD).map(PathBuf::from) else {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        mock_core::build(Variant::Default);
        let message = format!(
            "Unable to open CD BIOS: {}",
            root.join("data/system/bios_CD_E.bin").display()
        );
        run_child(
            "refused_disc_shows_the_core_error_naming_the_checked_bios",
            &root,
            &[("MOCK_LOAD_ERROR", &message)],
        );
        return;
    };
    let dirs = DataDirs::under(&root.join("data"));
    dirs.create_all().unwrap();
    let core = root.join("cores/genesis_plus_gx_libretro.so");
    fs::create_dir_all(core.parent().unwrap()).unwrap();
    fs::copy(mock_core::build(Variant::Default), &core).unwrap();
    let disc = root.join("roms/megacd/Dune (Europe).cue");
    fs::create_dir_all(disc.parent().unwrap()).unwrap();
    fs::write(
        &disc,
        b"FILE \"Dune (Europe).iso\" BINARY\n  TRACK 01 MODE1/2048\n",
    )
    .unwrap();
    let content = GameContent::Files(vec![disc.clone()]);

    // The check before the launch: the BIOS of the region of the name tag, missing.
    let config = Config::default();
    let setup = emu_ui::prepare_launch_firmware(
        "genesis_plus_gx",
        &content,
        &dirs,
        &config,
        &root.join("options.toml"),
    );
    let notice = setup.notice.expect("BIOS missing notice");
    assert!(notice.contains("bios_CD_E.bin"), "{notice}");

    let prefs = CorePreferences::builtin();
    let spec = content_spec("genesis_plus_gx", &content, &prefs);
    assert_eq!(
        spec,
        ContentSpec::File(disc.clone()),
        "a CD is handed over in place"
    );
    let resolved = resolve_game("genesis_plus_gx", spec, &dirs);
    let err =
        match Emulator::load_content_as(&core, resolved.spec, resolved.identity, options(&dirs)) {
            Ok(_) => panic!("the mock core refuses the game"),
            Err(e) => UiError::from(e),
        };
    let core_error = err.core_error().expect("core error captured").to_owned();
    assert!(
        core_error.contains("bios_CD_E.bin"),
        "same file as the check: {core_error}"
    );
    assert!(
        core_error.starts_with("Unable to open CD BIOS"),
        "{core_error}"
    );
    // A refusal without any ERROR line keeps the generic message.
    let other = UiError::Content("x".into());
    assert_eq!(other.core_error(), None);
}
