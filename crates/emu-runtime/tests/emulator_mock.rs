// The mock core only builds on Unix; on Windows most helpers are unused.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
//! `Emulator` phase 2 (SRAM, savestates, zip, options) against the mock core (ADR 0009). Unix only: the mock
//! core is built with the system C compiler.
#![cfg(unix)]

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, Emulator, EmulatorOptions, InputBackend, JoypadButton, MemoryKind, NullInput,
    RuntimeError, STATE_SLOTS, ScriptedInput, StateFileError, StateHeader, decode_state,
    encode_state,
};
use mock_core::Variant;
use tempfile::TempDir;

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const ROM: &[u8] = b"mock content: any bytes seed the image";

struct Env {
    tmp: TempDir,
}

impl Env {
    fn new() -> Self {
        let env = Self {
            tmp: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(env.roms()).unwrap();
        env
    }

    fn root(&self) -> &Path {
        self.tmp.path()
    }

    fn roms(&self) -> PathBuf {
        self.root().join("roms")
    }

    fn options(&self) -> EmulatorOptions {
        let data = self.root().join("data");
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
        }
    }

    fn rom(&self) -> PathBuf {
        let path = self.roms().join("Game (World).bin");
        fs::write(&path, ROM).unwrap();
        path
    }

    fn load(&self, variant: Variant, rom: &Path) -> Result<Emulator, RuntimeError> {
        Emulator::load(&mock_core::build(variant), rom, self.options())
    }
}

/// Runs `n` frames; returns the fingerprint after each one and all audio samples.
fn run(emu: &mut Emulator, input: &mut dyn InputBackend, n: usize) -> (Vec<u64>, Vec<i16>) {
    let mut audio = CollectSink::default();
    let prints = (0..n)
        .map(|_| {
            emu.run_frame(input, &mut audio).unwrap();
            emu.frame().fingerprint()
        })
        .collect();
    (prints, audio.samples)
}

fn dir_names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Temporary content directories created by `Emulator::load` for `need_fullpath` cores.
fn content_temp_dirs() -> BTreeSet<PathBuf> {
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            e.file_name()
                .to_string_lossy()
                .starts_with("emu-rust-content-")
                .then(|| e.path())
        })
        .collect()
}

#[test]
fn identity_and_paths() {
    let _guard = lock();
    let env = Env::new();
    let emu = env.load(Variant::Default, &env.rom()).unwrap();
    assert_eq!(emu.core_id(), "mock_core_default");
    assert_eq!(emu.game_name(), "Game (World)");
    let opts = env.options();
    assert_eq!(
        emu.sram_path(),
        opts.save_dir.join("mock_core_default/Game (World).srm")
    );
    assert_eq!(
        emu.state_path(9).unwrap(),
        opts.state_dir.join("mock_core_default/Game (World).state9")
    );
    assert!(matches!(
        emu.state_path(STATE_SLOTS),
        Err(RuntimeError::InvalidSlot(10))
    ));
    // The core got `<saves>/<core_id>/` as its save directory; the state folder exists too.
    assert!(
        opts.save_dir
            .join("mock_core_default/mock-was-here.txt")
            .is_file()
    );
    assert!(opts.state_dir.join("mock_core_default").is_dir());
    assert!(
        emu.memory(MemoryKind::SaveRam)
            .is_some_and(|m| m.len() == 2048)
    );
    assert!(emu.memory(MemoryKind::VideoRam).is_none());
}

#[test]
fn serialize_round_trip_is_exact() {
    let _guard = lock();
    let env = Env::new();
    let mut emu = env.load(Variant::Default, &env.rom()).unwrap();
    run(&mut emu, &mut NullInput, 20);
    let state = emu.serialize_state().unwrap();
    assert_eq!(state.len(), 64);
    let (prints, audio) = run(&mut emu, &mut NullInput, 30);
    emu.unserialize_state(&state).unwrap();
    assert_eq!(emu.serialize_state().unwrap(), state);
    let (again, audio_again) = run(&mut emu, &mut NullInput, 30);
    assert_eq!(prints, again);
    assert_eq!(audio, audio_again);
    assert!(!audio.is_empty());

    assert!(emu.unserialize_state(b"garbage").is_err());
}

