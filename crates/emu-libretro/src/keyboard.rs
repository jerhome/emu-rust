//! Keyboard and mouse vocabulary of `libretro.h` (ADR 0015): `RETROK_*`, `RETROKMOD_*`, `RETRO_DEVICE_ID_MOUSE_*`.

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
        KEY_NAMES
            .binary_search_by_key(&self.0, |&(code, _)| code)
            .ok()
            .map(|i| KEY_NAMES[i].1)
    }

    /// Inverse of [`RetroKey::name`], ASCII case-insensitive.
    pub fn from_name(name: &str) -> Option<Self> {
        KEY_NAMES
            .iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(name))
            .map(|&(code, _)| RetroKey(code))
    }
}

/// `enum retro_key` of libretro.h, sorted by value; names are the identifiers without `RETROK_`, lower-cased.
const KEY_NAMES: &[(u16, &str)] = &[
    (0, "unknown"),
    (8, "backspace"),
    (9, "tab"),
    (12, "clear"),
    (13, "return"),
    (19, "pause"),
    (27, "escape"),
    (32, "space"),
    (33, "exclaim"),
    (34, "quotedbl"),
    (35, "hash"),
    (36, "dollar"),
    (38, "ampersand"),
    (39, "quote"),
    (40, "leftparen"),
    (41, "rightparen"),
    (42, "asterisk"),
    (43, "plus"),
    (44, "comma"),
    (45, "minus"),
    (46, "period"),
    (47, "slash"),
    (48, "0"),
    (49, "1"),
    (50, "2"),
    (51, "3"),
    (52, "4"),
    (53, "5"),
    (54, "6"),
    (55, "7"),
    (56, "8"),
    (57, "9"),
    (58, "colon"),
    (59, "semicolon"),
    (60, "less"),
    (61, "equals"),
    (62, "greater"),
    (63, "question"),
    (64, "at"),
    (91, "leftbracket"),
    (92, "backslash"),
    (93, "rightbracket"),
    (94, "caret"),
    (95, "underscore"),
    (96, "backquote"),
    (97, "a"),
    (98, "b"),
    (99, "c"),
    (100, "d"),
    (101, "e"),
    (102, "f"),
    (103, "g"),
    (104, "h"),
    (105, "i"),
    (106, "j"),
    (107, "k"),
    (108, "l"),
    (109, "m"),
    (110, "n"),
    (111, "o"),
    (112, "p"),
    (113, "q"),
    (114, "r"),
    (115, "s"),
    (116, "t"),
    (117, "u"),
    (118, "v"),
    (119, "w"),
    (120, "x"),
    (121, "y"),
    (122, "z"),
    (123, "leftbrace"),
    (124, "bar"),
    (125, "rightbrace"),
    (126, "tilde"),
    (127, "delete"),
    (256, "kp0"),
    (257, "kp1"),
    (258, "kp2"),
    (259, "kp3"),
    (260, "kp4"),
    (261, "kp5"),
    (262, "kp6"),
    (263, "kp7"),
    (264, "kp8"),
    (265, "kp9"),
    (266, "kp_period"),
    (267, "kp_divide"),
    (268, "kp_multiply"),
    (269, "kp_minus"),
    (270, "kp_plus"),
    (271, "kp_enter"),
    (272, "kp_equals"),
    (273, "up"),
    (274, "down"),
    (275, "right"),
    (276, "left"),
    (277, "insert"),
    (278, "home"),
    (279, "end"),
    (280, "pageup"),
    (281, "pagedown"),
    (282, "f1"),
    (283, "f2"),
    (284, "f3"),
    (285, "f4"),
    (286, "f5"),
    (287, "f6"),
    (288, "f7"),
    (289, "f8"),
    (290, "f9"),
    (291, "f10"),
    (292, "f11"),
    (293, "f12"),
    (294, "f13"),
    (295, "f14"),
    (296, "f15"),
    (300, "numlock"),
    (301, "capslock"),
    (302, "scrollock"),
    (303, "rshift"),
    (304, "lshift"),
    (305, "rctrl"),
    (306, "lctrl"),
    (307, "ralt"),
    (308, "lalt"),
    (309, "rmeta"),
    (310, "lmeta"),
    (311, "lsuper"),
    (312, "rsuper"),
    (313, "mode"),
    (314, "compose"),
    (315, "help"),
    (316, "print"),
    (317, "sysreq"),
    (318, "break"),
    (319, "menu"),
    (320, "power"),
    (321, "euro"),
    (322, "undo"),
    (323, "oem_102"),
    (324, "browser_back"),
    (325, "browser_forward"),
    (326, "browser_refresh"),
    (327, "browser_stop"),
    (328, "browser_search"),
    (329, "browser_favorites"),
    (330, "browser_home"),
    (331, "volume_mute"),
    (332, "volume_down"),
    (333, "volume_up"),
    (334, "media_next"),
    (335, "media_prev"),
    (336, "media_stop"),
    (337, "media_play_pause"),
    (338, "launch_mail"),
    (339, "launch_media"),
    (340, "launch_app1"),
    (341, "launch_app2"),
];

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
    /// `true` once the core called `RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK` with a non-null callback.
    pub fn has_keyboard_callback(&self) -> bool {
        crate::callbacks::keyboard_callback().is_some()
    }

    /// Delivers one event to the core's keyboard callback, outside `retro_run` (`&mut self` guarantees it) and on
    /// the core thread (`Core` is `!Send`). No-op without a callback or without loaded content. libretro reports
    /// no errors for keyboard events.
    pub fn send_key_event(&mut self, event: KeyEvent) {
        if !self.game_loaded() {
            return;
        }
        // Copied out first: the callback may re-enter the environment.
        let Some(callback) = crate::callbacks::keyboard_callback() else {
            return;
        };
        // SAFETY: `callback` was registered by this core, whose library stays loaded while `self` lives; we are
        // on the owning thread and outside `retro_run`, with content loaded. `keycode` is below `RETROK_LAST`
        // (`RetroKey` invariant). It is C code: no Rust panic can unwind out of it.
        unsafe {
            callback(
                event.down,
                std::ffi::c_uint::from(event.key.raw()),
                event.character,
                event.modifiers.0,
            )
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_libretro_h() {
        assert_eq!(RetroKey::new(13).and_then(RetroKey::name), Some("return"));
        assert_eq!(RetroKey::new(97).and_then(RetroKey::name), Some("a"));
        assert_eq!(
            RetroKey::new(271).and_then(RetroKey::name),
            Some("kp_enter")
        );
        assert_eq!(RetroKey::new(305).and_then(RetroKey::name), Some("rctrl"));
        assert_eq!(RetroKey::new(315).and_then(RetroKey::name), Some("help"));
        assert_eq!(RetroKey::new(322).and_then(RetroKey::name), Some("undo"));
        assert_eq!(
            RetroKey::new(341).and_then(RetroKey::name),
            Some("launch_app2")
        );
        assert_eq!(RetroKey::UNKNOWN.name(), Some("unknown"));
        // Unassigned values.
        assert_eq!(RetroKey::new(1).and_then(RetroKey::name), None);
        assert_eq!(RetroKey::new(200).and_then(RetroKey::name), None);
        assert_eq!(RetroKey::new(297).and_then(RetroKey::name), None);
        assert!(RetroKey::new(RetroKey::LAST).is_none());
    }

    #[test]
    fn table_is_sorted_unique_and_invertible() {
        assert!(KEY_NAMES.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(KEY_NAMES.iter().all(|&(code, _)| code < RetroKey::LAST));
        for &(code, name) in KEY_NAMES {
            assert_eq!(name, name.to_ascii_lowercase());
            assert_eq!(
                RetroKey::from_name(name).map(RetroKey::raw),
                Some(code),
                "{name}"
            );
        }
        assert_eq!(
            RetroKey::from_name("KP_Enter").map(RetroKey::raw),
            Some(271)
        );
        assert_eq!(RetroKey::from_name("nope"), None);
        // 158 named keys of libretro.h + RETROK_UNKNOWN.
        assert_eq!(KEY_NAMES.len(), 159);
    }

    #[test]
    fn modifiers() {
        let m = KeyModifiers(KeyModifiers::SHIFT | KeyModifiers::CAPSLOCK);
        assert!(m.contains(KeyModifiers::SHIFT));
        assert!(m.contains(KeyModifiers::CAPSLOCK));
        assert!(!m.contains(KeyModifiers::CTRL));
        assert!(!m.contains(KeyModifiers::SHIFT | KeyModifiers::CTRL));
    }
}
