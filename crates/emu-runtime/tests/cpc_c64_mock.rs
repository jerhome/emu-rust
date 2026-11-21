// The mock core only builds on Unix; on Windows most helpers are unused.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024): the mock core reads its protocol with getenv().
#![allow(unsafe_code)]
//! Version 5.0 runtime (design v5.0 §6, « Runtime »): per-game save folder (`SaveScope::Game`), simulated key
//! presses (`pulse_key`) and fast-forward requested by the core (env 64) against the mock core. The `MOCK_*`
//! variables are process-global: this binary holds a single test running the scenarios in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, InputBackend, NullInput, PULSE_FRAMES,
    RetroKey, SaveScope, ScriptedInput,
};

const VARS: [&str; 5] = [
    "MOCK_KEYBOARD",
    "MOCK_NO_GAME",
    "MOCK_FASTFORWARD",
    "MOCK_FF_RATIO",
    "MOCK_SAVEDIR_VISIBLE",
];

/// Sets exactly `vars` among the mock protocol variables.
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

struct Env {
    root: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let root = mock_core::output_dir()
            .join("emu-runtime-cpc-c64")
            .join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("roms")).unwrap();
        Self { root }
    }

    fn options(&self) -> EmulatorOptions {
        let data = self.root.join("data");
        EmulatorOptions {
            system_dir: data.join("system"),
            save_dir: data.join("saves"),
            cache_dir: data.join("cache"),
            provide_vfs: false,
            state_dir: data.join("states"),
            assets_dir: data.join("assets"),
            core_variables: Vec::new(),
            restore_added_disks: false,
            core_save_scope: SaveScope::Core,
            fastforward_override: false,
            variable_state_size: false,
        }
    }

    fn file(&self, name: &str, data: &[u8]) -> PathBuf {
        let path = self.root.join("roms").join(name);
        fs::write(&path, data).unwrap();
        path
    }
}

/// Register byte of the mock's display (band, cell), from the RGBA frame (gray: R = value).
fn cell(emu: &Emulator, band: usize, cell: usize) -> u8 {
    let frame = emu.frame();
    let width = frame.width() as usize;
    frame.pixels()[((8 * band + 4) * width + 8 * cell + 4) * 4]
}

fn u16_at(emu: &Emulator, band: usize, c: usize) -> u16 {
    u16::from_le_bytes([cell(emu, band, c), cell(emu, band, c + 1)])
}

fn u32_at(emu: &Emulator, band: usize, c: usize) -> u32 {
    u32::from_le_bytes([0, 1, 2, 3].map(|i| cell(emu, band, c + i)))
}

fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

fn step(emu: &mut Emulator, input: &mut dyn InputBackend) -> emu_runtime::FrameStatus {
    emu.run_frame(input, &mut CollectSink::default()).unwrap()
}

#[cfg(unix)]
#[test]
fn cpc_c64_runtime_on_mock_core() {
    let lib = mock_core::build(mock_core::Variant::Default);
    game_save_scope(&lib);
    key_pulses(&lib);
    fastforward_requests(&lib);
    set_protocol(&[]);
}

/// `GET_SAVE_DIRECTORY` = `<saves>/<core>/<save_name>/` (created before `Core::load`); SRAM and states stay flat.
fn game_save_scope(lib: &Path) {
    set_protocol(&[("MOCK_SAVEDIR_VISIBLE", "1")]);
    let env = Env::new("scope");
    let rom = env.file("Jeu.bin", b"disk one");
    let options = EmulatorOptions {
        core_save_scope: SaveScope::Game,
        ..env.options()
    };
    let saves_core = options.save_dir.join(emu_runtime::core_id_from_path(lib));
    let mut emu = Emulator::load_content(lib, ContentSpec::File(rom), options.clone()).unwrap();
    let dir = emu.identity().core_save_dir.clone();
    assert_eq!(dir, saves_core.join("Jeu"));
    assert!(dir.is_dir(), "created before the core is loaded");
    step(&mut emu, &mut NullInput);
    assert_eq!(
        u32_at(&emu, 5, 0),
        fnv1a(dir.to_str().unwrap()),
        "core got it"
    );
    assert_eq!(
        emu.sram_path(),
        saves_core.join("Jeu.srm"),
        "SRAM stays flat"
    );
    assert_eq!(
        emu.state_path(0).unwrap().parent().unwrap(),
        options.state_dir.join(emu_runtime::core_id_from_path(lib))
    );
    drop(emu);

    // Without content: `no-game/`.
    set_protocol(&[("MOCK_NO_GAME", "1")]);
    let emu = Emulator::load_content(lib, ContentSpec::NoGame, options.clone()).unwrap();
    assert_eq!(emu.identity().core_save_dir, saves_core.join("no-game"));
    assert!(saves_core.join("no-game").is_dir());
    drop(emu);
    set_protocol(&[("MOCK_SAVEDIR_VISIBLE", "1")]);

    // `Core` (4.0): the core folder itself.
    let rom = env.file("Autre.bin", b"disk two");
    let mut emu = Emulator::load_content(lib, ContentSpec::File(rom), env.options()).unwrap();
    step(&mut emu, &mut NullInput);
    assert_eq!(emu.identity().core_save_dir, saves_core);
    assert_eq!(u32_at(&emu, 5, 0), fnv1a(saves_core.to_str().unwrap()));
}

