//! macOS « Afficher le terminal »: an app started from the Finder has no terminal, so with the setting on it
//! restarts itself inside Terminal.app (`open -a Terminal`) and exits. The restarted process has a terminal on
//! stderr, so it never restarts again.

/// Restart inside Terminal.app only when asked and not already attached to a terminal.
pub(crate) fn wants_terminal_restart(show_terminal: bool, stderr_is_terminal: bool) -> bool {
    show_terminal && !stderr_is_terminal
}

/// `Ok(true)`: Terminal.app was asked to run this executable, this process must exit. `Err`: message to log once
/// tracing is up. Command-line arguments are not forwarded (`open` cannot pass them to Terminal); a Finder
/// launch has none.
#[cfg(target_os = "macos")]
pub(crate) fn restart_in_terminal(show_terminal: bool) -> Result<bool, String> {
    use std::io::IsTerminal as _;
    if !wants_terminal_restart(show_terminal, std::io::stderr().is_terminal()) {
        return Ok(false);
    }
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let status = std::process::Command::new("/usr/bin/open")
        .args(["-a", "Terminal"])
        .arg(&exe)
        .status()
        .map_err(|e| format!("Terminal non lancé : {e}"))?;
    if status.success() {
        Ok(true)
    } else {
        Err(format!("Terminal non lancé ({status})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_only_from_outside_a_terminal_when_asked() {
        assert!(wants_terminal_restart(true, false));
        assert!(
            !wants_terminal_restart(true, true),
            "already in Terminal: no loop"
        );
        assert!(!wants_terminal_restart(false, false));
        assert!(!wants_terminal_restart(false, true));
    }
}
