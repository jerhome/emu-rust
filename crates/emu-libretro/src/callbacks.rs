//! Process-global libretro callback plumbing (ADR 0008).
//!
//! libretro callbacks carry no user pointer, so frontend state lives in thread-locals of the thread that
//! loaded the core. `Core` is `!Send`, hence every `retro_*` call, and every callback a well-behaved core makes
//! from inside it, happens on that thread. A callback arriving on another thread finds empty thread-locals and
//! gets a neutral answer (logged once). The process-wide `CORE_LOADED` flag enforces "one core per process".
//! The VFS table answered to `GET_VFS_INTERFACE` (`vfs.rs`) is the exception: it uses no thread-local state and
//! may be called from any thread.
//!
//! Lifetimes:
//! - `EnvState` (directories, options, geometry, messages, events): installed by `EnvInstall` from `Core::load` until
//!   `Core` is dropped (after `retro_deinit`).
//! - The `FrontendHandler`: installed by `HandlerGuard` only for the duration of one `retro_run`.
//! - Keyboard and disk control function pointers registered by the core (`SET_KEYBOARD_CALLBACK`,
//!   `SET_DISK_CONTROL(_EXT)_INTERFACE`): copied into `EnvState`, valid while the library is loaded. `Core` copies
//!   them out and calls them outside any `ENV` borrow (they may re-enter the environment, e.g. `SET_MESSAGE`).

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::{CoreConfig, FrontendHandler, RunEvents};
use crate::disk::DiskControlKind;
use crate::error::LibretroError;
use crate::log;
use crate::options::{self, OptionStore};
use crate::sys;
use crate::types::{
    AvInfo, CoreMessage, CoreOption, CoreOptionCategory, FastForwardOverride, GameGeometry,
    InputDevice, InputQuery, LogLevel, PixelFormat, SerializationQuirks, SystemTiming, VideoFrame,
};
use crate::vfs;

/// Players advertised through `GET_INPUT_MAX_USERS`.
const MAX_USERS: c_uint = 2;
/// Oldest OSD messages are dropped beyond this, so a frontend that never drains them stays bounded.
const MAX_PENDING_MESSAGES: usize = 32;
/// Quirk flags this frontend understands (`SET_SERIALIZATION_QUIRKS` zeroes the others).
const KNOWN_QUIRKS: u64 = SerializationQuirks::INCOMPLETE
    | SerializationQuirks::MUST_INITIALIZE
    | SerializationQuirks::CORE_VARIABLE_SIZE
    | SerializationQuirks::FRONT_VARIABLE_SIZE
    | SerializationQuirks::SINGLE_SESSION
    | SerializationQuirks::ENDIAN_DEPENDENT
    | SerializationQuirks::PLATFORM_DEPENDENT;

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

/// Environment state owned by the frontend between `Core::load` and `Drop`.
struct EnvState {
    system_dir: Option<CString>,
    save_dir: Option<CString>,
    assets_dir: Option<CString>,
    libretro_path: Option<CString>,
    overrides: Vec<(String, String)>,
    /// `GET_VARIABLE` hands out pointers into this store: they stay alive until the core declares its options
    /// again or is dropped.
    options: OptionStore,
    update_display: Option<sys::retro_core_options_update_display_callback_t>,
    messages: Vec<CoreMessage>,
    quirks: u64,
    fast_forwarding: bool,
    av_info: Option<AvInfo>,
    events: RunEvents,
    logged_commands: Vec<c_uint>,
    keyboard: Option<sys::retro_keyboard_event_t>,
    disk: Option<DiskInterface>,
    support_no_game: bool,
    /// `CoreConfig::provide_vfs`: answer `GET_VFS_INTERFACE` with `vfs::answer`.
    provide_vfs: bool,
    /// `CoreConfig::fastforward_override`: answer `SET_FASTFORWARDING_OVERRIDE`.
    fastforward_override: bool,
    /// Last request of the core (kept across runs).
    ff_state: Option<FastForwardOverride>,
    /// A request arrived while no handler was installed (loading): reported by the next handler installation.
    ff_pending: bool,
}

/// Disk control callbacks as registered; the basic interface leaves the extended entries `None`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DiskInterface {
    pub(crate) kind: DiskControlKind,
    pub(crate) fns: sys::retro_disk_control_ext_callback,
}

/// Removes the environment state of this thread on drop.
#[derive(Debug)]
pub(crate) struct EnvInstall {
    _not_send: PhantomData<*const ()>,
}

