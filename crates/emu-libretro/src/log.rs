//! `GET_LOG_INTERFACE`: the C shim `emu_libretro_log_printf` (src/log_shim.c) formats the message and calls
//! `emu_libretro_log_sink`, which emits a `tracing` event (ADR 0009).
//!
//! The sink never touches the frontend handler nor the environment state: cores may log from any thread and
//! at any time (including from `retro_deinit` or their own worker threads).

use std::ffi::{CStr, c_char, c_int};
use std::panic::catch_unwind;

use crate::sys;
use crate::types::LogLevel;

/// `tracing` target of every message logged by the core.
pub(crate) const TARGET: &str = "emu_libretro::core";

unsafe extern "C" {
    /// Variadic `retro_log_printf_t`, defined in `src/log_shim.c`.
    pub(crate) fn emu_libretro_log_printf(level: c_int, fmt: *const c_char, ...);
}

/// Value written for `GET_LOG_INTERFACE`.
pub(crate) fn log_callback() -> sys::retro_log_callback {
    sys::retro_log_callback {
        log: emu_libretro_log_printf,
    }
}

pub(crate) fn level_from_raw(level: c_int) -> LogLevel {
    match level {
        sys::RETRO_LOG_DEBUG => LogLevel::Debug,
        sys::RETRO_LOG_WARN => LogLevel::Warn,
        sys::RETRO_LOG_ERROR => LogLevel::Error,
        sys::RETRO_LOG_INFO => LogLevel::Info,
        // `RETRO_LOG_DUMMY` and anything out of range.
        _ => LogLevel::Info,
    }
}

/// Emits one core message on `TARGET`, without the trailing line break cores usually add.
pub(crate) fn emit(level: LogLevel, message: &str, truncated: bool) {
    let message = message.trim_end_matches(['\n', '\r']);
    match level {
        LogLevel::Debug => tracing::debug!(target: TARGET, truncated, "{message}"),
        LogLevel::Info => tracing::info!(target: TARGET, truncated, "{message}"),
        LogLevel::Warn => tracing::warn!(target: TARGET, truncated, "{message}"),
        LogLevel::Error => tracing::error!(target: TARGET, truncated, "{message}"),
    }
    #[cfg(test)]
    tests::record(level, message, truncated);
}

/// Called by the C shim only.
///
/// # Safety
/// `msg` is null or a NUL-terminated string valid for the duration of the call.
// SAFETY (attribute): the symbol name is unique to this crate and its signature matches the declaration in
// src/log_shim.c (`void emu_libretro_log_sink(int, const char *, int)`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn emu_libretro_log_sink(level: c_int, msg: *const c_char, truncated: c_int) {
    // A panic (e.g. from a tracing subscriber) must not unwind into C.
    let _ = catch_unwind(|| {
        if msg.is_null() {
            return;
        }
        // SAFETY: non-null and NUL-terminated per this function's contract (the shim terminates its buffer);
        // the borrow does not outlive the call.
        let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
        emit(level_from_raw(level), &text, truncated != 0);
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static RECORDED: RefCell<Vec<(LogLevel, String, bool)>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn record(level: LogLevel, message: &str, truncated: bool) {
        RECORDED.with(|r| r.borrow_mut().push((level, message.to_owned(), truncated)));
    }

    fn take() -> Vec<(LogLevel, String, bool)> {
        RECORDED.with(|r| std::mem::take(&mut *r.borrow_mut()))
    }

    #[test]
    fn levels_map_with_info_fallback() {
        assert_eq!(level_from_raw(0), LogLevel::Debug);
        assert_eq!(level_from_raw(1), LogLevel::Info);
        assert_eq!(level_from_raw(2), LogLevel::Warn);
        assert_eq!(level_from_raw(3), LogLevel::Error);
        assert_eq!(level_from_raw(c_int::MAX), LogLevel::Info);
        assert_eq!(level_from_raw(-1), LogLevel::Info);
    }

    #[test]
    fn shim_formats_arguments_and_strips_newline() {
        take();
        let log = log_callback().log;
        // SAFETY: format and arguments match (`%d` ← int, `%s` ← NUL-terminated string, `%.2f` ← double).
        unsafe {
            log(
                sys::RETRO_LOG_WARN,
                c"x=%d s=%s f=%.2f\n".as_ptr(),
                42 as c_int,
                c"abc".as_ptr(),
                1.5f64,
            );
            log(sys::RETRO_LOG_ERROR, std::ptr::null());
        }
        assert_eq!(
            take(),
            vec![(LogLevel::Warn, "x=42 s=abc f=1.50".to_owned(), false)]
        );
    }

    #[test]
    fn shim_truncates_to_1023_bytes() {
        take();
        let long = std::ffi::CString::new("y".repeat(5000)).unwrap();
        // SAFETY: `%s` ← NUL-terminated string.
        unsafe { emu_libretro_log_printf(sys::RETRO_LOG_INFO, c"%s".as_ptr(), long.as_ptr()) };
        let got = take();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1.len(), 1023);
        assert!(got[0].2);

        let exact = std::ffi::CString::new("z".repeat(1023)).unwrap();
        // SAFETY: as above.
        unsafe { emu_libretro_log_printf(sys::RETRO_LOG_INFO, c"%s".as_ptr(), exact.as_ptr()) };
        let got = take();
        assert_eq!(got[0].1.len(), 1023);
        assert!(!got[0].2);
    }

    #[test]
    fn shim_works_from_another_thread_without_core() {
        std::thread::spawn(|| {
            // SAFETY: format without conversions.
            unsafe { emu_libretro_log_printf(sys::RETRO_LOG_DEBUG, c"from a worker".as_ptr()) };
            assert_eq!(
                take(),
                vec![(LogLevel::Debug, "from a worker".to_owned(), false)]
            );
        })
        .join()
        .unwrap();
    }

    #[test]
    fn sink_ignores_null() {
        take();
        // SAFETY: null is allowed.
        unsafe { emu_libretro_log_sink(1, std::ptr::null(), 0) };
        assert!(take().is_empty());
    }
}
