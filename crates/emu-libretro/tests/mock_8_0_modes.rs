// The mock core only builds on Unix.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024).
#![allow(unsafe_code)]
//! 8.0 behaviours of the mock core (header of `tests/mock-core/mock_core.c`, "v8.0 protocol"): `SET_HW_RENDER`
//! recorded then refused, the hardware sentinel, runs without a frame, bounded and cleaned input descriptors.
//! `MOCK_*` variables are process-global: a single test runs the scenarios in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::path::{Path, PathBuf};

use emu_libretro::{
    Core, CoreConfig, FrameKind, FrontendHandler, HwContextKind, HwRenderRequest, InputQuery,
    LibretroError, MAX_INPUT_DESCRIPTORS, VideoFrame,
};

const VARS: [&str; 5] = [
    "MOCK_HW_RENDER",
    "MOCK_HW_REQUIRED",
    "MOCK_HW_FRAME",
    "MOCK_NO_FRAME",
    "MOCK_DESCRIPTORS",
];

/// Sets exactly `vars` among the variables of this file.
fn set_protocol(vars: &[(&str, &str)]) {
    for name in VARS {
        // SAFETY: the only test of this binary; no other thread reads or writes the environment meanwhile.
        unsafe { std::env::remove_var(name) };
    }
    for (name, value) in vars {
        // SAFETY: as above.
        unsafe { std::env::set_var(name, value) };
    }
}

#[derive(Default)]
struct Frames {
    software: usize,
}

impl FrontendHandler for Frames {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        if frame.is_some() {
            self.software += 1;
        }
    }
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, _query: InputQuery) -> i16 {
        0
    }
}

fn out_dir(name: &str) -> PathBuf {
    let dir = mock_core::output_dir().join("emu-libretro-8-0").join(name);
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

/// Loads the mock core with `vars` and its game.
fn start(lib: &Path, name: &str, vars: &[(&str, &str)]) -> (Core, Result<(), LibretroError>) {
    set_protocol(vars);
    let dir = out_dir(name);
    let mut core = Core::load(lib, config(&dir)).unwrap();
    let loaded = core.load_game(&dir.join("game.mock")).map(|_| ());
    (core, loaded)
}

#[cfg(unix)]
#[test]
fn hw_render_frames_and_descriptors() {
    let lib = mock_core::build(mock_core::Variant::Default);

    // Default: no request, software frames, no descriptors.
    let (mut core, loaded) = start(&lib, "default", &[]);
    loaded.unwrap();
    assert_eq!(core.hw_render_request(), None);
    assert!(core.input_descriptors().is_empty());
    assert_eq!(core.take_frame_kind(), FrameKind::Missing, "no run yet");
    let mut frames = Frames::default();
    core.run(&mut frames).unwrap();
    assert_eq!(frames.software, 1);
    assert_eq!(core.take_frame_kind(), FrameKind::Software);
    assert_eq!(core.take_frame_kind(), FrameKind::Missing, "flag reset");
    drop(core);

    // Request recorded then refused: the core falls back to software and keeps running.
    let (mut core, loaded) = start(&lib, "fallback", &[("MOCK_HW_RENDER", "3,3,3")]);
    loaded.expect("a refusal alone is not fatal for the mock core");
    assert_eq!(
        core.hw_render_request(),
        Some(&HwRenderRequest::new(HwContextKind::OpenGlCore, 3, 3))
    );
    core.run(&mut Frames::default()).unwrap();
    assert_eq!(core.take_frame_kind(), FrameKind::Software);
    drop(core);

    // Refused and required: the load fails, the request is still visible.
    let (core, loaded) = start(
        &lib,
        "required",
        &[("MOCK_HW_RENDER", "6,1,2"), ("MOCK_HW_REQUIRED", "1")],
    );
    assert!(
        matches!(loaded, Err(LibretroError::LoadGameFailed { .. })),
        "{loaded:?}"
    );
    assert_eq!(
        core.hw_render_request(),
        Some(&HwRenderRequest::new(HwContextKind::Vulkan, 1, 2))
    );
    drop(core);

    // Unknown context value kept as is.
    let (core, loaded) = start(&lib, "unknown", &[("MOCK_HW_RENDER", "42,0,0")]);
    loaded.unwrap();
    assert_eq!(
        core.hw_render_request().map(|r| r.context),
        Some(HwContextKind::Unknown(42))
    );
    drop(core);

    // Sentinel: nothing reaches the handler.
    let (mut core, loaded) = start(&lib, "sentinel", &[("MOCK_HW_FRAME", "1")]);
    loaded.unwrap();
    let mut frames = Frames::default();
    core.run(&mut frames).unwrap();
    assert_eq!(frames.software, 0);
    assert_eq!(core.take_frame_kind(), FrameKind::HwSentinel);
    drop(core);

    // No frame at all.
    let (mut core, loaded) = start(&lib, "no-frame", &[("MOCK_NO_FRAME", "1")]);
    loaded.unwrap();
    core.run(&mut Frames::default()).unwrap();
    assert_eq!(core.take_frame_kind(), FrameKind::Missing);
    drop(core);

    // 300 descriptors: bounded, cleaned, empty text skipped.
    let (core, loaded) = start(&lib, "descriptors", &[("MOCK_DESCRIPTORS", "1")]);
    loaded.unwrap();
    let descs = core.input_descriptors();
    assert_eq!(descs.len(), MAX_INPUT_DESCRIPTORS);
    assert_eq!(descs[0].description, "Fire");
    assert_eq!(descs[1].description, "Jump[31mend");
    assert_eq!(descs[2].description, "A".repeat(64));
    assert_eq!(
        (descs[3].port, descs[3].id, descs[3].description.as_str()),
        (0, 4, "Pad button 4"),
        "the empty entry [3] is skipped"
    );
    let last = descs.last().unwrap();
    assert_eq!((last.port, last.device, last.index, last.id), (0, 1, 0, 0));
    assert_eq!(last.description, "Pad button 256");
    assert!(descs.iter().all(|d| {
        d.description.chars().count() <= 64
            && !d
                .description
                .chars()
                .any(|c| c.is_control() || c == '\u{202E}')
    }));
    drop(core);

    // Both are per game: cleared by unload_game.
    let (mut core, loaded) = start(
        &lib,
        "unload",
        &[("MOCK_DESCRIPTORS", "1"), ("MOCK_HW_RENDER", "3,3,3")],
    );
    loaded.unwrap();
    assert!(core.hw_render_request().is_some());
    assert!(!core.input_descriptors().is_empty());
    core.unload_game();
    assert_eq!(core.hw_render_request(), None);
    assert!(core.input_descriptors().is_empty());
    drop(core);

    set_protocol(&[]);
}