#[test]
fn state_slots_survive_sessions() {
    let _guard = lock();
    let env = Env::new();
    let rom = env.rom();
    let (prints, audio) = {
        let mut emu = env.load(Variant::Default, &rom).unwrap();
        assert!(emu.state_info(3).is_none());
        run(&mut emu, &mut NullInput, 15);
        let path = emu.save_state(3).unwrap();
        assert_eq!(path, emu.state_path(3).unwrap());
        let (header, _) = emu.state_info(3).unwrap();
        assert_eq!(header.core_id, "mock_core_default");
        assert_eq!(header.core_version, "1.0");
        assert_eq!(header.game, "Game (World)");
        assert!(header.created_unix > 1_700_000_000);

        let (prints, audio) = run(&mut emu, &mut NullInput, 25);
        emu.load_state(3).unwrap();
        assert_eq!(
            run(&mut emu, &mut NullInput, 25),
            (prints.clone(), audio.clone())
        );
        assert!(matches!(
            emu.save_state(10),
            Err(RuntimeError::InvalidSlot(10))
        ));
        assert!(matches!(emu.load_state(4), Err(RuntimeError::Io { .. })));
        let dir = emu.state_path(3).unwrap().parent().unwrap().to_path_buf();
        assert_eq!(
            dir_names(&dir),
            BTreeSet::from(["Game (World).state3".into()])
        );
        (prints, audio)
    };

    // Second session: the file alone restores the exact state.
    let mut emu = env.load(Variant::Default, &rom).unwrap();
    run(&mut emu, &mut NullInput, 3);
    emu.load_state(3).unwrap();
    assert_eq!(run(&mut emu, &mut NullInput, 25), (prints, audio));
}

#[test]
fn bad_state_files_leave_the_game_intact() {
    let _guard = lock();
    let env = Env::new();
    let mut emu = env.load(Variant::Default, &env.rom()).unwrap();
    run(&mut emu, &mut NullInput, 10);
    let path = emu.save_state(0).unwrap();
    let good = fs::read(&path).unwrap();
    run(&mut emu, &mut NullInput, 7);
    let before = emu.serialize_state().unwrap();

    let (header, payload) = decode_state(&good).unwrap();
    let foreign = encode_state(
        &StateHeader {
            core_id: "snes9x".into(),
            ..header.clone()
        },
        payload,
    );
    let mut flipped = good.clone();
    let last_payload_byte = flipped.len() - 9;
    flipped[last_payload_byte] ^= 1;
    // Valid container, payload the core rejects.
    let rejected = encode_state(&header, &[0u8; 64]);

    let cases: [(&str, &[u8]); 4] = [
        ("truncated", &good[..good.len() - 1]),
        ("flipped", &flipped),
        ("foreign", &foreign),
        ("rejected", &rejected),
    ];
    for (what, bytes) in cases {
        fs::write(&path, bytes).unwrap();
        let err = emu.load_state(0).unwrap_err();
        match what {
            "truncated" => assert!(matches!(
                err,
                RuntimeError::StateFile(StateFileError::Truncated)
            )),
            "flipped" => assert!(matches!(
                err,
                RuntimeError::StateFile(StateFileError::Checksum)
            )),
            "foreign" => assert!(
                matches!(&err, RuntimeError::StateFile(StateFileError::WrongCore { expected, found })
                    if expected == "mock_core_default" && found == "snes9x"),
                "{err}"
            ),
            _ => assert!(matches!(err, RuntimeError::Libretro(_)), "{err}"),
        }
        assert_eq!(emu.serialize_state().unwrap(), before, "{what}");
    }
    // Unreadable header: not listed in the menu.
    fs::write(&path, &good[..20]).unwrap();
    assert!(emu.state_info(0).is_none());
}

