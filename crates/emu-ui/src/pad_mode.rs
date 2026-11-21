//! Pad mode of a core that declares several controller devices (PlayStation: digital pad or DualShock; design
//! v7.0 §2.4 and §3.5). Pure logic; the device id comes from `CorePreferences::pad_devices`.
use emu_assets::cores::PadDevices;

/// Pad mode: `"digital"` (original pad, stick converted to the D-pad) or `"dualshock"` (sticks passed through).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PadMode {
    Digital,
    DualShock,
}

impl PadMode {
    /// Key of `PadDevices::modes`, of `[input.pad_device]` and of the game's `[input] pad_device`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            PadMode::Digital => "digital",
            PadMode::DualShock => "dualshock",
        }
    }

    /// The other mode (menu « Manette : Numérique / DualShock »).
    pub(crate) fn toggled(self) -> Self {
        match self {
            PadMode::Digital => PadMode::DualShock,
            PadMode::DualShock => PadMode::Digital,
        }
    }
}

/// Ports that get the device of the pad mode (design v7.0 §2.4: the two pads of the console).
pub(crate) const PAD_PORTS: [u32; 2] = [0, 1];

/// Mode of a launch (design v7.0 §2.4): `--pad-device` of this run, else the game's `[input] pad_device`, else
/// `[input.pad_device] <core>`, else the core's default. `None` when the core declares no `pad_devices`.
pub(crate) fn launch_pad_mode(
    devices: Option<&PadDevices>,
    cli: Option<&str>,
    game: Option<&str>,
    config: Option<&str>,
) -> Option<(PadMode, u32)> {
    let devices = devices?;
    // An unknown CLI key falls through to the game's choice (`pad_mode` skips unknown keys level by level).
    let wanted = cli
        .filter(|k| devices.modes.contains_key(k.trim()))
        .or(game);
    Some(pad_mode(devices, config, wanted))
}

/// `(port, id)` of [`PAD_PORTS`] for `EmulatorOptions::port_devices`.
pub(crate) fn port_devices(id: u32) -> Vec<(u32, u32)> {
    PAD_PORTS.iter().map(|&port| (port, id)).collect()
}

/// Effective mode and its device id: game, else config, else `devices.default`; an unknown key (or a mode the
/// core does not list) falls back to the default.
pub(crate) fn pad_mode(
    devices: &PadDevices,
    config: Option<&str>,
    game: Option<&str>,
) -> (PadMode, u32) {
    let lookup = |key: Option<&str>| {
        let key = key?.trim();
        let id = *devices.modes.get(key)?;
        let mode = match key {
            "digital" => PadMode::Digital,
            "dualshock" => PadMode::DualShock,
            _ => return None,
        };
        Some((mode, id))
    };
    // A game or config key that is unknown falls through to the next level, then to the default.
    lookup(game)
        .or_else(|| lookup(config))
        .or_else(|| lookup(Some(&devices.default)))
        .or_else(|| {
            // Malformed table (default not a known mode): the numeric pad, id from the table when present.
            let id = devices.modes.get("digital").copied().unwrap_or(1);
            Some((PadMode::Digital, id))
        })
        .unwrap_or((PadMode::Digital, 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn devices(default: &str) -> PadDevices {
        PadDevices {
            modes: BTreeMap::from([("digital".to_owned(), 1), ("dualshock".to_owned(), 517)]),
            default: default.to_owned(),
        }
    }

    #[test]
    fn game_beats_config_beats_default() {
        let d = devices("digital");
        assert_eq!(pad_mode(&d, None, None), (PadMode::Digital, 1));
        assert_eq!(
            pad_mode(&d, Some("dualshock"), None),
            (PadMode::DualShock, 517)
        );
        assert_eq!(
            pad_mode(&d, Some("dualshock"), Some("digital")),
            (PadMode::Digital, 1)
        );
        assert_eq!(
            pad_mode(&d, Some("digital"), Some("dualshock")),
            (PadMode::DualShock, 517)
        );
    }

    #[test]
    fn unknown_key_falls_back() {
        let d = devices("digital");
        assert_eq!(pad_mode(&d, Some("nope"), None), (PadMode::Digital, 1));
        // An unknown game key does not hide a valid config key.
        assert_eq!(
            pad_mode(&d, Some("dualshock"), Some("nope")),
            (PadMode::DualShock, 517)
        );
        assert_eq!(
            pad_mode(&devices("dualshock"), Some("x"), Some("y")),
            (PadMode::DualShock, 517)
        );
    }

    #[test]
    fn mode_the_core_does_not_list_is_ignored() {
        let mut d = devices("digital");
        d.modes.remove("dualshock");
        assert_eq!(pad_mode(&d, Some("dualshock"), None), (PadMode::Digital, 1));
    }

    #[test]
    fn cli_beats_game_beats_config() {
        let d = devices("digital");
        assert_eq!(launch_pad_mode(None, Some("dualshock"), None, None), None);
        assert_eq!(
            launch_pad_mode(Some(&d), None, None, None),
            Some((PadMode::Digital, 1))
        );
        assert_eq!(
            launch_pad_mode(Some(&d), None, Some("dualshock"), Some("digital")),
            Some((PadMode::DualShock, 517))
        );
        assert_eq!(
            launch_pad_mode(Some(&d), Some("digital"), Some("dualshock"), None),
            Some((PadMode::Digital, 1))
        );
        // A mode the core does not list on the command line: the game decides.
        let mut only = devices("digital");
        only.modes.remove("dualshock");
        assert_eq!(
            launch_pad_mode(Some(&only), Some("dualshock"), Some("digital"), None),
            Some((PadMode::Digital, 1))
        );
        assert_eq!(port_devices(517), vec![(0, 517), (1, 517)]);
        assert_eq!(PadMode::Digital.toggled(), PadMode::DualShock);
        assert_eq!(PadMode::DualShock.toggled(), PadMode::Digital);
    }

    #[test]
    fn keys_round_trip() {
        assert_eq!(PadMode::Digital.key(), "digital");
        assert_eq!(PadMode::DualShock.key(), "dualshock");
    }
}
