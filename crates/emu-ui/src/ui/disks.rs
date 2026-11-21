//! Disk menu (ADR 0015 §5): list with the core's labels, insert, eject, add an image of the same folder, reset
//! the working copies; 2.1: added disks (remove), clean-up of unreferenced copies (checked rows only, nothing by
//! default), « Reprendre » an orphan. Every row is a focusable button or check box (keyboard / pad).

use egui::{Button, Color32, Context, RichText, ScrollArea, Ui};
use emu_runtime::OrphanKind;

use super::{CleanDialog, DisksView, Overlay, UiAction, UiData, nav, screen_window, text};

const WARN: Color32 = Color32::from_rgb(255, 190, 80);

const ROW_HEIGHT: f32 = 26.0;

pub(super) fn show(
    ctx: &Context,
    overlay: &mut Overlay,
    data: &UiData<'_>,
    actions: &mut Vec<UiAction>,
) {
    let disc = data.disks.is_some_and(|d| d.disc);
    screen_window(ctx, text::disks_title(disc), |ui, max_height| {
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
            ui.colored_label(WARN, text::DISKS_RESET_CONFIRM);
            if row(ui, text::NO.to_owned(), &mut overlay.focus_first) {
                overlay.confirm_reset = false;
            }
            if row(ui, text::DISKS_RESET_YES.to_owned(), &mut false) {
                overlay.confirm_reset = false;
                actions.push(UiAction::ResetDisks);
            }
            return;
        }
        if overlay.disk_forget.is_some() {
            forget_dialog(ui, w, overlay, disks, actions);
            return;
        }
        if let Some(mut clean) = overlay.clean.take() {
            if clean_dialog(
                ui,
                w,
                max_height,
                &mut overlay.focus_first,
                &mut clean,
                disks,
                actions,
            ) {
                overlay.clean = Some(clean);
            } else {
                overlay.focus_first = true;
            }
            return;
        }
        let mut open_forget = None;
        let status = match disks.current {
            None => text::DISKS_EMPTY_DRIVE.to_owned(),
            Some(i) => disks
                .slots
                .iter()
                .find(|s| s.index == i)
                .map_or_else(String::new, |s| {
                    text::media_inserted(disc, i, disks.slots.len(), &s.label)
                }),
        };
        ui.label(RichText::new(status).strong());
        ScrollArea::vertical()
            .max_height((max_height - 220.0).max(80.0))
            .show(ui, |ui| {
                if disks.slots.is_empty() {
                    ui.label(text::disks_none(disc));
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
                if !disks.added.is_empty() {
                    ui.label(RichText::new(text::DISKS_ADDED).weak());
                    for added in &disks.added {
                        let label = text::added_disk_label(
                            &added.name,
                            added.size,
                            added.source_missing,
                            added.inserted,
                        );
                        if row(ui, label, &mut false) {
                            open_forget = Some(added.copy.clone());
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
        let clean = disks.orphans.is_some() && row(ui, text::DISKS_CLEAN.to_owned(), &mut false);
        let reset = disks.can_reset && row(ui, text::DISKS_RESET.to_owned(), &mut false);
        if row(ui, text::BACK.to_owned(), &mut overlay.focus_first) {
            actions.push(UiAction::Back);
        }
        if reset {
            // Set after the rows of this pass: the confirmation's safe answer gets the focus next pass.
            overlay.confirm_reset = true;
            overlay.focus_first = true;
        }
        if clean {
            overlay.clean = Some(CleanDialog::default());
            overlay.focus_first = true;
        }
        if open_forget.is_some() {
            overlay.disk_forget = open_forget;
            overlay.focus_first = true;
        }
        ui.label(RichText::new(text::disks_hint(disc)).small().weak());
    });
}

fn button_row(ui: &mut Ui, w: f32, label: &str, enabled: bool, focus: &mut bool) -> bool {
    let resp = ui.add_enabled(enabled, Button::new(label).min_size([w, ROW_HEIGHT].into()));
    nav(&resp, focus);
    resp.clicked()
}

/// « Retirer » of one added disk; deleting the copy asks again (name, size, date shown).
fn forget_dialog(
    ui: &mut Ui,
    w: f32,
    overlay: &mut Overlay,
    disks: &DisksView,
    actions: &mut Vec<UiAction>,
) {
    let Some(disk) = disks
        .added
        .iter()
        .find(|d| Some(&d.copy) == overlay.disk_forget.as_ref())
    else {
        // The list changed under the dialog: back to it.
        overlay.disk_forget = None;
        overlay.focus_first = true;
        return;
    };
    ui.label(RichText::new(text::forget_title(&disk.name)).strong());
    if overlay.disk_delete_confirm {
        ui.colored_label(
            WARN,
            text::forget_delete_confirm(&disk.name, disk.size, disk.modified),
        );
        if button_row(ui, w, text::NO, true, &mut overlay.focus_first) {
            overlay.disk_delete_confirm = false;
            overlay.focus_first = true;
        }
        if button_row(ui, w, text::DISKS_FORGET_DELETE_YES, true, &mut false) {
            actions.push(UiAction::ForgetDisk {
                copy: disk.copy.clone(),
                delete: true,
            });
        }
        return;
    }
    if disk.inserted {
        ui.colored_label(WARN, text::DISKS_INSERTED_REFUSED);
    }
    if button_row(ui, w, text::CANCEL, true, &mut overlay.focus_first) {
        overlay.disk_forget = None;
        overlay.focus_first = true;
        return;
    }
    if button_row(ui, w, text::DISKS_FORGET, true, &mut false) {
        actions.push(UiAction::ForgetDisk {
            copy: disk.copy.clone(),
            delete: false,
        });
    }
    if button_row(ui, w, text::DISKS_FORGET_DELETE, true, &mut false) {
        overlay.disk_delete_confirm = true;
        overlay.focus_first = true;
    }
}

/// « Nettoyer les copies… »; returns `false` when the dialog is closed.
fn clean_dialog(
    ui: &mut Ui,
    w: f32,
    max_height: f32,
    focus_first: &mut bool,
    clean: &mut CleanDialog,
    disks: &DisksView,
    actions: &mut Vec<UiAction>,
) -> bool {
    let orphans = disks.orphans.as_deref().unwrap_or_default();
    // Only names still listed count (the folder may have changed since they were checked).
    clean
        .selected
        .retain(|name| orphans.iter().any(|o| &o.name == name));
    ui.label(RichText::new(text::DISKS_CLEAN_TITLE).strong());
    ui.label(text::DISKS_CLEAN_INTRO);
    if clean.confirm {
        let bytes = orphans
            .iter()
            .filter(|o| clean.selected.contains(&o.name))
            .map(|o| o.size)
            .sum();
        ui.colored_label(WARN, text::clean_confirm(clean.selected.len(), bytes));
        if button_row(ui, w, text::NO, true, focus_first) {
            clean.confirm = false;
            *focus_first = true;
        }
        if button_row(ui, w, text::DISKS_CLEAN_YES, true, &mut false) {
            actions.push(UiAction::CleanCopies(
                clean.selected.iter().cloned().collect(),
            ));
            clean.confirm = false;
        }
        return true;
    }
    ui.separator();
    if orphans.is_empty() {
        ui.label(text::DISKS_CLEAN_NONE);
    }
    ScrollArea::vertical()
        .max_height((max_height - 260.0).max(80.0))
        .show(ui, |ui| {
            for orphan in orphans {
                ui.horizontal(|ui| {
                    let mut checked = clean.selected.contains(&orphan.name);
                    let label = text::orphan_label(
                        &orphan.name,
                        orphan.kind == OrphanKind::StalePlaylist,
                        orphan.size,
                        orphan.modified,
                    );
                    let resp = ui.checkbox(&mut checked, label);
                    nav(&resp, focus_first);
                    if resp.changed() {
                        if checked {
                            clean.selected.insert(orphan.name.clone());
                        } else {
                            clean.selected.remove(&orphan.name);
                        }
                    }
                    if orphan.kind == OrphanKind::Copy && ui.button(text::DISKS_ADOPT).clicked() {
                        actions.push(UiAction::AdoptOrphan(orphan.name.clone()));
                    }
                });
            }
        });
    ui.separator();
    let count = clean.selected.len();
    let delete = button_row(
        ui,
        w,
        &text::clean_delete_button(count),
        count > 0,
        &mut false,
    );
    let back = button_row(ui, w, text::BACK, true, focus_first);
    if delete {
        // Set after the rows of this pass: the confirmation's safe answer gets the focus next pass.
        clean.confirm = true;
        *focus_first = true;
    }
    !back
}
