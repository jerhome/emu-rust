use std::ffi::{CStr, CString, c_char};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use libloading::Library;

use crate::callbacks::{self, EnvInstall, ProcessGuard};
use crate::error::LibretroError;
use crate::resident::{self, CopyFile};
use crate::sys;
use crate::types::{
    AvInfo, CoreMessage, CoreOption, CoreOptionCategory, FastForwardOverride, GameSource,
    InputQuery, MemoryKind, PixelFormat, SerializationQuirks, SystemInfo, VideoFrame,
};

/// Frontend-side configuration answered through environment callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreConfig {
    /// `GET_SYSTEM_DIRECTORY` (BIOS files).
    pub system_dir: PathBuf,
    /// `GET_SAVE_DIRECTORY`: where the core writes files it manages itself (emu-runtime passes
    /// `<saves>/<core_id>/`). SRAM exposed through `MemoryKind::SaveRam` is persisted by the frontend.
    pub save_dir: PathBuf,
    /// `GET_CORE_ASSETS_DIRECTORY` (phase 1 answered `system_dir`).
    pub assets_dir: PathBuf,
    /// Core option overrides (`key`, `value`); options not listed use the default declared by the core.
    pub variables: Vec<(String, String)>,
    /// Answer `GET_VFS_INTERFACE` (45) with the frontend's VFS v3 table (`cores.toml` `vfs = true`, ADR 0018).
    /// `false`: 45 is refused, as in 3.0.
    pub provide_vfs: bool,
    /// Where a private copy of the library is written when the original stays resident after unloading
    /// (ADR 0022). `None`: reloading a resident core fails with `LibretroError::CoreCopy`.
    pub copy_dir: Option<PathBuf>,
    /// Answer `SET_FASTFORWARDING_OVERRIDE` (64): `true` to a `NULL` probe, requests stored and reported by
    /// `RunEvents::fastforward` (`cores.toml` `fastforward_override`, design v5.0 §3.1). `false`: 64 is declined
    /// (4.0 behaviour).
    pub fastforward_override: bool,
    /// `unserialize` accepts a state shorter than the current `serialize_size()` even without the
    /// `CORE_VARIABLE_SIZE` quirk (VICE: the size depends on the inserted media; `cores.toml`
    /// `variable_state_size`). `false`: 4.0 behaviour. Safety note: with `true` the length guard is
    /// skipped, so memory safety relies on the core reading at most `size` bytes (or failing cleanly)
    /// on a short buffer; enable only for cores known to behave so.
    pub variable_state_size: bool,
}

/// Receives the per-frame callbacks while `Core::run` executes. Called on the thread that owns the `Core`.
pub trait FrontendHandler {
    /// `None` means the core duplicated the previous frame (`GET_CAN_DUPE` answers `true`).
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>);
    /// Interleaved stereo `i16` (L, R, L, R…). Single-sample callbacks are forwarded as a 2-sample slice.
    fn audio_samples(&mut self, samples: &[i16]);
    fn input_poll(&mut self);
    /// Only called for known devices; unknown devices read as 0 without reaching the handler.
    fn input_state(&mut self, query: InputQuery) -> i16;
}

/// Environment-driven events observed during one `Core::run`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct RunEvents {
    /// `SET_GEOMETRY` or `SET_SYSTEM_AV_INFO` was received; re-read `Core::av_info`.
    pub av_info_changed: bool,
    /// `RETRO_ENVIRONMENT_SHUTDOWN` was received.
    pub shutdown_requested: bool,
    /// Option list, values or visibility changed (`SET_VARIABLE`, `SET_CORE_OPTIONS_DISPLAY`, late
    /// `SET_CORE_OPTIONS*`): re-read `Core::options`.
    pub options_changed: bool,
    /// Last `SET_FASTFORWARDING_OVERRIDE` request seen during this run (or since the previous run, for a request
    /// sent while loading). Only with `CoreConfig::fastforward_override`.
    pub fastforward: Option<FastForwardOverride>,
}

