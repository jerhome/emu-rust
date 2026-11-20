//! egui overlay: in-game menu, key configuration, core options, OSD and stats. Everything is reachable with the
//! keyboard (focus navigation: arrows / Tab, Enter or Space to activate, Left / Right on value rows), so a
//! gamepad can drive it later by injecting the same egui key events.
//!
//! The drawing code only reads `UiData` and returns `UiAction`s; the app applies them (no emulator access
//! from inside an egui pass).

mod keys;
mod menu;
mod options;
mod osd;
pub(crate) mod text;

use std::time::Instant;

use egui::{Align2, Color32, Context, EventFilter, Id, Key, LayerId, Order, Response, Ui};
use emu_runtime::{CoreOption, CoreOptionCategory, JoypadButton};
use winit::keyboard::KeyCode;

pub use self::osd::{Osd, OsdLevel};
use crate::Screen;
use crate::config::{Config, DEFAULT_PROFILE, Hotkeys};
use crate::hotkeys::HotkeyAction;
use crate::keyboard::{KeyMap, PadStyle};

/// Which `[input.keyboard.<profile>]` the key screen edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The running core's own profile (created from the effective layout on first edit).
    Core,
    Default,
}

impl Profile {
    pub fn key(self, core_id: &str) -> &str {
        match self {
            Profile::Core => core_id,
            Profile::Default => DEFAULT_PROFILE,
        }
    }
}

/// Layout shown (and edited) for `profile`.
pub fn profile_map(
    config: &Config,
    core_profile: &str,
    style: PadStyle,
    profile: Profile,
) -> KeyMap {
    match profile {
        Profile::Core => config.keymap_for(core_profile, style),
        Profile::Default => config
            .input
            .keyboard
            .get(DEFAULT_PROFILE)
            .cloned()
            .unwrap_or_else(|| KeyMap::default_for(style)),
    }
}

/// Result of a key press while the key screen waits for a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureOutcome {
    Cancelled,
    Cleared,
    /// The key is a frontend shortcut: shortcuts win over the game, so it cannot be bound.
    Refused(HotkeyAction),
    /// The button now has exactly this key. `moved_from`: the key was bound elsewhere (conflict resolved by
    /// moving it).
    Bound {
        moved_from: Option<(usize, JoypadButton)>,
    },
}

/// Applies a captured key to `map` for (`port`, `button`). The menu key cancels, Backspace clears.
pub fn apply_capture(
    map: &mut KeyMap,
    hotkeys: &Hotkeys,
    key: KeyCode,
    port: usize,
    button: JoypadButton,
) -> CaptureOutcome {
    if key == hotkeys.menu {
        return CaptureOutcome::Cancelled;
    }
    if key == KeyCode::Backspace {
        for k in map.keys_for(port, button) {
            map.unbind(k);
        }
        return CaptureOutcome::Cleared;
    }
    if let Some(action) = hotkeys.action_for(key) {
        return CaptureOutcome::Refused(action);
    }
    let moved_from = map
        .bindings()
        .iter()
        .find(|(k, p, b)| *k == key && (*p, *b) != (port, button))
        .map(|(_, p, b)| (*p, *b));
    for k in map.keys_for(port, button) {
        map.unbind(k);
    }
    map.bind(key, port, button);
    CaptureOutcome::Bound { moved_from }
}

/// Next value of `opt` after moving by `delta` (wraps); `None` if the option has no values.
pub fn cycle_value(opt: &CoreOption, delta: i32) -> Option<String> {
    let n = i32::try_from(opt.values.len()).ok().filter(|n| *n > 0)?;
    let i = opt
        .values
        .iter()
        .position(|v| v.value == opt.current)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(0);
    let next = usize::try_from((i + delta).rem_euclid(n)).ok()?;
    Some(opt.values[next].value.clone())
}

