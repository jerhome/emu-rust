//! « Clavier et souris » screen of a computer game: keyboard mode and capture (saved for this game), mouse
//! sensitivity (all games), host key reminder.

use egui::{Button, Context, RichText};

use super::{Overlay, UiAction, UiData, nav, screen_window, stepper, text};
use crate::computer::input::KeyboardMode;

const ROW_HEIGHT: f32 = 26.0;

pub(super) fn show(
    ctx: &Context,
    overlay: &mut Overlay,
    data: &UiData<'_>,
    actions: &mut Vec<UiAction>,
) {
    screen_window(ctx, text::COMPUTER_TITLE, |ui, _| {
        let w = ui.available_width();
        let Some(view) = data.computer else {
            return;
        };
        let value_row = |ui: &mut egui::Ui, title: &str, value: &str, focus: &mut bool| {
            ui.label(title);
            let resp =
                ui.add(Button::new(format!("<  {value}  >")).min_size([w, ROW_HEIGHT].into()));
            nav(&resp, focus);
            stepper(ui, &resp)
        };
        let mode = match view.mode {
            KeyboardMode::Keyboard => text::MODE_KEYBOARD,
            KeyboardMode::Joystick => text::MODE_JOYSTICK,
        };
        if value_row(ui, text::KEYBOARD_MODE, mode, &mut overlay.focus_first) != 0 {
            actions.push(UiAction::ToggleKeyboardMode);
        }
        let capture = if view.auto_capture {
            text::ON
        } else {
            text::OFF
        };
        if value_row(ui, text::AUTO_CAPTURE, capture, &mut false) != 0 {
            actions.push(UiAction::SetAutoCapture(!view.auto_capture));
        }
        let delta = value_row(
            ui,
            text::SENSITIVITY,
            &text::sensitivity_value(view.sensitivity),
            &mut false,
        );
        if delta != 0 {
            actions.push(UiAction::StepSensitivity(delta));
        }
        ui.separator();
        let resp = ui.add(Button::new(text::VKBD).min_size([w, ROW_HEIGHT].into()));
        nav(&resp, &mut false);
        if resp.clicked() {
            actions.push(UiAction::ToggleVirtualKeyboard);
        }
        let resp = ui.add(Button::new(text::BACK).min_size([w, ROW_HEIGHT].into()));
        nav(&resp, &mut false);
        if resp.clicked() {
            actions.push(UiAction::Back);
        }
        ui.label(
            RichText::new(text::host_key_line(view.host_key))
                .small()
                .weak(),
        );
        ui.label(RichText::new(text::host_hint(view.host_key)).small().weak());
    });
}
