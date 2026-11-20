// The mock core only builds on Unix.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024) and the raw host below calls the core's C entry points.
#![allow(unsafe_code)]
//! Behaviours the mock core gained ahead of the 4.0 - 6.0 frontends (header of `tests/mock-core/mock_core.c`,
//! "Future-version protocol"). Part 1 drives the core through `Core` (what the current frontend can do: the
//! requests it does not implement yet are declined, and the dumps say so). Part 2 is a tiny raw libretro host
//! written here, because the frontend side of VFS (DF1) and of `retro_reset` with keys (DF1) does not exist yet:
//! it provides the VFS, the keyboard and the analog inputs itself and checks what the core does with them.
//! `MOCK_*` variables are process-global: a single test runs every scenario in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::path::{Path, PathBuf};

use emu_libretro::{
    Core, CoreConfig, FrontendHandler, InputDevice, InputQuery, LibretroError, SerializationQuirks,
    VideoFrame,
};

const VARS: [&str; 14] = [
    "MOCK_VFS",
    "MOCK_RESET_KEYS",
    "MOCK_ANALOG",
    "MOCK_FASTFORWARD",
    "MOCK_FF_RATIO",
    "MOCK_SAVEDIR_VISIBLE",
    "MOCK_STATE_SIZE",
    "MOCK_LOAD_ERROR",
    "MOCK_FPS",
    "MOCK_AV_CHANGE",
    "MOCK_KEYBOARD",
    "MOCK_DISK",
    "MOCK_DISK_DUMP",
    "MOCK_ENV_PROBE",
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
        .join("emu-libretro-future")
        .join(name);
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
    }
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join("saves").join(name))
        .unwrap_or_else(|e| panic!("{name} missing: {e}"))
}

fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

#[derive(Default)]
struct Probe {
    frame: Vec<u8>,
    pitch: usize,
    samples: usize,
    shift_down: bool,
}

impl FrontendHandler for Probe {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        if let Some(f) = frame {
            self.frame = f.data.to_vec();
            self.pitch = f.pitch;
        }
    }
    fn audio_samples(&mut self, samples: &[i16]) {
        self.samples = samples.len();
    }
    fn input_poll(&mut self) {}
    fn input_state(&mut self, q: InputQuery) -> i16 {
        i16::from(q.device == InputDevice::Keyboard && q.id == 304 && self.shift_down)
    }
}

impl Probe {
    fn cell(&self, band: usize, cell: usize) -> u8 {
        self.frame[(8 * band + 4) * self.pitch + (8 * cell + 4) * 4]
    }
    fn u32_at(&self, band: usize, cell: usize) -> u32 {
        u32::from_le_bytes([
            self.cell(band, cell),
            self.cell(band, cell + 1),
            self.cell(band, cell + 2),
            self.cell(band, cell + 3),
        ])
    }
}

#[cfg(unix)]
#[test]
fn future_modes_on_mock_core() {
    let lib = mock_core::build(mock_core::Variant::Default);
    defaults_unchanged(&lib);
    fastforward_declined(&lib);
    variable_state_size(&lib);
    load_error(&lib);
    directories_visible(&lib);
    native_rate_and_hot_change(&lib);
    reset_without_handler(&lib);
    vfs_declined(&lib);
    raw::vfs(&lib);
    raw::reset_keys(&lib);
    raw::analog(&lib);
    set_protocol(&[]);
}

