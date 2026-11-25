//! Selectable messages with a « Copier » button (7.1: error text must be copyable into a bug report).
//! The copied text is the full plain string, never the displayed (possibly wrapped or truncated) form.
//! The Windows fatal dialog (`MessageBoxW`) is not concerned: it already supports a native Ctrl+C.

use egui::{Color32, Label, RichText, Ui};

use crate::ui::text;

/// How long « Copié » stays next to the button, in seconds.
const FEEDBACK_SECS: f64 = 2.0;

/// Plain text of several lines, one per line, empty lines dropped.
pub(crate) fn joined<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> String {
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

/// « Copier » button with a short « Copié » confirmation; `payload` is what lands in the clipboard.
pub(crate) fn copy_button(ui: &mut Ui, payload: &str) {
    copy_button_labelled(ui, text::COPY, payload);
}

pub(crate) fn copy_button_labelled(ui: &mut Ui, label: &str, payload: &str) {
    let id = ui.scope_id().with(("copy-feedback", payload));
    let now = ui.input(|i| i.time);
    let button = ui.button(RichText::new(label).small());
    if button.clicked() {
        ui.ctx().copy_text(payload.to_owned());
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    if let Some(at) = ui.data(|d| d.get_temp::<f64>(id)) {
        if now - at < FEEDBACK_SECS {
            ui.label(RichText::new(text::COPIED).small().weak());
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        } else {
            ui.data_mut(|d| d.remove::<f64>(id));
        }
    }
}

/// A selectable, wrapping, coloured message followed by its « Copier » button.
pub(crate) fn message(ui: &mut Ui, color: Color32, msg: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.add(Label::new(RichText::new(msg).color(color)).selectable(true));
        copy_button(ui, msg);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_keeps_full_text_and_drops_empty_lines() {
        assert_eq!(joined(["a", "", "b c"]), "a\nb c");
        assert_eq!(joined(Vec::<String>::new()), "");
    }

    #[test]
    fn message_and_button_render_without_panic() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            message(ui, crate::theme::ERROR, "le core a refusé le jeu : x");
        });
        out.textures_delta.clear();
    }
}
