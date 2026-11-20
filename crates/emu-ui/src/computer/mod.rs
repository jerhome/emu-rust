//! Pure input logic of computer systems (ADR 0015): positional key map, host key, keyboard / mouse backends.
//! No window here; `app.rs` feeds it winit events (task U2).
//! Not wired yet: `app.rs` uses it in task U2 (remove this allow then).
#![allow(dead_code)]

pub(crate) mod host;
pub(crate) mod input;
pub(crate) mod keymap;
