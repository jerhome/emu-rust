//! Immediate process exit for the dry-run child.

/// Ends the process at once with `code`, skipping `atexit` handlers, C++ static destructors,
/// Rust destructors and (Windows) `DLL_PROCESS_DETACH` of the loaded libraries.
///
/// Use it only in the dry-run child (watchdog or final exit), where a stuck core must not be able to
/// block the exit. Everywhere else prefer `std::process::exit`: nothing is flushed here, so
/// buffered output and files not yet synced are lost.
pub fn exit_immediately(code: i32) -> ! {
    #[cfg(unix)]
    {
        // SAFETY: `_exit` takes a plain integer, has no preconditions and never returns; it only
        // terminates this process without running user-space exit handlers.
        unsafe { libc::_exit(code) }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle that is always valid for this process;
        // `TerminateProcess` on it takes no pointers. On success it does not return.
        unsafe { TerminateProcess(GetCurrentProcess(), code as u32) };
        // Fallback if the call failed: abort rather than return into a state the caller gave up on.
        std::process::abort()
    }
    #[cfg(not(any(unix, windows)))]
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    use super::exit_immediately;
    use std::process::Command;

    const ENV: &str = "EMU_RUNTIME_EXIT_IMMEDIATELY_CHILD";

    struct Marker(std::path::PathBuf);
    impl Drop for Marker {
        fn drop(&mut self) {
            let _ = std::fs::write(&self.0, b"dropped");
        }
    }

    #[test]
    fn exits_with_code_without_running_destructors() {
        if let Ok(path) = std::env::var(ENV) {
            let _marker = Marker(path.into());
            exit_immediately(42);
        }
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "exit::tests::exits_with_code_without_running_destructors",
                "--nocapture",
            ])
            .env(ENV, &marker)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(42));
        assert!(!marker.exists(), "Drop ran: exit was not immediate");
    }
}
