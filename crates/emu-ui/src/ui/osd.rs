//! On-screen notifications (core messages, frontend events) and the F3 stats overlay.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Context, Frame, Id, Order, RichText};

use super::{StatsView, text};

const MAX_ITEMS: usize = 5;
/// Messages kept for the in-game menu (« Derniers messages »).
const MAX_HISTORY: usize = 10;
pub const DEFAULT_DURATION: Duration = Duration::from_secs(3);
const MIN_DURATION: Duration = Duration::from_secs(1);
const MAX_DURATION: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
struct Item {
    text: String,
    level: OsdLevel,
    until: Instant,
}

/// A past message, kept after its toast has expired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Local `HH:MM:SS`.
    pub time: String,
    pub level: OsdLevel,
    pub text: String,
}

impl HistoryEntry {
    /// Plain line used for the clipboard.
    pub fn line(&self) -> String {
        format!("{} {}", self.time, self.text)
    }
}

#[derive(Debug, Default)]
pub struct Osd {
    items: VecDeque<Item>,
    /// Oldest first, bounded by `MAX_HISTORY`.
    history: VecDeque<HistoryEntry>,
}

impl Osd {
    /// Adds a message shown for `duration` (clamped to 1–10 s); the oldest is dropped beyond 5.
    pub fn push(
        &mut self,
        text: impl Into<String>,
        level: OsdLevel,
        duration: Duration,
        now: Instant,
    ) {
        let duration = duration.clamp(MIN_DURATION, MAX_DURATION);
        let text = text::sanitize_core_text(&text.into());
        // Warnings and errors also reach the Journal panel; infos are routine and already logged by their callers.
        match level {
            OsdLevel::Warn => tracing::warn!(target: "osd", "{text}"),
            OsdLevel::Error => tracing::error!(target: "osd", "{text}"),
            OsdLevel::Info => {}
        }
        self.record(
            &text,
            level,
            chrono::Local::now().format("%H:%M:%S").to_string(),
        );
        self.items.push_back(Item {
            text,
            level,
            until: now + duration,
        });
        while self.items.len() > MAX_ITEMS {
            self.items.pop_front();
        }
    }

    fn record(&mut self, text: &str, level: OsdLevel, time: String) {
        self.history.push_back(HistoryEntry {
            time,
            level,
            text: text.to_owned(),
        });
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
    }

    /// Latest messages of the session, most recent first.
    pub fn history(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.history.iter().rev()
    }

    /// All the history as plain text, most recent first, one message per line.
    pub fn history_text(&self) -> String {
        crate::copyable::joined(self.history().map(HistoryEntry::line))
    }

    pub fn info(&mut self, text: impl Into<String>, now: Instant) {
        self.push(text, OsdLevel::Info, DEFAULT_DURATION, now);
    }

    pub fn error(&mut self, text: impl Into<String>, now: Instant) {
        self.push(text, OsdLevel::Error, DEFAULT_DURATION * 2, now);
    }

    /// Drops expired messages.
    pub fn prune(&mut self, now: Instant) {
        self.items.retain(|i| i.until > now);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// When the next message expires (to schedule a repaint while the game is paused).
    pub fn next_expiry(&self) -> Option<Instant> {
        self.items.iter().map(|i| i.until).min()
    }
}

fn panel() -> Frame {
    Frame::new()
        .fill(Color32::from_black_alpha(190))
        .inner_margin(6.0)
        .corner_radius(4.0)
}

/// `top`: anchored at the top-left instead (the virtual keyboard owns the bottom of the screen).
pub(super) fn show(ctx: &Context, osd: &Osd, now: Instant, top: bool) {
    if osd.items.iter().all(|i| i.until <= now) {
        return;
    }
    egui::Area::new(Id::unique("osd"))
        .order(Order::Foreground)
        .anchor(
            if top {
                Align2::LEFT_TOP
            } else {
                Align2::LEFT_BOTTOM
            },
            [10.0, if top { 10.0 } else { -10.0 }],
        )
        .interactable(false)
        .show(ctx, |ui| {
            panel().show(ui, |ui| {
                for item in osd.items.iter().filter(|i| i.until > now) {
                    let color = match item.level {
                        OsdLevel::Info => Color32::WHITE,
                        OsdLevel::Warn => Color32::from_rgb(255, 210, 90),
                        OsdLevel::Error => Color32::from_rgb(255, 110, 100),
                    };
                    ui.label(RichText::new(&item.text).color(color));
                }
            });
        });
}

pub(super) fn show_stats(ctx: &Context, s: &StatsView) {
    egui::Area::new(Id::unique("stats"))
        .order(Order::Foreground)
        .anchor(Align2::RIGHT_TOP, [-10.0, 10.0])
        .interactable(false)
        .show(ctx, |ui| {
            panel().show(ui, |ui| {
                ui.label(RichText::new(text::STATS_TITLE).strong());
                for line in [
                    text::stats_fps(s.fps),
                    text::stats_frame(s.frame),
                    text::stats_audio(s.buffered_ms, s.target_ms),
                    text::stats_xruns(s.underruns, s.dropped_samples),
                    text::stats_audio_status(s.audio_status, s.recoveries),
                ] {
                    ui.label(RichText::new(line).monospace().color(Color32::WHITE));
                }
            });
        });
}
