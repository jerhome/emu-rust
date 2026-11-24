//! In-memory ring of the latest log lines, shown in the settings « Journal » pane.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Lines kept by the process-wide buffer.
pub const LOG_CAPACITY: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: Level,
    pub target: String,
    pub message: String,
}

/// Shared ring buffer; clones point to the same lines.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    cap: usize,
    lines: Arc<Mutex<VecDeque<LogLine>>>,
}

impl PartialEq for LogBuffer {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.lines, &other.lines)
    }
}

impl LogBuffer {
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            lines: Arc::new(Mutex::new(VecDeque::with_capacity(cap))),
        }
    }

    /// Process-wide buffer (capacity 500): `emu-app` stacks `LogBuffer::global().layer()` and the settings page
    /// reads the same one.
    pub fn global() -> LogBuffer {
        static GLOBAL: OnceLock<LogBuffer> = OnceLock::new();
        GLOBAL.get_or_init(|| LogBuffer::new(LOG_CAPACITY)).clone()
    }

    pub fn push(&self, line: LogLine) {
        let mut lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        if lines.len() >= self.cap {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// Oldest first.
    pub fn snapshot(&self) -> Vec<LogLine> {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.lines.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `tracing` layer feeding this buffer.
    pub fn layer(&self) -> LogBufferLayer {
        LogBufferLayer(self.clone())
    }
}

pub struct LogBufferLayer(LogBuffer);

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

impl<S: Subscriber> Layer<S> for LogBufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut v = MessageVisitor::default();
        event.record(&mut v);
        let meta = event.metadata();
        self.0.push(LogLine {
            level: *meta.level(),
            target: meta.target().to_owned(),
            message: format!("{}{}", v.message, v.fields),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn ring_drops_oldest() {
        let b = LogBuffer::new(3);
        for i in 0..5 {
            b.push(LogLine {
                level: Level::INFO,
                target: "t".into(),
                message: i.to_string(),
            });
        }
        let m: Vec<_> = b.snapshot().into_iter().map(|l| l.message).collect();
        assert_eq!(m, ["2", "3", "4"]);
    }

    #[test]
    fn layer_captures_events() {
        let b = LogBuffer::new(10);
        let sub = tracing_subscriber::registry().with(b.layer());
        tracing::subscriber::with_default(sub, || {
            tracing::warn!("core introuvable");
            tracing::error!(code = 3, "échec");
        });
        let s = b.snapshot();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].level, Level::WARN);
        assert_eq!(s[0].message, "core introuvable");
        assert_eq!(s[1].message, "échec code=3");
    }
}
