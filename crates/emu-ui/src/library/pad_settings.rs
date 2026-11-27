//! Settings › Manette: attached pads, deadzone, trigger threshold, hotkey button, and « press a button » remap
//! per system, and the learning of an unrecognized pad (7.2). The pad backend lives in the host loop
//! (`library/host.rs`); this page only sees snapshots and exchanges facts with it through the `pad_*` methods.

use egui::{ComboBox, RichText, Slider, Ui};
use emu_runtime::JoypadButton;
use std::collections::BTreeMap;

use super::{Level, LibraryUi, text, weak};
use crate::config_gamepad::{GamepadConfig, HotkeyButton};
use crate::gamepad::{
    LearnProgress, LearnSession, LearnedMap, PadButton, PadId, PadLayout, PadMap, RawInput,
    UnrecognizedPad, learned_maps, set_binding,
};
use crate::keyboard::PadStyle;
use crate::pad_mode::PadMode;
use crate::theme;

/// Systems of the remap page: label, core id (profile key base), pad style, library system id.
const SYSTEMS: [(&str, &str, PadStyle, &str); 6] = [
    (
        "Mega Drive",
        "genesis_plus_gx",
        PadStyle::MegaDrive,
        "megadrive",
    ),
    (
        "Master System",
        "genesis_plus_gx",
        PadStyle::MasterSystem,
        "mastersystem",
    ),
    (
        "Game Gear",
        "genesis_plus_gx",
        PadStyle::GameGear,
        "gamegear",
    ),
    ("Super Nintendo", "snes9x", PadStyle::Snes, "snes"),
    ("Game Boy", "gambatte", PadStyle::GameBoy, "gb"),
    ("NES", "mesen", PadStyle::Nes, "nes"),
];

/// Profile key of a remap-page entry: the one the game of that system reads (`profile_key_for`).
fn profile_of(system_id: &str, style: PadStyle, core_id: &str) -> String {
    emu_library::system(system_id).map_or_else(
        || style.profile_key(core_id),
        |system| crate::keyboard::profile_key_for(system, style, core_id),
    )
}

const DPAD_TARGETS: [JoypadButton; 4] = [
    JoypadButton::Up,
    JoypadButton::Down,
    JoypadButton::Left,
    JoypadButton::Right,
];

#[derive(Default)]
pub(super) struct PadUi {
    /// A backend exists (not `--no-gamepad`, not disabled, started).
    available: bool,
    pads: Vec<(usize, String)>,
    system: usize,
    /// Game button waiting for a physical press.
    learning: Option<JoypadButton>,
    /// Slider / toggle edited: the host applies the new settings to the live backend.
    settings_changed: bool,
    /// Attached pads that can be configured (id, GUID, name, has a learned map).
    learnable: Vec<LearnPad>,
    learn: Option<LearnScreen>,
    /// A learned map was saved or forgotten: the host reloads `learned_maps`.
    learned_changed: bool,
    /// « Manette non reconnue » notices not yet acted upon, oldest first (one per connection).
    unrecognized: Vec<LearnPad>,
    /// Stage of the learning screen drawn last (0 choose, 1 running, 2 review): a new stage moves the keyboard
    /// focus to its main button, so the whole screen is usable without the mouse.
    shown_stage: Option<u8>,
}

/// A pad offered by the « Configurer » list; filled by the host from `GamepadInput::pads()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LearnPad {
    pub id: PadId,
    pub guid: String,
    pub name: String,
}

enum Stage {
    Choose,
    /// `current() == None` is the review stage.
    Running(LearnSession),
}

/// State of the learning screen; the egui view only reads it and calls its methods.
pub(super) struct LearnScreen {
    pad: Target,
    layout: PadLayout,
    stage: Stage,
    notice: Option<String>,
}

struct Target {
    id: PadId,
    guid: String,
    name: String,
}

impl LearnScreen {
    fn new(pad: &LearnPad) -> Self {
        Self {
            pad: Target {
                id: pad.id,
                guid: pad.guid.clone(),
                name: pad.name.clone(),
            },
            layout: PadLayout::Snes,
            stage: Stage::Choose,
            notice: None,
        }
    }

