//! In-game menu (menu key).

use egui::{Button, Context, RichText};

use super::{Overlay, UiAction, UiData, nav, screen_window, stepper, text};
use crate::Screen;
use crate::hotkeys::step_slot;

const ROW_HEIGHT: f32 = 28.0;

pub(super) fn show(
    ctx: &Context,
    overlay: &mut Overlay,
    data: &UiData<'_>,
    actions: &mut Vec<UiAction>,
) {
    let title = format!("{} — {}", text::MENU_TITLE, data.game);
    screen_window(ctx, &title, |ui, _| {
        let w = ui.available_width();
        let button = |ui: &mut egui::Ui, label: &str, enabled: bool, focus: &mut bool| {
            let resp = ui.add_enabled(enabled, Button::new(label).min_size([w, ROW_HEIGHT].into()));
            nav(&resp, focus);
            resp.clicked()
        };

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
    });
}
