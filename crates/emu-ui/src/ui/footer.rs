//! Footer shown on every screen outside the game (library, settings, cores, help, journal, wizard, captures) and at
//! the bottom of the in-game menu
//! (ADR 0023 §1): `emu-rust <version> — dev by claude@jerhome` and the « Journal » button with its unread counter.

use egui::{RichText, Ui};

use super::text;
use crate::log::LogBuffer;
use crate::{CREDIT, VERSION};

/// Content of the footer: credit line (left) and the « Journal » button (right). Returns `true` when the button
/// was activated (click, or Enter / Space while focused).
pub(crate) fn contents(ui: &mut Ui) -> bool {
    let (errors, warnings) = LogBuffer::global().unread();
    let mut open = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(text::footer_credit(VERSION, CREDIT))
                .small()
                .weak(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = text::log_button_label(errors, warnings);
            let label = if errors > 0 {
                RichText::new(label).color(crate::theme::ERROR)
            } else if warnings > 0 {
                RichText::new(label).color(crate::theme::WARN)
            } else {
                RichText::new(label)
            };
            open = ui.small_button(label).clicked();
        });
    });
    open
}

/// Bottom panel of a full-window screen. Add it before the other panels so it stays the lowest one.
pub(crate) fn panel(ui: &mut Ui) -> bool {
    egui::Panel::bottom("footer").show(ui, contents).inner
}
