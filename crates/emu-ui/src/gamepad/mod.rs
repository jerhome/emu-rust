//! Gamepad support (ADR 0016, docs/design/phase6-manette.md). Everything but `gilrs_source` is pure logic fed by
//! the injectable `GamepadSource`, so it is tested headless with `ScriptedPadSource`. No `unsafe` here.
//!
//! Combination with other backends: `GamepadInput` is one more `InputBackend` next to `KeyboardInput` (library
//! games) or `ComputerKeyboard`/`ComputerMouse` (computer mode); backends OR their buttons into the same
//! `InputFrame`, so the pad needs no mode switch. Hotkeys: `take_hotkeys` yields `HotkeyAction`s, which the app
//! wraps in `HostAction::Hotkey` (computer mode) or uses directly (consoles).

#![allow(dead_code, unused_imports)] // Until wired into app.rs (T3).

#[cfg(feature = "gamepad")]
mod gilrs_source;
mod input;
mod mapping;
mod source;

#[cfg(feature = "gamepad")]
pub use gilrs_source::GilrsSource;
pub use input::{GamepadInput, GamepadNotice, GamepadSettings};
pub use mapping::{PadButton, PadMap, retropad_from_name, retropad_name};
pub use source::{
    GamepadError, GamepadSource, PadAxis, PadEvent, PadEventKind, PadId, ScriptedPadSource,
};
