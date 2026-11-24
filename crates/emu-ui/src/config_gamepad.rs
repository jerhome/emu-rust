//! `[input.gamepad]` section of `config.toml` (ADR 0016). Every key optional; unknown keys kept in `extra`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which pad button acts as the hotkey modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HotkeyButton {
    /// PS button.
    Mode,
    /// Share / Back, when the system captures PS.
    Select,
}

/// Per-model overrides, keyed by lowercase hex GUID in `GamepadConfig::devices`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadDeviceConfig {
    /// Forced RetroPad port (0-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadzone: Option<f32>,
    /// Display name of a learned pad (7.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Shape chosen while learning (`PadLayout::config_name`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    /// Per-pad hotkey button; `None`: the global one (or Select for a learned pad without Mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotkey_button: Option<HotkeyButton>,
    /// Learned map: positional button name -> raw token (`b2`, `+a0`). Empty: nothing learned.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub map: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadConfig {
    pub enabled: bool,
    pub deadzone: f32,
    pub trigger_threshold: f32,
    pub hotkey_button: HotkeyButton,
    pub pause_on_disconnect: bool,
    /// Profile key (`PadStyle::profile_key`) -> positional button name (`south`) -> RetroPad button name (`b`).
    /// Partial: unlisted buttons keep the default.
    pub bindings: BTreeMap<String, BTreeMap<String, String>>,
    pub devices: BTreeMap<String, GamepadDeviceConfig>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            deadzone: 0.25,
            trigger_threshold: 0.5,
            hotkey_button: HotkeyButton::Mode,
            pause_on_disconnect: false,
            bindings: BTreeMap::new(),
            devices: BTreeMap::new(),
            extra: toml::Table::new(),
        }
    }
}

impl GamepadConfig {
    /// Nothing to write: a 2.0 file (no `[input.gamepad]`) stays unchanged after a save.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_section_round_trips_and_keeps_unknown_keys() {
        let text = r#"
enabled = false
deadzone = 0.15
hotkey_button = "select"
futur = "x"

[bindings.genesis_plus_gx]
south = "b"

[devices."030000004c050000c405000000000000"]
port = 1
deadzone = 0.2
autre = 3
"#;
        let c: GamepadConfig = toml::from_str(text).unwrap();
        assert!(!c.enabled);
        assert_eq!(c.hotkey_button, HotkeyButton::Select);
        assert_eq!(c.trigger_threshold, 0.5);
        assert_eq!(c.bindings["genesis_plus_gx"]["south"], "b");
        let d = &c.devices["030000004c050000c405000000000000"];
        assert_eq!((d.port, d.deadzone), (Some(1), Some(0.2)));
        assert!(d.extra.contains_key("autre") && c.extra.contains_key("futur"));
        let again: GamepadConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(again, c);
        assert!(!c.is_default());
        assert!(GamepadConfig::default().is_default());
    }

    #[test]
    fn config_without_learned_fields_still_reads() {
        let c: GamepadConfig = toml::from_str("[devices.abc]\nport = 0\n").unwrap();
        let d = &c.devices["abc"];
        assert!(d.name.is_none() && d.layout.is_none() && d.map.is_empty());
        assert!(!toml::to_string(&c).unwrap().contains("map"));
    }

    #[test]
    fn learned_fields_round_trip() {
        let text = "[devices.abc]\nname = \"USB Gamepad\"\nlayout = \"snes\"\nhotkey_button = \"select\"\n\
                    [devices.abc.map]\ndpup = \"-a1\"\nsouth = \"b2\"\n";
        let c: GamepadConfig = toml::from_str(text).unwrap();
        let d = &c.devices["abc"];
        assert_eq!(d.hotkey_button, Some(HotkeyButton::Select));
        assert_eq!(d.map["south"], "b2");
        let again: GamepadConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(again, c);
    }
}
