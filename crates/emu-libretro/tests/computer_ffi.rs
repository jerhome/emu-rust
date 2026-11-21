// The mock core only builds on Unix; on Windows most helpers are unused.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024): the mock core reads its protocol with getenv().
#![allow(unsafe_code)]
//! Version 2.0 FFI (keyboard callback, keyboard / mouse polling, disk control 13 / 58, no-game start) against the
//! extended mock core (protocol at the top of `tests/mock-core/mock_core.c`). The mock reads `MOCK_*` environment
//! variables, which are process-global: this binary holds a single test that runs the scenarios in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::path::{Path, PathBuf};

use emu_libretro::{
    Core, CoreConfig, DiskControlKind, FrontendHandler, InputDevice, InputQuery, KeyEvent,
    KeyModifiers, LibretroError, PixelFormat, RetroKey, VideoFrame, mouse_id,
};

const VARS: [&str; 7] = [
    "MOCK_DISK_NO_INITIAL",
    "MOCK_ENV_PROBE",
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

fn out_dir(name: &str) -> PathBuf {
    let dir = mock_core::output_dir()
        .join("emu-libretro-computer")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("saves")).unwrap();
    dir
}

fn load(lib: &Path, dir: &Path) -> Core {
    Core::load(
        lib,
        CoreConfig {
            system_dir: dir.join("system"),
            save_dir: dir.join("saves"),
            assets_dir: dir.join("assets"),
            variables: Vec::new(),
            provide_vfs: false,
            copy_dir: None,
            fastforward_override: false,
            variable_state_size: false,
        },
    )
    .expect("load mock core")
}

/// Keeps the last frame and answers keyboard / mouse queries from fixed state.
#[derive(Default)]
struct Probe {
    frame: Vec<u8>,
    pitch: usize,
    keys_down: Vec<u32>,
    mouse: [i16; 3],
}

impl FrontendHandler for Probe {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        if let Some(f) = frame {
            assert_eq!(f.format, PixelFormat::Xrgb8888);
            self.frame = f.data.to_vec();
            self.pitch = f.pitch;
        }
    }
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, q: InputQuery) -> i16 {
        match q.device {
            InputDevice::Keyboard => i16::from(self.keys_down.contains(&q.id)),
            InputDevice::Mouse => match q.id {
                mouse_id::X => self.mouse[0],
                mouse_id::Y => self.mouse[1],
                mouse_id::LEFT => self.mouse[2],
                _ => 0,
            },
            _ => 0,
        }
    }
}

impl Probe {
    /// Register byte of the mock's display (band, cell).
    fn cell(&self, band: usize, cell: usize) -> u8 {
        let offset = (8 * band + 4) * self.pitch + (8 * cell + 4) * 4;
        self.frame[offset]
    }
    fn u16_at(&self, band: usize, cell: usize) -> u16 {
        u16::from_le_bytes([self.cell(band, cell), self.cell(band, cell + 1)])
    }
}

fn key(raw: u16) -> RetroKey {
    RetroKey::new(raw).unwrap()
}

#[cfg(unix)]
#[test]
fn computer_features_on_mock_core() {
    let lib = mock_core::build(mock_core::Variant::Default);
    no_features(&lib);
    keyboard_and_mouse(&lib);
    disk_ext(&lib);
    disk_basic(&lib);
    puae_like(&lib);
    no_game(&lib);
    set_protocol(&[]);
}

fn no_features(lib: &Path) {
    set_protocol(&[]);
    let dir = out_dir("none");
    let mut core = load(lib, &dir);
    assert!(!core.has_keyboard_callback());
    assert!(!core.supports_no_game());
    assert!(core.disk_control().is_none());
    assert!(matches!(
        core.load_no_game(),
        Err(LibretroError::NoGameUnsupported)
    ));
    let rom = dir.join("game.mock");
    std::fs::write(&rom, b"x").unwrap();
    core.load_game(&rom).unwrap();
    // No callback: no-op.
    core.send_key_event(KeyEvent {
        down: true,
        key: key(97),
        character: 0,
        modifiers: KeyModifiers::default(),
    });
}