pub(crate) fn install_env(config: &CoreConfig, core_path: &Path) -> EnvInstall {
    let state = EnvState {
        system_dir: path_to_cstring(&config.system_dir, "system_dir"),
        save_dir: path_to_cstring(&config.save_dir, "save_dir"),
        assets_dir: path_to_cstring(&config.assets_dir, "assets_dir"),
        libretro_path: path_to_cstring(core_path, "core path"),
        overrides: config.variables.clone(),
        options: OptionStore::default(),
        update_display: None,
        messages: Vec::new(),
        quirks: 0,
        fast_forwarding: false,
        av_info: None,
        events: RunEvents::default(),
        logged_commands: Vec::new(),
        keyboard: None,
        disk: None,
        support_no_game: false,
        provide_vfs: config.provide_vfs,
        fastforward_override: config.fastforward_override,
        ff_state: None,
        ff_pending: false,
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
///
/// Paths that are not valid UTF-8 are logged (`label` names the path). On Windows they are converted lossily,
/// because libretro expects UTF-8 there (not the ANSI code page); on Unix the raw bytes are passed unchanged.
pub(crate) fn path_to_cstring(path: &Path, label: &str) -> Option<CString> {
    if path.to_str().is_none() {
        tracing::warn!(
            path = %path.display(),
            "{label} is not valid UTF-8{}",
            if cfg!(windows) { ": converted lossily, the core may not find it" } else { "" }
        );
    }
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
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

pub(crate) fn serialization_quirks() -> SerializationQuirks {
    SerializationQuirks(with_env(|env| env.quirks).unwrap_or(0))
}

pub(crate) fn set_fast_forwarding(enabled: bool) {
    with_env(|env| env.fast_forwarding = enabled);
}

pub(crate) fn fastforward_override() -> Option<FastForwardOverride> {
    with_env(|env| env.ff_state).flatten()
}

pub(crate) fn take_messages() -> Vec<CoreMessage> {
    with_env(|env| std::mem::take(&mut env.messages)).unwrap_or_default()
}

/// The core's `SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK`, to be called *outside* `with_env` (it re-enters the
/// environment with `SET_CORE_OPTIONS_DISPLAY`).
pub(crate) fn update_display_callback() -> Option<sys::retro_core_options_update_display_callback_t>
{
    with_env(|env| env.update_display).flatten()
}

/// To be called outside `with_env` (see the module documentation).
pub(crate) fn keyboard_callback() -> Option<sys::retro_keyboard_event_t> {
    with_env(|env| env.keyboard).flatten()
}

/// To be called outside `with_env` (see the module documentation).
pub(crate) fn disk_interface() -> Option<DiskInterface> {
    with_env(|env| env.disk).flatten()
}

pub(crate) fn supports_no_game() -> bool {
    with_env(|env| env.support_no_game).unwrap_or(false)
}

pub(crate) fn options() -> Vec<CoreOption> {
    with_env(|env| env.options.snapshot()).unwrap_or_default()
}

pub(crate) fn option_categories() -> Vec<CoreOptionCategory> {
    with_env(|env| env.options.categories()).unwrap_or_default()
}

pub(crate) fn set_option(key: &str, value: &str) -> Result<(), LibretroError> {
    with_env(|env| env.options.set(key, value))
        .unwrap_or_else(|| Err(LibretroError::UnknownOption(key.to_owned())))
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
    with_env(|env| {
        env.events = RunEvents::default();
        if std::mem::take(&mut env.ff_pending) {
            env.events.fastforward = env.ff_state;
        }
    });
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
            sys::RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY => {
                // SAFETY: `const char **`.
                unsafe { write_dir(data, self.assets_dir.as_deref()) }
            }
            sys::RETRO_ENVIRONMENT_GET_LIBRETRO_PATH => {
                // SAFETY: `const char **`.
                unsafe { write_dir(data, self.libretro_path.as_deref()) }
            }
            sys::RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION => {
                // SAFETY: `unsigned *`.
                unsafe { write_out::<c_uint>(data, 2) }
            }
            sys::RETRO_ENVIRONMENT_SET_VARIABLES => {
                // SAFETY: `const struct retro_variable *`, array terminated by a null key.
                let decls = unsafe { options::parse_legacy(data.cast_const().cast()) };
                self.declare_options(Vec::new(), decls);
                true
            }
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS => {
                // SAFETY: `const struct retro_core_option_definition *`, terminated by a null key.
                let decls = unsafe { options::parse_v1(data.cast_const().cast()) };
                self.declare_options(Vec::new(), decls);
                true
            }
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL => {
                // SAFETY: `const struct retro_core_options_intl *`; only the English `us` array is read.
                let Some(intl) = (unsafe { read_in::<sys::retro_core_options_intl>(data) }) else {
                    return false;
                };
                // SAFETY: `us` is a definition array terminated by a null key (libretro.h).
                let decls = unsafe { options::parse_v1(intl.us) };
                self.declare_options(Vec::new(), decls);
                true
            }
            // libretro.h: with options version >= 2 the return value means "categories supported".
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 => {
                // SAFETY: `const struct retro_core_options_v2 *`.
                let (categories, decls) = unsafe { options::parse_v2(data.cast_const().cast()) };
                self.declare_options(categories, decls);
                true
            }
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL => {
                // SAFETY: `const struct retro_core_options_v2_intl *`; only the English `us` set is read.
                let Some(intl) = (unsafe { read_in::<sys::retro_core_options_v2_intl>(data) })
                else {
                    return false;
                };
                // SAFETY: `us` is null or a valid `retro_core_options_v2` (libretro.h).
                let (categories, decls) = unsafe { options::parse_v2(intl.us) };
                self.declare_options(categories, decls);
                true
            }
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY => {
                // SAFETY: `const struct retro_core_option_display *`.
                let Some(display) = (unsafe { read_in::<sys::retro_core_option_display>(data) })
                else {
                    return false;
                };
                if display.key.is_null() {
                    return false;
                }
                // SAFETY: non-null NUL-terminated key valid for the call (libretro.h).
                let key = unsafe { CStr::from_ptr(display.key) };
                if self.options.set_visible(key, display.visible != 0) {
                    self.events.options_changed = true;
                }
                true
            }
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK => {
                // SAFETY: `const struct retro_core_options_update_display_callback *`; null unregisters.
                let cb =
                    unsafe { read_in::<sys::retro_core_options_update_display_callback>(data) };
                self.update_display = cb.and_then(|cb| cb.callback);
                true
            }
            sys::RETRO_ENVIRONMENT_SET_VARIABLE => {
                // libretro.h: a null `data` queries support.
                if data.is_null() {
                    return true;
                }
                // SAFETY: `const struct retro_variable *`, non-null.
                let Some(var) = (unsafe { read_in::<sys::retro_variable>(data) }) else {
                    return false;
                };
                if var.key.is_null() || var.value.is_null() {
                    return false;
                }
                // SAFETY: non-null NUL-terminated strings valid for the call (libretro.h).
                let (key, value) = unsafe { (CStr::from_ptr(var.key), CStr::from_ptr(var.value)) };
                let ok = self.options.set_from_core(key, value);
                if ok {
                    self.events.options_changed = true;
                }
                ok
            }
            sys::RETRO_ENVIRONMENT_GET_VARIABLE => {
                // SAFETY: `struct retro_variable *`.
                unsafe { self.get_variable(data.cast()) }
            }
            sys::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
                let updated = self.options.take_updated();
                // SAFETY: `bool *`.
                unsafe { write_out(data, updated) }
            }
            sys::RETRO_ENVIRONMENT_GET_LOG_INTERFACE => {
                // SAFETY: `struct retro_log_callback *`.
                unsafe { write_out(data, log::log_callback()) }
            }
            sys::RETRO_ENVIRONMENT_SET_MESSAGE => {
                // SAFETY: `const struct retro_message *`.
                let Some(message) = (unsafe { read_in::<sys::retro_message>(data) }) else {
                    return false;
                };
                // SAFETY: `msg` is null or a NUL-terminated string valid for the call.
                let Some(text) = (unsafe { copy_message(message.msg) }) else {
                    return false;
                };
                let fps = self
                    .av_info
                    .map(|av| av.timing.fps)
                    .filter(|fps| fps.is_finite() && *fps > 0.0)
                    .unwrap_or(60.0);
                let duration =
                    std::time::Duration::try_from_secs_f64(f64::from(message.frames) / fps)
                        .unwrap_or_default();
                self.push_message(CoreMessage {
                    text,
                    duration,
                    level: LogLevel::Info,
                });
                true
            }
            sys::RETRO_ENVIRONMENT_SET_MESSAGE_EXT => {
                // SAFETY: `const struct retro_message_ext *`.
                let Some(message) = (unsafe { read_in::<sys::retro_message_ext>(data) }) else {
                    return false;
                };
                // SAFETY: `msg` is null or a NUL-terminated string valid for the call.
                let Some(text) = (unsafe { copy_message(message.msg) }) else {
                    return false;
                };
                let level = log::level_from_raw(message.level);
                // Unknown targets are treated as OSD.
                let (to_log, to_osd) = match message.target {
                    sys::RETRO_MESSAGE_TARGET_ALL => (true, true),
                    sys::RETRO_MESSAGE_TARGET_LOG => (true, false),
                    sys::RETRO_MESSAGE_TARGET_OSD => (false, true),
                    _ => (false, true),
                };
                if to_log {
                    log::emit(level, &text, false);
                }
                if to_osd {
                    self.push_message(CoreMessage {
                        text,
                        duration: std::time::Duration::from_millis(u64::from(message.duration)),
                        level,
                    });
                }
                true
            }
            sys::RETRO_ENVIRONMENT_GET_MESSAGE_INTERFACE_VERSION => {
                // SAFETY: `unsigned *`.
                unsafe { write_out::<c_uint>(data, 1) }
            }
            sys::RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS => {
                // In/out `uint64_t *`: unknown flags are zeroed; variable sizes are supported because
                // `Core::serialize` asks for the size on every call.
                // SAFETY: `uint64_t *`.
                let Some(mut quirks) = (unsafe { read_in::<u64>(data) }) else {
                    return false;
                };
                quirks &= KNOWN_QUIRKS;
                if quirks & SerializationQuirks::CORE_VARIABLE_SIZE != 0 {
                    quirks |= SerializationQuirks::FRONT_VARIABLE_SIZE;
                }
                self.quirks = quirks;
                // SAFETY: as above, valid for writes.
                unsafe { write_out(data, quirks) }
            }
            sys::RETRO_ENVIRONMENT_GET_FASTFORWARDING => {
                // SAFETY: `bool *`.
                unsafe { write_out(data, self.fast_forwarding) }
            }
            sys::RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS => {
                // SAFETY: `unsigned *`.
                unsafe { write_out(data, MAX_USERS) }
            }
            sys::RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES => {
                // SAFETY: `uint64_t *`, bit n = RETRO_DEVICE n.
                unsafe {
                    write_out::<u64>(
                        data,
                        (1 << sys::RETRO_DEVICE_JOYPAD)
                            | (1 << sys::RETRO_DEVICE_MOUSE)
                            | (1 << sys::RETRO_DEVICE_KEYBOARD),
                    )
                }
            }
            // No overscan preference: the core keeps its own default (it is not told to crop or not).
            sys::RETRO_ENVIRONMENT_GET_OVERSCAN => false,
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
            sys::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME => {
                // SAFETY: `const bool *`; read as `u8` so a byte other than 0/1 from a buggy core is not
                // an invalid Rust `bool` (undefined behaviour).
                let Some(supported) = (unsafe { read_in::<u8>(data) }) else {
                    return false;
                };
                self.support_no_game = supported != 0;
                true
            }
            sys::RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK => {
                // SAFETY: `const struct retro_keyboard_callback *`. The function pointer is copied; it points
                // into the core library, which stays loaded as long as this state exists.
                let Some(cb) = (unsafe { read_in::<sys::retro_keyboard_callback>(data) }) else {
                    return false;
                };
                self.keyboard = cb.callback;
                true
            }
            sys::RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE => {
                // libretro.h: NULL deregisters.
                // SAFETY: `const struct retro_disk_control_callback *`; the pointers are copied (see above).
                self.disk =
                    unsafe { read_in::<sys::retro_disk_control_callback>(data) }.map(|cb| {
                        DiskInterface {
                            kind: DiskControlKind::Basic,
                            fns: sys::retro_disk_control_ext_callback {
                                set_eject_state: cb.set_eject_state,
                                get_eject_state: cb.get_eject_state,
                                get_image_index: cb.get_image_index,
                                set_image_index: cb.set_image_index,
                                get_num_images: cb.get_num_images,
                                replace_image_index: cb.replace_image_index,
                                add_image_index: cb.add_image_index,
                                set_initial_image: None,
                                get_image_path: None,
                                get_image_label: None,
                            },
                        }
                    });
                true
            }
            sys::RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE => {
                // libretro.h: NULL deregisters.
                // SAFETY: `const struct retro_disk_control_ext_callback *`; the pointers are copied (see above).
                self.disk =
                    unsafe { read_in::<sys::retro_disk_control_ext_callback>(data) }.map(|fns| {
                        DiskInterface {
                            kind: DiskControlKind::Extended,
                            fns,
                        }
                    });
                true
            }
            sys::RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION => {
                // SAFETY: `unsigned *`.
                unsafe { write_out::<c_uint>(data, 1) }
            }
            sys::RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS
            | sys::RETRO_ENVIRONMENT_SET_CONTROLLER_INFO
            | sys::RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL
            | sys::RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS
            | sys::RETRO_ENVIRONMENT_SET_MEMORY_MAPS
            | sys::RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO
            | sys::RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE => true,
            // Hardware rendering is phase 7.
            sys::RETRO_ENVIRONMENT_SET_HW_RENDER => {
                self.log_unhandled(cmd);
                false
            }
            // Opt-in per core (ADR 0018). The table is thread-safe and needs no `EnvState`.
            sys::RETRO_ENVIRONMENT_GET_VFS_INTERFACE if self.provide_vfs => {
                // SAFETY: `struct retro_vfs_interface_info *` (in/out), valid for the call per this function's
                // contract.
                unsafe { vfs::answer(data) }
            }
            // Opt-in per core (design v5.0 §3.1). `NULL` is the support probe.
            sys::RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE if self.fastforward_override => {
                // SAFETY: `const struct retro_fastforwarding_override *` or `NULL`; read once, copied.
                if let Some(raw) = unsafe { read_in::<sys::retro_fastforwarding_override>(data) } {
                    let request = FastForwardOverride {
                        ratio: if raw.ratio.is_finite() && raw.ratio >= 0.0 {
                            raw.ratio
                        } else {
                            0.0
                        },
                        fastforward: raw.fastforward,
                        notification: raw.notification,
                        inhibit_toggle: raw.inhibit_toggle,
                    };
                    self.ff_state = Some(request);
                    self.events.fastforward = Some(request);
                    if HOT
                        .try_with(|hot| hot.handler.get().is_none())
                        .unwrap_or(true)
                    {
                        self.ff_pending = true;
                    }
                }
                true
            }
            // Optional interfaces some cores (puae) probe at startup: declined on purpose, so they are not
            // reported as unhandled. The core falls back to its own implementation.
            sys::RETRO_ENVIRONMENT_GET_PERF_INTERFACE
            | sys::RETRO_ENVIRONMENT_GET_VFS_INTERFACE
            | sys::RETRO_ENVIRONMENT_GET_LED_INTERFACE
            | sys::RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE => {
                self.log_declined(cmd);
                false
            }
            // Rumble, frame-time / audio callbacks…
            _ => {
                self.log_unhandled(cmd);
                false
            }
        }
    }

    fn log_declined(&mut self, cmd: c_uint) {
        if !self.logged_commands.contains(&cmd) {
            self.logged_commands.push(cmd);
            tracing::debug!(
                cmd,
                base = cmd & 0xffff,
                "libretro environment command declined (not supported by this frontend)"
            );
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

    fn declare_options(
        &mut self,
        categories: Vec<CoreOptionCategory>,
        decls: Vec<options::OptionDecl>,
    ) {
        self.options.declare(categories, decls, &self.overrides);
        self.events.options_changed = true;
    }

    fn push_message(&mut self, message: CoreMessage) {
        if self.messages.len() >= MAX_PENDING_MESSAGES {
            self.messages.remove(0);
        }
        self.messages.push(message);
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
        let found = self.options.get(key);
        // SAFETY: valid for writes per contract. The string lives in `self.options` until the core declares
        // its options again or is dropped.
        unsafe { (*var).value = found.unwrap_or(std::ptr::null()) };
        found.is_some()
    }
}

/// Copies a message text; `None` for null.
///
/// # Safety
/// `msg` is null or a NUL-terminated string valid for the call.
unsafe fn copy_message(msg: *const c_char) -> Option<String> {
    if msg.is_null() {
        return None;
    }
    // SAFETY: per contract; copied immediately.
    Some(
        unsafe { CStr::from_ptr(msg) }
            .to_string_lossy()
            .into_owned(),
    )
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
            assets_dir: PathBuf::from("/tmp/assets"),
            variables,
            provide_vfs: false,
            copy_dir: None,
            fastforward_override: false,
            variable_state_size: false,
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
        let _install = install_env(
            &config(vec![
                ("md_region".into(), "pal".into()),
                ("md_bad".into(), "nope".into()),
            ]),
            Path::new("/cores/x_libretro.so"),
        );

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
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
            (&raw mut version).cast()
        ));
        assert_eq!(version, 2);
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY,
            (&raw mut dir).cast()
        ));
        // SAFETY: as above.
        assert_eq!(unsafe { CStr::from_ptr(dir) }, c"/tmp/assets");
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_LIBRETRO_PATH,
            (&raw mut dir).cast()
        ));
        // SAFETY: as above.
        assert_eq!(unsafe { CStr::from_ptr(dir) }, c"/cores/x_libretro.so");

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
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            (&raw mut updated).cast()
        ));
        assert!(!updated);
        set_option("md_plain", "2x").unwrap();
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            (&raw mut updated).cast()
        ));
        assert!(updated);
        assert_eq!(get(c"md_plain"), (true, Some(c"2x".to_owned())));
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            (&raw mut updated).cast()
        ));
        assert!(!updated);
        assert!(matches!(
            set_option("md_plain", "3x"),
            Err(LibretroError::InvalidOptionValue { .. })
        ));
        let opts = options();
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].label, "Region");
        assert_eq!(opts[0].default, "auto");
        assert_eq!(opts[0].current, "pal");
        assert!(option_categories().is_empty());

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
        let mut log_cb = sys::retro_log_callback {
            log: crate::log::emu_libretro_log_printf,
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
            (&raw mut log_cb).cast()
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
    fn fastforward_override_opt_in() {
        let mut cfg = config(Vec::new());
        cfg.fastforward_override = true;
        let _install = install_env(&cfg, Path::new("/cores/x_libretro.so"));
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE,
            std::ptr::null_mut()
        ));
        assert_eq!(fastforward_override(), None);
        // Request outside a run (loading): kept and reported by the next handler installation.
        let mut req = sys::retro_fastforwarding_override {
            ratio: f32::NAN,
            fastforward: true,
            notification: true,
            inhibit_toggle: false,
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE,
            (&raw mut req).cast()
        ));
        let expected = FastForwardOverride {
            ratio: 0.0,
            fastforward: true,
            notification: true,
            inhibit_toggle: false,
        };
        assert_eq!(fastforward_override(), Some(expected));
        let mut handler = Recorder::default();
        let mut slot: &mut dyn FrontendHandler = &mut handler;
        let guard = install_handler(&mut slot);
        assert_eq!(guard.finish().0.fastforward, Some(expected));
        // Second run: nothing new, state kept.
        let mut slot: &mut dyn FrontendHandler = &mut handler;
        let guard = install_handler(&mut slot);
        req.ratio = -3.0;
        req.fastforward = false;
        req.notification = false;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE,
            (&raw mut req).cast()
        ));
        let events = guard.finish().0;
        assert_eq!(events.fastforward.map(|f| f.fastforward), Some(false));
        req.ratio = 4.0;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE,
            (&raw mut req).cast()
        ));
        assert_eq!(fastforward_override().map(|f| f.ratio), Some(4.0));
        let mut slot: &mut dyn FrontendHandler = &mut handler;
        assert_eq!(
            install_handler(&mut slot).finish().0.fastforward,
            Some(fastforward_override().unwrap()),
            "request outside a run after a run is pending again"
        );
    }

    #[test]
    fn phase2_environment_commands() {
        let _install = install_env(&config(Vec::new()), Path::new("/cores/x_libretro.so"));
        set_av_info(Some(AvInfo {
            geometry: GameGeometry {
                base_width: 1,
                base_height: 1,
                max_width: 1,
                max_height: 1,
                aspect_ratio: 0.0,
            },
            timing: SystemTiming {
                fps: 50.0,
                sample_rate: 48000.0,
            },
        }));

        let mut ff = true;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_FASTFORWARDING,
            (&raw mut ff).cast()
        ));
        assert!(!ff);
        set_fast_forwarding(true);
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_FASTFORWARDING,
            (&raw mut ff).cast()
        ));
        assert!(ff);

        let mut users: c_uint = 0;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS,
            (&raw mut users).cast()
        ));
        assert_eq!(users, 2);
        let mut caps: u64 = 0;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES,
            (&raw mut caps).cast()
        ));
        assert_eq!(
            caps,
            (1 << sys::RETRO_DEVICE_JOYPAD)
                | (1 << sys::RETRO_DEVICE_MOUSE)
                | (1 << sys::RETRO_DEVICE_KEYBOARD)
        );
        let mut overscan = true;
        assert!(!env(
            sys::RETRO_ENVIRONMENT_GET_OVERSCAN,
            (&raw mut overscan).cast()
        ));
        let mut scratch = [0u8; 64];
        for cmd in [
            sys::RETRO_ENVIRONMENT_GET_PERF_INTERFACE,
            sys::RETRO_ENVIRONMENT_GET_VFS_INTERFACE,
            sys::RETRO_ENVIRONMENT_GET_LED_INTERFACE,
            sys::RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE, // `false` case: opt-in off
        ] {
            assert!(!env(cmd, scratch.as_mut_ptr().cast()), "{cmd}");
            assert!(!env(cmd, std::ptr::null_mut()), "{cmd} null");
        }
        assert!(scratch.iter().all(|b| *b == 0), "declined: nothing written");
        let mut msg_version: c_uint = 0;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_MESSAGE_INTERFACE_VERSION,
            (&raw mut msg_version).cast()
        ));
        assert_eq!(msg_version, 1);
        for cmd in [
            sys::RETRO_ENVIRONMENT_SET_MEMORY_MAPS,
            sys::RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO,
            sys::RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE,
        ] {
            assert!(env(cmd, std::ptr::null_mut()));
        }

        assert_eq!(serialization_quirks(), SerializationQuirks::default());
        let mut quirks: u64 = SerializationQuirks::MUST_INITIALIZE
            | SerializationQuirks::CORE_VARIABLE_SIZE
            | (1 << 40);
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS,
            (&raw mut quirks).cast()
        ));
        assert_eq!(
            quirks,
            SerializationQuirks::MUST_INITIALIZE
                | SerializationQuirks::CORE_VARIABLE_SIZE
                | SerializationQuirks::FRONT_VARIABLE_SIZE
        );
        assert_eq!(serialization_quirks().0, quirks);

        let mut message = sys::retro_message {
            msg: c"hello".as_ptr(),
            frames: 100,
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_MESSAGE,
            (&raw mut message).cast()
        ));
        let mut ext = sys::retro_message_ext {
            msg: c"to the log only".as_ptr(),
            duration: 1500,
            priority: 1,
            level: sys::RETRO_LOG_WARN,
            target: sys::RETRO_MESSAGE_TARGET_LOG,
            type_: 0,
            progress: -1,
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_MESSAGE_EXT,
            (&raw mut ext).cast()
        ));
        ext.msg = c"osd".as_ptr();
        ext.target = sys::RETRO_MESSAGE_TARGET_OSD;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_MESSAGE_EXT,
            (&raw mut ext).cast()
        ));
        message.msg = std::ptr::null();
        assert!(!env(
            sys::RETRO_ENVIRONMENT_SET_MESSAGE,
            (&raw mut message).cast()
        ));
        assert_eq!(
            take_messages(),
            vec![
                CoreMessage {
                    text: "hello".into(),
                    duration: std::time::Duration::from_secs(2),
                    level: LogLevel::Info,
                },
                CoreMessage {
                    text: "osd".into(),
                    duration: std::time::Duration::from_millis(1500),
                    level: LogLevel::Warn,
                },
            ]
        );
        assert!(take_messages().is_empty());
        message.msg = c"spam".as_ptr();
        for _ in 0..100 {
            env(
                sys::RETRO_ENVIRONMENT_SET_MESSAGE,
                (&raw mut message).cast(),
            );
        }
        assert_eq!(take_messages().len(), MAX_PENDING_MESSAGES);
    }

    extern "C" fn hide_opt_b() -> u8 {
        let display = sys::retro_core_option_display {
            key: c"opt_b".as_ptr(),
            visible: 0,
        };
        // SAFETY: `const struct retro_core_option_display *`.
        u8::from(unsafe {
            environment(
                sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY,
                (&raw const display).cast_mut().cast(),
            )
        })
    }

    #[test]
    fn v2_options_through_environment() {
        let _install = install_env(
            &config(vec![("opt_a".into(), "red".into())]),
            Path::new("/cores/x_libretro.so"),
        );
        let mut values = [sys::retro_core_option_value {
            value: std::ptr::null(),
            label: std::ptr::null(),
        }; sys::RETRO_NUM_CORE_OPTION_VALUES_MAX];
        values[0].value = c"red".as_ptr();
        values[1].value = c"green".as_ptr();
        let def = |key: &'static CStr| sys::retro_core_option_v2_definition {
            key: key.as_ptr(),
            desc: c"Desc".as_ptr(),
            desc_categorized: c"Cat desc".as_ptr(),
            info: std::ptr::null(),
            info_categorized: std::ptr::null(),
            category_key: c"video".as_ptr(),
            values,
            default_value: c"green".as_ptr(),
        };
        let defs = [
            def(c"opt_a"),
            def(c"opt_b"),
            sys::retro_core_option_v2_definition {
                key: std::ptr::null(),
                ..def(c"x")
            },
        ];
        let cats = [
            sys::retro_core_option_v2_category {
                key: c"video".as_ptr(),
                desc: c"Video".as_ptr(),
                info: std::ptr::null(),
            },
            sys::retro_core_option_v2_category {
                key: std::ptr::null(),
                desc: std::ptr::null(),
                info: std::ptr::null(),
            },
        ];
        let us = sys::retro_core_options_v2 {
            categories: cats.as_ptr(),
            definitions: defs.as_ptr(),
        };
        let mut intl = sys::retro_core_options_v2_intl {
            us: &raw const us,
            local: std::ptr::null(),
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL,
            (&raw mut intl).cast()
        ));
        let opts = options();
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].current, "red");
        assert_eq!(opts[1].current, "green");
        assert_eq!(opts[0].label, "Cat desc");
        assert_eq!(opts[0].category.as_deref(), Some("video"));
        assert_eq!(option_categories().len(), 1);

        let mut cb = sys::retro_core_options_update_display_callback {
            callback: Some(hide_opt_b),
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK,
            (&raw mut cb).cast()
        ));
        let callback = update_display_callback().unwrap();
        // SAFETY: test callback, no preconditions; called outside any environment borrow.
        assert!(unsafe { callback() } != 0);
        assert!(!options()[1].visible);

        let mut recorder = Recorder::default();
        let mut slot: &mut dyn FrontendHandler = &mut recorder;
        let guard = install_handler(&mut slot);
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_VARIABLE,
            std::ptr::null_mut()
        ));
        let mut var = sys::retro_variable {
            key: c"opt_a".as_ptr(),
            value: c"green".as_ptr(),
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_VARIABLE,
            (&raw mut var).cast()
        ));
        var.value = c"blue".as_ptr();
        assert!(!env(
            sys::RETRO_ENVIRONMENT_SET_VARIABLE,
            (&raw mut var).cast()
        ));
        let (events, _) = guard.finish();
        assert!(events.options_changed);
        assert_eq!(options()[0].current, "green");

        cb.callback = None;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK,
            (&raw mut cb).cast()
        ));
        assert!(update_display_callback().is_none());
    }

    #[test]
    fn geometry_and_shutdown_events() {
        let _install = install_env(&config(Vec::new()), Path::new("/cores/x_libretro.so"));
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
        let _install = install_env(&config(Vec::new()), Path::new("/cores/x_libretro.so"));
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
        let _install = install_env(&config(Vec::new()), Path::new("/cores/x_libretro.so"));
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

    extern "C" fn test_key(_down: bool, _keycode: c_uint, _character: u32, _modifiers: u16) {}
    extern "C" fn test_eject(_ejected: bool) -> u8 {
        1
    }
    extern "C" fn test_num_images() -> c_uint {
        3
    }
    extern "C" fn test_initial(_index: c_uint, _path: *const c_char) -> u8 {
        1
    }

    #[test]
    fn computer_environment_commands() {
        let _install = install_env(&config(Vec::new()), Path::new("/cores/x_libretro.so"));

        // SET_SUPPORT_NO_GAME stores the value.
        assert!(!supports_no_game());
        let mut flag = true;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
            (&raw mut flag).cast()
        ));
        assert!(supports_no_game());
        flag = false;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
            (&raw mut flag).cast()
        ));
        assert!(!supports_no_game());
        assert!(!env(
            sys::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
            std::ptr::null_mut()
        ));

        // Keyboard callback: registered, then unregistered by a NULL callback.
        assert!(keyboard_callback().is_none());
        let mut kb = sys::retro_keyboard_callback {
            callback: Some(test_key),
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK,
            (&raw mut kb).cast()
        ));
        assert!(keyboard_callback().is_some());
        kb.callback = None;
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK,
            (&raw mut kb).cast()
        ));
        assert!(keyboard_callback().is_none());
        assert!(!env(
            sys::RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK,
            std::ptr::null_mut()
        ));

        // Disk control: version 1, basic then extended, NULL deregisters.
        let mut version: c_uint = 0;
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION,
            (&raw mut version).cast()
        ));
        assert_eq!(version, 1);
        assert!(disk_interface().is_none());
        let mut basic = sys::retro_disk_control_callback {
            set_eject_state: Some(test_eject),
            get_eject_state: None,
            get_image_index: None,
            set_image_index: None,
            get_num_images: Some(test_num_images),
            replace_image_index: None,
            add_image_index: None,
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE,
            (&raw mut basic).cast()
        ));
        let disk = disk_interface().expect("basic interface");
        assert_eq!(disk.kind, DiskControlKind::Basic);
        assert!(disk.fns.set_eject_state.is_some() && disk.fns.get_num_images.is_some());
        assert!(disk.fns.set_initial_image.is_none());
        // SAFETY: `test_num_images` takes no argument and has no precondition.
        assert_eq!(unsafe { disk.fns.get_num_images.unwrap()() }, 3);

        let mut ext = sys::retro_disk_control_ext_callback {
            set_eject_state: Some(test_eject),
            get_eject_state: None,
            get_image_index: None,
            set_image_index: None,
            get_num_images: None,
            replace_image_index: None,
            add_image_index: None,
            set_initial_image: Some(test_initial),
            get_image_path: None,
            get_image_label: None,
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE,
            (&raw mut ext).cast()
        ));
        let disk = disk_interface().expect("extended interface");
        assert_eq!(disk.kind, DiskControlKind::Extended);
        assert!(disk.fns.set_initial_image.is_some() && disk.fns.get_num_images.is_none());
        assert!(env(
            sys::RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE,
            std::ptr::null_mut()
        ));
        assert!(disk_interface().is_none());
    }

    #[test]
    fn keyboard_and_mouse_queries_reach_handler() {
        let _install = install_env(&config(Vec::new()), Path::new("/cores/x_libretro.so"));
        let mut recorder = Recorder::default();
        {
            let mut slot: &mut dyn FrontendHandler = &mut recorder;
            let guard = install_handler(&mut slot);
            // RETROK_a, then mouse X and left button.
            assert_eq!(input_state(0, sys::RETRO_DEVICE_KEYBOARD, 0, 97), 7);
            assert_eq!(
                input_state(0, sys::RETRO_DEVICE_MOUSE, 0, crate::mouse_id::X),
                7
            );
            assert_eq!(
                input_state(0, sys::RETRO_DEVICE_MOUSE, 0, crate::mouse_id::LEFT),
                7
            );
            let (_, panicked) = guard.finish();
            assert!(!panicked);
        }
        let devices: Vec<_> = recorder.queries.iter().map(|q| (q.device, q.id)).collect();
        assert_eq!(
            devices,
            vec![
                (InputDevice::Keyboard, 97),
                (InputDevice::Mouse, crate::mouse_id::X),
                (InputDevice::Mouse, crate::mouse_id::LEFT),
            ]
        );
    }

    #[test]
    fn vfs_interface_is_opt_in() {
        let mut cfg = config(Vec::new());
        cfg.provide_vfs = true;
        let _install = install_env(&cfg, Path::new("/cores/dosbox_pure_libretro.so"));
        let mut info = sys::retro_vfs_interface_info {
            required_interface_version: 3,
            iface: std::ptr::null_mut(),
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_VFS_INTERFACE,
            (&raw mut info).cast()
        ));
        assert_eq!(info.required_interface_version, 3);
        assert!(!info.iface.is_null());
        // An older version is served with the v3 table (prefix-compatible layout).
        let mut old = sys::retro_vfs_interface_info {
            required_interface_version: 1,
            iface: std::ptr::null_mut(),
        };
        assert!(env(
            sys::RETRO_ENVIRONMENT_GET_VFS_INTERFACE,
            (&raw mut old).cast()
        ));
        assert_eq!((old.required_interface_version, old.iface), (3, info.iface));
        // Newer than 3: refused, nothing written.
        let mut newer = sys::retro_vfs_interface_info {
            required_interface_version: 4,
            iface: std::ptr::null_mut(),
        };
        assert!(!env(
            sys::RETRO_ENVIRONMENT_GET_VFS_INTERFACE,
            (&raw mut newer).cast()
        ));
        assert_eq!(newer.required_interface_version, 4);
        assert!(newer.iface.is_null());
        assert!(!env(
            sys::RETRO_ENVIRONMENT_GET_VFS_INTERFACE,
            std::ptr::null_mut()
        ));
        // The base number without the EXPERIMENTAL bit is not the libretro.h command.
        assert!(!env(45, (&raw mut newer).cast()));
    }
}