impl RunEvents {
    /// Field-wise "or".
    fn merged(self, other: Self) -> Self {
        Self {
            av_info_changed: self.av_info_changed || other.av_info_changed,
            shutdown_requested: self.shutdown_requested || other.shutdown_requested,
            options_changed: self.options_changed || other.options_changed,
            // The later request wins.
            fastforward: other.fastforward.or(self.fastforward),
        }
    }
}

/// A loaded libretro core. At most one per process; bound to the thread that loaded it (ADR 0008).
///
/// Drop order: `retro_unload_game` (if a game is loaded), `retro_deinit`, library unload, then the
/// process-wide guard is released.
///
/// A library that stays mapped after unloading (ADR 0022) is reloaded from a private copy written under
/// `CoreConfig::copy_dir`; the first load of a library in a process is always direct.
pub struct Core {
    api: sys::CoreApi,
    /// `Some` until `Drop`; the function pointers in `api` are valid only while it is loaded.
    library: Option<Library>,
    /// Environment state for this thread; removed after `retro_deinit`.
    env: Option<EnvInstall>,
    system_info: SystemInfo,
    av_info: Option<AvInfo>,
    pixel_format: PixelFormat,
    game: Option<LoadedGame>,
    /// `retro_run` calls since the game was loaded (`MUST_INITIALIZE` quirk).
    frames_run: u64,
    /// Events raised during `reset_with`, reported by the next `run` (installing a handler clears them).
    pending_events: RunEvents,
    /// `CoreConfig::variable_state_size`.
    variable_state_size: bool,
    /// Paths handed to the disk control interface (`replace_image_index`, `set_initial_image`). libretro.h does
    /// not say whether the core copies them, so they are kept until `retro_unload_game` / drop.
    disk_paths: Vec<CString>,
    /// Registry key of the library given to `load` (ADR 0022); journals and `GET_LIBRETRO_PATH` use the
    /// original path, never the copy.
    origin: PathBuf,
    /// File actually opened: the original, or a private copy when the original stayed resident.
    loaded_from: PathBuf,
    /// Windows only: the private copy, deleted after unloading if its image is gone (Unix deletes it right
    /// after `dlopen`).
    copy: Option<CopyFile>,
    // Declared last so it is released after everything above.
    _guard: ProcessGuard,
    // Implementation fields are private to emu-libretro; this marker makes `Core` !Send + !Sync.
    _not_send: PhantomData<*const ()>,
}

/// Buffers handed to `retro_load_game`; cores may keep pointers to them until `retro_unload_game`.
struct LoadedGame {
    /// `None` for `load_no_game`.
    _path: Option<CString>,
    _data: Option<Vec<u8>>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("system_info", &self.system_info)
            .field("av_info", &self.av_info)
            .field("pixel_format", &self.pixel_format)
            .field("game_loaded", &self.game.is_some())
            .finish_non_exhaustive()
    }
}

/// Resolves `name` and copies the function pointer out of the library.
///
/// # Safety
/// `T` must be the function pointer type that `libretro.h` declares for `name`.
unsafe fn symbol<T: Copy>(library: &Library, name: &'static CStr) -> Result<T, LibretroError> {
    // SAFETY: `T` matches the C declaration per this function's contract. The copied pointer is only used
    // while `library` stays loaded (`Core` keeps it until `Drop`, after the last call).
    unsafe { library.get::<T>(name) }
        .map(|sym| *sym)
        .map_err(|_| LibretroError::MissingSymbol(name.to_str().unwrap_or("?")))
}

