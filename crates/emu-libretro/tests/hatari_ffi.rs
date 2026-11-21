//! Real hatari core (Atari ST, 2.0): the computer-oriented FFI (keyboard callback, disk control, no content)
//! against a core that really implements them. `#[ignore]`: run `scripts/fetch-test-assets.sh` first.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_libretro::{
    Core, CoreConfig, DiskControlKind, FrontendHandler, GameSource, InputQuery, LibretroError,
    VideoFrame,
};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn require(path: PathBuf) -> PathBuf {
    assert!(
        path.is_file(),
        "missing {}: lancer scripts/fetch-test-assets.sh",
        path.display()
    );
    path
}

/// Fresh directories, EmuTOS installed as `tos.img`.
fn load(name: &str) -> (Core, PathBuf) {
    let dir = root()
        .join("target/test-output/emu-libretro-hatari")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["system", "saves", "assets"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    std::fs::copy(
        require(root().join("tests/roms/system/tos.img")),
        dir.join("system/tos.img"),
    )
    .unwrap();
    let core = Core::load(
        &require(root().join("tests/cores/hatari_libretro.so")),
        CoreConfig {
            system_dir: dir.join("system"),
            save_dir: dir.join("saves"),
            assets_dir: dir.join("assets"),
            variables: Vec::new(),
            provide_vfs: false,
        },
    )
    .expect("load hatari");
    (core, dir)
}

#[derive(Default)]
struct Sink {
    frames: usize,
}

impl FrontendHandler for Sink {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        self.frames += usize::from(frame.is_some());
    }
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, _q: InputQuery) -> i16 {
        0
    }
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn system_info_and_disk_control_with_a_floppy() {
    let _guard = lock();
    let (mut core, dir) = load("floppy");
    let info = core.system_info().clone();
    assert_eq!(info.library_name.to_lowercase(), "hatari");
    assert!(info.need_fullpath, "hatari opens its floppies itself");
    for ext in ["st", "msa", "m3u"] {
        assert!(
            info.valid_extensions.iter().any(|e| e == ext),
            "extension {ext} missing from {:?}",
            info.valid_extensions
        );
    }
    assert!(core.supports_no_game());

    let floppy = dir.join("nova.st");
    std::fs::copy(require(root().join("tests/roms/nova-defense.st")), &floppy).unwrap();
    {
        let mut disks = core
            .disk_control()
            .expect("hatari registers a disk control");
        // The initial image is only valid before the content is loaded.
        assert!(disks.set_initial_image(0, &floppy).is_ok());
    }
    core.load_game_from(GameSource::Path(floppy.clone()))
        .expect("load floppy");
    let mut sink = Sink::default();
    for _ in 0..30 {
        core.run(&mut sink).expect("run");
    }
    assert!(sink.frames > 0);
    assert!(
        core.has_keyboard_callback(),
        "hatari registers a keyboard callback"
    );

    let mut disks = core.disk_control().expect("disk control");
    assert_eq!(disks.kind(), DiskControlKind::Extended);
    assert_eq!(disks.num_images(), 1);
    assert_eq!(disks.image_index(), 0);
    assert!(!disks.is_ejected());
    let path = disks.image_path(0).expect("hatari reports the path");
    assert_eq!(path.file_name(), floppy.file_name());
    assert!(matches!(
        disks.set_initial_image(0, &floppy),
        Err(LibretroError::GameAlreadyLoaded)
    ));

    // Add an empty slot, then remove it again with `replace_image(None)`.
    disks.set_ejected(true).unwrap();
    disks.add_image().expect("add_image");
    assert_eq!(disks.num_images(), 2);
    disks.replace_image(1, None).expect("remove the new slot");
    assert_eq!(disks.num_images(), 1);
    disks.set_ejected(false).unwrap();
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn loads_without_content() {
    let _guard = lock();
    let (mut core, _dir) = load("nogame");
    assert!(core.supports_no_game());
    let av = core.load_no_game().expect("load_game(NULL)");
    assert!(av.geometry.base_width >= 320 && av.geometry.base_height >= 200);
    let mut sink = Sink::default();
    for _ in 0..30 {
        core.run(&mut sink).expect("run");
    }
    assert!(core.has_keyboard_callback());
    assert!(matches!(
        core.load_no_game(),
        Err(LibretroError::GameAlreadyLoaded)
    ));
}
