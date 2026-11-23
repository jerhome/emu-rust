//! In-game menu (menu key).

use egui::{Button, Color32, Context, RichText};

use super::log_panel::LogEvent;
use super::{OsdLevel, Overlay, UiAction, UiData, nav, screen_window, stepper, text};
use crate::Screen;
use crate::computer::eightbit::TapeAction;
use crate::hotkeys::step_slot;

const ROW_HEIGHT: f32 = 28.0;

pub(super) fn show(
    ctx: &Context,
    overlay: &mut Overlay,
    data: &UiData<'_>,
    actions: &mut Vec<UiAction>,
) {
    let title = format!("{} — {}", text::MENU_TITLE, data.game);
    if overlay.log_open {
        screen_window(ctx, text::LOG_TITLE, |ui, max_height| {
            ui.set_height(max_height.min(420.0));
            if overlay.log_panel.show(ui, false) == Some(LogEvent::Close) {
                overlay.log_open = false;
                overlay.focus_first = true;
            }
        });
        return;
    }
    screen_window(ctx, &title, |ui, _| {
        let w = ui.available_width();
        let button = |ui: &mut egui::Ui, label: &str, enabled: bool, focus: &mut bool| {
            let resp = ui.add_enabled(enabled, Button::new(label).min_size([w, ROW_HEIGHT].into()));
            nav(&resp, focus);
            resp.clicked()
        };

        if let Some(confirm) = overlay.confirm_load {
            ui.label(RichText::new(text::STATE_CONFIRM_TITLE).strong());
            ui.colored_label(
                Color32::from_rgb(255, 190, 80),
                text::state_confirm(confirm.slot, confirm.has_disks),
            );
            // The safe answer comes first and has the focus.
            if button(ui, text::NO, true, &mut overlay.focus_first) {
                overlay.confirm_load = None;
                if confirm.from_menu {
                    overlay.focus_first = true;
                } else {
                    actions.push(UiAction::Resume);
                }
            }
            if button(ui, text::STATE_LOAD_YES, true, &mut false) {
                overlay.confirm_load = None;
                actions.push(UiAction::ConfirmLoadState(confirm.slot));
            }
            ui.label(RichText::new(text::NAV_HINT).small().weak());
            return;
        }

        if overlay.confirm_dos_reset {
            ui.label(RichText::new(text::DOS_RESET_TITLE).strong());
            ui.colored_label(Color32::from_rgb(255, 190, 80), text::DOS_RESET_CONFIRM);
            if button(ui, text::NO, true, &mut overlay.focus_first) {
                overlay.confirm_dos_reset = false;
                overlay.focus_first = true;
            }
            if button(ui, text::DOS_RESET_YES, true, &mut false) {
                overlay.confirm_dos_reset = false;
                actions.push(UiAction::ResetDisks);
            }
            ui.label(RichText::new(text::NAV_HINT).small().weak());
            return;
        }

        if button(ui, text::RESUME, true, &mut overlay.focus_first) {
            actions.push(UiAction::Resume);
        }
        if button(ui, text::RESET, true, &mut false) {
            actions.push(UiAction::Reset);
        }
        ui.separator();

        let saved = data.slots.get(usize::from(data.slot)).copied().flatten();
        let resp = ui.add(
            Button::new(format!(
                "{} :   <  {}  >",
                text::SLOT,
                text::slot_value(data.slot)
            ))
            .min_size([w, ROW_HEIGHT].into()),
        );
        nav(&resp, &mut false);
        let delta = stepper(ui, &resp);
        if delta != 0 {
            let step = i8::try_from(delta.clamp(-1, 1)).unwrap_or(0);
            actions.push(UiAction::SelectSlot(step_slot(data.slot, step)));
        }
        let info = saved.map_or_else(
            || text::SLOT_EMPTY.to_owned(),
            |t| text::slot_saved_at(&text::format_unix_utc(t)),
        );
        ui.label(RichText::new(info).weak());
        if button(ui, text::SAVE_STATE, true, &mut false) {
            actions.push(UiAction::SaveState);
        }
        if button(ui, text::LOAD_STATE, saved.is_some(), &mut false) {
            actions.push(UiAction::LoadState);
        }
        ui.separator();

        if data.dos {
            if button(ui, text::DOS_CHANGE_PROGRAM, true, &mut false) {
                actions.push(UiAction::ChangeStartProgram);
            }
            // Restarting goes through the library (the game is stopped before its files are deleted).
            if data.hosted && button(ui, text::DOS_RESET, true, &mut false) {
                overlay.confirm_dos_reset = true;
                overlay.focus_first = true;
            }
        }
        if let Some(disks) = data.disks
            && button(ui, text::disks_menu(disks.disc), true, &mut false)
        {
            actions.push(UiAction::Open(Screen::Disks));
        }
        if data.eightbit.tape {
            ui.horizontal(|ui| {
                ui.label(text::TAPE);
                for (action, label) in [
                    (TapeAction::Play, text::TAPE_PLAY),
                    (TapeAction::Stop, text::TAPE_STOP),
                    (TapeAction::Rewind, text::TAPE_REWIND),
                ] {
                    let resp = ui.add(Button::new(label).min_size([0.0, ROW_HEIGHT].into()));
                    nav(&resp, &mut false);
                    if resp.clicked() {
                        actions.push(UiAction::Tape(action));
                    }
                }
            });
        }
        if let Some(joy) = data.eightbit.joyport {
            let label = text::joyport_row(joy.forced.unwrap_or(joy.port), joy.forced.is_some());
            if button(ui, &label, joy.forced.is_none(), &mut false) {
                actions.push(UiAction::ToggleJoyport);
            }
        }
        if let Some(dualshock) = data.pad_dualshock
            && button(ui, &text::pad_mode_row(dualshock), true, &mut false)
        {
            actions.push(UiAction::TogglePadMode);
        }
        if data.computer.is_some() {
            if button(ui, text::VKBD, true, &mut false) {
                actions.push(UiAction::ToggleVirtualKeyboard);
            }
            if button(ui, text::COMPUTER, true, &mut false) {
                actions.push(UiAction::Open(Screen::Computer));
            }
        }
        if button(ui, text::KEYS, true, &mut false) {
            actions.push(UiAction::Open(Screen::Keys));
        }
        if button(ui, text::CORE_OPTIONS, true, &mut false) {
            actions.push(UiAction::Open(Screen::CoreOptions));
        }
        ui.separator();
        let quit = if data.hosted {
            text::BACK_TO_LIBRARY
        } else {
            text::QUIT
        };
        if button(ui, quit, true, &mut false) {
            actions.push(UiAction::Quit);
        }
        ui.label(RichText::new(text::NAV_HINT).small().weak());
        recent_messages(ui, &overlay.osd);
        ui.separator();
        if super::footer::contents(ui) {
            overlay.log_panel.open();
            overlay.log_open = true;
        }
    });
}

/// Latest toasts, selectable, with one « Copier » for all of them; hidden when there is none.
fn recent_messages(ui: &mut egui::Ui, osd: &super::Osd) {
    if osd.history().next().is_none() {
        return;
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new(text::RECENT_MESSAGES).strong());
        crate::copyable::copy_button(ui, &osd.history_text());
    });
    for entry in osd.history() {
        let color = match entry.level {
            OsdLevel::Info => Color32::LIGHT_GRAY,
            OsdLevel::Warn => Color32::from_rgb(255, 210, 90),
            OsdLevel::Error => Color32::from_rgb(255, 110, 100),
        };
        ui.add(egui::Label::new(RichText::new(entry.line()).small().color(color)).selectable(true));
    }
}
