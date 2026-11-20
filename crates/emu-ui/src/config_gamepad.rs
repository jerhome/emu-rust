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
