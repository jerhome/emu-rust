//! Process-global libretro callback plumbing (ADR 0008).
//!
//! libretro callbacks carry no user pointer, so frontend state lives in thread-locals of the thread that
//! loaded the core. `Core` is `!Send`, hence every `retro_*` call, and every callback a well-behaved core makes
//! from inside it, happens on that thread. A callback arriving on another thread finds empty thread-locals and
//! gets a neutral answer (logged once). The process-wide `CORE_LOADED` flag enforces "one core per process".
//!
//! Lifetimes:
//! - `EnvState` (directories, variables, geometry, events): installed by `EnvInstall` from `Core::load` until
//!   `Core` is dropped (after `retro_deinit`).
//! - The `FrontendHandler`: installed by `HandlerGuard` only for the duration of one `retro_run`.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::{CoreConfig, FrontendHandler, RunEvents};
use crate::error::LibretroError;
use crate::sys;
use crate::types::{
    AvInfo, GameGeometry, InputDevice, InputQuery, PixelFormat, SystemTiming, VideoFrame,
};

static CORE_LOADED: AtomicBool = AtomicBool::new(false);
static FOREIGN_THREAD_LOGGED: AtomicBool = AtomicBool::new(false);

/// Process-wide "a core is loaded" token. Released on drop.
#[derive(Debug)]
pub(crate) struct ProcessGuard(());

impl ProcessGuard {
    pub(crate) fn acquire() -> Result<Self, LibretroError> {
        CORE_LOADED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ProcessGuard(()))
            .map_err(|_| LibretroError::AlreadyLoaded)
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        CORE_LOADED.store(false, Ordering::Release);
    }
}

/// State touched on every video / audio / input callback; kept out of the `RefCell` to stay cheap.
struct Hot {
    /// True on the thread that owns the loaded core.
    owner: Cell<bool>,
    /// Points to a `&mut dyn FrontendHandler` living on `Core::run`'s stack (see `install_handler`).
    handler: Cell<Option<NonNull<c_void>>>,
    /// A panic was caught at the FFI boundary since the handler was installed.
    panicked: Cell<bool>,
    pixel_format: Cell<PixelFormat>,
}

thread_local! {
    static HOT: Hot = const {
        Hot {
            owner: Cell::new(false),
            handler: Cell::new(None),
            panicked: Cell::new(false),
            pixel_format: Cell::new(PixelFormat::Xrgb1555),
        }
    };
    static ENV: RefCell<Option<EnvState>> = const { RefCell::new(None) };
}

struct CoreVariable {
    key: CString,
    value: CString,
}

/// Environment state owned by the frontend between `Core::load` and `Drop`.
struct EnvState {
    system_dir: Option<CString>,
    save_dir: Option<CString>,
    overrides: Vec<(String, String)>,
    /// `GET_VARIABLE` hands out pointers into these strings: they stay alive until the next `SET_VARIABLES`
    /// or until the core is dropped.
    variables: Vec<CoreVariable>,
    av_info: Option<AvInfo>,
    events: RunEvents,
    logged_commands: Vec<c_uint>,
}

/// Removes the environment state of this thread on drop.
#[derive(Debug)]
pub(crate) struct EnvInstall {
    _not_send: PhantomData<*const ()>,
}

pub(crate) fn install_env(config: &CoreConfig) -> EnvInstall {
    let state = EnvState {
        system_dir: path_to_cstring(&config.system_dir),
        save_dir: path_to_cstring(&config.save_dir),
        overrides: config.variables.clone(),
        variables: Vec::new(),
        av_info: None,
        events: RunEvents::default(),
        logged_commands: Vec::new(),
    };
    ENV.with(|env| *env.borrow_mut() = Some(state));
    HOT.with(|hot| {
        hot.owner.set(true);
        hot.handler.set(None);
        hot.panicked.set(false);
        hot.pixel_format.set(PixelFormat::default());
    });
    EnvInstall {
        _not_send: PhantomData,
    }
}

impl Drop for EnvInstall {
    fn drop(&mut self) {
        let _ = ENV.try_with(|env| {
            if let Ok(mut env) = env.try_borrow_mut() {
                *env = None;
            }
        });
        let _ = HOT.try_with(|hot| {
            hot.owner.set(false);
            hot.handler.set(None);
            hot.pixel_format.set(PixelFormat::default());
        });
    }
}

