//! The only filter of the « add a system » view (ADR 0025 §8, §9): what a `.info` allows.
#![allow(unused_variables)]

use emu_assets::cores::CoreInfo;

/// Verdict for one core of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Addability {
    /// Can be added; `warnings` are shown on the sheet.
    Ok { warnings: Vec<AddWarning> },
    /// Needs hardware rendering: greyed « prévu en 9.0 ».
    HardwareRender,
    /// Disc machines only: greyed « prévu en 8.4 ».
    DiscOnly,
    /// Arcade: greyed « prévu en 8.6 ».
    Arcade,
    /// Not proposed (no usable extension, not an emulator…).
    NotProposed,
}

/// Warning of the sheet of a core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddWarning {
    Experimental,
    /// The core wants the keyboard (`needs_kbd_mouse_focus`): keyboard systems come in 8.5.
    KeyboardNeeded,
    /// `savestate = false` in the `.info`.
    NoSavestates,
    /// Core absent from `cores.toml`: « contenu non vérifié ».
    UnverifiedContent,
}

/// `CoreInfo` + the rules of `emu-library` (extensions, tiers).
pub fn addability(info: &CoreInfo) -> Addability {
    todo!("lot PU2")
}
