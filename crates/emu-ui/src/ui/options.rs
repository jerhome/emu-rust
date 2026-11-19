//! Core options screen: v2 categories, values changed with Left / Right and applied immediately.

use egui::{Button, Context, Grid, RichText, ScrollArea};

use super::{
    Overlay, UiAction, UiData, cycle_value, group_options, locked_banner, nav, screen_window,
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
        let groups = group_options(data.options, data.categories);
        let mut focused_info: Option<&str> = None;
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
                                    ui.label(&opt.label);
                                    let value = opt
                                        .values
                                        .iter()
                                        .find(|v| v.value == opt.current)
                                        .map_or(opt.current.as_str(), |v| v.label.as_str());
                                    let resp = ui.add(
                                        Button::new(format!("<  {value}  >"))
                                            .min_size([w * 0.45, ROW_HEIGHT].into()),
                                    );
                                    nav(&resp, &mut overlay.focus_first);
                                    if resp.has_focus() {
                                        focused_info = opt.info.as_deref();
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
        ui.label(RichText::new(focused_info.unwrap_or(text::OPTIONS_HINT)).weak());
        if !groups.is_empty() {
            let resp = ui.add(Button::new(text::OPTIONS_DEFAULTS).min_size([w, ROW_HEIGHT].into()));
            nav(&resp, &mut false);
            if resp.clicked() {
                actions.push(UiAction::DefaultOptions);
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