/// Converts a path to a C string; `None` if it contains an interior NUL.
pub(crate) fn path_to_cstring(path: &Path) -> Option<CString> {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    // libretro expects UTF-8 paths on Windows (RetroArch converts them the same way).
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().into_owned().into_bytes();
    CString::new(bytes).ok()
}

fn with_env<R>(f: impl FnOnce(&mut EnvState) -> R) -> Option<R> {
    ENV.try_with(|env| {
        let mut env = env.try_borrow_mut().ok()?;
        env.as_mut().map(f)
    })
    .ok()
    .flatten()
}

pub(crate) fn pixel_format() -> PixelFormat {
    HOT.try_with(|hot| hot.pixel_format.get())
        .unwrap_or_default()
}

pub(crate) fn av_info() -> Option<AvInfo> {
    with_env(|env| env.av_info).flatten()
}

pub(crate) fn set_av_info(av_info: Option<AvInfo>) {
    with_env(|env| env.av_info = av_info);
}

/// Keeps a `FrontendHandler` reachable from the callbacks; removes it on drop (including during unwinding).
pub(crate) struct HandlerGuard<'a> {
    _slot: PhantomData<&'a mut ()>,
}

/// Installs `slot` as the callback target and resets the per-run events.
///
/// The double reference keeps the stored pointer thin; the guard borrows `slot` so it cannot be moved or
/// dropped while installed.
pub(crate) fn install_handler<'a>(slot: &'a mut &mut dyn FrontendHandler) -> HandlerGuard<'a> {
    let ptr = NonNull::from(slot).cast::<c_void>();
    HOT.with(|hot| {
        hot.handler.set(Some(ptr));
        hot.panicked.set(false);
    });
    with_env(|env| env.events = RunEvents::default());
    HandlerGuard { _slot: PhantomData }
}

impl HandlerGuard<'_> {
    /// Uninstalls the handler and returns the events of the run and whether a callback panicked.
    pub(crate) fn finish(self) -> (RunEvents, bool) {
        let events = with_env(|env| env.events).unwrap_or_default();
        let panicked = HOT.try_with(|hot| hot.panicked.get()).unwrap_or(false);
        (events, panicked)
    }
}

impl Drop for HandlerGuard<'_> {
    fn drop(&mut self) {
        let _ = HOT.try_with(|hot| hot.handler.set(None));
    }
}

fn note_foreign_thread() {
    if CORE_LOADED.load(Ordering::Acquire) && !FOREIGN_THREAD_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "libretro callback received on a thread that does not own the core; ignored"
        );
    }
}

fn mark_panicked() {
    let _ = HOT.try_with(|hot| hot.panicked.set(true));
}

/// Runs `f` without letting a panic cross the C boundary.
fn ffi_boundary<R>(neutral: R, f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => {
            mark_panicked();
            neutral
        }
    }
}

/// Calls `f` with the installed handler; `neutral` outside `Core::run`, on a foreign thread, or after a panic.
fn with_handler<R>(neutral: R, f: impl FnOnce(&mut dyn FrontendHandler) -> R) -> R {
    let result = HOT.try_with(|hot| {
        if hot.panicked.get() {
            return None;
        }
        let Some(ptr) = hot.handler.get() else {
            if !hot.owner.get() {
                note_foreign_thread();
            }
            return None;
        };
        // SAFETY: `ptr` was created by `install_handler` from a `&mut &mut dyn FrontendHandler` that the
        // `HandlerGuard` keeps borrowed; the guard clears the slot before that borrow ends, so the pointee is
        // alive. We are on the owning thread (thread-local) and `Core::run` does not touch the handler while
        // `retro_run` executes; the handler cannot re-enter the core (`Core` is mutably borrowed), so this is
        // the only live reference to it.
        let handler = unsafe { ptr.cast::<&mut dyn FrontendHandler>().as_mut() };
        match catch_unwind(AssertUnwindSafe(|| f(&mut **handler))) {
            Ok(value) => Some(value),
            Err(_) => {
                hot.panicked.set(true);
                None
            }
        }
    });
    result.ok().flatten().unwrap_or(neutral)
}

