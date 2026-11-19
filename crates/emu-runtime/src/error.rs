use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    #[error(transparent)]
    Libretro(#[from] emu_libretro::LibretroError),
    #[error("cannot write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("PNG encoding failed: {0}")]
    Png(String),
    #[error("OpenGL setup failed: {0}")]
    Gl(String),
}
