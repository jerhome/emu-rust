//! Input of computer systems (ADR 0015): positional key map, host key, keyboard / mouse backends, the session that
//! routes winit events between them, and the virtual keyboard. `app.rs` feeds it winit events.

// Used by U3b/U4: drop the allows once wired.
#[allow(dead_code)]
pub(crate) mod amiga;
#[allow(dead_code)]
pub(crate) mod computed;
pub(crate) mod host;
pub(crate) mod input;
pub(crate) mod keymap;
pub(crate) mod session;
pub(crate) mod vkbd;