/// Builds the frame passed by the core. Outer `None`: ignore the call; inner `None`: dupe.
///
/// # Safety
/// If `data` is a real frame pointer it must point to at least `pitch * (height - 1) + width * bpp` readable
/// bytes that stay valid for `'a` (libretro guarantees this for the duration of the callback).
unsafe fn frame_from_raw<'a>(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
    format: PixelFormat,
) -> Option<Option<VideoFrame<'a>>> {
    if data.is_null() {
        return Some(None);
    }
    if data == sys::RETRO_HW_FRAME_BUFFER_VALID || width == 0 || height == 0 {
        return None;
    }
    let row = usize::try_from(width)
        .ok()?
        .checked_mul(format.bytes_per_pixel())?;
    if pitch < row {
        return None;
    }
    let len = pitch
        .checked_mul(usize::try_from(height - 1).ok()?)?
        .checked_add(row)?;
    if isize::try_from(len).is_err() {
        return None;
    }
    // SAFETY: non-null, not the HW sentinel; size and lifetime per this function's contract; `len` was just
    // checked to be <= isize::MAX as `from_raw_parts` requires. `u8` has no alignment requirement.
    let data = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
    Some(Some(VideoFrame {
        data,
        width,
        height,
        pitch,
        format,
    }))
}

/// `retro_video_refresh_t`.
///
/// # Safety
/// Called by the core with a frame buffer valid for the duration of the call (libretro contract).
pub(crate) unsafe extern "C" fn video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    ffi_boundary((), || {
        // SAFETY: libretro guarantees the buffer covers `height` rows of `pitch` bytes (last row may be
        // shorter) until this callback returns; the frame does not escape `with_handler`.
        let Some(frame) = (unsafe { frame_from_raw(data, width, height, pitch, pixel_format()) })
        else {
            return;
        };
        with_handler((), |handler| handler.video_refresh(frame));
    });
}

/// `retro_audio_sample_t`.
pub(crate) extern "C" fn audio_sample(left: i16, right: i16) {
    ffi_boundary((), || {
        with_handler((), |handler| handler.audio_samples(&[left, right]));
    });
}

/// `retro_audio_sample_batch_t`.
///
/// # Safety
/// `data` must point to `frames * 2` interleaved samples valid for the duration of the call.
pub(crate) unsafe extern "C" fn audio_sample_batch(data: *const i16, frames: usize) -> usize {
    ffi_boundary(0, || {
        if data.is_null() || frames == 0 {
            return 0;
        }
        // The slice must span at most isize::MAX bytes (`from_raw_parts` precondition).
        let Some(len) = frames.checked_mul(2).filter(|len| {
            len.checked_mul(size_of::<i16>())
                .is_some_and(|b| isize::try_from(b).is_ok())
        }) else {
            return 0;
        };
        // SAFETY: libretro guarantees `frames` stereo frames (2 × i16 each) at `data`, which is non-null and
        // i16-aligned per the C type; the byte size was checked to be <= isize::MAX above; the slice does not
        // outlive this call.
        let samples = unsafe { std::slice::from_raw_parts(data, len) };
        with_handler((), |handler| handler.audio_samples(samples));
        frames
    })
}

/// `retro_input_poll_t`.
pub(crate) extern "C" fn input_poll() {
    ffi_boundary((), || with_handler((), |handler| handler.input_poll()));
}

/// `retro_input_state_t`.
pub(crate) extern "C" fn input_state(
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
) -> i16 {
    ffi_boundary(0, || {
        let Some(device) = InputDevice::from_raw(device) else {
            return 0;
        };
        let query = InputQuery {
            port,
            device,
            index,
            id,
        };
        with_handler(0, |handler| handler.input_state(query))
    })
}

/// `retro_environment_t`.
///
/// # Safety
/// `data` must be what `libretro.h` documents for `cmd` (possibly null), valid for the duration of the call.
pub(crate) unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    ffi_boundary(false, || {
        // SAFETY: forwarded from this function's contract.
        let handled = with_env(|env| unsafe { env.handle(cmd, data) });
        handled.unwrap_or_else(|| {
            let owner = HOT.try_with(|hot| hot.owner.get()).unwrap_or(false);
            if !owner {
                note_foreign_thread();
            }
            false
        })
    })
}

