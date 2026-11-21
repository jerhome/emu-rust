// The mock core only builds on Unix; on Windows most helpers are unused.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
//! Phase 2 FFI features (options, savestates, memory, messages, in-memory content) on the mock core
//! (ADR 0009, not ignored) and on genesis_plus_gx (`#[ignore]`, ADR 0007). One `Core` per process (ADR 0008):
//! every test takes `LOCK`.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_libretro::{
    Core, CoreConfig, FrontendHandler, GameSource, InputDevice, InputQuery, JOYPAD_ID_MASK,
    JoypadButton, LibretroError, MemoryKind, VideoFrame,
};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn out_dir(name: &str) -> PathBuf {
    let dir = mock_core::output_dir()
        .join("emu-libretro-phase2")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(dir: &Path, variables: Vec<(String, String)>) -> CoreConfig {
    CoreConfig {
        system_dir: dir.join("system"),
        save_dir: dir.join("saves"),
        assets_dir: dir.join("assets"),
        variables,
        provide_vfs: false,
    }
}

/// Hashes every frame and audio batch; can hold buttons on port 0.
#[derive(Default)]
struct Recorder {
    video: Vec<u64>,
    audio: DefaultHasher,
    held: u16,
    /// Interleaved samples received per audio callback.
    per_frame: Vec<usize>,
}

impl Recorder {
    fn audio_hash(&self) -> u64 {
        self.audio.finish()
    }
}

impl FrontendHandler for Recorder {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        let mut h = DefaultHasher::new();
        if let Some(f) = frame {
            (f.width, f.height, f.data).hash(&mut h);
        }
        self.video.push(h.finish());
    }
    fn audio_samples(&mut self, samples: &[i16]) {
        samples.hash(&mut self.audio);
        self.per_frame.push(samples.len());
    }
    fn input_poll(&mut self) {}
    fn input_state(&mut self, query: InputQuery) -> i16 {
        if query.port != 0 || query.device != InputDevice::Joypad {
            return 0;
        }
        if query.id == JOYPAD_ID_MASK {
            return self.held as i16;
        }
        i16::from(query.id < 16 && self.held & (1 << query.id) != 0)
    }
}

fn run(core: &mut Core, frames: usize) -> Recorder {
    let mut rec = Recorder::default();
    for _ in 0..frames {
        core.run(&mut rec).expect("run");
    }
    rec
}

/// N frames, snapshot, M frames, restore, M frames: identical video, and identical audio if `exact_audio`.
///
/// genesis_plus_gx does not serialize its audio resampler phase: after `unserialize` the 1470/1472 samples
/// per frame cadence shifts and the first few frames of audio differ, so only the sample count is compared.
fn assert_savestate_round_trip(core: &mut Core, n: usize, m: usize, exact_audio: bool) {
    run(core, n);
    let mut state = Vec::new();
    core.serialize(&mut state).expect("serialize");
    assert_eq!(state.len(), core.serialize_size());
    let first = run(core, m);
    core.unserialize(&state).expect("unserialize");
    let second = run(core, m);
    assert_eq!(first.video, second.video);
    if exact_audio {
        assert_eq!(first.audio_hash(), second.audio_hash());
    } else {
        let total = |r: &Recorder| r.per_frame.iter().sum::<usize>();
        assert!(total(&first).abs_diff(total(&second)) <= 4);
    }
}

#[cfg(unix)]
mod mock {
    use super::*;
    use mock_core::Variant;

    fn content(dir: &Path) -> PathBuf {
        let rom = dir.join("game.mock");
        std::fs::write(&rom, b"phase 2 content").unwrap();
        rom
    }

    #[test]
    fn savestate_round_trip_and_errors() {
        let _lock = lock();
        let dir = out_dir("savestate");
        let mut core = Core::load(
            &mock_core::build(Variant::Default),
            config(&dir, Vec::new()),
        )
        .expect("load");
        assert_eq!(core.serialize_size(), 0);
        assert!(matches!(
            core.serialize(&mut Vec::new()),
            Err(LibretroError::NoGame)
        ));
        assert!(matches!(core.unserialize(&[]), Err(LibretroError::NoGame)));
        assert!(core.memory(MemoryKind::SaveRam).is_none());

        core.load_game(&content(&dir)).expect("load game");
        assert_eq!(core.serialize_size(), 64);
        assert_savestate_round_trip(&mut core, 5, 20, true);

        // Buffer capacity is reused.
        let mut buf = Vec::with_capacity(256);
        core.serialize(&mut buf).unwrap();
        assert_eq!((buf.len(), buf.capacity()), (64, 256));
        assert_eq!(&buf[..4], b"MOCK");

        let mut bad = buf.clone();
        bad[0] = b'X';
        assert!(matches!(
            core.unserialize(&bad),
            Err(LibretroError::UnserializeFailed)
        ));
        assert!(matches!(
            core.unserialize(&buf[..10]),
            Err(LibretroError::UnserializeFailed)
        ));
        // Refused by the frontend: a fixed-size core would otherwise read past the end of the slice.
        assert!(matches!(
            core.unserialize(&buf[..buf.len() - 1]),
            Err(LibretroError::UnserializeFailed)
        ));
        assert!(!core.serialization_quirks().contains(1));
    }

