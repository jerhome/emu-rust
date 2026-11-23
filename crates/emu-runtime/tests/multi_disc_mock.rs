#![cfg(unix)]
// `std::env::set_var` is `unsafe` (edition 2024): the mock core reads its protocol with getenv().
#![allow(unsafe_code)]
//! `MultiDisc::DiskControl` (7.0.1, ADR 0020 amendment) on the mock core: the first image goes to
//! `retro_load_game`, the others are appended through the disk control interface, no `.m3u` is written.
//! `MOCK_*` variables are process-global: a single test runs the scenarios in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};

use emu_runtime::{
    ContentSpec, Emulator, EmulatorOptions, MediaPolicy, MediaRules, MediaSet, MultiDisc,
    NullInput, NullSink, RuntimeError,
};

fn set_disk(mode: Option<&str>) {
    for name in [
        "MOCK_DISK",
        "MOCK_DISK_DUMP",
        "MOCK_DISK_REPLACE_CLOSED",
        "MOCK_NO_GAME",
    ] {
        // SAFETY: the only test of this binary; no other thread reads or writes the environment meanwhile.
        unsafe { std::env::remove_var(name) };
    }
    if let Some(mode) = mode {
        // SAFETY: as above.
        unsafe {
            std::env::set_var("MOCK_DISK", mode);
            // Like genesis_plus_gx / picodrive: images can be replaced with the tray closed.
            std::env::set_var("MOCK_DISK_REPLACE_CLOSED", "1");
        }
    }
}

fn options(data: &Path) -> EmulatorOptions {
    EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: true,
        core_save_scope: emu_runtime::SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices: Vec::new(),
        cache_dir: data.join("cache"),
        provide_vfs: false,
    }
}

fn spec(images: Vec<PathBuf>) -> ContentSpec {
    ContentSpec::Media {
        set: MediaSet {
            name: "Game (Europe)".into(),
            images,
            playlist: None,
            launch: None,
        },
        rules: MediaRules {
            multi_disc: MultiDisc::DiskControl,
            media_direct: vec!["cue".into()],
            ..MediaRules::new(MediaPolicy::Direct)
        },
    }
}

#[test]
fn disk_control_multi_disc_on_mock_core() {
    let lib = mock_core::build(mock_core::Variant::NeedFullpath);
    let root = mock_core::output_dir().join("emu-runtime-multi-disc");
    let _ = fs::remove_dir_all(&root);
    let discs = root.join("Game (Europe) (Disc 1)");
    fs::create_dir_all(&discs).unwrap();
    let d1 = discs.join("Game (Disc 1).cue");
    let d2 = discs.join("Game (Disc 2).cue");
    fs::write(&d1, b"disc one").unwrap();
    fs::write(&d2, b"disc two").unwrap();
    let (d1, d2) = (fs::canonicalize(d1).unwrap(), fs::canonicalize(d2).unwrap());

    for (mode, name) in [("ext", "ext"), ("basic", "basic")] {
        set_disk(Some(mode));
        let data = root.join(format!("data-{name}"));
        let mut emu =
            Emulator::load_content(&lib, spec(vec![d1.clone(), d2.clone()]), options(&data))
                .unwrap_or_else(|e| panic!("{mode}: {e}"));
        emu.run_frame(&mut NullInput, &mut NullSink).unwrap();
        // No generated playlist: the core was given the first disc itself.
        let prepared = emu.prepared_media().unwrap();
        assert_eq!(prepared.images, [d1.clone(), d2.clone()], "{mode}");
        assert!(
            !prepared.content.extension().is_some_and(|e| e == "m3u"),
            "{mode}: {:?}",
            prepared.content
        );
        let mut disks = emu.disks().unwrap();
        let slots = disks.slots();
        assert_eq!(slots.len(), 2, "{mode}");
        assert_eq!(slots[0].path.as_deref(), Some(d1.as_path()), "{mode}");
        // Basic interface has no paths: the prepared list gives them (same order).
        assert_eq!(slots[1].path.as_deref(), Some(d2.as_path()), "{mode}");
        assert_eq!(disks.current(), Some(0), "{mode}");
        disks.insert(1).unwrap_or_else(|e| panic!("{mode}: {e}"));
        assert_eq!(disks.current(), Some(1), "{mode}");
    }

    // No disk control interface at all: the extra images cannot be added.
    set_disk(None);
    let data = root.join("data-none");
    let result = Emulator::load_content(&lib, spec(vec![d1.clone(), d2.clone()]), options(&data));
    match result {
        Err(RuntimeError::NoDiskControl) => {}
        Err(e) => panic!("expected NoDiskControl, got {e}"),
        Ok(_) => panic!("expected NoDiskControl, got a running game"),
    }
}
