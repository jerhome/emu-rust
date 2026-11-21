//! Log capture (ADR 0023 §3): a `tracing` layer feeding a bounded in-memory ring, shown by the « Journal » panel
//! and followed by `<data>/logs/emu-rust.log`. Installed by `emu-app` next to the stderr output, which stays.
//! Core lines are cleaned (`sanitize_core_line`) before they are stored; secrets are never logged (`Secret`).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

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
    state: Mutex<State>,
    /// Folder of the log file as given to [`LogBuffer::attach_file`] (even when the attach failed).
    dir: Mutex<Option<PathBuf>>,
}

#[derive(Debug)]
struct State {
    ring: VecDeque<LogRecord>,
    next_seq: u64,
    errors: u32,
    warnings: u32,
    file: Option<FileSink>,
}

#[derive(Debug)]
struct FileSink {
    file: File,
    written: u64,
    truncated: bool,
}

static GLOBAL: LogBuffer = LogBuffer::new();

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl LogBuffer {
    const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                ring: VecDeque::new(),
                next_seq: 1,
                errors: 0,
                warnings: 0,
                file: None,
            }),
            dir: Mutex::new(None),
        }
    }

    /// The buffer fed by [`capture_layer`].
    pub fn global() -> &'static LogBuffer {
        &GLOBAL
    }

    /// Records with `seq > since_seq`, oldest first (`0`: everything still held).
    pub fn snapshot(&self, since_seq: u64) -> Vec<LogRecord> {
        lock(&self.state)
            .ring
            .iter()
            .filter(|r| r.seq > since_seq)
            .cloned()
            .collect()
    }

    /// `(errors, warnings)` recorded since the last [`LogBuffer::mark_read`] (footer counter).
    pub fn unread(&self) -> (u32, u32) {
        let s = lock(&self.state);
        (s.errors, s.warnings)
    }

    /// Resets the unread counters (the « Journal » panel was opened).
    pub fn mark_read(&self) {
        let mut s = lock(&self.state);
        s.errors = 0;
        s.warnings = 0;
    }

    /// Folder of the log file (set by [`LogBuffer::attach_file`]), for « Ouvrir le dossier des journaux ».
    pub fn log_dir(&self) -> Option<PathBuf> {
        lock(&self.dir).clone()
    }

    /// Rotates `<path>` to `<path>.1`, writes what the ring already holds, then follows it (cap
    /// [`MAX_LOG_FILE_BYTES`], mode 0600 on Unix). A failed rotation or open (second instance) is returned once
    /// and leaves the ring working without a file.
    pub fn attach_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            *lock(&self.dir) = Some(parent.to_owned());
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let mut previous = path.as_os_str().to_owned();
            previous.push(".1");
            // Windows refuses to replace an existing target on some versions: remove it first. A second instance
            // holding the file open makes either step fail, which is reported to the caller.
            let previous = PathBuf::from(previous);
            if previous.exists() {
                std::fs::remove_file(&previous)?;
            }
            std::fs::rename(path, &previous)?;
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        let mut sink = FileSink {
            file,
            written: 0,
            truncated: false,
        };
        let mut s = lock(&self.state);
        for record in &s.ring {
            sink.write(record);
        }
        s.file = Some(sink);
        Ok(())
    }

    fn push(&self, level: LogLevel, origin: LogOrigin, target: &str, message: &str) {
        let message = match origin {
            LogOrigin::Core => crate::sanitize_core_line(message),
            LogOrigin::App => message.to_owned(),
        };
        let message = truncate_chars(&message, MAX_LOG_MESSAGE);
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let mut s = lock(&self.state);
        self.store(&mut s, unix_ms, level, origin, target, message);
    }

    fn store(
        &self,
        s: &mut State,
        unix_ms: u64,
        level: LogLevel,
        origin: LogOrigin,
        target: &str,
        message: String,
    ) {
        let record = LogRecord {
            seq: s.next_seq,
            unix_ms,
            level,
            origin,
            target: target.to_owned(),
            message,
        };
        s.next_seq += 1;
        match level {
            LogLevel::Error => s.errors = s.errors.saturating_add(1),
            LogLevel::Warn => s.warnings = s.warnings.saturating_add(1),
            LogLevel::Info | LogLevel::Debug => {}
        }
        if let Some(sink) = &mut s.file {
            sink.write(&record);
        }
        if s.ring.len() >= MAX_LOG_RECORDS {
            s.ring.pop_front();
        }
        s.ring.push_back(record);
    }

    /// Panic hook entry: never blocks (the panicking thread may hold the lock itself).
    fn push_nonblocking(&self, message: String) {
        let Ok(mut s) = self.state.try_lock() else {
            return;
        };
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let message = truncate_chars(&message, MAX_LOG_MESSAGE);
        self.store(
            &mut s,
            unix_ms,
            LogLevel::Error,
            LogOrigin::App,
            "emu_ui::panic",
            message,
        );
    }
}

