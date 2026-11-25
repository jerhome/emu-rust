//! Tracing setup (ADR 0023 §3): the « Journal » capture (`emu_ui::log`: ring, panel, `<data>/logs/emu-rust.log`),
//! and stderr only when someone can read it.

use std::io;
use std::path::{Path, PathBuf};

use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// The program lives in the `emu_app` library: the binaries' own targets never log.
const DEFAULT_LOG_FILTER: &str = "warn,emu_app=info,emu_ui=info,emu_runtime=info,emu_libretro=info";

/// `show_terminal` (`[app]`), a terminal on stderr, or an explicit `RUST_LOG` (a developer, a test reading
/// stderr): logs also go to stderr. Otherwise (double-click, Finder) only the file and the UI get them.
pub(crate) fn stderr_wanted(
    show_terminal: bool,
    stderr_is_terminal: bool,
    rust_log_set: bool,
) -> bool {
    show_terminal || stderr_is_terminal || rust_log_set
}

/// Window modes (Library, Run) own the log file; CLI commands never rotate or erase it.
pub(crate) fn owns_log_file(command: Option<&crate::cli::Command>) -> bool {
    matches!(
        command,
        Some(crate::cli::Command::Library(_) | crate::cli::Command::Run(_))
    )
}

static STDERR_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True once `init_tracing` installed the stderr layer.
pub(crate) fn stderr_layer_active() -> bool {
    STDERR_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn log_file_path(root: &Path) -> PathBuf {
    root.join("logs").join("emu-rust.log")
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
    STDERR_ACTIVE.store(stderr.is_some(), std::sync::atomic::Ordering::Relaxed);
    tracing_subscriber::registry()
        .with(filter)
        .with(emu_ui::log::capture_layer())
        .with(stderr)
        .init();
    let Some(root) = root else { return };
    match emu_ui::log::LogBuffer::global().attach_file(&log_file_path(root)) {
        // Another instance holds the log: the buffer already shows its own warning in the « Journal » panel.
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
        Err(e) => tracing::warn!("fichier journal indisponible : {e}"),
        Ok(()) => {}
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
    fn only_window_modes_own_the_log_file() {
        assert!(!owns_log_file(None));
        assert!(!owns_log_file(Some(&crate::cli::Command::Help)));
        assert!(!owns_log_file(Some(&crate::cli::Command::Version)));
    }

    #[test]
    fn log_file_is_under_the_data_root() {
        assert_eq!(
            log_file_path(Path::new("/d")),
            Path::new("/d").join("logs").join("emu-rust.log")
        );
    }
}
