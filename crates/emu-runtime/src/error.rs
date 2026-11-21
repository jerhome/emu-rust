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
    /// The core's disk list is full: remove an added disk first.
    #[error("at most {max} disk images: remove an added disk first")]
    DiskListFull { max: usize },
    /// The running core registered no disk control interface.
    #[error("the core does not support disk swapping")]
    NoDiskControl,
    /// `Disks::forget_added` on the inserted disk: eject or insert another one first.
    #[error("the disk is inserted: eject it first")]
    DiskInserted,
    /// A removal outside what `remove_orphan_copies` / `forget_added` may delete (referenced copy, manifest,
    /// link, not a plain file name, folder that is not a `*.disks` of the core's save folder). Nothing deleted.
    #[error("refused to delete {path}: {reason}")]
    RemovalRefused { path: PathBuf, reason: String },
}