    fn begin(&mut self) {
        self.stage = Stage::Running(LearnSession::new(self.layout));
        self.notice = None;
    }

    fn session(&self) -> Option<&LearnSession> {
        match &self.stage {
            Stage::Running(s) => Some(s),
            Stage::Choose => None,
        }
    }

    /// Index of the current step, or the step count when everything is done.
    fn position(&self) -> usize {
        let steps = self.layout.steps();
        self.session()
            .and_then(|s| s.current())
            .and_then(|c| steps.iter().position(|x| x.button == c.button))
            .unwrap_or(steps.len())
    }

    fn is_review(&self) -> bool {
        self.session().is_some_and(|s| s.current().is_none())
    }

    fn feed(&mut self, input: RawInput, value: f32) {
        let layout = self.layout;
        let Stage::Running(session) = &mut self.stage else {
            return;
        };
        match session.feed(input, value) {
            LearnProgress::AlreadyUsed(button) => {
                let label = layout
                    .steps()
                    .iter()
                    .find(|s| s.button == button)
                    .map_or("", |s| s.label);
                self.notice = Some(text::learn_used(label));
            }
            LearnProgress::Recorded | LearnProgress::Done => self.notice = None,
            LearnProgress::Waiting => {}
        }
    }

    fn skip(&mut self) {
        if let Stage::Running(s) = &mut self.stage {
            s.skip();
            self.notice = None;
        }
    }

    fn back(&mut self) {
        if let Stage::Running(s) = &mut self.stage {
            s.back();
            self.notice = None;
        }
    }

    /// The map as it stands (review stage), consuming the screen.
    fn finish(self) -> Option<(LearnPad, PadLayout, LearnedMap)> {
        let Stage::Running(session) = self.stage else {
            return None;
        };
        let pad = LearnPad {
            id: self.pad.id,
            guid: self.pad.guid,
            name: self.pad.name,
        };
        Some((pad, self.layout, session.finish()))
    }
}

/// Writes a learned map under the pad's GUID, keeping the other device fields (port, deadzone, …).
pub(super) fn store_learned(
    config: &mut GamepadConfig,
    pad: &LearnPad,
    layout: PadLayout,
    map: &LearnedMap,
) {
    let d = config.devices.entry(pad.guid.clone()).or_default();
    d.name = Some(pad.name.clone());
    d.layout = Some(layout.config_name().to_owned());
    d.map = map.to_config();
}

/// Removes what learning wrote; `true` when there was something to forget.
pub(super) fn forget_learned(config: &mut GamepadConfig, guid: &str) -> bool {
    // Learned maps are keyed by lowercase GUID; a hand-edited config may use another case.
    let Some(key) = config
        .devices
        .keys()
        .find(|k| k.eq_ignore_ascii_case(guid))
        .cloned()
    else {
        return false;
    };
    let guid = key.as_str();
    let Some(d) = config.devices.get_mut(guid) else {
        return false;
    };
    let had = !d.map.is_empty() || d.layout.is_some() || d.name.is_some();
    d.map.clear();
    d.layout = None;
    d.name = None;
    if *d == Default::default() {
        config.devices.remove(guid);
    }
    had
}

/// Cores whose `pad_devices` offer a DualShock mode (PlayStation), catalog order.
pub(super) fn dualshock_cores(prefs: &emu_assets::cores::CorePreferences) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, cores) in prefs.systems() {
        for core in cores {
            let has = prefs
                .pad_devices(core)
                .is_some_and(|d| d.modes.contains_key(PadMode::DualShock.key()));
            if has && !out.iter().any(|c| c == core.as_str()) {
                out.push(core.as_str().to_owned());
            }
        }
    }
    out
}

/// Digital is the default: it is written as no entry at all (a 6.0 `config.toml` stays unchanged).
pub(super) fn set_dualshock_default(
    map: &mut std::collections::BTreeMap<String, String>,
    cores: &[String],
    dualshock: bool,
) {
    for core in cores {
        if dualshock {
            map.insert(core.clone(), PadMode::DualShock.key().to_owned());
        } else {
            map.remove(core);
        }
    }
}