fn resolve_api(library: &Library) -> Result<sys::CoreApi, LibretroError> {
    // SAFETY: for every `symbol` call below, the type of each field is the prototype declared in `libretro.h`
    // for the named symbol.
    unsafe {
        Ok(sys::CoreApi {
            set_environment: symbol(library, c"retro_set_environment")?,
            set_video_refresh: symbol(library, c"retro_set_video_refresh")?,
            set_audio_sample: symbol(library, c"retro_set_audio_sample")?,
            set_audio_sample_batch: symbol(library, c"retro_set_audio_sample_batch")?,
            set_input_poll: symbol(library, c"retro_set_input_poll")?,
            set_input_state: symbol(library, c"retro_set_input_state")?,
            init: symbol(library, c"retro_init")?,
            deinit: symbol(library, c"retro_deinit")?,
            api_version: symbol(library, c"retro_api_version")?,
            get_system_info: symbol(library, c"retro_get_system_info")?,
            get_system_av_info: symbol(library, c"retro_get_system_av_info")?,
            set_controller_port_device: symbol(library, c"retro_set_controller_port_device")?,
            reset: symbol(library, c"retro_reset")?,
            run: symbol(library, c"retro_run")?,
            load_game: symbol(library, c"retro_load_game")?,
            unload_game: symbol(library, c"retro_unload_game")?,
            serialize_size: symbol(library, c"retro_serialize_size")?,
            serialize: symbol(library, c"retro_serialize")?,
            unserialize: symbol(library, c"retro_unserialize")?,
            get_memory_data: symbol(library, c"retro_get_memory_data")?,
            get_memory_size: symbol(library, c"retro_get_memory_size")?,
        })
    }
}

/// Copies a possibly-null C string.
///
/// # Safety
/// `ptr` is null or a valid NUL-terminated string.
unsafe fn owned_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: non-null and NUL-terminated per contract; copied immediately.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// # Safety
/// The string fields of `raw` are null or valid NUL-terminated C strings.
unsafe fn system_info_from_raw(raw: &sys::retro_system_info) -> SystemInfo {
    // SAFETY: per this function's contract; the strings are copied.
    let (library_name, library_version, extensions) = unsafe {
        (
            owned_string(raw.library_name),
            owned_string(raw.library_version),
            owned_string(raw.valid_extensions),
        )
    };
    SystemInfo {
        library_name,
        library_version,
        valid_extensions: extensions
            .split('|')
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(str::to_ascii_lowercase)
            .collect(),
        need_fullpath: raw.need_fullpath != 0,
        block_extract: raw.block_extract != 0,
    }
}

/// Writes and loads a private copy of `path` (ADR 0022). Returns the library, the copy's path and, on Windows,
/// the copy to delete once its image is gone (on Unix the file is deleted as soon as `dlopen` succeeded).
fn open_private_copy(
    path: &Path,
    copy_dir: Option<&Path>,
) -> Result<(Library, PathBuf, Option<CopyFile>), LibretroError> {
    let err = |source| LibretroError::CoreCopy {
        path: path.to_path_buf(),
        source,
    };
    let dir = copy_dir.ok_or_else(|| {
        err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no directory configured for private core copies",
        ))
    })?;
    let copy_path = resident::write_private_copy(path, dir).map_err(err)?;
    let copy = CopyFile::new(copy_path.clone());
    let library = resident::load_copy(&copy_path).map_err(err)?;
    tracing::debug!(
        core = %path.display(),
        copy = %copy_path.display(),
        "loaded private copy of a resident core library"
    );
    #[cfg(unix)]
    {
        // The image stays mapped after unlink; the inode cannot be reused while it is.
        drop(copy);
        Ok((library, copy_path, None))
    }
    #[cfg(not(unix))]
    Ok((library, copy_path, Some(copy)))
}