fn keyboard_and_mouse(lib: &Path) {
    set_protocol(&[("MOCK_KEYBOARD", "1"), ("MOCK_MOUSE", "1")]);
    let dir = out_dir("keyboard");
    let mut core = load(lib, &dir);
    assert!(core.has_keyboard_callback());
    let event = KeyEvent {
        down: true,
        key: key(97),
        character: u32::from('A'),
        modifiers: KeyModifiers(KeyModifiers::SHIFT),
    };
    // Before content: dropped.
    core.send_key_event(event);
    let rom = dir.join("game.mock");
    std::fs::write(&rom, b"x").unwrap();
    core.load_game(&rom).unwrap();
    core.send_key_event(event);
    core.send_key_event(KeyEvent {
        down: false,
        key: RetroKey::from_name("rctrl").unwrap(),
        character: 0,
        modifiers: KeyModifiers::default(),
    });

    let mut probe = Probe {
        keys_down: vec![97, 305],
        mouse: [5, -3, 1],
        ..Probe::default()
    };
    core.run(&mut probe).unwrap();
    // Band 0: callback (newest event = rctrl up).
    assert_eq!(probe.cell(0, 0), 2, "events received");
    assert_eq!(probe.u16_at(0, 1), 305);
    assert_eq!(
        probe.cell(0, 3),
        0,
        "newest is an up event without character"
    );
    // Band 4: history, newest first.
    assert_eq!(probe.u16_at(4, 0), 305);
    assert_eq!(probe.u16_at(4, 2), 97);
    // Band 1: polling and counters.
    assert_eq!(probe.cell(1, 0), 2, "keys polled down");
    assert_eq!(probe.u16_at(1, 1), 97);
    assert_eq!(probe.u16_at(1, 3), 305);
    assert_eq!(
        (probe.cell(1, 5), probe.cell(1, 6), probe.cell(1, 7)),
        (1, 1, 1)
    );
    // Band 2: mouse.
    assert_eq!(probe.u16_at(2, 0) as i16, 5);
    assert_eq!(probe.u16_at(2, 2) as i16, -3);
    assert_eq!(probe.cell(2, 4) & 1, 1, "left held");
    assert_eq!(probe.cell(2, 5), 1, "left presses");
}

fn disk_ext(lib: &Path) {
    set_protocol(&[("MOCK_DISK", "ext"), ("MOCK_DISK_DUMP", "1")]);
    let dir = out_dir("disk-ext");
    for name in ["d1.st", "d2.st", "d3.st"] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let m3u = dir.join("game.m3u");
    std::fs::write(&m3u, "d1.st\nd2.st\n").unwrap();
    let mut core = load(lib, &dir);
    {
        let mut disks = core.disk_control().expect("disk control");
        assert_eq!(disks.kind(), DiskControlKind::Extended);
        assert!(matches!(
            disks.set_ejected(true),
            Err(LibretroError::NoGame)
        ));
        assert_eq!(disks.num_images(), 0);
        assert!(matches!(
            disks.set_initial_image(1, Path::new("a\0b")),
            Err(LibretroError::Io { .. })
        ));
        disks.set_initial_image(1, &dir.join("d2.st")).unwrap();
    }
    core.load_game(&m3u).unwrap();
    let mut disks = core.disk_control().unwrap();
    assert!(matches!(
        disks.set_initial_image(0, &dir.join("d1.st")),
        Err(LibretroError::GameAlreadyLoaded)
    ));
    assert_eq!(disks.num_images(), 2);
    assert_eq!(disks.image_index(), 1, "initial image applied");
    assert!(!disks.is_ejected());
    assert_eq!(disks.image_label(0).as_deref(), Some("d1"));
    assert_eq!(disks.image_path(1), Some(dir.join("d2.st")));
    assert_eq!(disks.image_path(9), None);

    assert!(matches!(
        disks.set_image_index(0),
        Err(LibretroError::DiskControlFailed("set_image_index"))
    ));
    disks.set_ejected(true).unwrap();
    assert!(disks.is_ejected());
    disks.set_image_index(0).unwrap();
    disks.add_image().unwrap();
    assert_eq!(disks.num_images(), 3);
    assert_eq!(disks.image_label(2), None, "empty slot");
    disks.replace_image(2, Some(&dir.join("d3.st"))).unwrap();
    assert_eq!(disks.image_label(2).as_deref(), Some("d3"));
    disks.replace_image(0, None).unwrap();
    assert_eq!(disks.num_images(), 2);
    assert_eq!(disks.image_label(0).as_deref(), Some("d2"));
    disks.set_ejected(false).unwrap();
    assert!(matches!(
        disks.replace_image(0, None),
        Err(LibretroError::DiskControlFailed("replace_image_index"))
    ));
    let dump = std::fs::read_to_string(dir.join("saves/mock-disk.txt")).unwrap();
    assert!(
        dump.contains("interface=ext") && dump.contains("num=2"),
        "{dump}"
    );

    let mut probe = Probe::default();
    core.run(&mut probe).unwrap();
    let flags = probe.cell(3, 7);
    assert_eq!(
        flags & 0b1_1001,
        0b1_1001,
        "ext, replace and add seen: {flags:#b}"
    );
    assert_eq!(probe.cell(3, 6), 2, "initial image index + 1");
}

