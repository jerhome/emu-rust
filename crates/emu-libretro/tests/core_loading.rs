//! `Core` loading tests. One `Core` per process (ADR 0008): every test takes `LOCK`.
//! Tests needing the real core are `#[ignore]` (ADR 0007); run `scripts/fetch-test-assets.sh` first.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_libretro::{
    Core, CoreConfig, FrontendHandler, InputDevice, InputQuery, LibretroError, PixelFormat,
    VideoFrame,
};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn config() -> CoreConfig {
    let out = workspace_root().join("target/test-output/emu-libretro");
    CoreConfig {
        system_dir: out.join("system"),
        save_dir: out.join("saves"),
        variables: Vec::new(),
    }
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

fn test_rom() -> PathBuf {
    require(workspace_root().join("tests/roms/240pSuite-1.21.bin"))
}

#[test]
fn load_missing_library_fails_and_releases_guard() {
    let _lock = lock();
    let path = Path::new("/nonexistent/emu-rust/core_libretro.so");
    for _ in 0..2 {
        let err = Core::load(path, config()).unwrap_err();
        assert!(matches!(err, LibretroError::Load { .. }), "{err:?}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn load_library_without_libretro_symbols_fails() {
    let _lock = lock();
    let err = Core::load(Path::new("libc.so.6"), config()).unwrap_err();
    assert!(
        matches!(err, LibretroError::MissingSymbol("retro_set_environment")),
        "{err:?}"
    );
}

#[derive(Default)]
struct Counter {
    frames: u32,
    dupes: u32,
    last_size: (u32, u32),
    format: Option<PixelFormat>,
    samples: usize,
    polls: u32,
    queries: u32,
    panic_on_poll: bool,
}

impl FrontendHandler for Counter {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        match frame {
            Some(f) => {
                assert!(f.pitch >= f.width as usize * f.format.bytes_per_pixel());
                self.frames += 1;
                self.last_size = (f.width, f.height);
                self.format = Some(f.format);
            }
            None => self.dupes += 1,
        }
    }
    fn audio_samples(&mut self, samples: &[i16]) {
        self.samples += samples.len();
    }
    fn input_poll(&mut self) {
        assert!(!self.panic_on_poll, "requested panic");
        self.polls += 1;
    }
    fn input_state(&mut self, query: InputQuery) -> i16 {
        assert_eq!(query.device, InputDevice::Joypad);
        self.queries += 1;
        0
    }
}

#[test]
#[ignore = "needs tests/cores/genesis_plus_gx (scripts/fetch-test-assets.sh)"]
fn genesis_plus_gx_runs_frames() {
    let _lock = lock();
    let core_path = genesis_core();
    let rom = test_rom();

    let mut core = Core::load(&core_path, config()).expect("load core");
    assert!(
        core.system_info().library_name.contains("Genesis Plus GX"),
        "{:?}",
        core.system_info()
    );
    assert!(
        core.system_info()
            .valid_extensions
            .iter()
            .any(|e| e == "md")
    );
    assert!(matches!(
        Core::load(&core_path, config()),
        Err(LibretroError::AlreadyLoaded)
    ));
    assert!(matches!(
        core.run(&mut Counter::default()),
        Err(LibretroError::NoGame)
    ));

    let av = core.load_game(&rom).expect("load game");
    assert!((av.timing.fps - 59.92).abs() < 0.5, "{av:?}");
    assert!(av.timing.sample_rate > 0.0);
    assert_eq!(core.av_info(), Some(&av));
    assert!(matches!(
        core.load_game(&rom),
        Err(LibretroError::GameAlreadyLoaded)
    ));

    let mut counter = Counter::default();
    for _ in 0..120 {
        let events = core.run(&mut counter).expect("run");
        assert!(!events.shutdown_requested);
    }
    assert_eq!(counter.frames + counter.dupes, 120);
    assert!(counter.frames > 0);
    assert_eq!(counter.polls, 120);
    assert_eq!(counter.format, Some(core.pixel_format()));
    let (w, h) = counter.last_size;
    assert!(w > 0 && h > 0 && w <= av.geometry.max_width && h <= av.geometry.max_height);
    // ~44.1/48 kHz stereo at ~60 fps: at least 1000 interleaved samples per frame.
    assert!(counter.samples > 120 * 1000, "{} samples", counter.samples);

    // A panicking handler is reported, not propagated through C.
    let mut panicking = Counter {
        panic_on_poll: true,
        ..Counter::default()
    };
    assert!(matches!(
        core.run(&mut panicking),
        Err(LibretroError::CallbackPanicked)
    ));
    // The core keeps working afterwards.
    core.run(&mut counter).expect("run after panic");

    core.reset();
    core.unload_game();
    assert_eq!(core.av_info(), None);
    core.unload_game();
    drop(core);

    // The guard is released on drop: a new core can be loaded.
    let core = Core::load(&core_path, config()).expect("reload core");
    drop(core);
}
