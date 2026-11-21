// The mock core only builds on Unix.
#![cfg(unix)]
//! 5.0 integration of the UI layer (design v5.0 §6, « UI ») on the mock core named like cap32: the per-game save
//! folder chosen by `resolve_game` from `cores.toml` (`save_scope = "game"`) reaches the core, and the fast-forward
//! the core requests (env 64) drives `SpeedState` the way `App` does, `GET_FASTFORWARDING` reporting it back. The
//! `MOCK_*` variables are given to a child process (the environment of this process is never changed).

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, NullInput, SaveScope, Speed,
};
use emu_ui::assets::cores::CorePreferences;
use emu_ui::{
    DataDirs, GameContent, SpeedState, content_spec, core_fast_forward_ratio, resolve_game,
};
use mock_core::Variant;

const CHILD: &str = "EMU_UI_TEST_EIGHTBIT_CHILD";

fn run_child(test: &str, root: &Path, vars: &[(&str, &str)]) {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", test, "--test-threads=1"])
        .env(CHILD, root);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    assert!(cmd.status().unwrap().success(), "child test failed");
}

/// The mock core under the file name of cap32 (`core_id_from_path` → `cap32`).
fn cap32_core(root: &Path) -> PathBuf {
    let core = root.join("cores/cap32_libretro.so");
    fs::create_dir_all(core.parent().unwrap()).unwrap();
    fs::copy(mock_core::build(Variant::Default), &core).unwrap();
    core
}

fn cell(emu: &Emulator, band: usize, cell: usize) -> u8 {
    let frame = emu.frame();
    let width = frame.width() as usize;
    frame.pixels()[((8 * band + 4) * width + 8 * cell + 4) * 4]
}

fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

#[test]
fn cap32_game_folder_and_requested_fast_forward() {
    let Some(root) = std::env::var_os(CHILD).map(PathBuf::from) else {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        // Built here: the child only copies it.
        mock_core::build(Variant::Default);
        run_child(
            "cap32_game_folder_and_requested_fast_forward",
            &root,
            &[
                ("MOCK_SAVEDIR_VISIBLE", "1"),
                ("MOCK_FASTFORWARD", "run,get"),
                ("MOCK_FF_RATIO", "0"),
            ],
        );
        return;
    };
    let dirs = DataDirs::under(&root.join("data"));
    dirs.create_all().unwrap();
    let core = cap32_core(&root);
    let game = root.join("roms/cpc/Sorcery.mock");
    fs::create_dir_all(game.parent().unwrap()).unwrap();
    fs::write(&game, b"disk").unwrap();

    let prefs = CorePreferences::builtin();
    let content = GameContent::Files(vec![game.clone()]);
    let spec = content_spec("cap32", &content, &prefs);
    assert_eq!(spec, ContentSpec::File(game.clone()), "cap32 is direct");
    let resolved = resolve_game("cap32", spec, &dirs);
    let game_dir = dirs.saves.join("cap32").join("Sorcery");
    assert_eq!(resolved.identity.core_save_dir, game_dir);

    let mut emu = Emulator::load_content_as(
        &core,
        resolved.spec,
        resolved.identity,
        EmulatorOptions {
            system_dir: dirs.system.clone(),
            save_dir: dirs.saves.clone(),
            state_dir: dirs.states.clone(),
            assets_dir: dirs.assets.clone(),
            core_variables: Vec::new(),
            cache_dir: dirs.cache_dir(),
            provide_vfs: false,
            restore_added_disks: true,
            core_save_scope: SaveScope::Game,
            fastforward_override: true,
            variable_state_size: false,
            port_devices: Vec::new(),
        },
    )
    .expect("load");
    // The App loop: on `fastforward_changed`, the core's request sets the speed, the effective state is reported.
    let mut speed = SpeedState::default();
    let mut changes = Vec::new();
    for frame in 0..14 {
        let status = emu
            .run_frame(&mut NullInput, &mut CollectSink::default())
            .unwrap();
        if status.fastforward_changed {
            speed.core_fast_forward_ratio = emu
                .fastforward_override()
                .filter(|o| o.fastforward)
                .map(|o| core_fast_forward_ratio(o.ratio));
            emu.set_fast_forwarding(speed.core_fast_forward());
            changes.push((frame, speed.speed(4.0)));
        }
        if frame == 7 {
            assert!(speed.audio_paused(), "muted while loading fast");
        }
    }
    assert_eq!(
        changes,
        vec![(5, Speed::FastForward { ratio: 2.0 }), (10, Speed::Normal)],
        "ratio 0 = design default 2.0"
    );
    let dir = game_dir.to_str().unwrap();
    let got = u32::from_le_bytes([0, 1, 2, 3].map(|i| cell(&emu, 5, i)));
    assert_eq!(got, fnv1a(dir), "GET_SAVE_DIRECTORY = the game's folder");
    let report = fs::read_to_string(game_dir.join("mock-ff.txt")).unwrap();
    assert!(
        report.contains("run_on=1") && report.contains("run_off=1"),
        "{report}"
    );
    assert!(report.contains("get=0"), "back to normal: {report}");
    drop(emu);
    assert_eq!(fs::read(&game).unwrap(), b"disk", "original untouched");
}