/// Callback events and polled state of a pulse, a queue of two, and a key the user holds.
fn key_pulses(lib: &Path) {
    set_protocol(&[("MOCK_KEYBOARD", "1")]);
    let env = Env::new("pulses");
    let rom = env.file("Game.bin", b"disk");
    let mut emu = Emulator::load_content(lib, ContentSpec::File(rom), env.options()).unwrap();
    let f13 = RetroKey::new(294).unwrap();
    emu.pulse_key(f13, PULSE_FRAMES);
    step(&mut emu, &mut NullInput);
    assert_eq!(cell(&emu, 0, 0), 1, "press event delivered");
    assert_eq!(u16_at(&emu, 0, 1), 294);
    assert_eq!(
        (cell(&emu, 1, 0), u16_at(&emu, 1, 1)),
        (1, 294),
        "polled down"
    );
    for _ in 1..PULSE_FRAMES {
        step(&mut emu, &mut NullInput);
        assert_eq!(
            (cell(&emu, 0, 0), cell(&emu, 1, 0)),
            (1, 1),
            "held, no event"
        );
    }
    step(&mut emu, &mut NullInput);
    assert_eq!(cell(&emu, 0, 0), 2, "release event");
    assert_eq!((cell(&emu, 1, 5), cell(&emu, 1, 6)), (1, 1), "downs / ups");
    assert_eq!(cell(&emu, 1, 0), 0, "nothing polled down");

    // Two queued pulses of the same key: two presses, a released frame in between.
    emu.pulse_key(f13, 2);
    emu.pulse_key(f13, 2);
    let mut held = Vec::new();
    for _ in 0..7 {
        step(&mut emu, &mut NullInput);
        held.push(cell(&emu, 1, 0));
    }
    assert_eq!(held, [1, 1, 0, 1, 1, 0, 0]);
    assert_eq!((cell(&emu, 1, 5), cell(&emu, 1, 6)), (3, 3));

    // The user holds `a` across a pulse of `a`: never released by the pulse.
    let a = RetroKey::new(97).unwrap();
    let mut input = ScriptedInput::new().key(a, 0..100);
    let start = emu.frame_count();
    let mut input = Offset(&mut input, start);
    step(&mut emu, &mut input); // user press
    emu.pulse_key(a, 2);
    for _ in 0..5 {
        step(&mut emu, &mut input);
        assert_eq!(u16_at(&emu, 1, 1), 97, "still held");
    }
    assert_eq!((cell(&emu, 1, 5), cell(&emu, 1, 6)), (4, 3), "no release");
}

/// Replays a scripted backend from frame 0 whatever the emulator's frame count.
struct Offset<'a>(&'a mut ScriptedInput, u64);

impl InputBackend for Offset<'_> {
    fn poll(&mut self, frame_index: u64, state: &mut emu_runtime::InputFrame) {
        self.0.poll(frame_index - self.1, state);
    }
}

/// `FrameStatus::fastforward_changed` once per new request (on at frame 5, off at frame 10), only with the opt-in;
/// a request made while loading is reported by the first frame.
fn fastforward_requests(lib: &Path) {
    let env = Env::new("ff");
    let rom = env.file("Game.bin", b"tape");
    let on = EmulatorOptions {
        fastforward_override: true,
        ..env.options()
    };
    let run = |options: EmulatorOptions| {
        let mut emu = Emulator::load_content(lib, ContentSpec::File(rom.clone()), options).unwrap();
        (0..15)
            .map(|frame| {
                let status = step(&mut emu, &mut NullInput);
                (
                    frame,
                    status.fastforward_changed,
                    emu.fastforward_override(),
                )
            })
            .filter(|(_, changed, _)| *changed)
            .map(|(frame, _, ff)| {
                let ff = ff.unwrap();
                (frame, ff.fastforward, ff.ratio)
            })
            .collect::<Vec<_>>()
    };

    set_protocol(&[("MOCK_FASTFORWARD", "null,run")]);
    let changes = run(on.clone());
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!((changes[0].1, changes[0].2), (true, 2.0));
    assert_eq!((changes[1].1, changes[1].2), (false, 2.0));
    assert!(changes[0].0 < changes[1].0);

    set_protocol(&[("MOCK_FASTFORWARD", "load,run")]);
    let changes = run(on.clone());
    assert_eq!(
        changes.len(),
        2,
        "on at load (frame 5 repeats it), off: {changes:?}"
    );
    assert_eq!((changes[0].0, changes[0].1), (0, true));
    assert!(!changes[1].1);

    // Opt-out (4.0): never reported.
    let mut emu =
        Emulator::load_content(lib, ContentSpec::File(rom.clone()), env.options()).unwrap();
    for _ in 0..15 {
        assert!(!step(&mut emu, &mut NullInput).fastforward_changed);
    }
    assert_eq!(emu.fastforward_override(), None);
}
