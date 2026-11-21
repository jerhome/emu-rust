//! « Journal » panel (ADR 0023 §3): the in-memory log with a level filter and « Ouvrir le dossier des journaux ».
//! No « Enregistrer sous… »: the file in the log folder is the export.

use std::sync::mpsc::Receiver;

use egui::{Color32, Key, RichText, ScrollArea, Ui};

use super::text;
use crate::log::{LogBuffer, LogLevel, LogOrigin, LogRecord, MAX_LOG_RECORDS, format_time};
use crate::platform::{FolderOutcome, spawn_open_folder};

/// Which records are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LogFilter {
    Errors,
    /// Warnings and errors.
    Warnings,
    #[default]
    All,
}

impl LogFilter {
    pub(crate) fn admits(self, level: LogLevel) -> bool {
        match self {
            Self::Errors => level == LogLevel::Error,
            Self::Warnings => level <= LogLevel::Warn,
            Self::All => true,
        }
    }
}

/// What the panel asks of its host.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LogEvent {
    Close,
}

#[derive(Debug, Default)]
pub(crate) struct LogPanel {
    filter: LogFilter,
    records: Vec<LogRecord>,
    last_seq: u64,
    folder: Option<Receiver<FolderOutcome>>,
    folder_note: Option<String>,
    focus_first: bool,
}

impl LogPanel {
    /// Called when the panel opens: unread counters reset, list reloaded, focus on « Retour ».
    pub(crate) fn open(&mut self) {
        LogBuffer::global().mark_read();
        self.records.clear();
        self.last_seq = 0;
        self.folder = None;
        self.folder_note = None;
        self.focus_first = true;
    }

    fn refresh(&mut self) {
        let fresh = LogBuffer::global().snapshot(self.last_seq);
        if let Some(last) = fresh.last() {
            self.last_seq = last.seq;
        }
        self.records.extend(fresh);
        let extra = self.records.len().saturating_sub(MAX_LOG_RECORDS);
        self.records.drain(..extra);
        // Messages recorded while the panel is open are read by definition.
        LogBuffer::global().mark_read();
    }

    fn poll_folder(&mut self) {
        let Some(rx) = &self.folder else { return };
        match rx.try_recv() {
            Ok(FolderOutcome::Opened) => self.folder = None,
            Ok(FolderOutcome::ShowPath { path, .. }) => {
                self.folder_note = Some(text::log_folder_path(&path.display().to_string()));
                self.folder = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.folder = None,
        }
    }

    /// Records the filter lets through, oldest first.
    pub(crate) fn visible(&self) -> impl Iterator<Item = &LogRecord> {
        self.records.iter().filter(|r| self.filter.admits(r.level))
    }

    /// Draws the panel in `ui`; `Close` when « Retour » was used or Escape pressed.
    /// `escape`: the panel closes itself on Escape (the in-game overlay routes that key through `Overlay::back`).
    pub(crate) fn show(&mut self, ui: &mut Ui, escape: bool) -> Option<LogEvent> {
        self.refresh();
        self.poll_folder();
        let mut event = None;
        ui.heading(text::LOG_TITLE);
        ui.horizontal_wrapped(|ui| {
            let back = ui.button(text::BACK);
            if std::mem::take(&mut self.focus_first) {
                back.request_focus();
            }
            if back.clicked() {
                event = Some(LogEvent::Close);
            }
            for (filter, label) in [
                (LogFilter::Errors, text::LOG_FILTER_ERRORS),
                (LogFilter::Warnings, text::LOG_FILTER_WARNINGS),
                (LogFilter::All, text::LOG_FILTER_ALL),
            ] {
                if ui.selectable_label(self.filter == filter, label).clicked() {
                    self.filter = filter;
                }
            }
            let dir = LogBuffer::global().log_dir();
            let button = ui.add_enabled(
                dir.is_some() && self.folder.is_none(),
                egui::Button::new(text::LOG_OPEN_FOLDER),
            );
            if button.clicked()
                && let Some(dir) = dir
            {
                self.folder = Some(spawn_open_folder(dir));
            }
        });
        if let Some(note) = &self.folder_note {
            ui.label(RichText::new(note).small());
        }
        ui.separator();
        let (page_up, page_down, home, end) = ui.input(|i| {
            (
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::PageDown),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
            )
        });
        let page = (ui.available_height() - 24.0).max(40.0);
        let mut shown = false;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(page)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if page_up {
                    ui.scroll_with_delta(egui::vec2(0.0, page));
                }
                if page_down {
                    ui.scroll_with_delta(egui::vec2(0.0, -page));
                }
                if home {
                    ui.scroll_to_cursor(Some(egui::Align::TOP));
                }
                for record in self.visible() {
                    shown = true;
                    ui.add(egui::Label::new(row(record)).wrap().selectable(true));
                }
                if end || !shown {
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                }
                if !shown {
                    ui.label(RichText::new(text::LOG_EMPTY).weak());
                }
            });
        ui.label(RichText::new(text::LOG_HINT).small().weak());
        if escape && ui.input(|i| i.key_pressed(Key::Escape)) {
            event = Some(LogEvent::Close);
        }
        event
    }
}

fn row(r: &LogRecord) -> RichText {
    let color = match r.level {
        LogLevel::Error => Color32::from_rgb(255, 120, 120),
        LogLevel::Warn => Color32::from_rgb(255, 190, 80),
        LogLevel::Info => Color32::from_gray(210),
        LogLevel::Debug => Color32::from_gray(140),
    };
    let origin = match r.origin {
        LogOrigin::App => "",
        LogOrigin::Core => concat!(" [", "core", "]"),
    };
    RichText::new(format!(
        "{} {}{origin} {}",
        format_time(r.unix_ms),
        r.level.label(),
        r.message
    ))
    // Normal size: the small style was hard to read (FU1 feedback).
    .monospace()
    .color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_admit_the_expected_levels() {
        use LogLevel::*;
        let admitted = |f: LogFilter| [Error, Warn, Info, Debug].map(|l| f.admits(l));
        assert_eq!(admitted(LogFilter::Errors), [true, false, false, false]);
        assert_eq!(admitted(LogFilter::Warnings), [true, true, false, false]);
        assert_eq!(admitted(LogFilter::All), [true; 4]);
    }

    #[test]
    fn button_label_counts_unread_messages() {
        assert_eq!(text::log_button_label(0, 0), "Journal");
        assert_eq!(text::log_button_label(1, 0), "Journal (1 erreur)");
        assert_eq!(
            text::log_button_label(2, 3),
            "Journal (2 erreurs, 3 avertissements)"
        );
    }

    #[test]
    fn footer_credit_line() {
        assert_eq!(
            text::footer_credit("7.1.0", "dev by claude@jerhome"),
            "emu-rust 7.1.0 — dev by claude@jerhome"
        );
    }
}