#[test]
fn sram_persists_between_sessions() {
    let _guard = lock();
    let env = Env::new();
    let rom = env.rom();
    let sram_path;
    let expected: Vec<u8>;
    {
        let mut emu = env.load(Variant::Default, &rom).unwrap();
        sram_path = emu.sram_path();
        run(&mut emu, &mut NullInput, 5);
        assert!(
            !emu.flush_sram().unwrap(),
            "unchanged SRAM must not be written"
        );
        assert!(!sram_path.exists());

        let mut start = ScriptedInput::new().press(0, JoypadButton::Start, 7..8);
        run(&mut emu, &mut start, 10);
        let sram = emu.memory(MemoryKind::SaveRam).unwrap();
        assert_eq!(&sram[..5], &[7, 0, 0, 0, 0xA5]);
        assert!(emu.flush_sram().unwrap());
        assert!(!emu.flush_sram().unwrap());
        assert_eq!(
            fs::read(&sram_path).unwrap(),
            emu.memory(MemoryKind::SaveRam).unwrap()
        );

        // Changed again but not flushed: `Drop` writes it.
        let mut start = ScriptedInput::new().press(0, JoypadButton::Start, 20..21);
        run(&mut emu, &mut start, 10);
        expected = emu.memory(MemoryKind::SaveRam).unwrap().to_vec();
        assert_eq!(&expected[..5], &[20, 0, 0, 0, 0xA5]);
    }
    assert_eq!(fs::read(&sram_path).unwrap(), expected);
    // The game-identity registry (2.1) and its lock live there too.
    let names: BTreeSet<String> = dir_names(sram_path.parent().unwrap())
        .into_iter()
        .filter(|n| !n.starts_with(".emu-rust-games"))
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["Game (World).srm".into(), "mock-was-here.txt".into()]),
        "no temporary file left"
    );

    let mut emu = env.load(Variant::Default, &rom).unwrap();
    assert_eq!(
        emu.memory(MemoryKind::SaveRam).unwrap(),
        expected.as_slice()
    );
    run(&mut emu, &mut NullInput, 3);
    assert!(!emu.flush_sram().unwrap());
    drop(emu);

    // Shorter file: prefix restored, the rest is the core's own initial content.
    fs::write(&sram_path, [1, 2, 3]).unwrap();
    let emu = env.load(Variant::Default, &rom).unwrap();
    let sram = emu.memory(MemoryKind::SaveRam).unwrap();
    assert_eq!(&sram[..4], &[1, 2, 3, 0]);
    drop(emu);
    assert_eq!(
        fs::read(&sram_path).unwrap(),
        [1, 2, 3],
        "nothing changed: not rewritten"
    );
}

#[test]
fn unreadable_sram_fails_the_load_without_overwriting() {
    let _guard = lock();
    let env = Env::new();
    let rom = env.rom();
    let sram_path = {
        let emu = env.load(Variant::Default, &rom).unwrap();
        emu.sram_path()
    };
    // A directory where the file should be: reading fails with something other than NotFound.
    fs::create_dir_all(&sram_path).unwrap();
    assert!(matches!(
        env.load(Variant::Default, &rom),
        Err(RuntimeError::Io { path, .. }) if path == sram_path
    ));
    assert!(sram_path.is_dir());
}

#[test]
fn options_and_messages() {
    let _guard = lock();
    let env = Env::new();
    let mut emu = env.load(Variant::Default, &env.rom()).unwrap();
    let options = emu.options();
    let keys: Vec<_> = options.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys, ["mock_color", "mock_speed"]);
    assert!(
        options
            .iter()
            .all(|o| o.category.as_deref() == Some("video"))
    );
    let categories = emu.option_categories();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].key, "video");

    run(&mut emu, &mut NullInput, 12);
    let messages = emu.take_messages();
    assert!(
        messages.iter().any(|m| m.text.contains("frame 10")),
        "{messages:?}"
    );
    assert!(emu.take_messages().is_empty());

    let state = emu.serialize_state().unwrap();
    let (red, _) = run(&mut emu, &mut NullInput, 1);
    emu.unserialize_state(&state).unwrap();
    emu.set_option("mock_color", "green").unwrap();
    let (green, _) = run(&mut emu, &mut NullInput, 1);
    assert_ne!(red, green);
    assert_eq!(
        emu.options()
            .iter()
            .find(|o| o.key == "mock_color")
            .map(|o| o.current.as_str()),
        Some("green")
    );
    assert!(emu.set_option("mock_color", "purple").is_err());
    assert!(emu.set_option("no_such_key", "1").is_err());
    emu.set_fast_forwarding(true);
    emu.set_fast_forwarding(false);
}

#[test]
fn core_variables_are_applied() {
    let _guard = lock();
    let env = Env::new();
    let rom = env.rom();
    let mut emu = env.load(Variant::Default, &rom).unwrap();
    let (red, _) = run(&mut emu, &mut NullInput, 2);
    drop(emu);
    let mut options = env.options();
    options.core_variables = vec![("mock_color".into(), "blue".into())];
    let mut emu = Emulator::load(&mock_core::build(Variant::Default), &rom, options).unwrap();
    let (blue, _) = run(&mut emu, &mut NullInput, 2);
    assert_ne!(red, blue);
}

// ---- zip ----