    #[test]
    fn sram_is_exposed_and_writable() {
        let _lock = lock();
        let dir = out_dir("sram");
        let mut core = Core::load(
            &mock_core::build(Variant::Default),
            config(&dir, Vec::new()),
        )
        .expect("load");
        core.load_game(&content(&dir)).expect("load game");
        assert!(core.memory(MemoryKind::Rtc).is_none());
        assert!(core.memory(MemoryKind::SystemRam).is_none());
        assert_eq!(
            core.memory(MemoryKind::SaveRam).map(<[u8]>::len),
            Some(2048)
        );

        let sram = core.memory_mut(MemoryKind::SaveRam).unwrap();
        sram.fill(0);
        sram[100] = 7;
        let mut rec = Recorder {
            held: 1 << JoypadButton::Start.id(),
            ..Recorder::default()
        };
        for _ in 0..3 {
            core.run(&mut rec).unwrap();
        }
        let sram = core.memory(MemoryKind::SaveRam).unwrap();
        // Counter written while Start is held (2 after three frames at speed 1), marker byte, untouched byte.
        assert_eq!(&sram[..5], &[2, 0, 0, 0, 0xA5]);
        assert_eq!(sram[100], 7);
    }

    #[test]
    fn options_v2_with_categories_and_overrides() {
        let _lock = lock();
        let dir = out_dir("options-v2");
        let mut core = Core::load(
            &mock_core::build(Variant::Default),
            config(
                &dir,
                vec![
                    ("mock_speed".into(), "2".into()),
                    ("mock_color".into(), "purple".into()),
                ],
            ),
        )
        .expect("load");
        core.load_game(&content(&dir)).expect("load game");

        let categories = core.option_categories();
        assert_eq!(categories.len(), 1, "{categories:?}");
        let options = core.options();
        let keys: Vec<&str> = options.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(keys, ["mock_color", "mock_speed"]);
        assert!(options.iter().all(|o| o.visible));
        assert_eq!(
            options[0].category.as_deref(),
            Some(categories[0].key.as_str())
        );
        // Invalid override falls back to the default; valid one applies.
        assert_eq!(options[0].current, options[0].default);
        assert_eq!(options[1].current, "2");

        let before = run(&mut core, 2).video;
        core.reset();
        assert!(matches!(
            core.set_option("mock_color", "purple"),
            Err(LibretroError::InvalidOptionValue { .. })
        ));
        assert!(matches!(
            core.set_option("nope", "x"),
            Err(LibretroError::UnknownOption(_))
        ));
        core.set_option("mock_color", "blue").unwrap();
        let after = run(&mut core, 2).video;
        assert_ne!(before, after);
        assert_eq!(core.options()[0].current, "blue");
    }

    #[test]
    fn options_v1_and_legacy() {
        let _lock = lock();
        for variant in [Variant::OptionsV1, Variant::LegacyOptions] {
            let dir = out_dir("options-v1-legacy");
            let mut core =
                Core::load(&mock_core::build(variant), config(&dir, Vec::new())).expect("load");
            core.load_game(&content(&dir)).expect("load game");
            let options = core.options();
            assert_eq!(options.len(), 2, "{variant:?}");
            assert!(core.option_categories().is_empty());
            assert!(options.iter().all(|o| o.category.is_none()));
            assert_eq!(options[0].values.len(), 3);
            assert_eq!(options[1].current, "1");
            core.set_option("mock_speed", "3").unwrap();
            run(&mut core, 1);
            assert_eq!(core.options()[1].current, "3");
        }
    }

    #[test]
    fn messages_and_memory_content() {
        let _lock = lock();
        let dir = out_dir("messages");
        let rom = content(&dir);
        let mut core = Core::load(
            &mock_core::build(Variant::Default),
            config(&dir, Vec::new()),
        )
        .expect("load");
        core.load_game_from(GameSource::Memory {
            path: PathBuf::from("virtual/game.mock"),
            data: std::fs::read(&rom).unwrap(),
        })
        .expect("load from memory");
        let from_memory = run(&mut core, 12);
        let messages = core.take_messages();
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert_eq!(messages[0].text, "mock: frame 10");
        assert!(core.take_messages().is_empty());
        core.set_fast_forwarding(true);
        core.unload_game();

        // Same content from a path: same frames (seed = hash of the content).
        core.load_game(&rom).expect("load from path");
        assert_eq!(run(&mut core, 12).video, from_memory.video);
        assert!(matches!(
            core.load_game_from(GameSource::Path(rom.clone())),
            Err(LibretroError::GameAlreadyLoaded)
        ));
    }

