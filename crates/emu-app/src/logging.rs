//! Tracing setup (design v7.1 point 3): the UI « Journal » buffer, a log file under the data root, and stderr
//! only when someone can read it.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

// The binary's own target is its crate name (`emu_rust`, `emu_rust_console`), not the package name.
const DEFAULT_LOG_FILTER: &str = "warn,emu_app=info,emu_rust=info,emu_rust_console=info,emu_ui=info,\
                                  emu_runtime=info,emu_libretro=info";

/// Beyond this size the previous log is moved to `emu-rust.log.1` at startup (one generation kept).
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// `show_terminal` (`[app]`), a terminal on stderr, or an explicit `RUST_LOG` (a developer, a test reading
/// stderr): logs also go to stderr. Otherwise (double-click, Finder) only the file and the UI get them.
pub(crate) fn stderr_wanted(
    show_terminal: bool,
    stderr_is_terminal: bool,
    rust_log_set: bool,
) -> bool {
    show_terminal || stderr_is_terminal || rust_log_set
}

pub(crate) fn log_file_path(root: &Path) -> PathBuf {
    root.join("logs").join("emu-rust.log")
}

/// Opens `path` for appending, owner-only on Unix, after moving an oversized previous log aside.
fn open_log_file(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        std::fs::rename(path, PathBuf::from(old))?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        options.mode(0o600);
        let file = options.open(path)?;
        // `mode` only applies on creation: an older file keeps its permissions otherwise.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    options.open(path)
}

/// `root`: data root (`None` for `--help`, `--version` or a parse error: no file). Never logs secrets itself;
/// the crates never log credentials (CLAUDE.md).
pub(crate) fn init_tracing(root: Option<&Path>, show_terminal: bool) {
    use std::io::IsTerminal as _;
    let rust_log_set = std::env::var_os("RUST_LOG").is_some_and(|v| !v.is_empty());
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
    let stderr =
        stderr_wanted(show_terminal, io::stderr().is_terminal(), rust_log_set).then(|| {
            tracing_subscriber::fmt::layer()
                .with_writer(io::stderr)
                .with_ansi(false)
        });
    let mut file_error = None;
    let file = root
        .map(log_file_path)
        .and_then(|path| match open_log_file(&path) {
            Ok(f) => Some(
                tracing_subscriber::fmt::layer()
                    .with_writer(Mutex::new(f))
                    .with_ansi(false),
            ),
            Err(e) => {
                file_error = Some(format!("{} : {e}", path.display()));
                None
            }
        });
    tracing_subscriber::registry()
        .with(filter)
        .with(emu_ui::LogBuffer::global().layer())
        .with(file)
        .with(stderr)
        .init();
    if let Some(e) = file_error {
        tracing::warn!("journal fichier indisponible : {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_only_when_someone_reads_it() {
        assert!(!stderr_wanted(false, false, false));
        assert!(stderr_wanted(true, false, false));
        assert!(stderr_wanted(false, true, false));
        assert!(stderr_wanted(false, false, true));
    }

    #[test]
    fn log_file_rotates_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = log_file_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();
        let f = open_log_file(&path).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 0);
        let old = dir.path().join("logs").join("emu-rust.log.1");
        assert_eq!(std::fs::metadata(old).unwrap().len(), MAX_LOG_BYTES + 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn small_log_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = log_file_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"abc").unwrap();
        let f = open_log_file(&path).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 3);
    }
}
