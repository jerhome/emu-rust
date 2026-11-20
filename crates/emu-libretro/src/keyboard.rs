//! Keyboard and mouse vocabulary of `libretro.h` (ADR 0015): `RETROK_*`, `RETROKMOD_*`, `RETRO_DEVICE_ID_MOUSE_*`.
//!
//! Contract only (version 2.0): bodies are filled by the `libretro-ffi` agent.

/// A `retro_key` value (`RETROK_*`). Positional: it names the key at that place on a US layout, whatever the
/// host layout is (the emulated machine's own TOS / Kickstart layout gives the character).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RetroKey(u16);

impl RetroKey {
    /// `RETROK_UNKNOWN`.
    pub const UNKNOWN: RetroKey = RetroKey(0);
    /// `RETROK_LAST` (342 in libretro.h, 2026): every valid key is strictly below.
    pub const LAST: u16 = 342;

    /// `None` when `raw >= LAST`.
    pub const fn new(raw: u16) -> Option<Self> {
        if raw < Self::LAST {
            Some(RetroKey(raw))
        } else {
            None
        }
    }

    pub const fn raw(self) -> u16 {
        self.0
    }

    /// Stable lower-case name derived from the `RETROK_*` identifier (`"a"`, `"return"`, `"kp_enter"`, `"rctrl"`),
    /// used by keyboard layouts and logs. `None` for unassigned values.
    pub fn name(self) -> Option<&'static str> {
        todo!("libretro-ffi: RETROK_* name table")
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let _ = name;
        todo!("libretro-ffi: inverse of `name`")
    }
}

/// `RETROKMOD_*` bit set sent with keyboard events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct KeyModifiers(pub u16);

impl KeyModifiers {
    pub const SHIFT: u16 = 0x01;
    pub const CTRL: u16 = 0x02;
    pub const ALT: u16 = 0x04;
    pub const META: u16 = 0x08;
    pub const NUMLOCK: u16 = 0x10;
    pub const CAPSLOCK: u16 = 0x20;
    pub const SCROLLOCK: u16 = 0x40;

    pub const fn contains(self, flag: u16) -> bool {
        self.0 & flag == flag
    }
}

/// One `retro_keyboard_event_t` call: key transition and/or text character (UTF-32, 0 when none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub down: bool,
    /// `RetroKey::UNKNOWN` for a character-only event.
    pub key: RetroKey,
    pub character: u32,
    pub modifiers: KeyModifiers,
}

/// `RETRO_DEVICE_ID_MOUSE_*` ids (`InputQuery::id` when `InputQuery::device == InputDevice::Mouse`).
pub mod mouse_id {
    /// Relative motion since the previous `retro_run`, in host pixels (`i16`).
    pub const X: u32 = 0;
    pub const Y: u32 = 1;
    pub const LEFT: u32 = 2;
    pub const RIGHT: u32 = 3;
    pub const WHEEL_UP: u32 = 4;
    pub const WHEEL_DOWN: u32 = 5;
    pub const MIDDLE: u32 = 6;
    pub const HORIZ_WHEEL_UP: u32 = 7;
    pub const HORIZ_WHEEL_DOWN: u32 = 8;
    pub const BUTTON_4: u32 = 9;
    pub const BUTTON_5: u32 = 10;
}

impl crate::core::Core {
    /// `true` once the core called `RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK`.
    pub fn has_keyboard_callback(&self) -> bool {
        todo!("libretro-ffi")
    }

    /// Delivers one event to the core's keyboard callback, outside `retro_run` (the borrow rules guarantee it)
    /// and on the core thread. No-op without a callback. A panic or unwinding is impossible across the C
    /// callback (it is C code); errors are not reported by libretro.
    pub fn send_key_event(&mut self, event: KeyEvent) {
        let _ = event;
        todo!("libretro-ffi")
    }

    /// `RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME` was received with `true`.
    pub fn supports_no_game(&self) -> bool {
        todo!("libretro-ffi")
    }

    /// `retro_load_game(NULL)`: boots the machine without content (GEM desktop for Atari ST).
    /// [`crate::LibretroError::NoGameUnsupported`] when the core did not announce it.
    pub fn load_no_game(&mut self) -> Result<crate::types::AvInfo, crate::error::LibretroError> {
        todo!("libretro-ffi")
    }
}
