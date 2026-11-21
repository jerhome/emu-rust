//! `[computer]` section of `config.toml` (ADR 0015, design §5). Unknown keys are written back unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Keyboard mode of a game (also the value of `[input] keyboard_mode` in the game options).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyboardModeChoice {
    /// Default of an Atari ST game (user decision 2026-11-20: mostly mouse-driven point-and-click games; the
    /// design said joystick).
    #[default]
    Keyboard,
    Joystick,
}

/// `[computer] keyboard_mode` (design v5.0 §4, §10 n° 5): initial mode of a game without a remembered one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyboardModeSetting {
    /// `default_input` of the game's system (`emu_library::SystemInfo`): Joystick for CPC / C64, else Keyboard.
    #[default]
    Auto,
    Keyboard,
    Joystick,
}

impl KeyboardModeSetting {
    /// Never written while `Auto`.
    pub fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, from = "RawComputerConfig")]
pub struct ComputerConfig {
    /// 0.1 to 5.0 (clamped when used).
    pub mouse_sensitivity: f32,
    /// A click in the game area captures the pointer.
    pub auto_capture: bool,
    /// 5.0: initial mode of a game without a remembered one (`Auto` = the system's `default_input`).
    #[serde(skip_serializing_if = "KeyboardModeSetting::is_auto")]
    pub keyboard_mode: KeyboardModeSetting,
    /// `false`: never grab the pointer, always the uncaptured fallback (noVNC, WSLg setups where the grab
    /// misbehaves).
    pub grab_pointer: bool,
    /// EmuTOS variant id (`"192fr"`); absent: derived from the system language, else the catalogue default.
    pub emutos_variant: Option<String>,
    /// 5.0: let the core's fast-forward request (tape / disk loading) speed the emulation up (`Speed::FastForward`).
    pub auto_fast_load: bool,
    /// User key overrides per keyboard layout id (`[computer.remap.amiga]`): winit `KeyCode` name → libretro key
    /// name, above the layout file's own `[remap]`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub remap: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for ComputerConfig {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            auto_capture: true,
            keyboard_mode: KeyboardModeSetting::Auto,
            grab_pointer: true,
            emutos_variant: None,
            auto_fast_load: true,
            remap: BTreeMap::new(),
            extra: toml::Table::new(),
        }
    }
}

/// Deserialization shape of [`ComputerConfig`]: same keys, plus the migration of the 2.0-4.0 key
/// `default_keyboard_mode` (read only: `"joystick"` ⇒ `keyboard_mode = "joystick"`, `"keyboard"` ignored, never
/// written again).
#[derive(Deserialize)]
#[serde(default)]
struct RawComputerConfig {
    mouse_sensitivity: f32,
    auto_capture: bool,
    default_keyboard_mode: KeyboardModeChoice,
    keyboard_mode: KeyboardModeSetting,
    grab_pointer: bool,
    emutos_variant: Option<String>,
    auto_fast_load: bool,
    remap: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(flatten)]
    extra: toml::Table,
}

impl Default for RawComputerConfig {
    fn default() -> Self {
        let d = ComputerConfig::default();
        Self {
            mouse_sensitivity: d.mouse_sensitivity,
            auto_capture: d.auto_capture,
            default_keyboard_mode: KeyboardModeChoice::Keyboard,
            keyboard_mode: d.keyboard_mode,
            grab_pointer: d.grab_pointer,
            emutos_variant: d.emutos_variant,
            auto_fast_load: d.auto_fast_load,
            remap: d.remap,
            extra: d.extra,
        }
    }
}

impl From<RawComputerConfig> for ComputerConfig {
    fn from(r: RawComputerConfig) -> Self {
        // `"keyboard"` of the old key proves no choice (the whole table was written when any field changed), so
        // only `"joystick"` is carried over, and never over an explicit new key.
        let keyboard_mode = match (r.keyboard_mode, r.default_keyboard_mode) {
            (KeyboardModeSetting::Auto, KeyboardModeChoice::Joystick) => {
                KeyboardModeSetting::Joystick
            }
            (mode, _) => mode,
        };
        Self {
            mouse_sensitivity: r.mouse_sensitivity,
            auto_capture: r.auto_capture,
            keyboard_mode,
            grab_pointer: r.grab_pointer,
            emutos_variant: r.emutos_variant,
            auto_fast_load: r.auto_fast_load,
            remap: r.remap,
            extra: r.extra,
        }
    }
}

impl ComputerConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Variant to install: the configured one, else the system language's, else `None` (catalogue default).
    pub fn emutos_variant_or_system(&self) -> Option<String> {
        self.emutos_variant
            .clone()
            .or_else(emutos_variant_or_system)
    }
}

/// Variant for the system language (`sys-locale`), else `None`: what the UI installs when nothing is configured.
pub fn emutos_variant_or_system() -> Option<String> {
    sys_locale::get_locale().and_then(|l| variant_for_locale(&l))
}

