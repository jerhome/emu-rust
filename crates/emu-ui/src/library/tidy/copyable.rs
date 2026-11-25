//! Minimal stand-in for `crate::copyable` (selectable message + « Copier » button), which only exists on the 7.5
//! line. After the rebase on 7.5, delete this file and replace `super::copyable` by `crate::copyable` (same
//! signatures: `joined`, `copy_button_labelled`, `message`).

use egui::{Color32, Label, RichText, Ui};

use super::text;

/// Plain text of several lines, one per line, empty lines dropped.
pub(super) fn joined<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> String {
    let mut out = String::new();
    for line in lines {
        let line = line.as_ref();
        if line.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// A button that copies `payload` (the full plain text, never the displayed form).
pub(super) fn copy_button_labelled(ui: &mut Ui, label: &str, payload: &str) {
    if ui.button(RichText::new(label).small()).clicked() {
        ui.ctx().copy_text(payload.to_owned());
    }
}

/// A selectable, wrapping, coloured message followed by its « Copier » button.
pub(super) fn message(ui: &mut Ui, color: Color32, msg: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.add(Label::new(RichText::new(msg).color(color)).selectable(true));
        copy_button_labelled(ui, text::COPY, msg);
    });
}
