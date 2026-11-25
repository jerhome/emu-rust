//! egui overlay: in-game menu, key configuration, core options, OSD and stats. Everything is reachable with the
//! keyboard (focus navigation: arrows / Tab, Enter or Space to activate, Left / Right on value rows), so a
//! gamepad can drive it later by injecting the same egui key events.
//!
//! The drawing code only reads `UiData` and returns `UiAction`s; the app applies them (no emulator access
//! from inside an egui pass).

mod computer;
mod disks;
pub(crate) mod footer;
mod keys;
pub(crate) mod log_panel;
mod menu;
mod options;
mod osd;
pub(crate) mod text;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use egui::{Align2, Color32, Context, EventFilter, Id, Key, LayerId, Order, Response, Ui};
use emu_runtime::{
    AudioStatus, CoreOption, CoreOptionCategory, DiskSlot, JoypadButton, M3uLaunch, OrphanCopy,
};
use winit::keyboard::KeyCode;

pub use self::osd::{Osd, OsdLevel};
use crate::Screen;
use crate::computer::eightbit::TapeAction;
use crate::computer::input::KeyboardMode;
use crate::config::{Config, DEFAULT_PROFILE, Hotkeys};
use crate::hotkeys::HotkeyAction;
use crate::keyboard::{KeyMap, PadStyle, is_modifier_key};

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
    /// Ctrl / Alt / Super are reserved for the Ctrl+key and Alt+Enter shortcuts.
    RefusedModifier,
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
    if is_modifier_key(key) {
        return CaptureOutcome::RefusedModifier;
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
/// Options shown first, in this order, whatever their category (design v3.0 §4: model, Kickstart, video and
/// MultiDrive of puae / puae2021).
const PINNED_OPTIONS: &[&str] = &[
    "puae_model",
    "puae_kickstart",
    "puae_video_standard",
    "puae_floppy_multidrive",
];

#[cfg(test)]
pub fn group_options<'a>(
    options: &'a [CoreOption],
    categories: &'a [CoreOptionCategory],
) -> Vec<(String, Vec<&'a CoreOption>)> {
    group_options_with(options, categories, None)
}

/// [`group_options`] with the options of `cores.toml` `featured_options` for the game's system shown first under
/// their own title (« Réglages DOS », design v4.0 §4) instead of the built-in pinned list.
pub fn group_options_with<'a>(
    options: &'a [CoreOption],
    categories: &'a [CoreOptionCategory],
    featured: Option<&Featured>,
) -> Vec<(String, Vec<&'a CoreOption>)> {
    let known = |c: &str| categories.iter().any(|k| k.key == c);
    let (main_title, main_keys): (String, Vec<&str>) = match featured {
        Some(f) if !f.keys.is_empty() => {
            (f.title.clone(), f.keys.iter().map(String::as_str).collect())
        }
        _ => (text::OPTIONS_MAIN.to_owned(), PINNED_OPTIONS.to_vec()),
    };
    let pinned = |o: &CoreOption| main_keys.contains(&o.key.as_str());
    let mut groups = vec![
        (
            main_title.clone(),
            main_keys
                .iter()
                .filter_map(|k| options.iter().find(|o| o.visible && o.key == *k))
                .collect::<Vec<_>>(),
        ),
        (
            text::OPTIONS_GENERAL.to_owned(),
            options
                .iter()
                .filter(|o| {
                    o.visible && !pinned(o) && o.category.as_deref().is_none_or(|c| !known(c))
                })
                .collect::<Vec<_>>(),
        ),
    ];
    for cat in categories {
        groups.push((
            cat.label.clone(),
            options
                .iter()
                .filter(|o| {
                    o.visible && !pinned(o) && o.category.as_deref() == Some(cat.key.as_str())
                })
                .collect(),
        ));
    }
    groups.retain(|(_, opts)| !opts.is_empty());
    groups
}

/// Options shown first for the game's system (`CorePreferences::featured_options`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Featured {
    pub title: String,
    pub keys: Vec<String>,
}

/// What the app must do after an egui pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Resume,
    Reset,
    SelectSlot(u8),
    SaveState,
    LoadState,
    /// Load slot `n` after the player confirmed a state that may come from another file of the same name.
    ConfirmLoadState(u8),
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
    InsertDisk(u32),
    EjectDisk,
    AddDisk(PathBuf),
    /// Remove an added disk from the list (and delete its copy with `delete`, after confirmation).
    ForgetDisk {
        copy: PathBuf,
        delete: bool,
    },
    /// « Reprendre »: an unreferenced copy becomes an added disk again.
    AdoptOrphan(String),
    /// Delete the checked unreferenced copies (after confirmation).
    CleanCopies(Vec<String>),
    /// Delete the working copies and restart the game (after confirmation).
    ResetDisks,
    /// Restart the game (hosted: options that need it).
    Restart,
    /// DOS: reset with Shift held, the core shows its boot menu (« Changer de programme de démarrage »).
    ChangeStartProgram,
    ToggleVirtualKeyboard,
    ToggleKeyboardMode,
    SetAutoCapture(bool),
    StepSensitivity(i32),
    /// `true`: core option changes go to this game's options file.
    SetGameScope(bool),
    /// « Cassette » menu (design v5.0 §2.3): the core's tape key is pulsed.
    Tape(TapeAction),
    /// « Port joystick » (VICE, design v5.0 §2.5): 1 ⇄ 2, saved for this game.
    ToggleJoyport,
    /// « Commande de démarrage… » / « Programme à lancer… »: `[media] launch` of this game (empty: removed).
    SetLaunch(String),
    /// « Manette : Numérique / DualShock » (design v7.0 §2.4): applied at once, kept for this game.
    TogglePadMode,
}

