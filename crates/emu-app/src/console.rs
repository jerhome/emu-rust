//! Windows « Afficher le terminal » (design v7.1 point 3): the GUI build (`windows_subsystem = "windows"`) has
//! no console and `unsafe` (`AllocConsole`) is not allowed here, so it starts the console build shipped next to
//! it, once (`EMU_RELAUNCHED`), and exits.

use std::path::{Path, PathBuf};

/// Set on the relaunched process: never relaunch twice.
#[cfg(all(windows, not(feature = "console")))]
pub(crate) const RELAUNCHED_ENV: &str = "EMU_RELAUNCHED";

pub(crate) const CONSOLE_EXE: &str = "emu-rust-console.exe";

/// What the GUI build does at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relaunch {
    /// Keep running in this process.
    Stay,
    /// Start the console build with the same arguments, then exit.
    Console,
    /// `show_terminal` but no console build next to this exe: keep running, warn.
    Missing,
}

pub(crate) fn relaunch_decision(
    show_terminal: bool,
    already_relaunched: bool,
    console_exe_exists: bool,
) -> Relaunch {
    match (show_terminal && !already_relaunched, console_exe_exists) {
        (false, _) => Relaunch::Stay,
        (true, true) => Relaunch::Console,
        (true, false) => Relaunch::Missing,
    }
}

/// The console build expected next to `current_exe`.
pub(crate) fn console_exe_beside(current_exe: &Path) -> Option<PathBuf> {
    current_exe.parent().map(|d| d.join(CONSOLE_EXE))
}

/// `Ok(true)`: the console build was started, this process must exit. `Err`: message to log once tracing is up.
#[cfg(all(windows, not(feature = "console")))]
pub(crate) fn relaunch_in_console(show_terminal: bool) -> Result<bool, String> {
    use std::os::windows::process::CommandExt as _;
    // New console window for the child (its parent, this GUI process, has none).
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let already = std::env::var_os(RELAUNCHED_ENV).is_some();
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| console_exe_beside(&e));
    let exists = exe.as_deref().is_some_and(Path::is_file);
    match relaunch_decision(show_terminal, already, exists) {
        Relaunch::Stay => Ok(false),
        Relaunch::Missing => Err(format!(
            "terminal demandé mais {CONSOLE_EXE} est absent à côté de l'exécutable"
        )),
        Relaunch::Console => {
            let exe = exe.unwrap_or_default();
            std::process::Command::new(&exe)
                .args(std::env::args_os().skip(1))
                .env(RELAUNCHED_ENV, "1")
                .creation_flags(CREATE_NEW_CONSOLE)
                .spawn()
                .map(|_| true)
                .map_err(|e| format!("{} non lancé : {e}", exe.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunches_once_and_only_when_asked() {
        assert_eq!(relaunch_decision(false, false, true), Relaunch::Stay);
        assert_eq!(relaunch_decision(true, false, true), Relaunch::Console);
        assert_eq!(relaunch_decision(true, true, true), Relaunch::Stay);
        assert_eq!(relaunch_decision(true, false, false), Relaunch::Missing);
        assert_eq!(relaunch_decision(true, true, false), Relaunch::Stay);
    }

    #[test]
    fn console_exe_is_a_sibling() {
        let exe = Path::new("dir").join("emu-rust.exe");
        assert_eq!(
            console_exe_beside(&exe),
            Some(Path::new("dir").join(CONSOLE_EXE))
        );
    }
}
