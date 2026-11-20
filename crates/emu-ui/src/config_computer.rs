//! `[computer]` section of `config.toml` (ADR 0015, design §5). Unknown keys are written back unchanged.

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComputerConfig {
    /// 0.1 to 5.0 (clamped when used).
    pub mouse_sensitivity: f32,
    /// A click in the game area captures the pointer.
    pub auto_capture: bool,
    /// Mode of games launched from the library; the desktop (no game) is always `Keyboard`.
    pub default_keyboard_mode: KeyboardModeChoice,
    /// `false`: never grab the pointer, always the uncaptured fallback (noVNC, WSLg setups where the grab
    /// misbehaves).
    pub grab_pointer: bool,
    /// EmuTOS variant id (`"192fr"`); absent: derived from the system language, else the catalogue default.
    pub emutos_variant: Option<String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for ComputerConfig {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            auto_capture: true,
            default_keyboard_mode: KeyboardModeChoice::default(),
            grab_pointer: true,
            emutos_variant: None,
            extra: toml::Table::new(),
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
        assert_eq!(c.default_keyboard_mode, KeyboardModeChoice::Keyboard);
        assert_eq!(ComputerConfig::default().emutos_variant, None);
    }

    #[test]
    fn mode_and_unknown_keys_round_trip() {
        let text = "default_keyboard_mode = \"joystick\"\nemutos_variant = \"192de\"\nfuture = 3\n";
        let c: ComputerConfig = toml::from_str(text).unwrap();
        assert_eq!(c.default_keyboard_mode, KeyboardModeChoice::Joystick);
        assert_eq!(c.emutos_variant.as_deref(), Some("192de"));
        let back: ComputerConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        assert!(toml::to_string(&c).unwrap().contains("future = 3"));
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
