//! Key configuration screen: one row per libretro button, re-bound by pressing a key.

use egui::{Button, Context, Grid, RichText, ScrollArea};
use emu_runtime::MAX_PORTS;

use crate::keyboard::PadStyle;

use super::{
    Overlay, Profile, UiAction, UiData, locked_banner, nav, profile_map, screen_window, stepper,
    text,
};

const ROW_HEIGHT: f32 = 24.0;
pub(super) fn show(
    ctx: &Context,
    overlay: &mut Overlay,
    data: &UiData<'_>,
    actions: &mut Vec<UiAction>,
) {
    screen_window(ctx, text::KEYS, |ui, max_height| {
        locked_banner(ui, data, overlay, actions);
        let w = ui.available_width();
        let profile_text = match overlay.profile {
            Profile::Core => format!("{} ({})", text::PROFILE_CORE, data.core_id),
            Profile::Default => text::PROFILE_DEFAULT.to_owned(),
        };
        let resp = ui.add(
            Button::new(format!("{} :   <  {profile_text}  >", text::KEYS_PROFILE))
                .min_size([w, ROW_HEIGHT].into()),
        );
        nav(&resp, &mut overlay.focus_first);
        if stepper(ui, &resp) != 0 && overlay.capture.is_none() {
            overlay.profile = match overlay.profile {
                Profile::Core => Profile::Default,
                Profile::Default => Profile::Core,
            };
        }
        let resp = ui.add(
            Button::new(format!(
                "{} :   <  {}  >",
                text::KEYS_PLAYER,
                text::port_label(overlay.port)
            ))
            .min_size([w, ROW_HEIGHT].into()),
        );
        nav(&resp, &mut false);
        let delta = stepper(ui, &resp);
        if delta != 0 && overlay.capture.is_none() {
            let n = i32::try_from(MAX_PORTS).unwrap_or(1);
            let port = (i32::try_from(overlay.port).unwrap_or(0) + delta).rem_euclid(n);
            overlay.port = usize::try_from(port).unwrap_or(0);
        }
        ui.separator();

        let map = profile_map(data.config, data.core_id, overlay.profile);
        let port = overlay.port;
        let style = PadStyle::from_core_id(data.core_id);
        ScrollArea::vertical()
            .max_height((max_height - 220.0).max(80.0))
            .show(ui, |ui| {
                Grid::new("keys-grid")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for &button in style.buttons() {
                            ui.label(style.button_name(button));
                            let label = if overlay.capture == Some(button) {
                                "…".to_owned()
                            } else {
                                let keys = map.keys_for(port, button);
                                if keys.is_empty() {
                                    text::KEYS_UNBOUND.to_owned()
                                } else {
                                    keys.into_iter()
                                        .map(text::key_label)
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                }
                            };
                            let resp =
                                ui.add(Button::new(label).min_size([w * 0.6, ROW_HEIGHT].into()));
                            nav(&resp, &mut false);
                            if resp.clicked() {
                                overlay.capture = Some(button);
                                overlay.notice = Some(text::keys_capture(style, port, button));
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.separator();
        let notice = overlay.notice.as_deref().unwrap_or(text::KEYS_HINT);
        ui.label(RichText::new(notice).strong());
        let resp = ui.add(Button::new(text::KEYS_RESTORE).min_size([w, ROW_HEIGHT].into()));
        nav(&resp, &mut false);
        if resp.clicked() {
            actions.push(UiAction::RestoreKeys(overlay.profile));
        }
        let resp = ui.add(Button::new(text::BACK).min_size([w, ROW_HEIGHT].into()));
        nav(&resp, &mut false);
        if resp.clicked() {
            actions.push(UiAction::Back);
        }
        ui.label(RichText::new(text::NAV_HINT).small().weak());
    });
}