/// Visible options grouped by category: uncategorised ones first (« Général »), then categories in the core's
/// order. Empty groups are dropped.
pub fn group_options<'a>(
    options: &'a [CoreOption],
    categories: &'a [CoreOptionCategory],
) -> Vec<(String, Vec<&'a CoreOption>)> {
    let known = |c: &str| categories.iter().any(|k| k.key == c);
    let mut groups = vec![(
        text::OPTIONS_GENERAL.to_owned(),
        options
            .iter()
            .filter(|o| o.visible && o.category.as_deref().is_none_or(|c| !known(c)))
            .collect::<Vec<_>>(),
    )];
    for cat in categories {
        groups.push((
            cat.label.clone(),
            options
                .iter()
                .filter(|o| o.visible && o.category.as_deref() == Some(cat.key.as_str()))
                .collect(),
        ));
    }
    groups.retain(|(_, opts)| !opts.is_empty());
    groups
}

/// What the app must do after an egui pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Resume,
    Reset,
    SelectSlot(u8),
    SaveState,
    LoadState,
    Open(Screen),
    Back,
    Quit,
    SetOption {
        key: String,
        value: String,
    },
    DefaultOptions,
    RestoreKeys(Profile),
    /// Overwrite an invalid `config.toml` with the current settings (after confirmation).
    UnlockConfig,
}

/// F3 overlay figures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatsView {
    pub fps: f64,
    pub frame: u64,
    pub buffered_ms: Option<u128>,
    pub target_ms: Option<u128>,
    pub underruns: u64,
    pub dropped_samples: u64,
}

/// Read-only data for one egui pass.
pub struct UiData<'a> {
    pub game: &'a str,
    /// Key of the core profile in the config (see `PadStyle::profile_key`).
    pub core_id: &'a str,
    pub style: PadStyle,
    pub config: &'a Config,
    pub slot: u8,
    /// `created_unix` of each slot's state, `None` if the slot is empty (cached by the app).
    pub slots: &'a [Option<u64>],
    pub options: &'a [CoreOption],
    pub categories: &'a [CoreOptionCategory],
    /// Launched from the library: « Quitter » becomes « Retour à la bibliothèque ».
    pub hosted: bool,
    pub stats: Option<StatsView>,
    pub now: Instant,
}

/// Overlay state that lives across frames.
#[derive(Debug)]
pub struct Overlay {
    screen: Option<Screen>,
    focus_first: bool,
    profile: Profile,
    port: usize,
    capture: Option<JoypadButton>,
    notice: Option<String>,
    confirm_unlock: bool,
    pub osd: Osd,
    pub show_stats: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            screen: None,
            focus_first: false,
            profile: Profile::Core,
            port: 0,
            capture: None,
            notice: None,
            confirm_unlock: false,
            osd: Osd::default(),
            show_stats: false,
        }
    }
}

impl Overlay {
    pub fn screen(&self) -> Option<Screen> {
        self.screen
    }

    pub fn is_open(&self) -> bool {
        self.screen.is_some()
    }

    pub fn open(&mut self, screen: Screen) {
        self.screen = Some(screen);
        self.focus_first = true;
        self.capture = None;
        self.notice = None;
        self.confirm_unlock = false;
    }

    pub fn close(&mut self) {
        self.screen = None;
        self.capture = None;
    }

    /// Menu key / « Retour »: cancels a key capture, else goes up one level (sub-screen → menu → closed).
    pub fn back(&mut self) {
        if self.capture.take().is_some() {
            self.notice = Some(text::KEYS_CANCELLED.to_owned());
            return;
        }
        match self.screen {
            Some(Screen::Keys | Screen::CoreOptions) => self.open(Screen::Menu),
            Some(Screen::Menu) | None => self.close(),
        }
    }

    /// (profile, port, button) waiting for a key, if any. Key presses then bypass egui.
    pub fn capture(&self) -> Option<(Profile, usize, JoypadButton)> {
        self.capture.map(|b| (self.profile, self.port, b))
    }