/// EmuTOS 192 KB variant id for a locale such as `fr-FR`, `de_DE.UTF-8` or `en-GB`; `None` when EmuTOS has no
/// translation (the caller then uses the catalogue default).
pub fn variant_for_locale(locale: &str) -> Option<String> {
    let norm = locale.split(['.', '@']).next().unwrap_or(locale);
    let mut parts = norm.split(['-', '_']);
    let lang = parts.next()?.to_ascii_lowercase();
    let region = parts.next().map(str::to_ascii_uppercase);
    let id = match (lang.as_str(), region.as_deref()) {
        ("fr", _) => "192fr",
        ("de", Some("CH")) => "192sg",
        ("de", _) => "192de",
        ("en", Some("GB")) => "192uk",
        ("en", _) => "192us",
        ("es", _) => "192es",
        ("it", _) => "192it",
        ("nl", _) => "192nl",
        ("sv", _) => "192se",
        ("nb" | "nn" | "no", _) => "192no",
        ("fi", _) => "192fi",
        ("pl", _) => "192pl",
        ("cs", _) => "192cz",
        ("hu", _) => "192hu",
        ("ro", _) => "192ro",
        _ => return None,
    };
    Some(id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_file() {
        let c: ComputerConfig = toml::from_str("mouse_sensitivity = 2.0\n").unwrap();
        assert_eq!(c.mouse_sensitivity, 2.0);
        assert!(c.auto_capture);
        assert_eq!(c.keyboard_mode, KeyboardModeSetting::Auto);
        assert_eq!(ComputerConfig::default().emutos_variant, None);
    }

    #[test]
    fn mode_and_unknown_keys_round_trip() {
        let text = "default_keyboard_mode = \"joystick\"\nemutos_variant = \"192de\"\nfuture = 3\n";
        let c: ComputerConfig = toml::from_str(text).unwrap();
        assert_eq!(c.keyboard_mode, KeyboardModeSetting::Joystick);
        assert_eq!(c.emutos_variant.as_deref(), Some("192de"));
        let back: ComputerConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        // The old key is migration input only: never written, its value moves to `keyboard_mode`.
        assert_eq!(back.keyboard_mode, KeyboardModeSetting::Joystick);
        let out = toml::to_string(&c).unwrap();
        assert!(!out.contains("default_keyboard_mode"), "{out}");
        assert!(out.contains("keyboard_mode = \"joystick\"") && out.contains("future = 3"));
        let again: ComputerConfig = toml::from_str(&toml::to_string(&back).unwrap()).unwrap();
        assert_eq!(again.keyboard_mode, back.keyboard_mode);
    }

    #[test]
    fn legacy_keyboard_value_is_ignored_and_explicit_key_wins() {
        let c: ComputerConfig = toml::from_str("default_keyboard_mode = \"keyboard\"\n").unwrap();
        assert_eq!(c.keyboard_mode, KeyboardModeSetting::Auto);
        assert!(c.is_default());
        let c: ComputerConfig =
            toml::from_str("default_keyboard_mode = \"joystick\"\nkeyboard_mode = \"keyboard\"\n")
                .unwrap();
        assert_eq!(c.keyboard_mode, KeyboardModeSetting::Keyboard);
        let c: ComputerConfig = toml::from_str("keyboard_mode = \"joystick\"\n").unwrap();
        assert_eq!(c.keyboard_mode, KeyboardModeSetting::Joystick);
    }

    /// A 2.0-4.0 `[computer]` table (old key included) is read without loss: nothing but the old key disappears.
    #[test]
    fn config_4_0_reads_back_unchanged() {
        let text = "mouse_sensitivity = 2.5\nauto_capture = false\ndefault_keyboard_mode = \"keyboard\"\n\
                    grab_pointer = false\nemutos_variant = \"192fr\"\n[remap.atari_st]\nContextMenu = \"RALT\"\n";
        let c: ComputerConfig = toml::from_str(text).unwrap();
        assert_eq!(c.mouse_sensitivity, 2.5);
        assert!(!c.auto_capture && !c.grab_pointer && c.auto_fast_load);
        assert_eq!(c.keyboard_mode, KeyboardModeSetting::Auto);
        assert_eq!(c.remap["atari_st"]["ContextMenu"], "RALT");
        let back: ComputerConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        assert!(
            !toml::to_string(&c)
                .unwrap()
                .contains("default_keyboard_mode")
        );
    }

    #[test]
    fn remap_table_round_trips() {
        let text = "mouse_sensitivity = 2.0\n[remap.amiga]\nContextMenu = \"RALT\"\n";
        let c: ComputerConfig = toml::from_str(text).unwrap();
        assert_eq!(c.remap["amiga"]["ContextMenu"], "RALT");
        assert!(c.extra.is_empty());
        let out = toml::to_string(&c).unwrap();
        let back: ComputerConfig = toml::from_str(&out).unwrap();
        assert_eq!(back, c);
        assert!(!c.is_default());
        assert!(
            !toml::to_string(&ComputerConfig::default())
                .unwrap()
                .contains("remap")
        );
    }

    #[test]
    fn locale_to_variant() {
        for (l, v) in [
            ("fr-FR", Some("192fr")),
            ("fr_CA.UTF-8", Some("192fr")),
            ("de-CH", Some("192sg")),
            ("de_DE", Some("192de")),
            ("en-GB", Some("192uk")),
            ("en-US", Some("192us")),
            ("nb-NO", Some("192no")),
            ("ja-JP", None),
            ("", None),
        ] {
            assert_eq!(variant_for_locale(l).as_deref(), v, "{l}");
        }
    }

    #[test]
    fn configured_variant_wins() {
        let c = ComputerConfig {
            emutos_variant: Some("192it".into()),
            ..ComputerConfig::default()
        };
        assert_eq!(c.emutos_variant_or_system().as_deref(), Some("192it"));
    }
}