/// No new variable set: no new file, fixed 64-byte state, 60 fps, 735 audio frames.
fn defaults_unchanged(lib: &Path) {
    set_protocol(&[]);
    let dir = out_dir("defaults");
    let mut core = Core::load(lib, config(&dir)).unwrap();
    assert_eq!(core.system_info().library_name, "Mock Core");
    let av = core.load_game(&dir.join("game.mock")).unwrap();
    assert_eq!(av.timing.fps, 60.0);
    assert_eq!(core.serialize_size(), 64);
    assert_eq!(core.serialization_quirks(), SerializationQuirks::default());
    let mut probe = Probe::default();
    let events = core.run(&mut probe).unwrap();
    assert!(!events.av_info_changed);
    assert_eq!(probe.samples, 735 * 2);
    drop(core);
    let mut files: Vec<_> = std::fs::read_dir(dir.join("saves"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(files, ["mock-was-here.txt"]);
}

/// 64 is declined by the current frontend (F3): the core sees `false` for NULL and for a struct. CF1 turns the
/// opt-in on, after which `null`, `load`, `run_on` and `run_off` become 1 for a core that opts in.
fn fastforward_declined(lib: &Path) {
    set_protocol(&[
        ("MOCK_FASTFORWARD", "null,load,run,get"),
        ("MOCK_FF_RATIO", "-1"),
    ]);
    let dir = out_dir("ff");
    let mut core = Core::load(lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert_eq!(
        read(&dir, "mock-ff.txt"),
        "null=0\nload=0\nrun_on=-1\nrun_off=-1\nget=-1\n"
    );
    let mut probe = Probe::default();
    core.run(&mut probe).unwrap();
    assert!(read(&dir, "mock-ff.txt").ends_with("get=0\n"));
    core.set_fast_forwarding(true);
    core.run(&mut probe).unwrap();
    assert!(
        read(&dir, "mock-ff.txt").ends_with("get=1\n"),
        "GET_FASTFORWARDING follows set_fast_forwarding"
    );
    for _ in 0..12 {
        core.run(&mut probe).unwrap();
    }
    assert_eq!(
        read(&dir, "mock-ff.txt"),
        "null=0\nload=0\nrun_on=0\nrun_off=0\nget=1\n"
    );
}

/// `MOCK_STATE_SIZE=100,200`: size 100 at first, 200 from frame 20; the quirk is announced; a state saved at the
/// small size is still accepted.
fn variable_state_size(lib: &Path) {
    set_protocol(&[("MOCK_STATE_SIZE", "100,200")]);
    let dir = out_dir("state");
    let mut core = Core::load(lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert!(
        core.serialization_quirks()
            .contains(SerializationQuirks::CORE_VARIABLE_SIZE)
    );
    assert_eq!(core.serialize_size(), 100);
    let mut small = Vec::new();
    core.serialize(&mut small).unwrap();
    assert_eq!(small.len(), 100);
    assert_eq!(&small[..4], b"MOCK");
    let mut probe = Probe::default();
    for _ in 0..21 {
        core.run(&mut probe).unwrap();
    }
    assert_eq!(core.serialize_size(), 200);
    let mut big = Vec::new();
    core.serialize(&mut big).unwrap();
    assert_eq!(big.len(), 200);
    core.unserialize(&small)
        .expect("small state, variable size");
    assert_eq!(core.serialize_size(), 100, "counter went back to 0");
    // Without the variable: fixed size and no quirk.
    drop(core);
    set_protocol(&[("MOCK_STATE_SIZE", "10")]);
    let mut core = Core::load(lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert_eq!(
        core.serialize_size(),
        64,
        "values below 64 are raised to 64"
    );
}

/// The ERROR / WARN lines themselves are checked once the frontend keeps them (GF1: `core_error`); here the
/// refusal and a clean teardown (the core logs again from `retro_deinit`).
fn load_error(lib: &Path) {
    set_protocol(&[(
        "MOCK_LOAD_ERROR",
        "mock core: a rather long explanation that must be truncated by the frontend",
    )]);
    let dir = out_dir("load-error");
    let mut core = Core::load(lib, config(&dir)).unwrap();
    let err = core.load_game(&dir.join("game.mock")).unwrap_err();
    assert!(matches!(err, LibretroError::LoadGameFailed(_)), "{err:?}");
    assert!(core.av_info().is_none());
    drop(core);
    // The refusal does not leak into the next load.
    set_protocol(&[]);
    let mut core = Core::load(lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
}

/// Band 5 shows FNV-1a of the directories the frontend gave.
fn directories_visible(lib: &Path) {
    set_protocol(&[("MOCK_SAVEDIR_VISIBLE", "1")]);
    let dir = out_dir("dirs");
    let cfg = config(&dir);
    let (save, system) = (
        cfg.save_dir.to_str().unwrap().to_owned(),
        cfg.system_dir.to_str().unwrap().to_owned(),
    );
    let mut core = Core::load(lib, cfg).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    let mut probe = Probe::default();
    core.run(&mut probe).unwrap();
    assert_eq!(probe.u32_at(5, 0), fnv1a(&save));
    assert_eq!(probe.u32_at(5, 4), fnv1a(&system));
}

/// 70 Hz at load; then a hot change to NTSC-like 59.83 at frame 10 with a shorter audio chunk.
fn native_rate_and_hot_change(lib: &Path) {
    set_protocol(&[("MOCK_FPS", "70.09"), ("MOCK_AV_CHANGE", "59.83")]);
    let dir = out_dir("fps");
    let mut core = Core::load(lib, config(&dir)).unwrap();
    let av = core.load_game(&dir.join("game.mock")).unwrap();
    assert_eq!(av.timing.fps, 70.09);
    let mut probe = Probe::default();
    core.run(&mut probe).unwrap();
    assert_eq!(probe.samples, 629 * 2, "44100 / 70.09 rounded");
    let mut changed_at = None;
    for i in 1..20 {
        if core.run(&mut probe).unwrap().av_info_changed {
            changed_at = Some(i);
            break;
        }
    }
    assert_eq!(changed_at, Some(10));
    assert_eq!(core.av_info().unwrap().timing.fps, 59.83);
    core.run(&mut probe).unwrap();
    assert_eq!(probe.samples, 737 * 2);
}

/// `Core::reset` has no input handler (until `reset_with`, DF1): the core sees no key during the reset.
fn reset_without_handler(lib: &Path) {
    set_protocol(&[("MOCK_RESET_KEYS", "1"), ("MOCK_KEYBOARD", "poll")]);
    let dir = out_dir("reset-plain");
    let mut core = Core::load(lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    let mut probe = Probe {
        shift_down: true,
        ..Probe::default()
    };
    core.run(&mut probe).unwrap();
    assert_eq!(probe.cell(1, 0), 1, "Shift is seen by the polling in a run");
    core.reset();
    assert_eq!(read(&dir, "mock-reset.txt"), "resets=1\nshift=0\ndown=\n");
}

/// The current frontend declines 45 (F3); DF1 makes it opt-in per core, so this stays the default case.
fn vfs_declined(lib: &Path) {
    set_protocol(&[("MOCK_VFS", "1")]);
    let dir = out_dir("vfs-declined");
    let mut core = Core::load(lib, config(&dir)).unwrap();
    core.load_game(&dir.join("game.mock")).unwrap();
    assert_eq!(
        read(&dir, "mock-vfs.txt"),
        "version=0\nopendir=unsupported\n"
    );
}

/// A minimal libretro host: environment, VFS (std::fs), input.
#[cfg(unix)]
mod raw {
    use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
    use std::path::Path;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use libloading::Library;

    use super::{out_dir, read, set_protocol};

    struct Dirs {
        system: CString,
        save: CString,
    }
    static DIRS: OnceLock<Dirs> = OnceLock::new();
    static SHIFT: AtomicBool = AtomicBool::new(false);
    static ANALOG: AtomicBool = AtomicBool::new(false);
    static POLLS: AtomicU32 = AtomicU32::new(0);

    const EXPERIMENTAL: c_uint = 0x10000;

    #[repr(C)]
    struct VfsInfo {
        required: u32,
        iface: *const Vfs,
    }
    struct File {
        file: std::fs::File,
    }
    struct Dir {
        entries: Vec<(CString, bool)>,
        pos: Option<usize>,
    }
    type FileP = *mut File;
    type DirP = *mut Dir;
    #[repr(C)]
    struct Vfs {
        get_path: unsafe extern "C" fn(FileP) -> *const c_char,
        open: unsafe extern "C" fn(*const c_char, c_uint, c_uint) -> FileP,
        close: unsafe extern "C" fn(FileP) -> c_int,
        size: unsafe extern "C" fn(FileP) -> i64,
        tell: unsafe extern "C" fn(FileP) -> i64,
        seek: unsafe extern "C" fn(FileP, i64, c_int) -> i64,
        read: unsafe extern "C" fn(FileP, *mut c_void, u64) -> i64,
        write: unsafe extern "C" fn(FileP, *const c_void, u64) -> i64,
        flush: unsafe extern "C" fn(FileP) -> c_int,
        remove: unsafe extern "C" fn(*const c_char) -> c_int,
        rename: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int,
        truncate: unsafe extern "C" fn(FileP, i64) -> i64,
        stat: unsafe extern "C" fn(*const c_char, *mut i32) -> c_int,
        mkdir: unsafe extern "C" fn(*const c_char) -> c_int,
        opendir: unsafe extern "C" fn(*const c_char, bool) -> DirP,
        readdir: unsafe extern "C" fn(DirP) -> bool,
        dirent_get_name: unsafe extern "C" fn(DirP) -> *const c_char,
        dirent_is_dir: unsafe extern "C" fn(DirP) -> bool,
        closedir: unsafe extern "C" fn(DirP) -> c_int,
    }
    // SAFETY: only function pointers.
    unsafe impl Sync for Vfs {}

    fn path_of(p: *const c_char) -> String {
        // SAFETY: the core passes NUL-terminated strings.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }

    unsafe extern "C" fn v_get_path(_: FileP) -> *const c_char {
        std::ptr::null()
    }
    unsafe extern "C" fn v_open(path: *const c_char, mode: c_uint, _: c_uint) -> FileP {
        let path = path_of(path);
        let file = if mode & 2 != 0 {
            std::fs::File::create(path)
        } else {
            std::fs::File::open(path)
        };
        file.map_or(std::ptr::null_mut(), |file| {
            Box::into_raw(Box::new(File { file }))
        })
    }
    unsafe extern "C" fn v_close(h: FileP) -> c_int {
        // SAFETY: `h` comes from `v_open` and is closed once.
        drop(unsafe { Box::from_raw(h) });
        0
    }
    unsafe extern "C" fn v_size(h: FileP) -> i64 {
        // SAFETY: `h` comes from `v_open`.
        unsafe { &*h }
            .file
            .metadata()
            .map_or(-1, |m| i64::try_from(m.len()).unwrap_or(-1))
    }
    unsafe extern "C" fn v_minus1(_: FileP) -> i64 {
        -1
    }
    unsafe extern "C" fn v_seek(_: FileP, _: i64, _: c_int) -> i64 {
        -1
    }
    unsafe extern "C" fn v_read(h: FileP, buf: *mut c_void, len: u64) -> i64 {
        use std::io::Read;
        // SAFETY: `h` from `v_open`; `buf` is writable for `len` bytes (VFS contract).
        let (file, out) = unsafe {
            (
                &mut *h,
                std::slice::from_raw_parts_mut(buf.cast::<u8>(), len as usize),
            )
        };
        file.file.read(out).map_or(-1, |n| n as i64)
    }
    unsafe extern "C" fn v_write(h: FileP, buf: *const c_void, len: u64) -> i64 {
        use std::io::Write;
        // SAFETY: `h` from `v_open`; `buf` is readable for `len` bytes (VFS contract).
        let (file, data) = unsafe {
            (
                &mut *h,
                std::slice::from_raw_parts(buf.cast::<u8>(), len as usize),
            )
        };
        file.file.write_all(data).map_or(-1, |()| len as i64)
    }
    unsafe extern "C" fn v_flush(_: FileP) -> c_int {
        0
    }
    unsafe extern "C" fn v_remove(_: *const c_char) -> c_int {
        -1
    }
    unsafe extern "C" fn v_rename(_: *const c_char, _: *const c_char) -> c_int {
        -1
    }
    unsafe extern "C" fn v_truncate(_: FileP, _: i64) -> i64 {
        -1
    }
    unsafe extern "C" fn v_stat(path: *const c_char, size: *mut i32) -> c_int {
        match std::fs::metadata(path_of(path)) {
            Ok(m) => {
                if !size.is_null() {
                    // SAFETY: non-null `int32_t *` from the core.
                    unsafe { *size = i32::try_from(m.len()).unwrap_or(i32::MAX) };
                }
                1 | if m.is_dir() { 2 } else { 0 }
            }
            Err(_) => 0,
        }
    }
    unsafe extern "C" fn v_mkdir(_: *const c_char) -> c_int {
        -1
    }
    unsafe extern "C" fn v_opendir(path: *const c_char, include_hidden: bool) -> DirP {
        let Ok(read) = std::fs::read_dir(path_of(path)) else {
            return std::ptr::null_mut();
        };
        let entries = read
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                (include_hidden || !name.starts_with('.'))
                    .then(|| (CString::new(name).unwrap(), e.path().is_dir()))
            })
            .collect();
        Box::into_raw(Box::new(Dir { entries, pos: None }))
    }
    unsafe extern "C" fn v_readdir(d: DirP) -> bool {
        // SAFETY: `d` from `v_opendir`.
        let d = unsafe { &mut *d };
        let next = d.pos.map_or(0, |p| p + 1);
        d.pos = Some(next);
        next < d.entries.len()
    }
    unsafe extern "C" fn v_name(d: DirP) -> *const c_char {
        // SAFETY: `d` from `v_opendir`; valid after a successful `readdir`.
        let d = unsafe { &*d };
        d.entries[d.pos.unwrap()].0.as_ptr()
    }
    unsafe extern "C" fn v_is_dir(d: DirP) -> bool {
        // SAFETY: as `v_name`.
        let d = unsafe { &*d };
        d.entries[d.pos.unwrap()].1
    }
    unsafe extern "C" fn v_closedir(d: DirP) -> c_int {
        // SAFETY: `d` from `v_opendir`, closed once.
        drop(unsafe { Box::from_raw(d) });
        0
    }

    static VFS: Vfs = Vfs {
        get_path: v_get_path,
        open: v_open,
        close: v_close,
        size: v_size,
        tell: v_minus1,
        seek: v_seek,
        read: v_read,
        write: v_write,
        flush: v_flush,
        remove: v_remove,
        rename: v_rename,
        truncate: v_truncate,
        stat: v_stat,
        mkdir: v_mkdir,
        opendir: v_opendir,
        readdir: v_readdir,
        dirent_get_name: v_name,
        dirent_is_dir: v_is_dir,
        closedir: v_closedir,
    };

    extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
        let dirs = DIRS.get().unwrap();
        match cmd {
            9 | 31 => {
                let dir = if cmd == 9 { &dirs.system } else { &dirs.save };
                // SAFETY: `const char **` from the core.
                unsafe { *data.cast::<*const c_char>() = dir.as_ptr() };
                true
            }
            10 => true,
            c if c == 45 | EXPERIMENTAL => {
                // SAFETY: `struct retro_vfs_interface_info *` from the core.
                unsafe { (*data.cast::<VfsInfo>()).iface = &raw const VFS };
                true
            }
            _ => false,
        }
    }
    extern "C" fn video(_: *const c_void, _: c_uint, _: c_uint, _: usize) {}
    extern "C" fn audio(_: *const i16, frames: usize) -> usize {
        frames
    }
    extern "C" fn poll() {
        POLLS.fetch_add(1, Ordering::SeqCst);
    }
    extern "C" fn input(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
        match device {
            3 => i16::from(id == 304 && SHIFT.load(Ordering::SeqCst)),
            5 if ANALOG.load(Ordering::SeqCst) => {
                (port as i16 + 1) * 1000 + (index as i16) * 100 + id as i16
            }
            _ => 0,
        }
    }

    #[repr(C)]
    struct GameInfo {
        path: *const c_char,
        data: *const c_void,
        size: usize,
        meta: *const c_void,
    }

    struct Raw {
        lib: Library,
    }

    impl Raw {
        fn start(lib: &Path, game: &Path) -> Raw {
            // SAFETY: loading the mock core built from tests/mock-core/mock_core.c; no initializers of note.
            let lib = unsafe { Library::new(lib) }.expect("dlopen mock core");
            let raw = Raw { lib };
            // SAFETY: the signatures below are those of libretro.h; the environment is set before `retro_init`.
            unsafe {
                raw.f::<extern "C" fn(extern "C" fn(c_uint, *mut c_void) -> bool)>(
                    b"retro_set_environment",
                )(environment);
                raw.f::<extern "C" fn(extern "C" fn(*const c_void, c_uint, c_uint, usize))>(
                    b"retro_set_video_refresh",
                )(video);
                raw.f::<extern "C" fn(extern "C" fn(*const i16, usize) -> usize)>(
                    b"retro_set_audio_sample_batch",
                )(audio);
                raw.f::<extern "C" fn(extern "C" fn())>(b"retro_set_input_poll")(poll);
                raw.f::<extern "C" fn(extern "C" fn(c_uint, c_uint, c_uint, c_uint) -> i16)>(
                    b"retro_set_input_state",
                )(input);
                raw.f::<extern "C" fn()>(b"retro_init")();
            }
            let path = CString::new(game.to_str().unwrap()).unwrap();
            let info = GameInfo {
                path: path.as_ptr(),
                data: std::ptr::null(),
                size: 0,
                meta: std::ptr::null(),
            };
            // SAFETY: `info` and `path` outlive the call; the core reads the file itself (data is NULL).
            let ok = unsafe {
                raw.f::<extern "C" fn(*const GameInfo) -> bool>(b"retro_load_game")(&info)
            };
            assert!(ok, "retro_load_game");
            raw
        }
        /// # Safety
        /// `T` must be the exact signature of the exported symbol.
        unsafe fn f<T: Copy>(&self, name: &[u8]) -> T {
            // SAFETY: forwarded to the caller.
            *unsafe { self.lib.get::<T>(name) }.expect("symbol")
        }
        fn call(&self, name: &[u8]) {
            // SAFETY: every symbol used here is `void fn(void)`.
            unsafe { self.f::<extern "C" fn()>(name)() };
        }
    }
    impl Drop for Raw {
        fn drop(&mut self) {
            self.call(b"retro_unload_game");
            self.call(b"retro_deinit");
        }
    }

    /// One directory for every raw scenario (`DIRS` is set once; the path string stays the same).
    fn start(lib: &Path, vars: &[(&str, &str)]) -> (std::path::PathBuf, Raw) {
        set_protocol(vars);
        let dir = out_dir("raw");
        let _ = DIRS.set(Dirs {
            system: CString::new(dir.join("system").to_str().unwrap()).unwrap(),
            save: CString::new(dir.join("saves").to_str().unwrap()).unwrap(),
        });
        let raw = Raw::start(lib, &dir.join("game.mock"));
        (dir, raw)
    }

    /// The core asks for VFS v3 and uses it for everything: directory listing (hidden entries skipped), write,
    /// read, size, stat, and a stat from another thread.
    pub fn vfs(lib: &Path) {
        set_protocol(&[("MOCK_VFS", "thread")]);
        let dir = out_dir("raw");
        std::fs::write(dir.join("system/a.bin"), b"abc").unwrap();
        std::fs::write(dir.join("system/.hidden"), b"h").unwrap();
        std::fs::create_dir(dir.join("system/sub")).unwrap();
        let _ = DIRS.set(Dirs {
            system: CString::new(dir.join("system").to_str().unwrap()).unwrap(),
            save: CString::new(dir.join("saves").to_str().unwrap()).unwrap(),
        });
        let _raw = Raw::start(lib, &dir.join("game.mock"));
        let dump = read(&dir, "mock-vfs.txt");
        let mut lines: Vec<&str> = dump.lines().collect();
        assert_eq!(&lines[..2], ["version=3", "opendir=ok"], "{dump}");
        let tail = lines.split_off(2);
        let (entries, rest): (Vec<&str>, Vec<&str>) =
            tail.iter().partition(|l| l.starts_with("entry="));
        let mut entries = entries;
        entries.sort();
        assert_eq!(entries, ["entry=a.bin|f", "entry=sub|d"], "{dump}");
        assert_eq!(
            rest,
            [
                "write=6",
                "size=6",
                "read=vfs-ok",
                "stat=1:6",
                "thread_stat=1:6"
            ],
            "{dump}"
        );
        assert_eq!(
            std::fs::read(dir.join("system/mock-vfs-out.bin")).unwrap(),
            b"vfs-ok"
        );
    }

    /// `retro_reset` polls the keyboard: the core sees Shift when the host answers during the reset.
    pub fn reset_keys(lib: &Path) {
        SHIFT.store(true, Ordering::SeqCst);
        let (dir, raw) = start(lib, &[("MOCK_RESET_KEYS", "1")]);
        let polls = POLLS.load(Ordering::SeqCst);
        raw.call(b"retro_reset");
        assert!(POLLS.load(Ordering::SeqCst) > polls, "input_poll called");
        assert_eq!(
            read(&dir, "mock-reset.txt"),
            "resets=1\nshift=1\ndown=304\n"
        );
        SHIFT.store(false, Ordering::SeqCst);
        raw.call(b"retro_reset");
        assert_eq!(read(&dir, "mock-reset.txt"), "resets=2\nshift=0\ndown=\n");
    }

    /// Sticks and the 16 analog buttons are read on the requested port.
    pub fn analog(lib: &Path) {
        ANALOG.store(true, Ordering::SeqCst);
        let (dir, raw) = start(lib, &[("MOCK_ANALOG", "1")]);
        raw.call(b"retro_run");
        let buttons: Vec<String> = (2200..2216).map(|v| v.to_string()).collect();
        assert_eq!(
            read(&dir, "mock-analog.txt"),
            format!(
                "port=1\nleft=2000,2001\nright=2100,2101\nbuttons={}\n",
                buttons.join(",")
            )
        );
        ANALOG.store(false, Ordering::SeqCst);
        raw.call(b"retro_run");
        assert!(
            read(&dir, "mock-analog.txt").contains("left=0,0"),
            "rewritten when a value changes"
        );
    }
}
