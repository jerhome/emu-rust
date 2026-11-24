//! Core options screen: v2 categories, values changed with Left / Right and applied immediately.

use egui::{Button, Context, Grid, Key, RichText, ScrollArea, TextEdit};
use emu_runtime::M3uLaunch;

use super::{
    LaunchView, Overlay, UiAction, UiData, cycle_value, group_options_with, locked_banner, nav,
    screen_window, stepper, text,
};
use crate::computer::eightbit::validate_launch;

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
        if let Some(launch) = &data.eightbit.launch
            && launch_editor(ui, overlay, launch, w, actions)
        {
            return;
        }
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
        // Design 5.0 §4: the launch row follows the system's featured group (« Réglages CPC / C64 »); without
        // such a group it comes first.
        let launch_in_grid = data.featured.is_some_and(|f| {
            !f.keys.is_empty() && groups.first().is_some_and(|(t, _)| *t == f.title)
        });
        if !launch_in_grid {
            ui.horizontal(|ui| launch_row_ui(ui, data, overlay, w, false));
        }
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
                                if launch_in_grid && std::ptr::eq(title, &groups[0].0) {
                                    launch_row_ui(ui, data, overlay, w, true);
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

/// Edits the launch text of the game while `overlay.launch_edit` is set; `true` while the editor is shown. Enter
/// saves a valid text, the menu key cancels (`Overlay::back`).
fn launch_editor(
    ui: &mut egui::Ui,
    overlay: &mut Overlay,
    launch: &LaunchView,
    w: f32,
    actions: &mut Vec<UiAction>,
) -> bool {
    let Some(buffer) = overlay.launch_edit.as_mut() else {
        return false;
    };
    let (title, hint) = match launch.kind {
        M3uLaunch::Program => (text::LAUNCH_PROGRAM, text::LAUNCH_PROGRAM_HINT),
        _ => (text::LAUNCH_COMMAND, text::LAUNCH_COMMAND_HINT),
    };
    ui.label(RichText::new(title).strong());
    ui.label(RichText::new(hint).weak());
    let resp = ui.add(
        TextEdit::singleline(buffer)
            .char_limit(64)
            .desired_width(w)
            .font(egui::TextStyle::Monospace),
    );
    if std::mem::take(&mut overlay.focus_first) {
        resp.request_focus();
    }
    let checked = validate_launch(launch.kind, buffer.trim());
    if let Err(e) = &checked {
        ui.colored_label(crate::theme::WARN, e);
    }
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let save = ui.add_enabled(
        checked.is_ok(),
        Button::new(text::LAUNCH_SAVE).min_size([w, ROW_HEIGHT].into()),
    );
    if (save.clicked() || enter) && checked.is_ok() {
        actions.push(UiAction::SetLaunch(buffer.trim().to_owned()));
        overlay.launch_edit = None;
        overlay.focus_first = true;
    }
    let back = ui.add(Button::new(text::BACK).min_size([w, ROW_HEIGHT].into()));
    if back.clicked() {
        overlay.launch_edit = None;
        overlay.focus_first = true;
    }
    ui.label(RichText::new(text::NAV_HINT).small().weak());
    true
}

/// « Commande de démarrage… » / « Programme à lancer… » (nothing for the other systems). `in_grid`: two cells of
/// the options grid (label, value) instead of one full-width button.
fn launch_row_ui(
    ui: &mut egui::Ui,
    data: &UiData<'_>,
    overlay: &mut Overlay,
    w: f32,
    in_grid: bool,
) {
    let Some(launch) = &data.eightbit.launch else {
        return;
    };
    let label = match launch.kind {
        M3uLaunch::Program => text::LAUNCH_PROGRAM,
        _ => text::LAUNCH_COMMAND,
    };
    let resp = if in_grid {
        ui.label(label);
        let value = launch.current.as_deref().filter(|t| !t.is_empty());
        ui.add(Button::new(value.unwrap_or("-")).min_size([w * 0.45, ROW_HEIGHT].into()))
    } else {
        ui.add(
            Button::new(text::launch_row(label, launch.current.as_deref()))
                .min_size([w, ROW_HEIGHT].into()),
        )
    };
    nav(&resp, &mut false);
    if resp.clicked() {
        overlay.launch_edit = Some(launch.current.clone().unwrap_or_default());
        overlay.focus_first = true;
    }
}