/// What puae needs: 28 / 45 / 46 / 64 declined, and an ext disk interface without `set_initial_image`.
fn puae_like(lib: &Path) {
    set_protocol(&[
        ("MOCK_DISK", "ext"),
        ("MOCK_DISK_DUMP", "1"),
        ("MOCK_DISK_NO_INITIAL", "1"),
        ("MOCK_ENV_PROBE", "1"),
    ]);
    let dir = out_dir("puae-like");
    std::fs::write(dir.join("d1.st"), "d1").unwrap();
    let mut core = load(lib, &dir);
    let env = std::fs::read_to_string(dir.join("saves/mock-env.txt")).unwrap();
    assert_eq!(env, "env28=0\nenv45=0\nenv46=0\nenv64=0\n");
    {
        let mut disks = core.disk_control().expect("disk control");
        assert_eq!(disks.kind(), DiskControlKind::Extended);
        assert!(matches!(
            disks.set_initial_image(0, &dir.join("d1.st")),
            Err(LibretroError::DiskControlUnsupported)
        ));
    }
    core.load_game(&dir.join("d1.st")).unwrap();
    let mut probe = Probe::default();
    core.run(&mut probe).unwrap();
    assert_eq!(probe.cell(3, 6), 0, "no initial image");
    let dump = std::fs::read_to_string(dir.join("saves/mock-disk.txt")).unwrap();
    assert!(dump.contains("initial=-"), "{dump}");
}

fn disk_basic(lib: &Path) {
    set_protocol(&[("MOCK_DISK", "basic")]);
    let dir = out_dir("disk-basic");
    let rom = dir.join("only.st");
    std::fs::write(&rom, b"st").unwrap();
    let mut core = load(lib, &dir);
    assert!(matches!(
        core.disk_control().unwrap().set_initial_image(0, &rom),
        Err(LibretroError::DiskControlUnsupported)
    ));
    core.load_game(&rom).unwrap();
    let mut disks = core.disk_control().unwrap();
    assert_eq!(disks.kind(), DiskControlKind::Basic);
    assert_eq!(disks.num_images(), 1);
    assert_eq!(disks.image_label(0), None);
    assert_eq!(disks.image_path(0), None);
}

fn no_game(lib: &Path) {
    set_protocol(&[("MOCK_NO_GAME", "1"), ("MOCK_KEYBOARD", "cb")]);
    let dir = out_dir("no-game");
    let mut core = load(lib, &dir);
    assert!(core.supports_no_game());
    let av = core.load_no_game().expect("load without content");
    assert_eq!((av.geometry.base_width, av.geometry.base_height), (64, 48));
    assert!(matches!(
        core.load_no_game(),
        Err(LibretroError::GameAlreadyLoaded)
    ));
    core.send_key_event(KeyEvent {
        down: true,
        key: RetroKey::UNKNOWN,
        character: u32::from('é'),
        modifiers: KeyModifiers::default(),
    });
    let mut probe = Probe::default();
    core.run(&mut probe).unwrap();
    assert_eq!(probe.cell(3, 7) & 0b10, 0b10, "loaded with load_game(NULL)");
    assert_eq!(probe.cell(0, 0), 1);
    assert_eq!(probe.cell(0, 3), 0b11, "down + character");
    assert_eq!(probe.u16_at(0, 4), 0xe9);
    core.unload_game();
    core.load_no_game().expect("reload without content");
}
