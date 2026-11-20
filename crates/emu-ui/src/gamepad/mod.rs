//! Gamepad support (ADR 0016, docs/design/phase6-manette.md). Everything but `gilrs_source` is pure logic fed by
//! the injectable `GamepadSource`, so it is tested headless with `ScriptedPadSource`. No `unsafe` here.
//!
//! Combination with other backends: `GamepadInput` is one more `InputBackend` next to `KeyboardInput` (library
//! games) or `ComputerKeyboard`/`ComputerMouse` (computer mode); backends OR their buttons into the same
//! `InputFrame`, so the pad needs no mode switch. Hotkeys: `take_hotkeys` yields `HotkeyAction`s, which the app
//! handles the same way in computer mode and for consoles.

#[cfg(feature = "gamepad")]
mod gilrs_source;
mod input;
mod mapping;
mod source;

#[cfg(feature = "gamepad")]
pub use gilrs_source::GilrsSource;
pub use input::{GamepadInput, GamepadNotice, GamepadSettings, NavKey};
pub use mapping::{PadButton, PadMap, retropad_from_name, retropad_name, set_binding};
pub use source::{
    GamepadError, GamepadSource, PadAxis, PadEvent, PadEventKind, PadId, ScriptedPadSource,
};

use crate::config_gamepad::GamepadConfig;

/// The pad backend the app owns: real source behind a box, so tests and a future SDL3 source plug in alike.
pub type PadBackend = GamepadInput<Box<dyn GamepadSource>>;

/// Opens the platform pad source. `Ok(None)`: disabled (`--no-gamepad`, `enabled = false`, or built without
/// the `gamepad` feature). `Err`: the backend could not start; the caller goes on with the keyboard alone.
/// The map is a placeholder until a game sets its own (`GamepadInput::set_map`).
pub(crate) fn open_backend(
    allowed: bool,
    config: &GamepadConfig,
) -> Result<Option<PadBackend>, GamepadError> {
    if !allowed || !config.enabled {
        tracing::info!("manette désactivée");
        return Ok(None);
    }
    open_platform(config)
}

#[cfg(feature = "gamepad")]
fn open_platform(config: &GamepadConfig) -> Result<Option<PadBackend>, GamepadError> {
    let source: Box<dyn GamepadSource> = Box::new(GilrsSource::new()?);
    Ok(Some(GamepadInput::new(
        source,
        PadMap::default_for(crate::keyboard::PadStyle::Generic),
        GamepadSettings::from_config(config),
    )))
}

#[cfg(not(feature = "gamepad"))]
fn open_platform(_config: &GamepadConfig) -> Result<Option<PadBackend>, GamepadError> {
    tracing::info!("manette non prise en charge par cette compilation (feature « gamepad »)");
    Ok(None)
}