    pub fn end_capture(&mut self, notice: String) {
        self.capture = None;
        self.notice = Some(notice);
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    /// Whether egui has anything to draw this frame.
    pub fn visible(&self) -> bool {
        self.is_open() || self.show_stats || !self.osd.is_empty()
    }
}

/// Dark theme with an unmistakable focus highlight (keyboard / pad navigation has no pointer to follow).
pub fn apply_style(ctx: &Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        // Screens appear at once (no fade): snappier with a pad, and `--screen` captures are deterministic.
        style.animation_time = 0.0;
        let focus = &mut style.visuals.widgets.active;
        focus.bg_fill = Color32::from_rgb(40, 90, 160);
        focus.weak_bg_fill = Color32::from_rgb(40, 90, 160);
        focus.bg_stroke = egui::Stroke::new(2.0, Color32::from_rgb(150, 200, 255));
        focus.fg_stroke = egui::Stroke::new(1.5, Color32::WHITE);
    });
}

/// Runs one egui pass: open screen, OSD, stats.
pub fn show(ctx: &Context, overlay: &mut Overlay, data: &UiData<'_>) -> Vec<UiAction> {
    let mut actions = Vec::new();
    if let Some(screen) = overlay.screen {
        ctx.layer_painter(LayerId::new(Order::Background, Id::unique("dim")))
            .rect_filled(ctx.content_rect(), 0.0, Color32::from_black_alpha(170));
        match screen {
            Screen::Menu => menu::show(ctx, overlay, data, &mut actions),
            Screen::Keys => keys::show(ctx, overlay, data, &mut actions),
            Screen::CoreOptions => options::show(ctx, overlay, data, &mut actions),
        }
    }
    osd::show(ctx, &overlay.osd, data.now);
    if overlay.show_stats
        && let Some(stats) = data.stats
    {
        osd::show_stats(ctx, &stats);
    }
    actions
}

/// Centered, fixed window used by every screen.
fn screen_window(ctx: &Context, title: &str, add: impl FnOnce(&mut Ui, f32)) {
    let area = ctx.content_rect();
    let width = (area.width() - 40.0).clamp(200.0, 620.0);
    let max_height = (area.height() - 90.0).max(120.0);
    egui::Window::new(title)
        .id(Id::unique("screen"))
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .min_width(width)
        .max_width(width)
        .show(ctx, |ui| add(ui, max_height));
}

/// Keyboard plumbing shared by all focusable rows: initial focus and keeping the focused row visible.
fn nav(resp: &Response, focus_first: &mut bool) {
    if std::mem::take(focus_first) {
        resp.request_focus();
    }
    if resp.gained_focus() {
        resp.scroll_to_me(None);
    }
}

/// A value row driven by Left / Right while focused (Enter or a click steps forward). Returns the step.
fn stepper(ui: &mut Ui, resp: &Response) -> i32 {
    let mut delta = 0;
    if resp.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                EventFilter {
                    horizontal_arrows: true,
                    ..EventFilter::default()
                },
            );
        });
        ui.input(|i| {
            delta -= i32::from(i.key_pressed(Key::ArrowLeft));
            delta += i32::from(i.key_pressed(Key::ArrowRight));
        });
    }
    if resp.clicked() {
        delta += 1;
    }
    delta
}

fn locked_banner(
    ui: &mut Ui,
    data: &UiData<'_>,
    overlay: &mut Overlay,
    actions: &mut Vec<UiAction>,
) {
    if !data.config.is_locked() {
        return;
    }
    ui.colored_label(Color32::from_rgb(255, 190, 80), text::CONFIG_LOCKED);
    if overlay.confirm_unlock {
        ui.horizontal(|ui| {
            ui.label(text::UNLOCK_CONFIRM);
            if ui.button(text::YES).clicked() {
                overlay.confirm_unlock = false;
                actions.push(UiAction::UnlockConfig);
            }
            if ui.button(text::NO).clicked() {
                overlay.confirm_unlock = false;
            }
        });
    } else if ui.button(text::UNLOCK).clicked() {
        overlay.confirm_unlock = true;
    }
    ui.separator();
}

#[cfg(test)]
mod tests;
