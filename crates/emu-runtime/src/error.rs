use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    #[error(transparent)]
    Libretro(#[from] emu_libretro::LibretroError),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("PNG encoding failed: {0}")]
    Png(String),
    #[error("OpenGL setup failed: {0}")]
    Gl(String),
    #[error("cannot read archive {path}: {message}")]
    Archive { path: PathBuf, message: String },
    /// No entry of the archive has an extension the core accepts.
    #[error("no loadable content in archive {0}")]
    NoContentInArchive(PathBuf),
    #[error("content {path} is too large ({size} bytes)")]
    ContentTooLarge { path: PathBuf, size: u64 },
    #[error("invalid savestate slot {0}")]
    InvalidSlot(u8),
    #[error(transparent)]
    StateFile(#[from] crate::saves::StateFileError),
    /// A media set is empty or has more than `MAX_MEDIA_IMAGES` images.
    #[error("invalid disk set: {0}")]
    InvalidMediaSet(String),
    /// A working copy could not be completed for lack of space; the partial copy has been removed.
    #[error("disk full while writing {path}")]
    DiskFull { path: PathBuf },
    /// The running core registered no disk control interface.
    #[error("the core does not support disk swapping")]
    NoDiskControl,
}