/// 8-bit computer entries of the menus (design v5.0 §4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EightBitView {
    /// The core declares tape keys (`cores.toml` `actions`): « Cassette » row.
    pub tape: bool,
    /// VICE: « Port joystick » row.
    pub joyport: Option<JoyportView>,
    /// The core takes a launch text (`m3u_launch`): « Commande de démarrage… » / « Programme à lancer… ».
    pub launch: Option<LaunchView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoyportView {
    /// Value of `vice_joyport` (1 or 2).
    pub port: u8,
    /// `(j1)` / `(j2)` in the file name: the core ignores the option until it restarts.
    pub forced: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchView {
    pub kind: M3uLaunch,
    /// `[media] launch` of this game.
    pub current: Option<String>,
}

/// Computer settings shown by the menus.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputerView {
    pub mode: KeyboardMode,
    pub auto_capture: bool,
    pub sensitivity: f32,
    pub host_key: KeyCode,
}

/// Disk menu content (read once when the screen opens or after a change).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisksView {
    pub slots: Vec<DiskSlot>,
    pub current: Option<u32>,
    /// Images of the game's folder not in the list yet.
    pub addable: Vec<PathBuf>,
    /// Working copies exist and the game can be restarted (library).
    pub can_reset: bool,
    /// Disks added from the menu (this session or restored), in order of addition.
    pub added: Vec<AddedDiskView>,
    /// Unreferenced files of the working-copy folder; `None` without such a folder.
    pub orphans: Option<Vec<OrphanCopy>>,
    /// CDs (`disc = true` system): « CD » instead of « Disquettes » (design v6.0 §3.5).
    pub disc: bool,
}

/// One added disk of the disk menu.
#[derive(Debug, Clone, PartialEq)]
pub struct AddedDiskView {
    pub copy: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// The file it was copied from is gone (the progress lives in the copy).
    pub source_missing: bool,
    pub inserted: bool,
}

/// « Nettoyer les copies… » dialog: nothing is checked by default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanDialog {
    pub selected: BTreeSet<String>,
    pub confirm: bool,
}

