use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LibretroError {
    /// Libretro callbacks are process-global: only one `Core` may be alive (ADR 0008).
    #[error("another libretro core is already loaded in this process")]
    AlreadyLoaded,
    #[error("cannot load core library {path}: {source}")]
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
}
