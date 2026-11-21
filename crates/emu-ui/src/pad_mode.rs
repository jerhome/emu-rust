//! Pad mode of a core that declares several controller devices (PlayStation: digital pad or DualShock; design
//! v7.0 §2.4 and §3.5). Pure logic; the device id comes from `CorePreferences::pad_devices`.
// The module is wired by PU1 / PU2; until then nothing calls it.
#![allow(dead_code)]

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
}

/// Effective mode and its device id: game, else config, else `devices.default`; an unknown key (or a mode the
/// core does not list) falls back to the default.
pub(crate) fn pad_mode(
    devices: &PadDevices,
    config: Option<&str>,
    game: Option<&str>,
) -> (PadMode, u32) {
    let _ = (devices, config, game);
    todo!("PU1: game > config > default; unknown key => default")
}
