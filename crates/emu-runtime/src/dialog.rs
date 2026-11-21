//! Native error box for a fatal error before any window exists (ADR 0023 §3). The only Windows call of 7.1
//! (`MessageBoxW` through `windows-sys`, reviewed by `reviewer`); elsewhere a no-op: stderr and the log file
//! already hold the message.

/// Blocking native error box on Windows; other OS: no-op (stderr and the log file already have the message).
pub fn fatal_dialog(title: &str, message: &str) {
    #[cfg(windows)]
    show(title, message);
    #[cfg(not(windows))]
    let _ = (title, message);
}

/// UTF-16, NUL-terminated; an interior NUL (which would cut the text) becomes U+FFFD.
#[cfg(any(windows, test))]
fn wide(text: &str) -> Vec<u16> {
    text.chars()
        .map(|c| if c == '\0' { '\u{FFFD}' } else { c })
        .collect::<String>()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn show(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let (title, message) = (wide(title), wide(message));
    // SAFETY: both buffers are valid, NUL-terminated UTF-16 strings (`wide` removes interior NULs) that outlive the
    // call; the parent window is null, so no window handle is dereferenced. The call blocks until dismissed and
    // its return value carries nothing we need.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_is_nul_terminated_without_interior_nul() {
        let w = wide("a\0é");
        assert_eq!(w.iter().filter(|&&u| u == 0).count(), 1);
        assert_eq!(w.last(), Some(&0));
        assert_eq!(String::from_utf16(&w[..w.len() - 1]).unwrap(), "a\u{FFFD}é");
    }

    #[test]
    fn fatal_dialog_does_not_panic_off_windows() {
        #[cfg(not(windows))]
        fatal_dialog("t", "m\0");
    }
}
