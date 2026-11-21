// The mock core only builds on Unix; on Windows most helpers are unused.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024): the mock core reads its protocol with getenv().
#![allow(unsafe_code)]
//! Version 2.0 runtime (keyboard events and polling, relative mouse, disk sets, working copies, disk swapping,
//! start without content) against the extended mock core (protocol at the top of `tests/mock-core/mock_core.c`).
//! The `MOCK_*` variables are process-global: this binary holds a single test running the scenarios in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};

use emu_runtime::{
    CollectSink, ContentSpec, DiskControlKind, Emulator, EmulatorOptions, InputBackend,
    MediaPolicy, MediaRules, MediaSet, MouseButton, NO_GAME_NAME, NullInput, RetroKey,
    RuntimeError, ScriptedInput, reset_working_copies,
};

const VARS: [&str; 5] = [
    "MOCK_KEYBOARD",
    "MOCK_MOUSE",
    "MOCK_DISK",
    "MOCK_DISK_DUMP",
    "MOCK_NO_GAME",
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
            .join("emu-runtime-computer")
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

fn step(emu: &mut Emulator, input: &mut dyn InputBackend) {
    emu.run_frame(input, &mut CollectSink::default()).unwrap();
}

fn key(raw: u16) -> RetroKey {
    RetroKey::new(raw).unwrap()
}

#[cfg(unix)]
#[test]
fn computer_runtime_on_mock_core() {
    let lib = mock_core::build(mock_core::Variant::Default);
    console_unchanged(&lib);
    keyboard_and_mouse(&lib);
    disks_working_copy(&lib);
    disks_basic_direct(&lib);
    no_game(&lib);
    set_protocol(&[]);
}

fn console_unchanged(lib: &Path) {
    set_protocol(&[]);
    let env = Env::new("console");
    let rom = env.file("Game.bin", b"rom");
    let mut emu = Emulator::load(lib, &rom, env.options()).unwrap();
    step(&mut emu, &mut NullInput);
    assert!(!emu.uses_keyboard() && !emu.uses_mouse());
    assert!(emu.disks().is_none());
    assert!(emu.prepared_media().is_none());
    assert_eq!(emu.game_name(), "Game");
}

fn keyboard_and_mouse(lib: &Path) {
    set_protocol(&[("MOCK_KEYBOARD", "1"), ("MOCK_MOUSE", "1")]);
    let env = Env::new("keyboard");
    let rom = env.file("Game.bin", b"rom");
    let mut emu = Emulator::load_content(lib, ContentSpec::File(rom), env.options()).unwrap();
    assert!(emu.uses_keyboard(), "keyboard callback registered");
    assert!(!emu.uses_mouse(), "not polled yet");
    let a = key(97);
    let ret = key(13);
    let mut input = ScriptedInput::new()
        .key(a, 1..3)
        .key(ret, 2..3)
        .mouse(5, -3, MouseButton::Left.bit(), 1..2)
        .mouse(2, 2, 0, 2..3);

    step(&mut emu, &mut input); // frame 0: nothing
    assert!(emu.uses_mouse());
    assert_eq!(cell(&emu, 0, 0), 0, "no event");
    assert_eq!(cell(&emu, 1, 0), 0, "no key polled down");

    step(&mut emu, &mut input); // frame 1: `a` down, mouse (5, -3) + left
    assert_eq!(cell(&emu, 0, 0), 1, "one event, delivered before retro_run");
    assert_eq!(u16_at(&emu, 0, 1), 97);
    assert_eq!(cell(&emu, 0, 3) & 1, 1, "down");
    assert_eq!((cell(&emu, 1, 0), u16_at(&emu, 1, 1)), (1, 97));
    assert_eq!(u16_at(&emu, 2, 0) as i16, 5);
    assert_eq!(u16_at(&emu, 2, 2) as i16, -3);
    assert_eq!(cell(&emu, 2, 4) & 1, 1, "left held");

    step(&mut emu, &mut input); // frame 2: return down, `a` held, mouse (2, 2), left released
    assert_eq!(cell(&emu, 0, 0), 2);
    assert_eq!(u16_at(&emu, 4, 0), 13, "newest first");
    assert_eq!(u16_at(&emu, 4, 2), 97);
    assert_eq!(
        (cell(&emu, 1, 0), u16_at(&emu, 1, 1), u16_at(&emu, 1, 3)),
        (2, 13, 97)
    );
    assert_eq!(u16_at(&emu, 2, 0) as i16, 7, "cumulative dx");
    assert_eq!(cell(&emu, 2, 4) & 1, 0);
    assert_eq!(cell(&emu, 2, 5), 1, "one left press");

    step(&mut emu, &mut input); // frame 3: both up (ordered as scripted), no motion
    assert_eq!(cell(&emu, 0, 0), 4);
    assert_eq!((cell(&emu, 1, 5), cell(&emu, 1, 6)), (2, 2), "downs / ups");
    assert_eq!(cell(&emu, 1, 0), 0, "nothing held");
    assert_eq!(u16_at(&emu, 2, 0) as i16, 7, "deltas do not repeat");
    assert_eq!(u16_at(&emu, 2, 2) as i16, -1);
}

fn disks_working_copy(lib: &Path) {
    set_protocol(&[("MOCK_DISK", "ext"), ("MOCK_DISK_DUMP", "1")]);
    let env = Env::new("disks-ext");
    let d1 = env.file("Game (Disk 1 of 2).bin", b"disk one");
    let d2 = env.file("Game (Disk 2 of 2).bin", b"disk two");
    let d3 = env.file("Extra.bin", b"disk three");
    let set = MediaSet {
        name: "Game".into(),
        images: vec![d1.clone(), d2.clone()],
        playlist: None,
    };
    let spec = ContentSpec::Media {
        set,
        rules: MediaRules::new(MediaPolicy::WorkingCopy),
    };
    let mut emu = Emulator::load_content(lib, spec, env.options()).unwrap();
    assert_eq!(emu.game_name(), "Game");
    let saves_core = env.options().save_dir.join(emu.core_id());
    let work = saves_core.join("Game.disks");
    let prepared = emu.prepared_media().unwrap().clone();
    assert_eq!(prepared.content, work.join("Game.m3u"));
    assert_eq!(prepared.work_dir.as_deref(), Some(work.as_path()));

    let mut disks = emu.disks().expect("disk control");
    assert_eq!(disks.kind(), DiskControlKind::Extended);
    let slots = disks.slots();
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].label, "Game (Disk 1 of 2)");
    assert_eq!(
        slots[1].path.as_deref(),
        Some(work.join("Game (Disk 2 of 2).bin").as_path())
    );
    assert_eq!(disks.current(), Some(0));
    assert_eq!(disks.insert_next().unwrap(), 1);
    assert_eq!(disks.current(), Some(1));
    assert_eq!(disks.insert_next().unwrap(), 0, "wraps");
    assert!(matches!(
        disks.insert(5),
        Err(RuntimeError::InvalidMediaSet(_))
    ));
    assert_eq!(disks.current(), Some(0), "unchanged after a refusal");

    assert_eq!(disks.add(&d3).unwrap(), 2);
    assert_eq!(
        disks.current(),
        Some(0),
        "added, not inserted, tray closed again"
    );
    let slots = disks.slots();
    assert_eq!(slots[2].label, "Extra");
    assert_eq!(
        slots[2].path.as_deref(),
        Some(work.join("Extra.bin").as_path())
    );
    disks.insert(2).unwrap();
    disks.eject().unwrap();
    assert_eq!(disks.current(), None);
    disks.insert(1).unwrap();
    step(&mut emu, &mut NullInput);
    assert_eq!(
        (cell(&emu, 3, 0), cell(&emu, 3, 1), cell(&emu, 3, 2)),
        (1, 3, 0)
    );
    assert_eq!(emu.prepared_media().unwrap().images.len(), 3);
    let dump = fs::read_to_string(saves_core.join("mock-disk.txt")).unwrap();
    assert!(dump.contains("index=1") && dump.contains("num=3"), "{dump}");
    drop(emu);

    // The core only ever saw copies; the user's files are untouched.
    assert_eq!(fs::read(&d1).unwrap(), b"disk one");
    assert_eq!(fs::read(&d2).unwrap(), b"disk two");
    assert_eq!(fs::read(&d3).unwrap(), b"disk three");
    assert_eq!(fs::read(work.join("Extra.bin")).unwrap(), b"disk three");
    assert!(reset_working_copies(&saves_core, "Game").unwrap());
    assert!(!work.exists());
}