/// Stored-only zip with full control over names and declared sizes (malicious archives).
fn build_zip(entries: &[(&str, &[u8], Option<u32>)]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data, declared) in entries {
        let offset = out.len() as u32;
        let size = declared.unwrap_or(data.len() as u32);
        let mut common = Vec::new();
        for v in [20u16, 0, 0, 0, 0] {
            common.extend_from_slice(&v.to_le_bytes()); // version, flags, stored, time, date
        }
        common.extend_from_slice(&crc32(data).to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&(name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&common);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&common);
        central.extend_from_slice(&[0; 8]); // comment len, disk, internal attrs, external attrs (part)
        central.extend_from_slice(&[0; 2]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out
}

fn write_zip(env: &Env, name: &str, entries: &[(&str, &[u8], Option<u32>)]) -> PathBuf {
    let path = env.roms().join(name);
    fs::write(&path, build_zip(entries)).unwrap();
    path
}

/// The zip's entry must give exactly the image and sound of the plain file, in both content modes.
#[test]
fn zip_matches_plain_file() {
    let _guard = lock();
    for variant in [Variant::Default, Variant::NeedFullpath] {
        let env = Env::new();
        let reference = {
            let mut emu = env.load(variant, &env.rom()).unwrap();
            run(&mut emu, &mut NullInput, 20)
        };
        let zip = write_zip(
            &env,
            "Zipped (Europe).zip",
            &[
                ("readme.txt", b"not a rom", None),
                ("sub/dir/game.bin", ROM, None),
                ("second.bin", b"other content", None),
            ],
        );
        let temp_before = content_temp_dirs();
        let mut emu = env.load(variant, &zip).unwrap();
        assert_eq!(emu.game_name(), "Zipped (Europe)");
        let created: Vec<_> = content_temp_dirs()
            .difference(&temp_before)
            .cloned()
            .collect();
        if variant == Variant::NeedFullpath {
            assert_eq!(created.len(), 1, "{created:?}");
            assert_eq!(dir_names(&created[0]), BTreeSet::from(["game.bin".into()]));
        } else {
            assert!(created.is_empty(), "{created:?}");
        }
        assert_eq!(run(&mut emu, &mut NullInput, 20), reference, "{variant:?}");
        drop(emu);
        assert!(
            created.iter().all(|d| !d.exists()),
            "temporary content left after drop: {created:?}"
        );
    }
}

#[test]
fn malicious_zips_are_contained() {
    let _guard = lock();
    for variant in [Variant::Default, Variant::NeedFullpath] {
        let env = Env::new();
        let roms = env.roms();
        let temp_before = content_temp_dirs();

        // Zip-slip: the entry's directories are ignored, only its base name is used.
        let zip = write_zip(
            &env,
            "slip.zip",
            &[("../../evil.bin", ROM, None), ("/abs/evil2.bin", ROM, None)],
        );
        let mut emu = env.load(variant, &zip).unwrap();
        run(&mut emu, &mut NullInput, 2);
        drop(emu);
        assert!(!roms.join("../evil.bin").exists());
        assert!(!roms.join("../../evil.bin").exists());
        assert!(!env.root().join("evil.bin").exists());
        assert_eq!(content_temp_dirs(), temp_before, "{variant:?}");

        let zip = write_zip(&env, "huge.zip", &[("big.bin", b"tiny", Some(u32::MAX))]);
        assert!(matches!(
            env.load(variant, &zip),
            Err(RuntimeError::ContentTooLarge { size, .. }) if size == u64::from(u32::MAX)
        ));

        let zip = write_zip(&env, "empty.zip", &[]);
        assert!(matches!(
            env.load(variant, &zip),
            Err(RuntimeError::NoContentInArchive(p)) if p == zip
        ));
        let zip = write_zip(
            &env,
            "docs.zip",
            &[("dir/", b"", None), ("a.txt", b"x", None)],
        );
        assert!(matches!(
            env.load(variant, &zip),
            Err(RuntimeError::NoContentInArchive(_))
        ));

        let zip = roms.join("broken.zip");
        fs::write(&zip, b"PK\x03\x04 definitely not an archive").unwrap();
        assert!(matches!(
            env.load(variant, &zip),
            Err(RuntimeError::Archive { .. })
        ));

        // Nothing written next to the archives, and every temporary directory cleaned up after failures.
        assert_eq!(
            dir_names(&roms),
            BTreeSet::from([
                "broken.zip".into(),
                "docs.zip".into(),
                "empty.zip".into(),
                "huge.zip".into(),
                "slip.zip".into(),
            ])
        );
        assert_eq!(content_temp_dirs(), temp_before, "{variant:?}");
    }
}
