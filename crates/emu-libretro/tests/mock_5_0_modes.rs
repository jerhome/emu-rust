// The mock core only builds on Unix.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024).
#![allow(unsafe_code)]
//! 5.0 behaviours of the mock core: a variable-size state WITHOUT the `CORE_VARIABLE_SIZE` quirk (VICE-like),
//! which only `CoreConfig::variable_state_size` makes usable. `MOCK_*` variables are process-global, so a
//! single test runs the scenarios in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::path::{Path, PathBuf};

use emu_libretro::{
    Core, CoreConfig, FrontendHandler, InputQuery, SerializationQuirks, VideoFrame,
};

struct Silent;

impl FrontendHandler for Silent {
    fn video_refresh(&mut self, _frame: Option<VideoFrame<'_>>) {}
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, _query: InputQuery) -> i16 {
        0
    }
}

fn set_env(vars: &[(&str, &str)]) {
    for name in ["MOCK_STATE_SIZE", "MOCK_STATE_SIZE_NOQUIRK"] {
        // SAFETY: the only test of this binary; no other thread touches the environment.
        unsafe { std::env::remove_var(name) };
    }
    for (name, value) in vars {
        // SAFETY: as above.
        unsafe { std::env::set_var(name, value) };
    }
}

fn out_dir(name: &str) -> PathBuf {
    let dir = mock_core::output_dir().join("emu-libretro-5-0").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("saves")).unwrap();
    std::fs::create_dir_all(dir.join("system")).unwrap();
    std::fs::write(dir.join("game.mock"), b"mock content").unwrap();
    dir
}

fn config(dir: &Path, variable_state_size: bool) -> CoreConfig {
    CoreConfig {
        system_dir: dir.join("system"),
        save_dir: dir.join("saves"),
        assets_dir: dir.join("assets"),
        variables: Vec::new(),
        provide_vfs: false,
        copy_dir: None,
        fastforward_override: false,
        variable_state_size,
    }
}

/// Saves a 100-byte state, grows the core's state to 200 bytes (frame 20), then tries to load the small one.
fn small_state_after_growth(lib: &Path, dir: &Path, variable_state_size: bool) -> bool {
    let mut core = Core::load(lib, config(dir, variable_state_size)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert_eq!(
        core.serialization_quirks(),
        SerializationQuirks::default(),
        "the quirk is not announced"
    );
    assert_eq!(core.serialize_size(), 100);
    let mut small = Vec::new();
    core.serialize(&mut small).unwrap();
    assert_eq!(small.len(), 100);
    for _ in 0..21 {
        core.run(&mut Silent).unwrap();
    }
    assert_eq!(core.serialize_size(), 200);
    let mut big = Vec::new();
    core.serialize(&mut big).unwrap();
    assert_eq!(big.len(), 200);
    let ok = core.unserialize(&small).is_ok();
    // The big state is always accepted.
    core.unserialize(&big).unwrap();
    ok
}

#[cfg(unix)]
#[test]
fn variable_state_size_without_quirk() {
    let lib = mock_core::build(mock_core::Variant::Default);
    set_env(&[
        ("MOCK_STATE_SIZE", "100,200"),
        ("MOCK_STATE_SIZE_NOQUIRK", "1"),
    ]);
    let dir = out_dir("noquirk");
    assert!(
        !small_state_after_growth(&lib, &dir, false),
        "fixed-size policy rejects a state smaller than serialize_size"
    );
    assert!(
        small_state_after_growth(&lib, &dir, true),
        "variable_state_size accepts it"
    );
    // With the quirk announced (no NOQUIRK), the flag is not needed.
    set_env(&[("MOCK_STATE_SIZE", "100,200")]);
    let mut core = Core::load(&lib, config(&dir, false)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert!(
        core.serialization_quirks()
            .contains(SerializationQuirks::CORE_VARIABLE_SIZE)
    );
    set_env(&[]);
}