/// Binds `physical` to `target` for the system. Other physical buttons (not the D-pad) that were bound to
/// `target` are freed, so the button has a single owner.
pub(super) fn remap(
    config: &mut GamepadConfig,
    style: PadStyle,
    profile: &str,
    target: JoypadButton,
    physical: PadButton,
) {
    let map = PadMap::from_config(style, profile, config);
    for (other, t) in map.bindings() {
        if *t == target && *other != physical && !is_dpad(*other) {
            set_binding(config, style, profile, *other, None);
        }
    }
    set_binding(config, style, profile, physical, Some(target));
}

fn is_dpad(button: PadButton) -> bool {
    matches!(
        button,
        PadButton::DPadUp | PadButton::DPadDown | PadButton::DPadLeft | PadButton::DPadRight
    )
}

/// Game buttons offered for a system: the non-directional targets of its default layout, in layout order.
pub(super) fn remappable_targets(style: PadStyle) -> Vec<JoypadButton> {
    let mut out = Vec::new();
    for (_, target) in PadMap::default_for(style).bindings() {
        if !DPAD_TARGETS.contains(target) && !out.contains(target) {
            out.push(*target);
        }
    }
    out
}

impl LibraryUi {
    /// Host -> page: backend state and attached pads, refreshed on every pad poll.
    pub(crate) fn set_pad_status(&mut self, available: bool, pads: Vec<(usize, String)>) {
        self.pad.available = available;
        self.pad.pads = pads;
    }

    /// The remap is waiting for a physical button: the host puts the backend in learning mode.
    pub(crate) fn pad_wants_learning(&self) -> bool {
        self.pad.learning.is_some()
    }

    pub(crate) fn pad_learned(&mut self, physical: PadButton) {
        let Some(target) = self.pad.learning.take() else {
            return;
        };
        if physical == PadButton::Mode {
            self.set_status(text::PAD_REMAP_MODE.to_owned(), Level::Warn);
            return;
        }
        let (_, core_id, style, system_id) = SYSTEMS[self.pad.system.min(SYSTEMS.len() - 1)];
        let profile = profile_of(system_id, style, core_id);
        remap(
            &mut self.config.input.gamepad,
            style,
            &profile,
            target,
            physical,
        );
        self.settings.dirty = true;
    }

    /// `true` once after a setting that the live backend must re-read was edited.
    pub(crate) fn take_pad_settings_changed(&mut self) -> bool {
        std::mem::take(&mut self.pad.settings_changed)
    }

    /// Host -> page: pads that can be configured, refreshed with `set_pad_status`. A notice or a learning
    /// session of a pad that is no longer there is dropped.
    pub(crate) fn set_learnable_pads(&mut self, pads: Vec<LearnPad>) {
        self.pad.learnable = pads;
        let learnable = &self.pad.learnable;
        self.pad
            .unrecognized
            .retain(|u| learnable.iter().any(|p| p.id == u.id));
        let gone = self
            .pad
            .learn
            .as_ref()
            .is_some_and(|l| !self.pad.learnable.iter().any(|p| p.id == l.pad.id));
        if gone {
            self.pad.learn = None;
            self.set_status(text::LEARN_DISCONNECTED.to_owned(), Level::Warn);
        }
    }

    /// Host -> page: unrecognized pads just plugged in (`GamepadInput::take_unrecognized`).
    pub(crate) fn pad_unrecognized(&mut self, pads: Vec<UnrecognizedPad>) {
        for p in pads {
            tracing::info!("manette non reconnue : {} ({})", p.name, p.guid);
            if !self.pad.unrecognized.iter().any(|u| u.id == p.id) {
                self.pad.unrecognized.push(LearnPad {
                    id: p.id,
                    guid: p.guid,
                    name: p.name,
                });
            }
        }
    }

