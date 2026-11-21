use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LibretroError {
    /// Libretro callbacks are process-global: only one `Core` may be alive (ADR 0008).
    #[error("another libretro core is already loaded in this process")]
    AlreadyLoaded,
    #[error(
        "cannot load core library {path} (missing file, wrong architecture or missing dependent library?): {source}"
    )]
    Load {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("core is missing required symbol `{0}`")]
    MissingSymbol(&'static str),
    #[error("unsupported libretro API version {found} (expected {expected})")]
    ApiVersion { found: u32, expected: u32 },
    #[error("cannot read game file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// `retro_load_game` returned false. `path` is empty for `load_no_game`. `core_error`: last `RETRO_LOG_ERROR`
    /// line logged by the core during that call (« Unable to open CD BIOS: … »), at most 512 characters, control
    /// characters removed; `None` when it logged none (design v6.0 §3.1, filled by GF1).
    #[error("core rejected game {}", path.display())]
    LoadGameFailed {
        path: PathBuf,
        core_error: Option<String>,
    },
    #[error("no game is loaded")]
    NoGame,
    #[error("a game is already loaded")]
    GameAlreadyLoaded,
    /// A `FrontendHandler` method panicked inside a C callback; the panic was caught at the FFI boundary.
    #[error("frontend callback panicked during retro_run")]
    CallbackPanicked,
    /// `GameSource::Memory` given to a core that sets `need_fullpath`.
    #[error("core needs a real file path for its content")]
    NeedsFullPath,
    /// `retro_serialize_size` returned 0.
    #[error("core does not support savestates")]
    SerializeUnsupported,
    #[error("core failed to serialize its state")]
    SerializeFailed,
    /// `retro_unserialize` returned false (wrong size, other core version, corrupt data).
    #[error("core rejected the savestate")]
    UnserializeFailed,
    #[error("unknown core option `{0}`")]
    UnknownOption(String),
    #[error("invalid value `{value}` for core option `{key}`")]
    InvalidOptionValue { key: String, value: String },
    /// The core registered no disk control interface, or the call needs the extended one.
    #[error("core does not support this disk control operation")]
    DiskControlUnsupported,
    /// A disk control callback returned `false` (`&'static str` = operation name).
    #[error("core refused disk control operation `{0}`")]
    DiskControlFailed(&'static str),
    /// `Core::set_controller_device`: `id` is neither `RETRO_DEVICE_NONE`, `RETRO_DEVICE_JOYPAD` nor declared by the
    /// core for `port` (design v7.0 §3.1).
    #[error("controller device {id} is not declared by the core for port {port}")]
    UnknownControllerDevice { port: u32, id: u32 },
    #[error("core cannot start without content")]
    NoGameUnsupported,
    /// The core library stayed mapped after a previous unload and reloading it needs a private copy (ADR 0022),
    /// which could not be written or loaded (or `CoreConfig::copy_dir` is `None`). `path` is the original library.
    #[error(
        "core {path} stayed in memory after unloading and no private copy could be loaded: {source}"
    )]
    CoreCopy {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl LibretroError {
    /// `core_error` of [`LibretroError::LoadGameFailed`] when there is one: shown instead of the generic hint.
    pub fn core_error(&self) -> Option<&str> {
        match self {
            LibretroError::LoadGameFailed { core_error, .. } => core_error.as_deref(),
            _ => None,
        }
    }
}