impl FileSink {
    fn write(&mut self, record: &LogRecord) {
        if self.truncated {
            return;
        }
        let mut line = format_record(record);
        line.push('\n');
        let cap = MAX_LOG_FILE_BYTES;
        let len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if self.written + len > cap {
            self.truncated = true;
            let _ = self
                .file
                .write_all(b"journal tronqu\xc3\xa9 (taille maximale atteinte)\n");
            return;
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.written += len;
        } else {
            // Disk full or file gone: stop trying, the ring keeps working.
            self.truncated = true;
        }
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &s[..end]),
        None => s.to_owned(),
    }
}

impl LogLevel {
    /// Fixed-width label of the log file and the panel.
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "ERREUR",
            Self::Warn => "AVERT.",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

/// `YYYY-MM-DD HH:MM:SS` (UTC).
pub fn format_time(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    format!(
        "{}:{:02}",
        crate::ui::text::format_unix_utc(secs),
        secs % 60
    )
}

/// One line of the log file: `<time> <level> [<origin>] <target>: <message>`.
pub fn format_record(r: &LogRecord) -> String {
    let origin = match r.origin {
        LogOrigin::App => "app",
        LogOrigin::Core => "core",
    };
    format!(
        "{} {:<6} [{origin}] {}: {}",
        format_time(r.unix_ms),
        r.level.label(),
        r.target,
        r.message
    )
}

/// `tracing_subscriber::Layer` feeding [`LogBuffer::global`]; composed by `emu-app` next to the stderr layer, so
/// it sees what the filter lets through (`warn`, `info` for our crates and the core; `RUST_LOG` wins).
pub fn capture_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    CaptureLayer(&GLOBAL)
}

/// Target of the libretro log callback (`emu_libretro::log::TARGET`).
const CORE_TARGET: &str = "emu_libretro::core";

struct CaptureLayer(&'static LogBuffer);

/// Collects the message and the other fields (`key=value`) of an event.
#[derive(Default)]
struct Fields {
    message: String,
    extra: String,
}

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.extra, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.extra, " {}={value}", field.name());
        }
    }
}

impl<S> tracing_subscriber::Layer<S> for CaptureLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let meta = event.metadata();
        let level = match *meta.level() {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            _ => LogLevel::Debug,
        };
        let origin = if meta.target() == CORE_TARGET {
            LogOrigin::Core
        } else {
            LogOrigin::App
        };
        let mut fields = Fields::default();
        event.record(&mut fields);
        // `truncated` of core lines is noise in the panel; other fields are kept.
        let extra = if origin == LogOrigin::Core {
            ""
        } else {
            fields.extra.as_str()
        };
        let message = format!("{}{extra}", fields.message);
        self.0.push(level, origin, meta.target(), &message);
    }
}