    /// Opens Réglages on the learning screen of `id` (« Configurer » of a notice). `false`: pad gone.
    pub(crate) fn open_pad_learning(&mut self, id: PadId) -> bool {
        self.pad.unrecognized.retain(|u| u.id != id);
        let Some(pad) = self.pad.learnable.iter().find(|p| p.id == id).cloned() else {
            return false;
        };
        if self.page != super::Page::Settings {
            self.set_page(super::Page::Settings);
        }
        self.settings.focus_first = false;
        self.pad.learning = None;
        self.pad.learn = Some(LearnScreen::new(&pad));
        self.pad.shown_stage = None;
        true
    }

    /// The oldest pending « Manette non reconnue » notice, if any.
    pub(crate) fn pad_notice(&self) -> Option<&LearnPad> {
        self.pad.unrecognized.first()
    }

    /// Notice line of the status bars (home and settings): « Manette non reconnue : <nom> », « Configurer »,
    /// « Ignorer ». Plain buttons: reachable with Tab and with the pad's navigation.
    pub(super) fn pad_notice_line(&mut self, ui: &mut Ui, enabled: bool) {
        let Some(pad) = self.pad_notice().cloned() else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(super::WARNING, text::pad_unrecognized(&pad.name));
            let enabled = enabled && self.pad.learn.is_none();
            if ui
                .add_enabled(enabled, egui::Button::new(text::LEARN_CONFIGURE))
                .clicked()
            {
                self.open_pad_learning(pad.id);
            }
            if ui.button(text::PAD_NOTICE_DISMISS).clicked() {
                self.pad.unrecognized.retain(|u| u.id != pad.id);
            }
        });
    }

    /// The pad whose raw inputs the host must route to `pad_raw_input` (`set_raw_capture`); `None`: no session.
    pub(crate) fn pad_learn_target(&self) -> Option<PadId> {
        self.pad.learn.as_ref().map(|l| l.pad.id)
    }

    /// Host -> page: one raw input of the pad in learning (`GamepadInput::take_raw`).
    pub(crate) fn pad_raw_input(&mut self, input: RawInput, value: f32) {
        if let Some(l) = &mut self.pad.learn {
            l.feed(input, value);
        }
    }

    /// `true` once after a learned map was saved or forgotten: the host calls `set_learned_maps(learned_maps(..))`.
    pub(crate) fn take_learned_changed(&mut self) -> bool {
        std::mem::take(&mut self.pad.learned_changed)
    }

    /// Learned maps of the current config.
    pub(crate) fn learned_maps(&self) -> BTreeMap<String, LearnedMap> {
        learned_maps(&self.config.input.gamepad)
    }

    /// Writes the config now (a learned map was saved or forgotten), with any other pending setting.
    pub(crate) fn save_pad_config(&mut self) {
        self.settings.dirty = false;
        self.save_config();
    }

    /// Escape while waiting for a button cancels the remap or the learning instead of leaving the page.
    pub(super) fn cancel_pad_learning(&mut self) -> bool {
        let learn = self.pad.learn.take().is_some();
        let remap = self.pad.learning.take().is_some();
        learn || remap
    }

    fn save_learning(&mut self) {
        let Some(screen) = self.pad.learn.take() else {
            return;
        };
        if let Some((pad, layout, map)) = screen.finish() {
            self.pad.unrecognized.retain(|u| u.id != pad.id);
            store_learned(&mut self.config.input.gamepad, &pad, layout, &map);
            self.settings.dirty = true;
            self.pad.learned_changed = true;
            self.set_status(text::LEARN_SAVED.to_owned(), Level::Info);
        }
    }

    /// « Configurer une manette »: list of pads, then the step-by-step screen.
    fn learn_section(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new(text::LEARN_TITLE)
                .size(theme::FONT_SECTION)
                .color(theme::TEXT),
        );
        if self.pad.learn.is_none() {
            self.pad.shown_stage = None;
            weak(ui, text::LEARN_NOTE);
            let pads = self.pad.learnable.clone();
            for pad in pads {
                let learned = self
                    .config
                    .input
                    .gamepad
                    .devices
                    .get(&pad.guid)
                    .is_some_and(|d| !d.map.is_empty());
                ui.horizontal(|ui| {
                    let mut line = text::pad_name(&pad.name);
                    if learned {
                        line = format!("{line} ({})", text::LEARN_LEARNED);
                    }
                    ui.label(line);
                    if ui.button(text::LEARN_CONFIGURE).clicked() {
                        self.pad.learn = Some(LearnScreen::new(&pad));
                    }
                    if learned && ui.button(text::LEARN_FORGET).clicked() {
                        forget_learned(&mut self.config.input.gamepad, &pad.guid);
                        self.settings.dirty = true;
                        self.pad.learned_changed = true;
                        self.set_status(text::LEARN_FORGOTTEN.to_owned(), Level::Info);
                    }
                });
            }
            return;
        }
        let mut save = false;
        let mut cancel = false;
        if let Some(l) = &mut self.pad.learn {
            let stage = match l.session() {
                None => 0,
                Some(_) if l.is_review() => 2,
                Some(_) => 1,
            };
            let refocus = self.pad.shown_stage != Some(stage);
            self.pad.shown_stage = Some(stage);
            let focus = |r: &egui::Response| {
                if refocus {
                    r.request_focus();
                    r.scroll_to_me(Some(egui::Align::Center));
                }
            };
            // The learning flow sits on a card so it reads as one focused task.
            let card = egui::Frame::new()
                .fill(theme::CARD)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(theme::SP_M);
            card.show(ui, |ui| {
                ui.label(
                    RichText::new(text::learn_title_for(&l.pad.name))
                        .size(theme::FONT_BODY)
                        .color(theme::TEXT_WEAK),
                );
                match l.session() {
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(text::LEARN_SHAPE);
                            for layout in [
                                PadLayout::Snes,
                                PadLayout::Nes,
                                PadLayout::MegaDrive3,
                                PadLayout::MegaDrive6,
                            ] {
                                ui.selectable_value(
                                    &mut l.layout,
                                    layout,
                                    text::learn_layout_label(layout),
                                );
                            }
                        });
                        ui.horizontal(|ui| {
                            let start = ui.button(text::LEARN_START);
                            focus(&start);
                            if start.clicked() {
                                l.begin();
                            }
                            cancel = ui.button(text::LEARN_CANCEL).clicked();
                        });
                    }
                    Some(session) => {
                        let steps = l.layout.steps();
                        let pos = l.position();
                        if let Some(step) = session.current() {
                            ui.label(
                                RichText::new(text::learn_prompt(step.label))
                                    .size(theme::FONT_TITLE)
                                    .color(theme::ACCENT),
                            );
                            ui.add(
                                egui::ProgressBar::new(pos as f32 / steps.len().max(1) as f32)
                                    .fill(theme::ACCENT)
                                    .desired_height(theme::SP_S),
                            );
                            weak(ui, text::learn_progress(pos, steps.len()));
                        } else {
                            ui.label(text::LEARN_DONE);
                        }
                        if let Some(n) = &l.notice {
                            ui.colored_label(theme::WARN, n);
                        }
                        let review = session.current().is_none();
                        for (i, (step, rec)) in steps.iter().zip(session.recorded()).enumerate() {
                            if i < pos || review {
                                let shown = rec.map_or_else(
                                    || text::LEARN_NOT_SET.to_owned(),
                                    |r| r.to_token(),
                                );
                                let color = if rec.is_some() {
                                    theme::OK
                                } else {
                                    theme::TEXT_WEAK
                                };
                                ui.label(
                                    RichText::new(format!("{} : {shown}", step.label)).color(color),
                                );
                            }
                        }
                    }
                }
                if l.session().is_some() {
                    let review = l.is_review();
                    let empty = l
                        .session()
                        .is_none_or(|s| s.recorded().iter().all(Option::is_none));
                    ui.horizontal(|ui| {
                        if !review {
                            let skip = ui.button(text::LEARN_SKIP);
                            focus(&skip);
                            if skip.clicked() {
                                l.skip();
                            }
                        }
                        let back = ui.button(text::LEARN_BACK);
                        if review && empty {
                            focus(&back);
                        }
                        if back.clicked() {
                            l.back();
                        }
                        cancel = ui.button(text::LEARN_CANCEL).clicked();
                        if review {
                            let save_button =
                                ui.add_enabled(!empty, egui::Button::new(text::LEARN_SAVE));
                            if !empty {
                                focus(&save_button);
                            }
                            if save_button.clicked() {
                                save = true;
                            }
                            if empty {
                                weak(ui, text::LEARN_EMPTY);
                            }
                        }
                    });
                }
            });
        }
        if cancel {
            self.pad.learn = None;
        } else if save {
            self.save_learning();
        }
    }

    pub(super) fn gamepad_section(&mut self, ui: &mut Ui) {
        self.section_title(ui, text::PAD_TITLE, super::settings::help_anchor::PADS);
        let mut changed = false;
        let g = &mut self.config.input.gamepad;
        changed |= ui.checkbox(&mut g.enabled, text::PAD_ENABLED).changed();
        if !g.enabled {
            weak(ui, text::PAD_DISABLED_NOTE);
        } else if !self.pad.available {
            weak(ui, text::PAD_UNAVAILABLE);
        } else if self.pad.pads.is_empty() {
            weak(ui, text::PAD_NONE);
        }
        for (port, name) in &self.pad.pads {
            ui.label(text::pad_line(*port, name));
        }
        changed |= ui
            .add(Slider::new(&mut g.deadzone, 0.05..=0.60).text(text::PAD_DEADZONE))
            .changed();
        changed |= ui
            .add(Slider::new(&mut g.trigger_threshold, 0.10..=0.95).text(text::PAD_TRIGGER))
            .changed();
        ui.horizontal(|ui| {
            ui.label(text::PAD_HOTKEY);
            changed |= ui
                .selectable_value(
                    &mut g.hotkey_button,
                    HotkeyButton::Mode,
                    text::PAD_HOTKEY_MODE,
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut g.hotkey_button,
                    HotkeyButton::Select,
                    text::PAD_HOTKEY_SELECT,
                )
                .changed();
        });
        changed |= ui
            .checkbox(&mut g.pause_on_disconnect, text::PAD_PAUSE_ON_DISCONNECT)
            .changed();
        if changed {
            self.settings.dirty = true;
            self.pad.settings_changed = true;
        }
        self.learn_section(ui);
        self.pad_mode_section(ui);
        self.remap_section(ui);
    }

    /// « Manette PlayStation » (design v7.0 §2.4): `[input.pad_device]` of every core that has a DualShock mode.
    /// A game's own choice (menu in game) still wins.
    fn pad_mode_section(&mut self, ui: &mut Ui) {
        // `cores.toml` is embedded: parsed once, not on every frame of the page.
        static CORES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let cores =
            CORES.get_or_init(|| dualshock_cores(&emu_assets::cores::CorePreferences::builtin()));
        if cores.is_empty() {
            return;
        }
        let map = &mut self.config.input.pad_device;
        let before = cores
            .iter()
            .all(|c| map.get(c).map(String::as_str) == Some(PadMode::DualShock.key()));
        let mut dualshock = before;
        ui.horizontal(|ui| {
            ui.label(text::PAD_PS_MODE);
            ui.selectable_value(&mut dualshock, false, text::PAD_PS_DIGITAL);
            ui.selectable_value(&mut dualshock, true, text::PAD_PS_DUALSHOCK);
        });
        weak(ui, text::PAD_PS_MODE_NOTE);
        if dualshock != before {
            set_dualshock_default(map, cores, dualshock);
            self.settings.dirty = true;
        }
    }

    fn remap_section(&mut self, ui: &mut Ui) {
        egui::CollapsingHeader::new(text::PAD_REMAP_TITLE).show(ui, |ui| {
            weak(ui, text::PAD_REMAP_NOTE);
            ui.horizontal(|ui| {
                ui.label(text::PAD_REMAP_SYSTEM);
                let before = self.pad.system;
                ComboBox::from_id_salt("pad-remap-system")
                    .selected_text(SYSTEMS[self.pad.system.min(SYSTEMS.len() - 1)].0)
                    .show_ui(ui, |ui| {
                        for (i, (label, ..)) in SYSTEMS.iter().enumerate() {
                            ui.selectable_value(&mut self.pad.system, i, *label);
                        }
                    });
                if before != self.pad.system {
                    self.pad.learning = None;
                }
            });
            let (_, core_id, style, system_id) = SYSTEMS[self.pad.system.min(SYSTEMS.len() - 1)];
            let profile = profile_of(system_id, style, core_id);
            let map = PadMap::from_config(style, &profile, &self.config.input.gamepad);
            let can_learn = self.pad.available && !self.pad.pads.is_empty();
            if !can_learn {
                weak(ui, text::PAD_REMAP_NO_PAD);
            }
            for target in remappable_targets(style) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(style.button_name(target)).strong());
                    let owners: Vec<&str> = map
                        .bindings()
                        .iter()
                        .filter(|(_, t)| *t == target)
                        .map(|(b, _)| text::pad_button_label(*b))
                        .collect();
                    ui.label(if owners.is_empty() {
                        text::PAD_REMAP_NONE.to_owned()
                    } else {
                        owners.join(", ")
                    });
                    if self.pad.learning == Some(target) {
                        ui.label(text::PAD_REMAP_WAIT);
                        if ui.button(text::PAD_REMAP_CANCEL).clicked() {
                            self.pad.learning = None;
                        }
                    } else if ui
                        .add_enabled(can_learn, egui::Button::new(text::PAD_REMAP_CHANGE))
                        .clicked()
                    {
                        self.pad.learning = Some(target);
                    }
                });
            }
            if ui.button(text::PAD_REMAP_RESET).clicked()
                && self
                    .config
                    .input
                    .gamepad
                    .bindings
                    .remove(&profile)
                    .is_some()
            {
                self.settings.dirty = true;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_gives_the_target_a_single_owner() {
        let mut c = GamepadConfig::default();
        let style = PadStyle::MegaDrive;
        let key = style.profile_key("genesis_plus_gx");
        // Default: Circle (East) -> RetroPad A (button C). Give C to Triangle.
        let default = PadMap::from_config(style, &key, &c);
        let owner = default
            .bindings()
            .iter()
            .find(|(_, t)| *t == JoypadButton::A)
            .map(|(b, _)| *b)
            .unwrap();
        remap(&mut c, style, &key, JoypadButton::A, PadButton::North);
        let map = PadMap::from_config(style, &key, &c);
        assert_eq!(map.target(PadButton::North), Some(JoypadButton::A));
        assert_eq!(map.target(owner), None);
        // Only real overrides are written.
        assert_eq!(c.bindings[&key][PadButton::North.config_name()], "a");
        assert_eq!(c.bindings[&key][owner.config_name()], "none");
    }

    #[test]
    fn remappable_targets_skip_the_dpad_and_repeat_nothing() {
        for (_, _, style, _) in SYSTEMS {
            let t = remappable_targets(style);
            assert!(!t.is_empty());
            assert!(t.iter().all(|b| !DPAD_TARGETS.contains(b)));
            for (i, b) in t.iter().enumerate() {
                assert!(!t[i + 1..].contains(b));
            }
        }
        assert!(remappable_targets(PadStyle::MegaDrive).contains(&JoypadButton::Start));
    }

    #[test]
    fn dualshock_default_covers_the_three_playstation_cores() {
        let prefs = emu_assets::cores::CorePreferences::builtin();
        let cores = dualshock_cores(&prefs);
        assert_eq!(cores, ["pcsx_rearmed", "swanstation", "mednafen_psx"]);
        let mut map = std::collections::BTreeMap::new();
        map.insert("other".to_owned(), "x".to_owned());
        set_dualshock_default(&mut map, &cores, true);
        assert_eq!(map.len(), 4);
        assert_eq!(map["swanstation"], "dualshock");
        set_dualshock_default(&mut map, &cores, false);
        assert_eq!(map.len(), 1, "digital is written as nothing");
    }

    fn pad() -> LearnPad {
        LearnPad {
            id: PadId(3),
            guid: "03000000790000001100000000000000".to_owned(),
            name: "USB Gamepad".to_owned(),
        }
    }

    fn press(l: &mut LearnScreen, input: RawInput) {
        l.feed(input, 1.0);
        l.feed(input, 0.0);
    }

    fn run_all(l: &mut LearnScreen) {
        l.begin();
        for code in 0..l.layout.steps().len() as u32 {
            press(l, RawInput::Button(code));
        }
    }

    #[test]
    fn full_sequence_reaches_review_and_writes_config() {
        let mut l = LearnScreen::new(&pad());
        l.layout = PadLayout::Nes;
        run_all(&mut l);
        assert!(l.is_review());
        let (p, layout, map) = l.finish().unwrap();
        let mut c = GamepadConfig::default();
        store_learned(&mut c, &p, layout, &map);
        let d = &c.devices[&p.guid];
        assert_eq!(d.name.as_deref(), Some("USB Gamepad"));
        assert_eq!(d.layout.as_deref(), Some("nes"));
        assert_eq!(map.bindings().len(), PadLayout::Nes.steps().len());
        assert_eq!(d.map, map.to_config());
        assert_eq!(learned_maps(&c)[&p.guid].to_config(), d.map);
    }

    #[test]
    fn skip_and_back_move_one_step() {
        let mut l = LearnScreen::new(&pad());
        l.begin();
        assert_eq!(l.position(), 0);
        press(&mut l, RawInput::Button(1));
        assert_eq!(l.position(), 1);
        l.skip();
        assert_eq!(l.position(), 2);
        l.back();
        l.back();
        assert_eq!(l.position(), 0);
        l.back();
        assert_eq!(l.position(), 0);
        let (_, _, map) = l.finish().unwrap();
        assert!(map.bindings().is_empty());
    }

    #[test]
    fn duplicate_input_is_refused_with_a_message() {
        let mut l = LearnScreen::new(&pad());
        l.begin();
        press(&mut l, RawInput::Button(1));
        press(&mut l, RawInput::Button(1));
        assert_eq!(l.position(), 1);
        assert!(
            l.notice
                .as_deref()
                .is_some_and(|n| n.contains("déjà utilisée"))
        );
        press(&mut l, RawInput::Button(2));
        assert_eq!(l.position(), 2);
        assert!(l.notice.is_none());
    }

    #[test]
    fn cancel_drops_the_session_and_leaves_config_alone() {
        let mut c = GamepadConfig::default();
        let before = c.clone();
        let mut l = LearnScreen::new(&pad());
        run_all(&mut l);
        drop(l);
        assert_eq!(c, before);
        let p = pad();
        let map = LearnedMap::from_config(&[("south".to_owned(), "b2".to_owned())].into());
        store_learned(&mut c, &p, PadLayout::Snes, &map);
        assert!(forget_learned(&mut c, &p.guid));
        assert_eq!(c, before, "forgetting removes the whole empty entry");
        assert!(!forget_learned(&mut c, &p.guid));
    }

    #[test]
    fn forget_keeps_other_device_fields() {
        let mut c = GamepadConfig::default();
        let p = pad();
        c.devices.entry(p.guid.clone()).or_default().port = Some(1);
        store_learned(&mut c, &p, PadLayout::Snes, &LearnedMap::default());
        forget_learned(&mut c, &p.guid);
        assert_eq!(c.devices[&p.guid].port, Some(1));
    }

    #[test]
    fn forget_ignores_guid_case_and_invalid_maps_are_not_loaded() {
        let mut c = GamepadConfig::default();
        let p = pad();
        let upper = p.guid.to_ascii_uppercase();
        let map = LearnedMap::from_config(&[("south".to_owned(), "b2".to_owned())].into());
        store_learned(&mut c, &p, PadLayout::Snes, &map);
        let d = c.devices.remove(&p.guid).unwrap();
        c.devices.insert(upper.clone(), d);
        assert!(forget_learned(&mut c, &p.guid.to_ascii_lowercase()));
        assert!(!c.devices.contains_key(&upper));

        let d = c.devices.entry(upper).or_default();
        d.map.insert("south".to_owned(), "garbage".to_owned());
        assert!(
            crate::gamepad::learned_maps(&c).is_empty(),
            "a map without valid binding is skipped"
        );
    }
}