/// Splits a legacy `SET_VARIABLES` value `"Description; a|b|c"` into its description and options.
pub(crate) fn parse_variable_value(raw: &str) -> Option<(&str, Vec<&str>)> {
    let (description, options) = raw.split_once(';')?;
    let options: Vec<&str> = options.trim_start().split('|').collect();
    if options.iter().any(|o| o.is_empty()) {
        return None;
    }
    Some((description.trim(), options))
}

/// Writes `value` through `data` if non-null.
///
/// # Safety
/// `data` is null or valid for a write of `T`.
unsafe fn write_out<T>(data: *mut c_void, value: T) -> bool {
    if data.is_null() {
        return false;
    }
    // SAFETY: non-null and valid for writes of `T` per this function's contract; `write` does not read or
    // drop the previous (possibly uninitialised) value.
    unsafe { data.cast::<T>().write(value) };
    true
}

/// Reads a `T` through `data` if non-null.
///
/// # Safety
/// `data` is null or valid for a read of an initialised `T`.
unsafe fn read_in<T: Copy>(data: *const c_void) -> Option<T> {
    if data.is_null() {
        return None;
    }
    // SAFETY: non-null and valid for reads of `T` per this function's contract.
    Some(unsafe { data.cast::<T>().read() })
}

impl EnvState {
    /// # Safety
    /// `data` matches the type documented for `cmd` in `libretro.h` and is valid for the call.
    unsafe fn handle(&mut self, cmd: c_uint, data: *mut c_void) -> bool {
        match cmd {
            // For every `write_out` / `read_in` below, the pointee type is the one `libretro.h` documents for
            // the command and this function's contract guarantees validity.
            sys::RETRO_ENVIRONMENT_GET_CAN_DUPE => {
                // SAFETY: `bool *`.
                unsafe { write_out(data, true) }
            }
            sys::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => {
                // SAFETY: `const enum retro_pixel_format *`; C enums are `int`-sized.
                let Some(raw) = (unsafe { read_in::<c_int>(data) }) else {
                    return false;
                };
                let format = match u32::try_from(raw) {
                    Ok(sys::RETRO_PIXEL_FORMAT_0RGB1555) => PixelFormat::Xrgb1555,
                    Ok(sys::RETRO_PIXEL_FORMAT_XRGB8888) => PixelFormat::Xrgb8888,
                    Ok(sys::RETRO_PIXEL_FORMAT_RGB565) => PixelFormat::Rgb565,
                    _ => {
                        tracing::debug!(raw, "core requested an unknown pixel format");
                        return false;
                    }
                };
                HOT.with(|hot| hot.pixel_format.set(format));
                true
            }
            sys::RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
                // SAFETY: `const char **`.
                unsafe { write_dir(data, self.system_dir.as_deref()) }
            }
            sys::RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => {
                // SAFETY: `const char **`.
                unsafe { write_dir(data, self.save_dir.as_deref()) }
            }
            // No dedicated assets directory in phase 1: answer the system directory.
            sys::RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY => {
                // SAFETY: `const char **`.
                unsafe { write_dir(data, self.system_dir.as_deref()) }
            }
            // `false` makes the core fall back to the legacy `SET_VARIABLES` interface.
            sys::RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION => false,
            sys::RETRO_ENVIRONMENT_SET_VARIABLES => {
                // SAFETY: `const struct retro_variable *`, array terminated by a null key.
                unsafe { self.set_variables(data.cast_const().cast()) };
                true
            }
            sys::RETRO_ENVIRONMENT_GET_VARIABLE => {
                // SAFETY: `struct retro_variable *`.
                unsafe { self.get_variable(data.cast()) }
            }
            sys::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
                // SAFETY: `bool *`.
                unsafe { write_out(data, false) };
                false
            }
            sys::RETRO_ENVIRONMENT_SET_GEOMETRY => {
                // SAFETY: `const struct retro_game_geometry *`.
                let Some(g) = (unsafe { read_in::<sys::retro_game_geometry>(data) }) else {
                    return false;
                };
                // Per libretro.h, max_width / max_height are not changed by SET_GEOMETRY.
                if let Some(av) = self.av_info.as_mut() {
                    av.geometry.base_width = g.base_width;
                    av.geometry.base_height = g.base_height;
                    av.geometry.aspect_ratio = g.aspect_ratio;
                    self.events.av_info_changed = true;
                }
                true
            }
            sys::RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO => {
                // SAFETY: `const struct retro_system_av_info *`.
                let Some(av) = (unsafe { read_in::<sys::retro_system_av_info>(data) }) else {
                    return false;
                };
                self.av_info = Some(av_info_from_raw(&av));
                self.events.av_info_changed = true;
                true
            }
            sys::RETRO_ENVIRONMENT_SHUTDOWN => {
                self.events.shutdown_requested = true;
                true
            }
            sys::RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => {
                // Some cores pass NULL and only look at the return value.
                // SAFETY: `bool *` or null.
                unsafe { write_out(data, true) };
                true
            }
            sys::RETRO_ENVIRONMENT_GET_LANGUAGE => {
                // SAFETY: `unsigned *`.
                unsafe { write_out(data, sys::RETRO_LANGUAGE_ENGLISH) }
            }
            sys::RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE => {
                // Bit 0: video enabled, bit 1: audio enabled.
                // SAFETY: `int *`.
                unsafe { write_out::<c_int>(data, 0b11) }
            }
            sys::RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS
            | sys::RETRO_ENVIRONMENT_SET_CONTROLLER_INFO
            | sys::RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL
            | sys::RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS
            | sys::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME => true,
            // GET_LOG_INTERFACE needs a variadic C callback (ADR 0008); SET_HW_RENDER is phase 7.
            sys::RETRO_ENVIRONMENT_GET_LOG_INTERFACE | sys::RETRO_ENVIRONMENT_SET_HW_RENDER => {
                self.log_unhandled(cmd);
                false
            }
            _ => {
                self.log_unhandled(cmd);
                false
            }
        }
    }

    fn log_unhandled(&mut self, cmd: c_uint) {
        if !self.logged_commands.contains(&cmd) {
            self.logged_commands.push(cmd);
            tracing::debug!(
                cmd,
                base = cmd & 0xffff,
                experimental = cmd & sys::RETRO_ENVIRONMENT_EXPERIMENTAL != 0,
                "unhandled libretro environment command"
            );
        }
    }

    /// # Safety
    /// `vars` is null or points to an array of `retro_variable` terminated by a null key, whose strings are
    /// valid NUL-terminated C strings for the duration of the call.
    unsafe fn set_variables(&mut self, mut vars: *const sys::retro_variable) {
        self.variables.clear();
        if vars.is_null() {
            return;
        }
        loop {
            // SAFETY: within the null-key-terminated array per contract; we stop at the terminator.
            let var = unsafe { vars.read() };
            if var.key.is_null() {
                break;
            }
            // SAFETY: advancing inside the array; the terminator has not been reached yet.
            vars = unsafe { vars.add(1) };
            if var.value.is_null() {
                continue;
            }
            // SAFETY: both are non-null, NUL-terminated C strings valid during the call; we copy them.
            let (key, raw) = unsafe { (CStr::from_ptr(var.key), CStr::from_ptr(var.value)) };
            let key_str = key.to_string_lossy();
            let raw = raw.to_string_lossy();
            let Some((_, options)) = parse_variable_value(&raw) else {
                tracing::debug!(key = %key_str, value = %raw, "malformed core variable declaration");
                continue;
            };
            let value = match self.overrides.iter().find(|(k, _)| *k == key_str) {
                Some((_, v)) if options.contains(&v.as_str()) => v.as_str(),
                Some((_, v)) => {
                    tracing::warn!(key = %key_str, value = %v, "core option override is not an allowed value; using the default");
                    options[0]
                }
                None => options[0],
            };
            let Ok(value) = CString::new(value) else {
                continue;
            };
            self.variables.push(CoreVariable {
                key: key.to_owned(),
                value,
            });
        }
    }

    /// # Safety
    /// `var` is null or a valid `retro_variable` whose `key` is null or a valid C string.
    unsafe fn get_variable(&self, var: *mut sys::retro_variable) -> bool {
        if var.is_null() {
            return false;
        }
        // SAFETY: non-null and valid per contract.
        let key_ptr = unsafe { (*var).key };
        if key_ptr.is_null() {
            return false;
        }
        // SAFETY: non-null NUL-terminated C string per contract.
        let key = unsafe { CStr::from_ptr(key_ptr) };
        let found = self.variables.iter().find(|v| v.key.as_c_str() == key);
        let value: *const c_char = found.map_or(std::ptr::null(), |v| v.value.as_ptr());
        // SAFETY: valid for writes per contract. The string lives in `self.variables` until the next
        // SET_VARIABLES or until the core is dropped.
        unsafe { (*var).value = value };
        found.is_some()
    }
}