/// Chains a panic hook that writes the message and location into the buffer (hence the log file: without a
/// console it is the only trace of a crash), then calls the previous hook.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let what = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panique sans message".to_owned());
        let place = info
            .location()
            .map_or_else(String::new, |l| format!(" ({}:{})", l.file(), l.line()));
        GLOBAL.push_nonblocking(format!("panique : {what}{place}"));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt as _;

    /// Records through the layer into a private buffer (the global one is shared by the whole test binary).
    fn capture_into(buffer: &'static LogBuffer, f: impl FnOnce()) {
        let subscriber = tracing_subscriber::registry().with(CaptureLayer(buffer));
        tracing::subscriber::with_default(subscriber, f);
    }

    fn leaked() -> &'static LogBuffer {
        Box::leak(Box::new(LogBuffer::new()))
    }

    #[test]
    fn ring_is_bounded_and_ordered() {
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

    #[test]
    fn oldest_goes_first_and_snapshot_cursor_works() {
        let b = leaked();
        capture_into(b, || {
            for i in 0..MAX_LOG_RECORDS + 5 {
                tracing::info!("n{i}");
            }
        });
        let all = b.snapshot(0);
        assert_eq!(all.len(), MAX_LOG_RECORDS);
        assert_eq!(all[0].message, "n5");
        let last = all.last().unwrap().seq;
        assert_eq!(last, u64::try_from(MAX_LOG_RECORDS + 5).unwrap());
        assert!(b.snapshot(last).is_empty());
        assert_eq!(b.snapshot(last - 1).len(), 1);
    }

    #[test]
    fn unread_counts_errors_and_warnings_until_marked_read() {
        let b = leaked();
        capture_into(b, || {
            tracing::error!("e");
            tracing::warn!("w1");
            tracing::warn!("w2");
            tracing::info!("i");
        });
        assert_eq!(b.unread(), (1, 2));
        b.mark_read();
        assert_eq!(b.unread(), (0, 0));
    }

    #[test]
    fn long_messages_are_truncated_by_characters() {
        let b = leaked();
        let long = "é".repeat(MAX_LOG_MESSAGE + 50);
        capture_into(b, || tracing::warn!("{long}"));
        let m = &b.snapshot(0)[0].message;
        assert_eq!(m.chars().count(), MAX_LOG_MESSAGE + 1);
        assert!(m.ends_with('…'));
    }

    #[test]
    fn core_lines_are_cleaned_and_flagged() {
        let b = leaked();
        capture_into(b, || {
            tracing::info!(target: "emu_libretro::core", truncated = false, "a\u{202e}b\tc\n");
            tracing::info!(target: "emu_ui::x", "plain");
        });
        let all = b.snapshot(0);
        assert_eq!(all[0].origin, LogOrigin::Core);
        assert!(!all[0].message.contains('\u{202e}'), "{:?}", all[0].message);
        assert!(!all[0].message.contains('\n'));
        assert!(!all[0].message.contains("truncated"));
        assert_eq!(all[1].origin, LogOrigin::App);
    }

    #[test]
    fn file_gets_the_ring_then_follows_and_rotates_next_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("emu-rust.log");
        let b = leaked();
        capture_into(b, || tracing::warn!("before"));
        b.attach_file(&path).unwrap();
        capture_into(b, || tracing::warn!("after"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("before") && text.contains("after"));
        assert!(text.find("before") < text.find("after"));
        assert_eq!(b.log_dir().as_deref(), path.parent());

        let second = leaked();
        capture_into(second, || tracing::warn!("again"));
        second.attach_file(&path).unwrap();
        let rotated = std::fs::read_to_string(dir.path().join("logs/emu-rust.log.1")).unwrap();
        assert!(rotated.contains("after"));
        let fresh = std::fs::read_to_string(&path).unwrap();
        assert!(fresh.contains("again") && !fresh.contains("after"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn failed_rotation_is_reported_and_the_ring_still_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("emu-rust.log");
        std::fs::write(&path, "old").unwrap();
        // `.1` is a non-empty directory: neither removed as a file nor replaced.
        let blocker = dir.path().join("emu-rust.log.1");
        std::fs::create_dir(&blocker).unwrap();
        std::fs::write(blocker.join("x"), "x").unwrap();
        let b = leaked();
        assert!(b.attach_file(&path).is_err());
        capture_into(b, || tracing::warn!("still here"));
        assert_eq!(b.snapshot(0).len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    }

    #[test]
    fn file_stops_at_its_cap_with_one_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("emu-rust.log");
        let b = leaked();
        b.attach_file(&path).unwrap();
        let chunk = "x".repeat(1500);
        capture_into(b, || {
            for _ in 0..2000 {
                tracing::warn!("{chunk}");
            }
        });
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size <= MAX_LOG_FILE_BYTES + 100, "{size}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("journal tronqué").count(), 1);
    }

    #[test]
    fn panic_message_reaches_the_buffer() {
        // The hook writes to the global buffer; the previous hook is restored so other tests stay quiet.
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        install_panic_hook();
        let result = std::panic::catch_unwind(|| panic!("boom-fu1"));
        assert!(result.is_err());
        std::panic::set_hook(before);
        assert!(
            LogBuffer::global()
                .snapshot(0)
                .iter()
                .any(|r| r.level == LogLevel::Error && r.message.contains("boom-fu1"))
        );
    }
}