impl Core {
    /// Opens the library, checks `retro_api_version`, sets the environment, calls `retro_init`, then installs the
    /// AV/input callbacks (RetroArch order; some cores, e.g. Mesen, crash if they are set before `retro_init`).
    pub fn load(path: &Path, config: CoreConfig) -> Result<Self, LibretroError> {
        let guard = ProcessGuard::acquire()?;
        let origin = resident::origin_key(path);
        // Declared before `library` so that, on an early error, the library is closed before the copy is removed.
        let mut copy = None;
        let mut loaded_from = path.to_path_buf();
        let library = if resident::is_resident(&origin) {
            // Never fall back to the resident image: its static state survived retro_deinit (ADR 0022).
            let (library, copy_path, kept) = open_private_copy(path, config.copy_dir.as_deref())?;
            copy = kept;
            loaded_from = copy_path;
            library
        } else {
            // SAFETY: loading a libretro core runs its initialisers; cores are trusted native code chosen by the
            // user (ADR 0001). No other core is loaded (guard held), so no global callback state is shared.
            unsafe { Library::new(path) }.map_err(|source| LibretroError::Load {
                path: path.to_path_buf(),
                source: Box::new(source),
            })?
        };
        let api = resolve_api(&library)?;

        // SAFETY: `api` was resolved from `library`, which is alive; retro_api_version has no preconditions.
        let version = unsafe { (api.api_version)() };
        if version != sys::RETRO_API_VERSION {
            return Err(LibretroError::ApiVersion {
                found: version,
                expected: sys::RETRO_API_VERSION,
            });
        }

        // Cores may call the environment from inside retro_set_environment, so the state goes first.
        let env = callbacks::install_env(&config, path);
        // SAFETY: `api` is valid while `library` lives. The callbacks are `'static` functions that tolerate
        // being called at any time (they answer neutrally when no handler is installed). Order follows
        // RetroArch: libretro.h only requires retro_set_environment before retro_init; the other setters
        // come after it because some cores (Mesen) store them in objects created by retro_init and crash
        // otherwise. They still precede retro_load_game and the first retro_run.
        unsafe {
            (api.set_environment)(callbacks::environment);
            (api.init)();
            (api.set_video_refresh)(callbacks::video_refresh);
            (api.set_audio_sample)(callbacks::audio_sample);
            (api.set_audio_sample_batch)(callbacks::audio_sample_batch);
            (api.set_input_poll)(callbacks::input_poll);
            (api.set_input_state)(callbacks::input_state);
        }

        let mut raw = sys::retro_system_info {
            library_name: std::ptr::null(),
            library_version: std::ptr::null(),
            valid_extensions: std::ptr::null(),
            need_fullpath: 0,
            block_extract: 0,
        };
        // SAFETY: `raw` is a valid, writable `retro_system_info`; the core fills it with static strings.
        unsafe { (api.get_system_info)(&raw mut raw) };
        // SAFETY: libretro requires the core to fill `raw` with null or static NUL-terminated strings.
        let system_info = unsafe { system_info_from_raw(&raw) };
        tracing::debug!(
            name = %system_info.library_name,
            version = %system_info.library_version,
            "libretro core loaded"
        );

        Ok(Core {
            api,
            library: Some(library),
            env: Some(env),
            system_info,
            av_info: None,
            pixel_format: callbacks::pixel_format(),
            game: None,
            frames_run: 0,
            pending_events: RunEvents::default(),
            variable_state_size: config.variable_state_size,
            disk_paths: Vec::new(),
            origin,
            loaded_from,
            copy,
            _guard: guard,
            _not_send: PhantomData,
        })
    }

    pub fn system_info(&self) -> &SystemInfo {
        &self.system_info
    }

    /// Calls `retro_load_game` then `retro_get_system_av_info`. When `need_fullpath` is false the file is read
    /// into memory and the buffer is kept alive until the game is unloaded. No archive extraction (see
    /// `load_game_from` with `GameSource::Memory`).
    pub fn load_game(&mut self, rom: &Path) -> Result<AvInfo, LibretroError> {
        self.load_game_from(GameSource::Path(rom.to_path_buf()))
    }

    /// `None` until a game is loaded; updated by `SET_GEOMETRY` / `SET_SYSTEM_AV_INFO`.
    pub fn av_info(&self) -> Option<&AvInfo> {
        self.av_info.as_ref()
    }

