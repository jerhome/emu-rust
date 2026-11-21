//! Core options screen: v2 categories, values changed with Left / Right and applied immediately.

use egui::{Button, Context, Grid, RichText, ScrollArea};

use super::{
    Overlay, UiAction, UiData, cycle_value, group_options_with, locked_banner, nav, screen_window,
    stepper, text,
};

const ROW_HEIGHT: f32 = 24.0;

pub(super) fn show(
    ctx: &Context,
    overlay: &mut Overlay,
    data: &UiData<'_>,
    actions: &mut Vec<UiAction>,
) {
    screen_window(ctx, text::CORE_OPTIONS, |ui, max_height| {
        locked_banner(ui, data, overlay, actions);
        let w = ui.available_width();
        let scope = if data.game_scope {
            text::SCOPE_GAME
        } else {
            text::SCOPE_ALL
        };
        ui.horizontal(|ui| {
            ui.label(text::OPTIONS_SCOPE);
            let resp = ui
                .add(Button::new(format!("<  {scope}  >")).min_size([w * 0.45, ROW_HEIGHT].into()));
            nav(&resp, &mut false);
            if stepper(ui, &resp) != 0 {
                actions.push(UiAction::SetGameScope(!data.game_scope));
            }
        });
        let groups = group_options_with(data.options, data.categories, data.featured);
        let mut focused_info: Option<String> = None;
        if groups.is_empty() {
            ui.label(text::OPTIONS_NONE);
        } else {
            ScrollArea::vertical()
                .max_height((max_height - 170.0).max(80.0))
                .show(ui, |ui| {
                    // One grid for all groups so the value column lines up.
                    Grid::new("options-grid")
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            for (title, opts) in &groups {
                                ui.label(RichText::new(title).strong().size(16.0));
                                ui.end_row();
                                for opt in opts {
                                    let label = text::sanitize_core_text(&opt.label);
                                    if data.game_overrides.contains_key(&opt.key) {
                                        ui.label(format!("{label} {}", text::OPTION_GAME_MARK));
                                    } else {
                                        ui.label(label);
                                    }
                                    let value = text::sanitize_core_text(
                                        opt.values
                                            .iter()
                                            .find(|v| v.value == opt.current)
                                            .map_or(opt.current.as_str(), |v| v.label.as_str()),
                                    );
                                    let resp = ui.add(
                                        Button::new(format!("<  {value}  >"))
                                            .min_size([w * 0.45, ROW_HEIGHT].into()),
                                    );
                                    nav(&resp, &mut overlay.focus_first);
                                    if resp.has_focus() {
                                        focused_info =
                                            opt.info.as_deref().map(text::sanitize_core_text);
                                    }
                                    if resp.has_focus()
                                        && data.soundfont_hint
                                        && opt.key == crate::computer::dos::MIDI_OPTION
                                    {
                                        focused_info = Some(text::DOS_SOUNDFONT_HINT.to_owned());
                                    }
                                    let delta = stepper(ui, &resp);
                                    if delta != 0
                                        && let Some(value) = cycle_value(opt, delta)
                                    {
                                        actions.push(UiAction::SetOption {
                                            key: opt.key.clone(),
                                            value,
                                        });
                                    }
                                    ui.end_row();
                                }
                            }
                        });
                });
        }
        ui.separator();
        // Shown for the focused row: no hover needed.
        ui.label(RichText::new(focused_info.as_deref().unwrap_or(text::OPTIONS_HINT)).weak());
        if !groups.is_empty() {
            let resp = ui.add(Button::new(text::OPTIONS_DEFAULTS).min_size([w, ROW_HEIGHT].into()));
            nav(&resp, &mut false);
            if resp.clicked() {
                actions.push(UiAction::DefaultOptions);
            }
        }
        if data.hosted {
            let resp = ui.add(Button::new(text::RESTART_GAME).min_size([w, ROW_HEIGHT].into()));
            nav(&resp, &mut false);
            if resp.clicked() {
                actions.push(UiAction::Restart);
            }
        }
        let resp = ui.add(Button::new(text::BACK).min_size([w, ROW_HEIGHT].into()));
        nav(&resp, &mut overlay.focus_first);
        if resp.clicked() {
            actions.push(UiAction::Back);
        }
        ui.label(RichText::new(text::NAV_HINT).small().weak());
    });
}
