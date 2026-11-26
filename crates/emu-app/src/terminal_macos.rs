//! macOS « Afficher le terminal »: an app started from the Finder has no terminal, so with the setting on it
//! restarts itself inside Terminal.app (`open -a Terminal`) and exits. The restarted process has a terminal on
//! stderr, so it never restarts again.

/// Restart inside Terminal.app only when asked and not already attached to a terminal.
pub(crate) fn wants_terminal_restart(show_terminal: bool, stderr_is_terminal: bool) -> bool {
    show_terminal && !stderr_is_terminal
}

/// Finder, Dock and `open` start a bundle without arguments (older macOS adds `-psn_...`, which the parser
/// ignores): only such a launch may move to Terminal.app. Decided on the parsed command line, never on the
/// arguments of the test or host process.
pub(crate) fn is_argumentless_launch(args: &[std::ffi::OsString]) -> bool {
    args.iter()
        .all(|a| a.to_string_lossy().starts_with("-psn_"))
}

/// Pure decision: a library launch without arguments (never a game, a subcommand or a usage error).
pub(crate) fn is_restart_candidate(
    command: Option<&crate::cli::Command>,
    args: &[std::ffi::OsString],
) -> bool {
    matches!(command, Some(crate::cli::Command::Library(_))) && is_argumentless_launch(args)
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
    fn only_an_argumentless_launch_may_restart() {
        let os = |a: &[&str]| a.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        assert!(is_argumentless_launch(&os(&[])));
        assert!(is_argumentless_launch(&os(&["-psn_0_12345"])));
        assert!(!is_argumentless_launch(&os(&["--list-cores"])));
        assert!(!is_argumentless_launch(&os(&["--data-dir", "x"])));
        assert!(!is_argumentless_launch(&os(&["-psn_0_1", "game.rom"])));
    }

    #[test]
    fn restart_candidate_follows_the_parsed_command() {
        let os = |a: &[&str]| a.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        let parse = |a: &[&str]| crate::cli::parse(os(a)).ok();
        assert!(is_restart_candidate(parse(&[]).as_ref(), &os(&[])));
        assert!(!is_restart_candidate(
            parse(&["--list-cores"]).as_ref(),
            &os(&["--list-cores"])
        ));
        assert!(!is_restart_candidate(None, &os(&[])));
    }

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