/// A state load waiting for the player's answer (default « Non »).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmLoad {
    pub slot: u8,
    /// Asked from the menu: « Non » goes back to it; from a shortcut: « Non » resumes the game.
    pub from_menu: bool,
    /// The game has disks (the warning says they may be modified).
    pub has_disks: bool,
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
    pub audio_status: AudioStatus,
    pub recoveries: u64,
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
    /// Computer system: host key, keyboard mode, mouse.
    pub computer: Option<&'a ComputerView>,
    /// `None`: the core has no disk control interface.
    pub disks: Option<&'a DisksView>,
    /// Core option changes apply to this game only.
    pub game_scope: bool,
    /// This game's core option overrides.
    pub game_overrides: &'a BTreeMap<String, String>,
    /// DOS game (dosbox_pure): boot program and « modifications du jeu » entries of the menu.
    pub dos: bool,
    /// Options shown first (« Réglages DOS »).
    pub featured: Option<&'a Featured>,
    /// Hint shown under the MIDI option when the free SoundFont is not installed.
    pub soundfont_hint: bool,
    /// CPC / C64 entries (empty for the other systems).
    pub eightbit: &'a EightBitView,
    /// Core with pad modes (PlayStation, design v7.0 §2.4): `Some(true)` = DualShock, `Some(false)` = digital pad.
    pub pad_dualshock: Option<bool>,
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
    confirm_reset: bool,
    /// « Réinitialiser les modifications du jeu… » asked from the menu (DOS).
    confirm_dos_reset: bool,
    /// Added disk whose « Retirer » dialog is open.
    disk_forget: Option<PathBuf>,
    /// « Retirer et supprimer la copie » asks first.
    disk_delete_confirm: bool,
    clean: Option<CleanDialog>,
    confirm_load: Option<ConfirmLoad>,
    /// Launch text being edited on the options screen (design v5.0 §4).
    launch_edit: Option<String>,
    pub osd: Osd,
    help: crate::help::HelpState,
    /// « Journal » screen (`Screen::Journal`).
    log_panel: log_panel::LogPanel,
    /// Set by the app each frame: the virtual keyboard covers the bottom, the toasts move to the top.
    pub vkbd_open: bool,
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
            confirm_reset: false,
            confirm_dos_reset: false,
            disk_forget: None,
            disk_delete_confirm: false,
            clean: None,
            confirm_load: None,
            launch_edit: None,
            osd: Osd::default(),
            help: crate::help::HelpState::default(),
            log_panel: log_panel::LogPanel::without_heading(),
            vkbd_open: false,
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
        if screen == Screen::Help && self.screen != Some(Screen::Help) {
            // In-game: the useful sections first (keys, then saves).
            self.help.open(Some("clavier"));
        }
        if screen == Screen::Journal {
            self.log_panel.open();
        }
        self.screen = Some(screen);
        self.focus_first = true;
        self.capture = None;
        self.notice = None;
        self.confirm_unlock = false;
        self.confirm_reset = false;
        self.confirm_dos_reset = false;
        self.disk_forget = None;
        self.disk_delete_confirm = false;
        self.clean = None;
        self.confirm_load = None;
        self.launch_edit = None;
    }

    /// Opens the confirmation of a state load on the menu screen (call after `open(Screen::Menu)` when asked
    /// from a shortcut). The safe answer gets the focus.
    pub fn ask_load_confirmation(&mut self, confirm: ConfirmLoad) {
        self.confirm_load = Some(confirm);
        self.focus_first = true;
    }

    /// Closes the disk sub-dialogs after an action was applied (list refreshed by the app).
    pub fn end_disk_dialog(&mut self) {
        self.disk_forget = None;
        self.disk_delete_confirm = false;
        self.clean = None;
        self.focus_first = true;
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
            Some(Screen::Menu) if self.confirm_load.is_some() => {
                let from_menu = self.confirm_load.take().is_some_and(|c| c.from_menu);
                if from_menu {
                    self.focus_first = true;
                } else {
                    self.close();
                }
            }
            Some(Screen::Menu) if self.confirm_dos_reset => {
                self.confirm_dos_reset = false;
                self.focus_first = true;
            }
            Some(Screen::CoreOptions) if self.launch_edit.is_some() => {
                self.launch_edit = None;
                self.focus_first = true;
            }
            Some(Screen::Disks) if self.confirm_reset => self.confirm_reset = false,
            Some(Screen::Disks) if self.disk_delete_confirm => {
                self.disk_delete_confirm = false;
                self.focus_first = true;
            }
            Some(Screen::Disks) if self.disk_forget.is_some() => {
                self.disk_forget = None;
                self.focus_first = true;
            }
            Some(Screen::Disks) if self.clean.as_ref().is_some_and(|c| c.confirm) => {
                if let Some(c) = &mut self.clean {
                    c.confirm = false;
                }
                self.focus_first = true;
            }
            Some(Screen::Disks) if self.clean.is_some() => {
                self.clean = None;
                self.focus_first = true;
            }
            Some(
                Screen::Keys
                | Screen::CoreOptions
                | Screen::Disks
                | Screen::Computer
                | Screen::Help
                | Screen::Journal,
            ) => {
                self.open(Screen::Menu);
            }
            Some(Screen::Menu | Screen::VirtualKeyboard) | None => self.close(),
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

/// Dark theme with an unmistakable focus highlight (see `theme`).
pub fn apply_style(ctx: &Context) {
    crate::theme::apply(ctx);
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
            Screen::Disks => disks::show(ctx, overlay, data, &mut actions),
            Screen::Computer => computer::show(ctx, overlay, data, &mut actions),
            Screen::Help => {
                let rect = ctx.content_rect();
                let closed = egui::Area::new(Id::unique("in-game-help"))
                    .order(Order::Foreground)
                    .fixed_pos(rect.min)
                    .show(ctx, |ui| {
                        ui.set_min_size(rect.size());
                        ui.set_max_size(rect.size());
                        crate::help::show(ui, crate::help::manual(), &mut overlay.help)
                    })
                    .inner;
                if closed {
                    overlay.open(Screen::Menu);
                }
            }
            Screen::Journal => {
                screen_window(ctx, text::LOG_TITLE, |ui, max_height| {
                    ui.set_height(max_height.min(460.0));
                    // Escape goes through `Overlay::back` (menu key), like every sub-screen.
                    if overlay.log_panel.show(ui, false) == Some(log_panel::LogEvent::Close) {
                        overlay.open(Screen::Menu);
                    }
                });
            }
            Screen::VirtualKeyboard => {}
        }
    }
    osd::show(
        ctx,
        &overlay.osd,
        data.now,
        overlay.screen == Some(Screen::VirtualKeyboard) || overlay.vkbd_open,
    );
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
pub(crate) fn stepper(ui: &mut Ui, resp: &Response) -> i32 {
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
    ui.colored_label(crate::theme::WARN, text::CONFIG_LOCKED);
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
