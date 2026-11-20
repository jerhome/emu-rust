use std::path::PathBuf;

/// Messages are user-facing French. They never quote file contents (DAT lines are reported by number only).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LibraryError {
    #[error("{path} : {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("base de la bibliothèque : {0}")]
    Database(String),
    /// The file was written by a newer emu-rust: never modified.
    #[error("base de la bibliothèque en version {found} (cette version gère jusqu'à {supported})")]
    NewerSchema { found: u32, supported: u32 },
    #[error("fichier DAT invalide (ligne {line}) : {message}")]
    Dat { line: usize, message: String },
    #[error("{path} : fichier trop gros")]
    TooLarge { path: PathBuf },
    #[error("{path} : archive illisible : {message}")]
    Archive { path: PathBuf, message: String },
    #[error("système inconnu : {0}")]
    UnknownSystem(String),
    #[error("opération annulée")]
    Cancelled,
}
