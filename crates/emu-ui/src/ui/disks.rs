//! Disk menu (ADR 0015 §5): list with the core's labels, insert, eject, add an image of the same folder, reset
//! the working copies. Every row is a focusable button (keyboard / pad).

use egui::{Button, Color32, Context, RichText, ScrollArea};

use super::{Overlay, UiAction, UiData, nav, screen_window, text};

const ROW_HEIGHT: f32 = 26.0;

pub(super) fn show(
    ctx: &Context,
    overlay: &mut Overlay,
    data: &UiData<'_>,
    actions: &mut Vec<UiAction>,
) {
    screen_window(ctx, text::DISKS_TITLE, |ui, max_height| {
        let w = ui.available_width();
        let row = |ui: &mut egui::Ui, label: String, focus: &mut bool| {
            let resp = ui.add(Button::new(label).min_size([w, ROW_HEIGHT].into()));
            nav(&resp, focus);
            resp.clicked()
        };
        let Some(disks) = data.disks else {
            ui.label(text::DISKS_UNAVAILABLE);
            if row(ui, text::BACK.to_owned(), &mut overlay.focus_first) {
                actions.push(UiAction::Back);
            }
            return;
        };
        if overlay.confirm_reset {
            ui.colored_label(Color32::from_rgb(255, 190, 80), text::DISKS_RESET_CONFIRM);
            if row(ui, text::NO.to_owned(), &mut overlay.focus_first) {
                overlay.confirm_reset = false;
            }
            if row(ui, text::DISKS_RESET_YES.to_owned(), &mut false) {
                overlay.confirm_reset = false;
                actions.push(UiAction::ResetDisks);
            }
            return;
        }
        let status = match disks.current {
            None => text::DISKS_EMPTY_DRIVE.to_owned(),
            Some(i) => disks
                .slots
                .iter()
                .find(|s| s.index == i)
                .map_or_else(String::new, |s| {
                    text::disk_inserted(i, disks.slots.len(), &s.label)
                }),
        };
        ui.label(RichText::new(status).strong());
        ScrollArea::vertical()
            .max_height((max_height - 220.0).max(80.0))
            .show(ui, |ui| {
                if disks.slots.is_empty() {
                    ui.label(text::DISKS_NONE);
                }
                for slot in &disks.slots {
                    let mark = if disks.current == Some(slot.index) {
                        format!("  ({})", text::DISKS_CURRENT)
                    } else {
                        String::new()
                    };
                    let label = format!(
                        "{}. {}{mark}",
                        slot.index + 1,
                        text::sanitize_core_text(&slot.label)
                    );
                    if row(ui, label, &mut overlay.focus_first) {
                        actions.push(UiAction::InsertDisk(slot.index));
                    }
                }
                if !disks.addable.is_empty() {
                    ui.label(RichText::new(text::DISKS_ADD).weak());
                    for path in &disks.addable {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if row(
                            ui,
                            format!("+ {}", text::sanitize_core_text(&name)),
                            &mut false,
                        ) {
                            actions.push(UiAction::AddDisk(path.clone()));
                        }
                    }
                }
            });
        ui.separator();
        if ui
            .add_enabled(
                disks.current.is_some(),
                Button::new(text::DISKS_EJECT).min_size([w, ROW_HEIGHT].into()),
            )
            .clicked()
        {
            actions.push(UiAction::EjectDisk);
        }
        let reset = disks.can_reset && row(ui, text::DISKS_RESET.to_owned(), &mut false);
        if row(ui, text::BACK.to_owned(), &mut overlay.focus_first) {
            actions.push(UiAction::Back);
        }
        if reset {
            // Set after the rows of this pass: the confirmation's safe answer gets the focus next pass.
            overlay.confirm_reset = true;
            overlay.focus_first = true;
        }
        ui.label(RichText::new(text::DISKS_HINT).small().weak());
    });
}
