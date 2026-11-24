// The mock core only builds on Unix.
#![cfg(unix)]
//! A scripted pad drives the mock core (ADR 0016, task T6): the image fingerprints must equal those of the
//! `ScriptedInput` holding the same RetroPad buttons, alone or ORed with the keyboard. Gilrs is not involved.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, Emulator, EmulatorOptions, InputBackend, InputFrame, JoypadButton as J, NullInput,
    ScriptedInput,
};
use emu_ui::{
    GamepadInput, GamepadSettings, KeyMap, KeyboardInput, PadButton as P, PadEvent, PadEventKind,
    PadId, PadMap, PadStyle, ScriptedPadSource,
};
use mock_core::Variant;

/// One libretro core per process (ADR 0008).
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const FRAMES: usize = 10;
const ID: PadId = PadId(1);

fn button(button: P, pressed: bool) -> PadEvent {
    PadEvent {
        id: ID,
        kind: PadEventKind::Button { button, pressed },
    }
}

fn connect() -> PadEvent {
    PadEvent {
        id: ID,
        kind: PadEventKind::Connected {
            name: "Wireless Controller".into(),
            guid: "030000004c050000c405000000000000".into(),
            raw: false,
            guessed: false,
        },
    }
}

/// One batch per frame; frame `n` sees the events of batch `n` (the pad is polled once per frame).
fn pad(style: PadStyle, batches: Vec<Vec<PadEvent>>) -> GamepadInput<ScriptedPadSource> {
    let mut source = ScriptedPadSource::new();
    for batch in batches {
        source.push_batch(batch);
    }
    GamepadInput::new(
        source,
        PadMap::default_for(style),
        GamepadSettings::default(),
    )
}

fn run(input: &mut dyn InputBackend) -> Vec<u64> {
    let dir = tempfile::tempdir().unwrap();
    let rom = dir.path().join("Game (World).bin");
    fs::write(&rom, b"mock content: any bytes seed the image").unwrap();
    let data = dir.path().join("data");
    let options = EmulatorOptions {
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
        port_devices: Vec::new(),
    };
    let mut emu = Emulator::load(&mock_core::build(Variant::Default), &rom, options).unwrap();
    let mut audio = CollectSink::default();
    (0..FRAMES)
        .map(|_| {
            emu.run_frame(input, &mut audio).unwrap();
            emu.frame().fingerprint()
        })
        .collect()
}

struct Both<'a>(&'a mut dyn InputBackend, &'a mut dyn InputBackend);

impl InputBackend for Both<'_> {
    fn poll(&mut self, frame_index: u64, state: &mut InputFrame) {
        self.0.poll(frame_index, state);
        self.1.poll(frame_index, state);
    }
}

#[test]
fn scripted_pad_gives_the_same_images_as_scripted_input() {
    let _guard = lock();
    let baseline = run(&mut NullInput);
    // Cross held during frames 3..6 (down event in batch 3, up in batch 6); Snes: Cross = RetroPad B.
    let mut batches = vec![Vec::new(); FRAMES];
    batches[0].push(connect());
    batches[3].push(button(P::South, true));
    batches[6].push(button(P::South, false));
    let from_pad = run(&mut pad(PadStyle::Snes, batches));
    let from_script = run(&mut ScriptedInput::new().press(0, J::B, 3..6));
    assert_eq!(from_pad, from_script);
    assert_ne!(from_pad, baseline, "the core saw the press");
    assert_eq!(from_pad[..3], baseline[..3]);
    assert_eq!(from_pad[6..], from_script[6..]);
}

#[test]
fn mapping_changes_what_the_core_sees() {
    let _guard = lock();
    // Circle is RetroPad A on every system; Square is Y on Snes but nothing on NES.
    let mut batches = vec![Vec::new(); FRAMES];
    batches[0].push(connect());
    batches[2].push(button(P::West, true));
    let snes = run(&mut pad(PadStyle::Snes, batches.clone()));
    let nes = run(&mut pad(PadStyle::Nes, batches));
    assert_eq!(snes, run(&mut ScriptedInput::new().press(0, J::Y, 2..10)));
    assert_eq!(nes, run(&mut NullInput));
}

#[test]
fn keyboard_and_pad_together_equal_both_scripted() {
    let _guard = lock();
    let keymap = KeyMap::default_for(PadStyle::Snes);
    let mut keyboard = KeyboardInput::new(keymap.clone());
    keyboard.handle_key(keymap.keys_for(0, J::Start)[0], true);
    let mut batches = vec![Vec::new(); FRAMES];
    batches[0].push(connect());
    batches[4].push(button(P::East, true));
    let mut pad = pad(PadStyle::Snes, batches);
    let combined = run(&mut Both(&mut keyboard, &mut pad));
    let expected = run(&mut ScriptedInput::new()
        .press(0, J::Start, 0..10)
        .press(0, J::A, 4..10));
    assert_eq!(combined, expected);
}

#[test]
fn ps_shortcuts_hide_buttons_from_the_core() {
    let _guard = lock();
    let baseline = run(&mut NullInput);
    // PS held, then Cross and Options: only hotkeys, so the images do not change.
    let mut batches = vec![Vec::new(); FRAMES];
    batches[0].extend([connect(), button(P::Mode, true)]);
    batches[2].extend([button(P::South, true), button(P::Start, true)]);
    assert_eq!(run(&mut pad(PadStyle::Snes, batches)), baseline);
}

#[test]
fn disconnect_releases_the_port_seen_by_the_core() {
    let _guard = lock();
    let mut batches = vec![Vec::new(); FRAMES];
    batches[0].extend([connect(), button(P::South, true)]);
    batches[5].push(PadEvent {
        id: ID,
        kind: PadEventKind::Disconnected,
    });
    let seen = run(&mut pad(PadStyle::Snes, batches));
    assert_eq!(seen, run(&mut ScriptedInput::new().press(0, J::B, 0..5)));
}
