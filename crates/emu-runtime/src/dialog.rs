//! Native error box for a fatal error before any window exists (ADR 0023 §3). The only Windows call of 7.1
//! (`MessageBoxW` through `windows-sys`, reviewed by `reviewer`); elsewhere a no-op: stderr and the log file
//! already hold the message.

/// Blocking native error box on Windows; other OS: no-op (stderr and the log file already have the message).
/// Until FR1 it is a no-op everywhere.
pub fn fatal_dialog(title: &str, message: &str) {
    let _ = (title, message);
}
