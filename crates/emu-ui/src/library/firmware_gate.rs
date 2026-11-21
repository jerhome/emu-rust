//! Pre-launch firmware gate of a library game (design v6.0 §2.2, §3.5): turns the single check of the 3.0
//! (`emu_assets::cores::check_core_firmware`) into what the library does. Pure, tested. Replaces the Mega CD
//! check of the 2.1 (`library/megacd.rs`, removed by GU2): no Mega CD specific code remains.
// GU2 wires the gate into `launch` (in place of `megacd_gate`); until then nothing calls it.
#![allow(dead_code)]

use emu_assets::cores::FirmwareCheck;

/// What the library does with a [`FirmwareCheck`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirmwareGate {
    /// `Ok` / `Free` / `Unverified` / `Builtin`, or the wanted file absent while the variant is unknown and
    /// another file of the table is present (2.1 rule: the core picks).
    Launch,
    /// `CoreFallback`: launch, the UI warns once per game.
    WarnOnce,
    /// `Missing` (no other file usable), `WrongHash`, `Unreadable`: the generic firmware dialog (« Importer un
    /// BIOS… », « Lancer quand même » except for `Missing`, « Annuler »), which keeps the `system/` path, « Copier le
    /// chemin » and « Réessayer » of the 2.1 dialog.
    Ask,
}

/// `variant_known`: the variant comes from the game (`Game::region`, else the header probe, else the name tag);
/// `false` when the table's default was used. Unknown variant + wanted file absent + `others_present` not empty
/// ⇒ `Launch` (design v6.0 §2.2: a `.chd` without tag, playable in 2.1 with the one BIOS present).
pub(crate) fn firmware_gate(check: &FirmwareCheck, variant_known: bool) -> FirmwareGate {
    let _ = (check, variant_known);
    todo!("GU2: firmware_gate (design v6.0 §2.2, §3.5)")
}
