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

/// Longest message handed to the box, in characters; longer text is cut and ends with an ellipsis.
#[cfg(any(windows, test))]
const MAX_MESSAGE_CHARS: usize = 2000;

#[cfg(any(windows, test))]
fn truncate_message(message: &str) -> std::borrow::Cow<'_, str> {
    match message.char_indices().nth(MAX_MESSAGE_CHARS) {
        None => message.into(),
        Some((cut, _)) => format!("{}…", &message[..cut]).into(),
    }
}

#[cfg(windows)]
fn show(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW,
    };
    let (title, message) = (wide(title), wide(&truncate_message(message)));
    // SAFETY: both buffers are valid, NUL-terminated UTF-16 strings (`wide` removes interior NULs) that outlive the
    // call; the parent window is null, so no window handle is dereferenced. The call blocks until dismissed and
    // its return value carries nothing we need.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
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
    fn long_messages_are_cut_on_a_char_boundary() {
        assert_eq!(truncate_message("court"), "court");
        let exact = "é".repeat(MAX_MESSAGE_CHARS);
        assert_eq!(truncate_message(&exact), exact.as_str());
        let long = "é".repeat(MAX_MESSAGE_CHARS + 5);
        let cut = truncate_message(&long);
        assert_eq!(cut.chars().count(), MAX_MESSAGE_CHARS + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn fatal_dialog_does_not_panic_off_windows() {
        #[cfg(not(windows))]
        fatal_dialog("t", "m\0");
    }
}
