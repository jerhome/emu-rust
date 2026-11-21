// The mock core only builds on Unix.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
//! `EmulatorOptions::port_devices` (design v7.0 §3.2) against the mock core's `MOCK_CONTROLLERS`.
#![allow(unsafe_code)]
#![cfg(unix)]

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::Path;

use emu_runtime::{Emulator, EmulatorOptions};
use mock_core::Variant;

fn options(root: &Path, port_devices: Vec<(u32, u32)>) -> EmulatorOptions {
    let data = root.join("data");
    EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        cache_dir: data.join("cache"),
        provide_vfs: false,
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: false,
        core_save_scope: emu_runtime::SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices,
    }
}

/// Journal of `retro_set_controller_port_device` calls, wherever the core's save folder is.
fn journal(root: &Path) -> String {
    fn walk(dir: &Path, out: &mut String) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .file_name()
                .is_some_and(|n| n == "mock-controllers.txt")
            {
                out.push_str(&fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut text = String::new();
    walk(&root.join("data/saves"), &mut text);
    text
}

#[test]
fn port_devices_are_applied_after_load_and_refusals_keep_the_joypad() {
    // SAFETY: the only test of this binary; no other thread touches the environment.
    unsafe { std::env::set_var("MOCK_CONTROLLERS", "1,517") };
    let lib = mock_core::build(Variant::Default);

    let tmp = tempfile::tempdir().unwrap();
    let rom = tmp.path().join("game.bin");
    fs::write(&rom, b"mock content").unwrap();
    let mut emu =
        Emulator::load(&lib, &rom, options(tmp.path(), vec![(0, 517), (1, 999)])).unwrap();
    let text = journal(tmp.path());
    assert!(text.contains("port=0 device=1\n"), "{text}");
    assert!(
        text.contains("port=0 device=517\n"),
        "applied after the joypad default: {text}"
    );
    assert!(!text.contains("device=999"), "{text}");
    assert!(
        text.find("port=0 device=1\n") < text.find("port=0 device=517\n"),
        "{text}"
    );
    assert_eq!(emu.controller_types(0).len(), 2);
    assert!(emu.set_port_device(0, 1).is_ok());
    assert!(emu.set_port_device(0, 999).is_err());
    drop(emu);

    // No request: only the JOYPAD default.
    let tmp = tempfile::tempdir().unwrap();
    let rom = tmp.path().join("game.bin");
    fs::write(&rom, b"mock content").unwrap();
    let emu = Emulator::load(&lib, &rom, options(tmp.path(), Vec::new())).unwrap();
    assert!(!journal(tmp.path()).contains("device=517"));
    drop(emu);
    // SAFETY: as above.
    unsafe { std::env::remove_var("MOCK_CONTROLLERS") };
}
