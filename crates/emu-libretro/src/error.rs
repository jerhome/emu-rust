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
    #[error("core rejected game {0}")]
    LoadGameFailed(PathBuf),
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
    #[error("core cannot start without content")]
    NoGameUnsupported,
}