    #[test]
    fn need_fullpath_rejects_memory_content() {
        let _lock = lock();
        let dir = out_dir("fullpath");
        let rom = content(&dir);
        let mut core = Core::load(
            &mock_core::build(Variant::NeedFullpath),
            config(&dir, Vec::new()),
        )
        .expect("load");
        assert!(core.system_info().need_fullpath);
        assert!(matches!(
            core.load_game_from(GameSource::Memory {
                path: rom.clone(),
                data: b"x".to_vec(),
            }),
            Err(LibretroError::NeedsFullPath)
        ));
        core.load_game(&rom).expect("load from path");
        run(&mut core, 1);
    }

    #[test]
    fn set_variable_from_the_core_reports_options_changed() {
        let _lock = lock();
        let dir = out_dir("dynamic");
        let mut core = Core::load(
            &mock_core::build(Variant::DynamicOptions),
            config(&dir, Vec::new()),
        )
        .expect("load");
        core.load_game(&content(&dir)).expect("load game");
        let mut rec = Recorder::default();
        let mut changed = Vec::new();
        for i in 0..30 {
            if core.run(&mut rec).expect("run").options_changed {
                changed.push(i);
            }
        }
        assert_eq!(changed, [20]);
        assert_eq!(core.options()[0].current, "blue");
        // The change shows in the image (blue is the dominant channel from frame 20).
        assert_ne!(rec.video[19], rec.video[20]);

        // Frontend-side change: GET_VARIABLE_UPDATE is true once, `options_changed` stays false.
        core.take_messages();
        core.set_option("mock_color", "green").unwrap();
        for _ in 0..4 {
            assert!(!core.run(&mut rec).expect("run").options_changed);
        }
        let updates = core
            .take_messages()
            .iter()
            .filter(|m| m.text.contains("variables updated"))
            .count();
        assert_eq!(updates, 1);
    }

    #[test]
    fn sixteen_bit_variants_report_their_format() {
        let _lock = lock();
        use emu_libretro::PixelFormat;
        struct Formats(Vec<(PixelFormat, usize)>);
        impl FrontendHandler for Formats {
            fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
                if let Some(f) = frame {
                    self.0.push((f.format, f.pitch));
                }
            }
            fn audio_samples(&mut self, _: &[i16]) {}
            fn input_poll(&mut self) {}
            fn input_state(&mut self, _: InputQuery) -> i16 {
                0
            }
        }
        for (variant, format) in [
            (Variant::Rgb565, PixelFormat::Rgb565),
            (Variant::Xrgb1555, PixelFormat::Xrgb1555),
        ] {
            let dir = out_dir("formats");
            let mut core =
                Core::load(&mock_core::build(variant), config(&dir, Vec::new())).expect("load");
            core.load_game(&content(&dir)).expect("load game");
            let mut seen = Formats(Vec::new());
            core.run(&mut seen).expect("run");
            // 64 pixels of 2 bytes plus 8 bytes of row padding.
            assert_eq!(seen.0, [(format, 136)], "{variant:?}");
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn require(path: PathBuf) -> PathBuf {
    assert!(
        path.is_file(),
        "missing {} — run scripts/fetch-test-assets.sh",
        path.display()
    );
    path
}

fn genesis_core() -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "genesis_plus_gx_libretro.dll"
    } else if cfg!(target_os = "macos") {
        "genesis_plus_gx_libretro.dylib"
    } else {
        "genesis_plus_gx_libretro.so"
    };
    require(workspace_root().join("tests/cores").join(name))
}

#[test]
#[ignore = "needs tests/cores/genesis_plus_gx (scripts/fetch-test-assets.sh)"]
fn genesis_plus_gx_savestate_options_memory() {
    let _lock = lock();
    let dir = out_dir("genesis");
    let rom = require(workspace_root().join("tests/roms/240pSuite-1.21.bin"));
    let mut core = Core::load(&genesis_core(), config(&dir, Vec::new())).expect("load core");

    // genesis_plus_gx sets need_fullpath (CD images are streamed from disk).
    assert!(core.system_info().need_fullpath);
    assert!(matches!(
        core.load_game_from(GameSource::Memory {
            path: PathBuf::from("240pSuite.bin"),
            data: std::fs::read(&rom).unwrap(),
        }),
        Err(LibretroError::NeedsFullPath)
    ));
    core.load_game(&rom).expect("load game");

    assert!(core.serialize_size() > 0);
    assert_savestate_round_trip(&mut core, 30, 90, false);
    assert!(matches!(
        core.unserialize(&[0u8; 16]),
        Err(LibretroError::UnserializeFailed)
    ));
    // The core is still usable after a rejected state.
    run(&mut core, 1);

    assert!(
        core.memory(MemoryKind::SystemRam)
            .is_some_and(|m| !m.is_empty())
    );

    let options = core.options();
    assert!(options.len() > 5, "{} options", options.len());
    assert!(!core.option_categories().is_empty());
    let region = options
        .iter()
        .find(|o| o.key == "genesis_plus_gx_region_detect")
        .expect("region option");
    assert!(region.values.iter().any(|v| v.value == region.current));
    assert!(matches!(
        core.set_option(&region.key, "not-a-region"),
        Err(LibretroError::InvalidOptionValue { .. })
    ));
}
