//! `[app]` table: settings of the application itself.

use serde::{Deserialize, Serialize};

/// `[app]`. Not written while default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Show the terminal (Windows: console build; elsewhere: stderr log). Read at start-up by `emu-app`.
    pub show_terminal: bool,
    /// Unknown keys, written back unchanged.
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl AppConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `[ui]`: interface state. Not written while default (an install that never ran the wizard keeps its file).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// The first-run wizard was finished or left (design v7.3 §4).
    pub wizard_done: bool,
    /// Unknown keys, written back unchanged.
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl UiConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn default_is_not_written() {
        let c = crate::Config::default();
        assert!(c.app.is_default());
        assert!(!toml::to_string(&c).unwrap().contains("[app]"));
    }

    #[test]
    fn show_terminal_round_trips() {
        let mut c = crate::Config::default();
        c.app.show_terminal = true;
        let text = toml::to_string(&c).unwrap();
        let back: crate::Config = toml::from_str(&text).unwrap();
        assert!(back.app.show_terminal);
    }

    #[test]
    fn wizard_done_is_written_only_when_set() {
        let mut c = crate::Config::default();
        assert!(!toml::to_string(&c).unwrap().contains("[ui]"));
        c.ui.wizard_done = true;
        let text = toml::to_string(&c).unwrap();
        assert!(text.contains("[ui]"));
        let back: crate::Config = toml::from_str(&text).unwrap();
        assert!(back.ui.wizard_done);
    }
}