fn disks_basic_direct(lib: &Path) {
    set_protocol(&[("MOCK_DISK", "basic")]);
    let env = Env::new("disks-basic");
    let d1 = env.file("Only.bin", b"only");
    let spec = ContentSpec::Media {
        set: MediaSet {
            name: "Only".into(),
            images: vec![d1.clone()],
            playlist: None,
        },
        rules: MediaRules::new(MediaPolicy::Direct),
    };
    let mut emu = Emulator::load_content(lib, spec, env.options()).unwrap();
    assert_eq!(emu.prepared_media().unwrap().content, d1);
    let mut disks = emu.disks().unwrap();
    assert_eq!(disks.kind(), DiskControlKind::Basic);
    let slots = disks.slots();
    assert_eq!(slots.len(), 1);
    // No path / label from the basic interface: taken from the prepared list.
    assert_eq!(slots[0].path.as_deref(), Some(d1.as_path()));
    assert_eq!(slots[0].label, "Only");
    assert_eq!(disks.insert_next().unwrap(), 0);
}

fn no_game(lib: &Path) {
    set_protocol(&[]);
    let env = Env::new("no-game");
    assert!(matches!(
        Emulator::load_content(lib, ContentSpec::NoGame, env.options()),
        Err(RuntimeError::Libretro(_))
    ));
    set_protocol(&[("MOCK_NO_GAME", "1"), ("MOCK_KEYBOARD", "cb")]);
    let mut emu = Emulator::load_content(lib, ContentSpec::NoGame, env.options()).unwrap();
    assert_eq!(emu.game_name(), NO_GAME_NAME);
    assert!(emu.prepared_media().is_none());
    assert!(emu.uses_keyboard());
    let mut input = ScriptedInput::new().key(key(32), 0..1);
    step(&mut emu, &mut input);
    assert_eq!(cell(&emu, 3, 7) & 0b10, 0b10, "loaded with load_game(NULL)");
    assert_eq!(cell(&emu, 0, 0), 1);
}