/// # Safety
/// `data` is null or valid for a write of `*const c_char`.
unsafe fn write_dir(data: *mut c_void, dir: Option<&CStr>) -> bool {
    match dir {
        // SAFETY: per contract; the string is owned by `EnvState` and outlives the core's use of it.
        Some(dir) => unsafe { write_out(data, dir.as_ptr()) },
        None => false,
    }
}

pub(crate) fn av_info_from_raw(av: &sys::retro_system_av_info) -> AvInfo {
    AvInfo {
        geometry: GameGeometry {
            base_width: av.geometry.base_width,
            base_height: av.geometry.base_height,
            max_width: av.geometry.max_width,
            max_height: av.geometry.max_height,
            aspect_ratio: av.geometry.aspect_ratio,
        },
        timing: SystemTiming {
            fps: av.timing.fps,
            sample_rate: av.timing.sample_rate,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn frame_from_raw_rejects_oversized_length() {
        let byte = 0u8;
        let ptr = (&raw const byte).cast::<c_void>();
        // height=2: len = isize::MAX + 2 fits in usize, so only the isize::try_from check can reject it.
        // SAFETY: the length check fails before any read of `ptr`.
        let r = unsafe { frame_from_raw(ptr, 1, 2, isize::MAX as usize, PixelFormat::Rgb565) };
        assert!(r.is_none());
    }

    fn config(variables: Vec<(String, String)>) -> CoreConfig {
        CoreConfig {
            system_dir: PathBuf::from("/tmp/sys"),
            save_dir: PathBuf::from("/tmp/saves"),
            variables,
        }
    }

    fn env(cmd: c_uint, data: *mut c_void) -> bool {
        // SAFETY: tests pass pointers of the type documented for `cmd`.
        unsafe { environment(cmd, data) }
    }

    #[derive(Default)]
    struct Recorder {
        frames: Vec<Option<(u32, u32, usize, usize)>>,
        samples: Vec<i16>,
        polls: u32,
        queries: Vec<InputQuery>,
        panic_on_poll: bool,
    }

    impl FrontendHandler for Recorder {
        fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
            self.frames
                .push(frame.map(|f| (f.width, f.height, f.pitch, f.data.len())));
        }
        fn audio_samples(&mut self, samples: &[i16]) {
            self.samples.extend_from_slice(samples);
        }
        fn input_poll(&mut self) {
            if self.panic_on_poll {
                panic!("handler panic (expected by test)");
            }
            self.polls += 1;
        }
        fn input_state(&mut self, query: InputQuery) -> i16 {
            self.queries.push(query);
            7
        }
    }

    #[test]
    fn parse_variable_value_legacy_format() {
        assert_eq!(
            parse_variable_value("Region; auto|ntsc-u|pal"),
            Some(("Region", vec!["auto", "ntsc-u", "pal"]))
        );
        assert_eq!(parse_variable_value("Lone;x"), Some(("Lone", vec!["x"])));
        assert_eq!(
            parse_variable_value("Has spaces; a b|c"),
            Some(("Has spaces", vec!["a b", "c"]))
        );
        assert_eq!(parse_variable_value("No separator"), None);
        assert_eq!(parse_variable_value("Empty; "), None);
        assert_eq!(parse_variable_value("Hole; a||b"), None);
    }

    #[test]
    fn environment_without_state_is_ignored() {
        let mut dupe = false;
        assert!(!env(
            sys::RETRO_ENVIRONMENT_GET_CAN_DUPE,
            (&raw mut dupe).cast()
        ));
        assert!(!dupe);
    }

    #[test]
    fn environment_commands() {
        let _install = install_env(&config(vec![
            ("md_region".into(), "pal".into()),
            ("md_bad".into(), "nope".into()),
        ]));

        let mut flag = false;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_CAN_DUPE,
            (&raw mut flag).cast()
        ));
        assert!(flag);

        let mut dir: *const c_char = std::ptr::null();
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY,
            (&raw mut dir).cast()
        ));
        // SAFETY: the environment just wrote a pointer to a C string owned by the installed state.
        assert_eq!(unsafe { CStr::from_ptr(dir) }, c"/tmp/sys");
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY,
            (&raw mut dir).cast()
        ));
        // SAFETY: as above.
        assert_eq!(unsafe { CStr::from_ptr(dir) }, c"/tmp/saves");

        let mut format: c_int = 1;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            (&raw mut format).cast()
        ));
        assert_eq!(pixel_format(), PixelFormat::Xrgb8888);
        format = 2;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            (&raw mut format).cast()
        ));
        assert_eq!(pixel_format(), PixelFormat::Rgb565);
        format = 9;
        assert!(!env(
            sys::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            (&raw mut format).cast()
        ));
        assert_eq!(pixel_format(), PixelFormat::Rgb565);

        let mut version: c_uint = 99;
        assert!(!env(
            sys::RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
            (&raw mut version).cast()
        ));

        let mut vars = [
            sys::retro_variable {
                key: c"md_region".as_ptr(),
                value: c"Region; auto|ntsc|pal".as_ptr(),
            },
            sys::retro_variable {
                key: c"md_bad".as_ptr(),
                value: c"Bad override; on|off".as_ptr(),
            },
            sys::retro_variable {
                key: c"md_plain".as_ptr(),
                value: c"Plain; 1x|2x".as_ptr(),
            },
            sys::retro_variable {
                key: std::ptr::null(),
                value: std::ptr::null(),
            },
        ];
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_VARIABLES,
            vars.as_mut_ptr().cast()
        ));
        let get = |key: &CStr| {
            let mut var = sys::retro_variable {
                key: key.as_ptr(),
                value: std::ptr::null(),
            };
            let ok = env(sys::RETRO_ENVIRONMENT_GET_VARIABLE, (&raw mut var).cast());
            // SAFETY: when non-null, `value` points into the installed state, alive for this test.
            (
                ok,
                (!var.value.is_null()).then(|| unsafe { CStr::from_ptr(var.value) }.to_owned()),
            )
        };
        assert_eq!(get(c"md_region"), (true, Some(c"pal".to_owned())));
        assert_eq!(get(c"md_bad"), (true, Some(c"on".to_owned())));
        assert_eq!(get(c"md_plain"), (true, Some(c"1x".to_owned())));
        assert_eq!(get(c"unknown"), (false, None));

        let mut updated = true;
        assert!(!env(
            sys::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            (&raw mut updated).cast()
        ));
        assert!(!updated);

        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_INPUT_BITMASKS,
            std::ptr::null_mut()
        ));
        let mut lang: c_uint = 42;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_LANGUAGE,
            (&raw mut lang).cast()
        ));
        assert_eq!(lang, sys::RETRO_LANGUAGE_ENGLISH);
        let mut av_enable: c_int = 0;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE,
            (&raw mut av_enable).cast()
        ));
        assert_eq!(av_enable, 3);
        let level: c_uint = 4;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL,
            (&raw const level).cast_mut().cast()
        ));
        assert!(!env(
            sys::RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
            std::ptr::null_mut()
        ));
        assert!(!env(
            sys::RETRO_ENVIRONMENT_SET_HW_RENDER,
            std::ptr::null_mut()
        ));
        assert!(!env(0xdead, std::ptr::null_mut()));
        assert!(!env(0xdead, std::ptr::null_mut()));
    }

    #[test]
    fn geometry_and_shutdown_events() {
        let _install = install_env(&config(Vec::new()));
        let mut geometry = sys::retro_game_geometry {
            base_width: 256,
            base_height: 224,
            max_width: 999,
            max_height: 999,
            aspect_ratio: 0.0,
        };
        // Without av_info yet, SET_GEOMETRY is accepted but has nothing to update.
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_GEOMETRY,
            (&raw mut geometry).cast()
        ));
        assert_eq!(av_info(), None);

        let mut av = sys::retro_system_av_info {
            geometry: sys::retro_game_geometry {
                base_width: 320,
                base_height: 224,
                max_width: 320,
                max_height: 480,
                aspect_ratio: 4.0 / 3.0,
            },
            timing: sys::retro_system_timing {
                fps: 59.92,
                sample_rate: 44100.0,
            },
        };
        set_av_info(Some(av_info_from_raw(&av)));

        let mut recorder = Recorder::default();
        let mut slot: &mut dyn FrontendHandler = &mut recorder;
        let guard = install_handler(&mut slot);
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_GEOMETRY,
            (&raw mut geometry).cast()
        ));
        assert!(env(sys::RETRO_ENVIRONMENT_SHUTDOWN, std::ptr::null_mut()));
        let (events, panicked) = guard.finish();
        assert!(!panicked);
        assert!(events.av_info_changed);
        assert!(events.shutdown_requested);
        let info = av_info().unwrap();
        assert_eq!(info.geometry.base_width, 256);
        assert_eq!(info.geometry.max_width, 320);
        assert_eq!(info.geometry.aspect_ratio, 0.0);

        av.timing.fps = 50.0;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO,
            (&raw mut av).cast()
        ));
        assert_eq!(av_info().unwrap().timing.fps, 50.0);
    }

    #[test]
    fn hot_callbacks_reach_handler_only_while_installed() {
        let _install = install_env(&config(Vec::new()));
        // 2x2 RGB565 frame with a padded pitch; last row unpadded.
        let pixels = [0u8; 8 + 4];

        // SAFETY: `pixels` holds pitch * (height - 1) + width * 2 bytes.
        unsafe { video_refresh(pixels.as_ptr().cast(), 2, 2, 8) };
        input_poll();

        let mut recorder = Recorder::default();
        {
            let mut slot: &mut dyn FrontendHandler = &mut recorder;
            let guard = install_handler(&mut slot);
            let mut format: c_int = 2;
            env(
                sys::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
                (&raw mut format).cast(),
            );
            // SAFETY: `pixels` holds pitch * (height - 1) + width * 2 bytes.
            unsafe { video_refresh(pixels.as_ptr().cast(), 2, 2, 8) };
            // SAFETY: null means dupe; nothing is read.
            unsafe { video_refresh(std::ptr::null(), 2, 2, 8) };
            // SAFETY: pitch smaller than a row is rejected before any read.
            unsafe { video_refresh(pixels.as_ptr().cast(), 8, 2, 8) };
            // SAFETY: the HW sentinel is rejected before any read.
            unsafe { video_refresh(sys::RETRO_HW_FRAME_BUFFER_VALID, 2, 2, 8) };
            audio_sample(1, -1);
            let batch = [10i16, 11, 12, 13];
            // SAFETY: `batch` holds 2 stereo frames.
            let consumed = unsafe { audio_sample_batch(batch.as_ptr(), 2) };
            assert_eq!(consumed, 2);
            input_poll();
            assert_eq!(input_state(1, sys::RETRO_DEVICE_JOYPAD, 0, 256), 7);
            assert_eq!(input_state(0, 0x7f, 0, 0), 0);
            let (_, panicked) = guard.finish();
            assert!(!panicked);
        }
        input_poll();

        assert_eq!(recorder.frames, vec![Some((2, 2, 8, 12)), None]);
        assert_eq!(recorder.samples, vec![1, -1, 10, 11, 12, 13]);
        assert_eq!(recorder.polls, 1);
        assert_eq!(
            recorder.queries,
            vec![InputQuery {
                port: 1,
                device: InputDevice::Joypad,
                index: 0,
                id: 256
            }]
        );
    }

    #[test]
    fn handler_panic_is_caught_and_reported() {
        let _install = install_env(&config(Vec::new()));
        let mut recorder = Recorder {
            panic_on_poll: true,
            ..Recorder::default()
        };
        let mut slot: &mut dyn FrontendHandler = &mut recorder;
        let guard = install_handler(&mut slot);
        input_poll();
        // After a panic the handler is no longer called.
        assert_eq!(input_state(0, sys::RETRO_DEVICE_JOYPAD, 0, 0), 0);
        let (_, panicked) = guard.finish();
        assert!(panicked);
        assert!(recorder.queries.is_empty());
    }
}