    /// Format of the frames passed to `FrontendHandler::video_refresh`.
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// Runs exactly one `retro_run` with `handler` installed as the callback target.
    pub fn run(&mut self, handler: &mut dyn FrontendHandler) -> Result<RunEvents, LibretroError> {
        if self.game.is_none() {
            return Err(LibretroError::NoGame);
        }
        let mut slot: &mut dyn FrontendHandler = handler;
        let installed = callbacks::install_handler(&mut slot);
        // SAFETY: a game is loaded and we are on the loading thread (`Core` is !Send). Callbacks reach
        // `handler` through the guard, which outlives this call.
        unsafe { (self.api.run)() };
        let (events, panicked) = installed.finish();
        let events = std::mem::take(&mut self.pending_events).merged(events);
        self.frames_run = self.frames_run.saturating_add(1);

        if self.av_info.is_some() {
            self.av_info = callbacks::av_info();
        }
        self.pixel_format = callbacks::pixel_format();
        if panicked {
            return Err(LibretroError::CallbackPanicked);
        }
        Ok(events)
    }

    /// Same as `load_game` for any `GameSource` (`load_game(p)` = `load_game_from(GameSource::Path(p))`).
    /// `Memory` keeps the buffer alive until `unload_game`, like a file read into memory.
    pub fn load_game_from(&mut self, game: GameSource) -> Result<AvInfo, LibretroError> {
        if self.game.is_some() {
            return Err(LibretroError::GameAlreadyLoaded);
        }
        let (rom, data) = match game {
            GameSource::Path(path) => {
                let data = if self.system_info.need_fullpath {
                    None
                } else {
                    Some(std::fs::read(&path).map_err(|source| LibretroError::Io {
                        path: path.clone(),
                        source,
                    })?)
                };
                (path, data)
            }
            GameSource::Memory { .. } if self.system_info.need_fullpath => {
                return Err(LibretroError::NeedsFullPath);
            }
            GameSource::Memory { path, data } => (path, Some(data)),
        };
        let path =
            callbacks::path_to_cstring(&rom, "game path").ok_or_else(|| LibretroError::Io {
                path: rom.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "path contains a NUL byte",
                ),
            })?;
        let info = sys::retro_game_info {
            path: path.as_ptr(),
            data: data
                .as_ref()
                .map_or(std::ptr::null(), |d| d.as_ptr().cast()),
            size: data.as_ref().map_or(0, Vec::len),
            meta: std::ptr::null(),
        };
        // SAFETY: `info` and the buffers it points to are valid for the call; `path` and `data` are moved into
        // `self.game` (heap contents do not move) and kept until retro_unload_game.
        let loaded = unsafe { (self.api.load_game)(&raw const info) } != 0;
        if !loaded {
            return Err(LibretroError::LoadGameFailed(rom));
        }
        Ok(self.after_load(LoadedGame {
            _path: Some(path),
            _data: data,
        }))
    }

    /// `retro_load_game(NULL)`: boots the machine without content (GEM desktop for Atari ST).
    /// [`LibretroError::NoGameUnsupported`] when the core did not send `SET_SUPPORT_NO_GAME` with `true`.
    pub fn load_no_game(&mut self) -> Result<AvInfo, LibretroError> {
        if self.game.is_some() {
            return Err(LibretroError::GameAlreadyLoaded);
        }
        if !self.supports_no_game() {
            return Err(LibretroError::NoGameUnsupported);
        }
        // SAFETY: libretro.h allows a NULL `game` once the core announced SET_SUPPORT_NO_GAME (checked above);
        // owning thread.
        let loaded = unsafe { (self.api.load_game)(std::ptr::null()) } != 0;
        if !loaded {
            return Err(LibretroError::LoadGameFailed(PathBuf::new()));
        }
        Ok(self.after_load(LoadedGame {
            _path: None,
            _data: None,
        }))
    }

    /// `SET_SUPPORT_NO_GAME` was received with `true`.
    pub fn supports_no_game(&self) -> bool {
        callbacks::supports_no_game()
    }

    pub(crate) fn game_loaded(&self) -> bool {
        self.game.is_some()
    }

    /// Keeps `path` alive until the game is unloaded (or the core dropped) and returns its address, which stays
    /// valid that long (the `CString` heap buffer does not move when the vector grows).
    pub(crate) fn retain_disk_path(&mut self, path: CString) -> *const c_char {
        // Deduplicated: repeated replace_image calls with the same path keep a single allocation.
        if let Some(existing) = self.disk_paths.iter().find(|p| **p == path) {
            return existing.as_ptr();
        }
        let ptr = path.as_ptr();
        self.disk_paths.push(path);
        ptr
    }

    /// Common tail of a successful `retro_load_game`.
    fn after_load(&mut self, game: LoadedGame) -> AvInfo {
        self.game = Some(game);
        self.frames_run = 0;

        let mut raw = sys::retro_system_av_info::default();
        // SAFETY: a game is loaded; `raw` is a valid, writable `retro_system_av_info`.
        unsafe { (self.api.get_system_av_info)(&raw mut raw) };
        let av_info = callbacks::av_info_from_raw(&raw);
        callbacks::set_av_info(Some(av_info));
        self.av_info = Some(av_info);
        self.pixel_format = callbacks::pixel_format();

        for port in 0..2 {
            // SAFETY: valid after retro_load_game; RETRO_DEVICE_JOYPAD is supported by every core.
            unsafe { (self.api.set_controller_port_device)(port, sys::RETRO_DEVICE_JOYPAD) };
        }
        av_info
    }

    /// `retro_serialize_size`; 0 when no game is loaded or savestates are unsupported. May change between
    /// frames for cores with `CORE_VARIABLE_SIZE`.
    pub fn serialize_size(&self) -> usize {
        if self.game.is_none() {
            return 0;
        }
        // SAFETY: a game is loaded; owning thread (`Core` is !Send); no preconditions otherwise.
        unsafe { (self.api.serialize_size)() }
    }

    /// `MUST_INITIALIZE`: no (un)serialization before the first `retro_run`.
    fn serialization_allowed(&self) -> bool {
        self.frames_run > 0
            || !self
                .serialization_quirks()
                .contains(SerializationQuirks::MUST_INITIALIZE)
    }

    /// Largest state accepted from a core; a bigger `retro_serialize_size` is treated as a bug in the core.
    const MAX_SERIALIZE_SIZE: usize = 256 * 1024 * 1024;

    /// Replaces `out` with the serialized state (`out` is resized to `serialize_size`; its capacity is
    /// reused, so a caller keeping the buffer does not allocate per call).
    pub fn serialize(&mut self, out: &mut Vec<u8>) -> Result<(), LibretroError> {
        if self.game.is_none() {
            return Err(LibretroError::NoGame);
        }
        if !self.serialization_allowed() {
            return Err(LibretroError::SerializeFailed);
        }
        let size = self.serialize_size();
        if size == 0 {
            return Err(LibretroError::SerializeUnsupported);
        }
        if size > Self::MAX_SERIALIZE_SIZE {
            return Err(LibretroError::SerializeFailed);
        }
        out.clear();
        out.resize(size, 0);
        // SAFETY: `out` holds exactly `size` writable bytes; the core does not keep the pointer after returning
        // (libretro contract). Owning thread, game loaded.
        let ok = unsafe { (self.api.serialize)(out.as_mut_ptr().cast(), size) } != 0;
        if !ok {
            out.clear();
            return Err(LibretroError::SerializeFailed);
        }
        Ok(())
    }

    /// `retro_unserialize`. Errors: `NoGame`, `SerializeUnsupported`, `UnserializeFailed`.
    pub fn unserialize(&mut self, data: &[u8]) -> Result<(), LibretroError> {
        if self.game.is_none() {
            return Err(LibretroError::NoGame);
        }
        if self.serialize_size() == 0 {
            return Err(LibretroError::SerializeUnsupported);
        }
        if !self.serialization_allowed() {
            return Err(LibretroError::UnserializeFailed);
        }
        // A fixed-size core reads `serialize_size()` bytes whatever `size` says.
        if !self.variable_state_size
            && !self
                .serialization_quirks()
                .contains(SerializationQuirks::CORE_VARIABLE_SIZE)
            && data.len() < self.serialize_size()
        {
            return Err(LibretroError::UnserializeFailed);
        }
        // SAFETY: `data` is readable for `data.len()` bytes during the call; the core copies what it needs
        // (libretro contract). With `variable_state_size` (or CORE_VARIABLE_SIZE) the length guard above is
        // skipped: soundness then relies on the core reading at most `size` bytes. Owning thread, game loaded.
        let ok = unsafe { (self.api.unserialize)(data.as_ptr().cast(), data.len()) } != 0;
        if ok {
            Ok(())
        } else {
            Err(LibretroError::UnserializeFailed)
        }
    }

    /// Flags from `SET_SERIALIZATION_QUIRKS` (empty if never sent).
    pub fn serialization_quirks(&self) -> SerializationQuirks {
        callbacks::serialization_quirks()
    }

    /// Pointer and length of a core memory region, `None` if absent or empty.
    fn memory_raw(&self, kind: MemoryKind) -> Option<(*mut u8, usize)> {
        self.game.as_ref()?;
        let id = match kind {
            MemoryKind::SaveRam => sys::RETRO_MEMORY_SAVE_RAM,
            MemoryKind::Rtc => sys::RETRO_MEMORY_RTC,
            MemoryKind::SystemRam => sys::RETRO_MEMORY_SYSTEM_RAM,
            MemoryKind::VideoRam => sys::RETRO_MEMORY_VIDEO_RAM,
        };
        // SAFETY: a game is loaded; owning thread; both functions accept any id.
        let (data, size) = unsafe {
            (
                (self.api.get_memory_data)(id),
                (self.api.get_memory_size)(id),
            )
        };
        if data.is_null() || size == 0 || isize::try_from(size).is_err() {
            return None;
        }
        Some((data.cast(), size))
    }

    /// Core-owned memory region; `None` if no game is loaded or the core reports a null pointer / size 0.
    /// Re-queried on every call (cores may move the buffer between frames).
    pub fn memory(&self, kind: MemoryKind) -> Option<&[u8]> {
        let (data, size) = self.memory_raw(kind)?;
        // SAFETY: libretro guarantees `size` bytes at `data` while the game stays loaded. The slice borrows
        // `self`, so no `run`, `reset`, `unserialize` or `unload_game` (all `&mut self`) can move or free the
        // buffer while it lives; size <= isize::MAX was checked; `u8` has no alignment requirement.
        Some(unsafe { std::slice::from_raw_parts(data, size) })
    }

    /// Mutable view, used to restore SRAM right after `load_game` (before the first `run`).
    pub fn memory_mut(&mut self, kind: MemoryKind) -> Option<&mut [u8]> {
        let (data, size) = self.memory_raw(kind)?;
        // SAFETY: as in `memory`; the exclusive borrow of `self` makes this the only Rust reference to the
        // region, and the core does not run while it lives.
        Some(unsafe { std::slice::from_raw_parts_mut(data, size) })
    }

    /// Snapshot of the options declared by the core (legacy, v1 or v2), in declaration order. Calls the
    /// core's `SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK` first if it registered one.
    pub fn options(&mut self) -> Vec<CoreOption> {
        if let Some(update_display) = callbacks::update_display_callback() {
            // SAFETY: registered by this core, whose library is loaded; called on the owning thread and outside
            // any environment borrow (it re-enters the environment with SET_CORE_OPTIONS_DISPLAY).
            // The returned value is ignored (u8 on the FFI side: any byte is valid).
            unsafe { update_display() };
        }
        callbacks::options()
    }

    /// v2 categories, in declaration order (empty for legacy / v1 cores).
    pub fn option_categories(&self) -> Vec<CoreOptionCategory> {
        callbacks::option_categories()
    }

    /// Validates `value` against the declared values, stores it and makes the next `GET_VARIABLE_UPDATE`
    /// return `true`. Takes effect when the core next reads it (usually during `run`).
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), LibretroError> {
        callbacks::set_option(key, value)
    }

    /// Value answered to `GET_FASTFORWARDING`.
    pub fn set_fast_forwarding(&mut self, enabled: bool) {
        callbacks::set_fast_forwarding(enabled);
    }

    /// Current `SET_FASTFORWARDING_OVERRIDE` state of the core (last request, including one sent outside
    /// `run`); `None` if it never sent one or `CoreConfig::fastforward_override` is off (design v5.0 §3.1, CF1).
    pub fn fastforward_override(&self) -> Option<FastForwardOverride> {
        callbacks::fastforward_override()
    }

    /// Drains the OSD messages received since the last call (`SET_MESSAGE`, `SET_MESSAGE_EXT`).
    pub fn take_messages(&mut self) -> Vec<CoreMessage> {
        callbacks::take_messages()
    }

    /// `retro_reset` with `handler` installed for its duration, exactly as in [`Core::run`] (same reentrancy
    /// guard, a `panic` of the handler is contained, handler removed on every path): input polled by the core
    /// inside `retro_reset` (dosbox_pure reads Shift to force its start menu) reaches `handler`. No-op without a
    /// game. [`Core::reset`] is unchanged (no handler: input reads as 0).
    ///
    /// Environment events raised during the reset (geometry, options…) are reported by the next [`Core::run`].
    /// A `panic` of `handler` is logged; the following `run` starts with a clean handler state.
    pub fn reset_with(&mut self, handler: &mut dyn FrontendHandler) {
        if self.game.is_none() {
            return;
        }
        let mut slot: &mut dyn FrontendHandler = handler;
        let installed = callbacks::install_handler(&mut slot);
        // SAFETY: a game is loaded and we are on the loading thread (`Core` is !Send). Callbacks reach `handler`
        // through the guard, which outlives this call and uninstalls it on drop, unwinding included.
        unsafe { (self.api.reset)() };
        let (events, panicked) = installed.finish();
        self.pending_events = self.pending_events.merged(events);
        if self.av_info.is_some() {
            self.av_info = callbacks::av_info();
        }
        self.pixel_format = callbacks::pixel_format();
        if panicked {
            tracing::warn!(
                "frontend handler panicked during retro_reset; later callbacks of the reset ignored"
            );
        }
    }

    pub fn reset(&mut self) {
        if self.game.is_some() {
            // SAFETY: a game is loaded; owning thread.
            unsafe { (self.api.reset)() };
        }
    }

    /// No-op if no game is loaded.
    pub fn unload_game(&mut self) {
        if self.game.is_none() {
            return;
        }
        // SAFETY: a game is loaded; owning thread. The game buffers are freed only afterwards.
        unsafe { (self.api.unload_game)() };
        self.game = None;
        self.disk_paths.clear();
        self.frames_run = 0;
        self.pending_events = RunEvents::default();
        self.av_info = None;
        callbacks::set_av_info(None);
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.unload_game();
        // SAFETY: retro_init succeeded in `load`; the library is still loaded and the environment state is
        // still installed (cores may call the environment from retro_deinit).
        unsafe { (self.api.deinit)() };
        if let Some(library) = self.library.take()
            && let Err(err) = library.close()
        {
            tracing::warn!(%err, "failed to unload libretro core");
        }
        if resident::still_loaded(&self.loaded_from) {
            if resident::mark_resident(&self.origin) {
                tracing::info!(
                    core = %self.origin.display(),
                    "core library stayed in memory after unloading; later loads in this session use a private copy"
                );
            }
            if let Some(copy) = self.copy.take() {
                copy.keep();
            }
        } else {
            self.copy = None;
        }
        self.env = None;
        // `_guard` is released after this body returns.
    }
}
