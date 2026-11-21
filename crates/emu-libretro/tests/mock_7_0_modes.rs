// The mock core only builds on Unix.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024).
#![allow(unsafe_code)]
//! 7.0 behaviours of the mock core: `MOCK_CONTROLLERS` (SET_CONTROLLER_INFO, 35) and the log of
//! `retro_set_controller_port_device`. `MOCK_*` variables are process-global: a single test runs the scenarios.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::path::{Path, PathBuf};

use emu_libretro::{Core, CoreConfig, FrontendHandler, InputQuery, VideoFrame};

struct Silent;

impl FrontendHandler for Silent {
    fn video_refresh(&mut self, _frame: Option<VideoFrame<'_>>) {}
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, _query: InputQuery) -> i16 {
        0
    }
}

fn out_dir(name: &str) -> PathBuf {
    let dir = mock_core::output_dir().join("emu-libretro-7-0").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("saves")).unwrap();
    std::fs::create_dir_all(dir.join("system")).unwrap();
    std::fs::write(dir.join("game.mock"), b"mock content").unwrap();
    dir
}

fn config(dir: &Path) -> CoreConfig {
    CoreConfig {
        system_dir: dir.join("system"),
        save_dir: dir.join("saves"),
        assets_dir: dir.join("assets"),
        variables: Vec::new(),
        provide_vfs: false,
        copy_dir: None,
        fastforward_override: false,
        variable_state_size: false,
    }
}

fn log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("saves/mock-controllers.txt")).unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn controller_info_and_device_log() {
    let lib = mock_core::build(mock_core::Variant::Default);

    // Default: nothing declared, nothing logged.
    // SAFETY: the only test of this binary; no other thread touches the environment.
    unsafe { std::env::remove_var("MOCK_CONTROLLERS") };
    let dir = out_dir("default");
    let mut core = Core::load(&lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert!(core.controller_types(0).is_empty());
    assert_eq!(log(&dir), "");
    drop(core);

    // Declared 1 / 517 (PS1 style): visible on ports 0 and 1, none on port 2; the frontend puts JOYPAD on 0 and 1
    // after load, then switches port 0 to the DualShock id.
    // SAFETY: as above.
    unsafe { std::env::set_var("MOCK_CONTROLLERS", "1,517") };
    let dir = out_dir("ps1");
    let mut core = Core::load(&lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    for port in 0..2 {
        let ids: Vec<u32> = core.controller_types(port).iter().map(|t| t.id).collect();
        assert_eq!(ids, [1, 517], "port {port}");
    }
    assert!(core.controller_types(2).is_empty());
    core.set_controller_device(0, 517).unwrap();
    core.run(&mut Silent).unwrap();
    assert!(
        core.set_controller_device(0, 261).is_err(),
        "261 not declared"
    );
    let text = log(&dir);
    assert!(text.contains("port=0 device=1\n"), "{text}");
    assert!(text.ends_with("port=0 device=517\n"), "{text}");
    drop(core);

    // SAFETY: as above.
    unsafe { std::env::set_var("MOCK_CONTROLLERS", "1,261") };
    let dir = out_dir("swan");
    let mut core = Core::load(&lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert_eq!(core.controller_types(1)[1].id, 261);
    // SAFETY: as above.
    unsafe { std::env::remove_var("MOCK_CONTROLLERS") };
}
