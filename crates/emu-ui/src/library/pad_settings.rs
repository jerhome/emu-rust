//! Settings › Manette: attached pads, deadzone, trigger threshold, hotkey button, and « press a button » remap
//! per system. The pad backend lives in the host loop; this page only sees a status snapshot and exchanges
//! two facts with it (`pad_wants_learning`, `pad_learned`).

use egui::{ComboBox, RichText, Slider, Ui};
use emu_runtime::JoypadButton;

use super::{Level, LibraryUi, text, weak};
use crate::config_gamepad::{GamepadConfig, HotkeyButton};
use crate::gamepad::{PadButton, PadMap, set_binding};
use crate::keyboard::PadStyle;
use crate::pad_mode::PadMode;

/// Systems of the remap page: label, core id (profile key base), pad style.
const SYSTEMS: [(&str, &str, PadStyle); 6] = [
    ("Mega Drive", "genesis_plus_gx", PadStyle::MegaDrive),
    ("Master System", "genesis_plus_gx", PadStyle::MasterSystem),
    ("Game Gear", "genesis_plus_gx", PadStyle::GameGear),
    ("Super Nintendo", "snes9x", PadStyle::Snes),
    ("Game Boy", "gambatte", PadStyle::GameBoy),
    ("NES", "mesen", PadStyle::Nes),
];

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
    core_id: &str,
    target: JoypadButton,
    physical: PadButton,
) {
    let profile = style.profile_key(core_id);
    let map = PadMap::from_config(style, &profile, config);
    for (other, t) in map.bindings() {
        if *t == target && *other != physical && !is_dpad(*other) {
            set_binding(config, style, &profile, *other, None);
        }
    }
    set_binding(config, style, &profile, physical, Some(target));
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
        let (_, core_id, style) = SYSTEMS[self.pad.system.min(SYSTEMS.len() - 1)];
        remap(
            &mut self.config.input.gamepad,
            style,
            core_id,
            target,
            physical,
        );
        self.settings.dirty = true;
    }

    /// `true` once after a setting that the live backend must re-read was edited.
    pub(crate) fn take_pad_settings_changed(&mut self) -> bool {
        std::mem::take(&mut self.pad.settings_changed)
    }

    /// Escape while waiting for a button cancels the remap instead of leaving the page.
    pub(super) fn cancel_pad_learning(&mut self) -> bool {
        self.pad.learning.take().is_some()
    }

    pub(super) fn gamepad_section(&mut self, ui: &mut Ui) {
        ui.label(RichText::new(text::PAD_TITLE).strong());
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
            let (_, core_id, style) = SYSTEMS[self.pad.system.min(SYSTEMS.len() - 1)];
            let profile = style.profile_key(core_id);
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
        remap(
            &mut c,
            style,
            "genesis_plus_gx",
            JoypadButton::A,
            PadButton::North,
        );
        let map = PadMap::from_config(style, &key, &c);
        assert_eq!(map.target(PadButton::North), Some(JoypadButton::A));
        assert_eq!(map.target(owner), None);
        // Only real overrides are written.
        assert_eq!(c.bindings[&key][PadButton::North.config_name()], "a");
        assert_eq!(c.bindings[&key][owner.config_name()], "none");
    }

    #[test]
    fn remappable_targets_skip_the_dpad_and_repeat_nothing() {
        for (_, _, style) in SYSTEMS {
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
}
