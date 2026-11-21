//! Log capture (ADR 0023 §3): a `tracing` layer feeding a bounded in-memory ring, shown by the « Journal » panel
//! and followed by `<data>/logs/emu-rust.log`. Installed by `emu-app` next to the stderr output, which stays.
//! Core lines are cleaned (`sanitize_core_line`) before they are stored; secrets are never logged (`Secret`).

use std::path::Path;

/// Bound of the ring: the oldest record goes when a new one comes.
pub const MAX_LOG_RECORDS: usize = 2000;
/// Longer messages are truncated (characters, not bytes).
pub const MAX_LOG_MESSAGE: usize = 2000;
/// The log file stops growing at this size (a « journal tronqué » line is written once).
pub const MAX_LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Where a record comes from: our crates, or a core (libretro log callback).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogOrigin {
    App,
    Core,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    /// Strictly increasing from 1 for the life of the process ([`LogBuffer::snapshot`] cursor).
    pub seq: u64,
    pub unix_ms: u64,
    pub level: LogLevel,
    pub origin: LogOrigin,
    /// `tracing` target (`emu_ui::library`, `emu_libretro::log`…).
    pub target: String,
    /// At most [`MAX_LOG_MESSAGE`] characters, control and bidi characters removed for core lines.
    pub message: String,
}

/// Process-wide ring of [`LogRecord`]s ([`LogBuffer::global`]); thread-safe.
#[derive(Debug)]
pub struct LogBuffer {
    // FU1: ring, unread counters, optional file; private.
    _private: (),
}

static GLOBAL: LogBuffer = LogBuffer { _private: () };

impl LogBuffer {
    /// The buffer fed by [`capture_layer`].
    pub fn global() -> &'static LogBuffer {
        &GLOBAL
    }

    /// Records with `seq > since_seq`, oldest first (`0`: everything still held).
    pub fn snapshot(&self, since_seq: u64) -> Vec<LogRecord> {
        let _ = since_seq;
        todo!("FU1: log ring")
    }

    /// `(errors, warnings)` recorded since the last [`LogBuffer::mark_read`] (footer counter).
    pub fn unread(&self) -> (u32, u32) {
        todo!("FU1: log ring")
    }

    /// Resets the unread counters (the « Journal » panel was opened).
    pub fn mark_read(&self) {
        todo!("FU1: log ring")
    }

    /// Rotates `<path>` to `<path>.1`, writes what the ring already holds, then follows it (cap
    /// [`MAX_LOG_FILE_BYTES`], mode 0600 on Unix). A failed rotation or open (second instance) is returned once
    /// and leaves the ring working without a file.
    pub fn attach_file(&self, path: &Path) -> std::io::Result<()> {
        let _ = path;
        todo!("FU1: log file")
    }
}

/// `tracing_subscriber::Layer` feeding [`LogBuffer::global`]; composed by `emu-app` next to the stderr layer, so
/// it sees what the filter lets through (`warn`, `info` for our crates and the core; `RUST_LOG` wins).
pub fn capture_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    CaptureLayer
}

/// Records nothing until FU1 implements `on_event`.
struct CaptureLayer;

impl<S> tracing_subscriber::Layer<S> for CaptureLayer where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>
{
}

/// Chains a panic hook that writes the message and location into the buffer (hence the log file: without a
/// console it is the only trace of a crash), then calls the previous hook.
pub fn install_panic_hook() {
    todo!("FU1: panic hook")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "FU1: the ring is a todo!() until then"]
    fn ring_is_bounded_and_ordered() {
        use tracing_subscriber::layer::SubscriberExt as _;
        let buffer = LogBuffer::global();
        let subscriber = tracing_subscriber::registry().with(capture_layer());
        tracing::subscriber::with_default(subscriber, || {
            for i in 0..=MAX_LOG_RECORDS {
                tracing::warn!(target: "emu_ui::log_test", "line {i}");
            }
        });
        let all = buffer.snapshot(0);
        assert!(all.len() <= MAX_LOG_RECORDS);
        assert!(all.windows(2).all(|w| w[0].seq < w[1].seq));
    }
}
