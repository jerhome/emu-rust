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
    /// Folder snapshot above `SnapshotLimits` (bytes, files or depth); nothing left behind.
    #[error("game folder too large for a snapshot (limit {limit})")]
    SnapshotTooLarge { limit: u64 },
    /// `snapshot_folder` cancelled; the temporary file has been removed.
    #[error("snapshot cancelled")]
    SnapshotCancelled,
    /// A file name of a game folder cannot be stored in a zip entry (control character, empty component…).
    #[error("file name cannot be stored in a snapshot: {0}")]
    UnrepresentableName(PathBuf),
    /// `ContentSpec::Folder::start` is not an 8.3 `exe` / `com` / `bat` at the root of the folder.
    #[error("invalid start program: {0}")]
    InvalidStart(String),
    /// `MediaSet::launch` refused (design v5.0 §3.2): not printable ASCII, longer than 64, line break, `:` `|` `#`
    /// with `M3uLaunch::Program`, or a launch text for a core without `m3u_launch`.
    #[error("invalid launch command: {0}")]
    InvalidLaunch(String),
}
