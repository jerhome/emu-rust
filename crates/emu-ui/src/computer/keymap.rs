//! Positional winit `KeyCode` → libretro `RetroKey` (US layout names): the emulated machine's own keyboard layout
//! (TOS) turns positions into characters, whatever the host layout is.

#[cfg(test)]
use std::collections::HashSet;

use emu_runtime::{KeyModifiers, RetroKey};
use winit::keyboard::{KeyCode, ModifiersState};

/// `RETROK_*` name (lower-case, see `RetroKey::name`) of a physical key; `None` for keys a computer has no use for.
fn name(code: KeyCode) -> Option<&'static str> {
    use KeyCode as K;
    Some(match code {
        K::KeyA => "a",
        K::KeyB => "b",
        K::KeyC => "c",
        K::KeyD => "d",
        K::KeyE => "e",
        K::KeyF => "f",
        K::KeyG => "g",
        K::KeyH => "h",
        K::KeyI => "i",
        K::KeyJ => "j",
        K::KeyK => "k",
        K::KeyL => "l",
        K::KeyM => "m",
        K::KeyN => "n",
        K::KeyO => "o",
        K::KeyP => "p",
        K::KeyQ => "q",
        K::KeyR => "r",
        K::KeyS => "s",
        K::KeyT => "t",
        K::KeyU => "u",
        K::KeyV => "v",
        K::KeyW => "w",
        K::KeyX => "x",
        K::KeyY => "y",
        K::KeyZ => "z",
        K::Digit0 => "0",
        K::Digit1 => "1",
        K::Digit2 => "2",
        K::Digit3 => "3",
        K::Digit4 => "4",
        K::Digit5 => "5",
        K::Digit6 => "6",
        K::Digit7 => "7",
        K::Digit8 => "8",
        K::Digit9 => "9",
        K::Backquote => "backquote",
        K::Minus => "minus",
        K::Equal => "equals",
        K::BracketLeft => "leftbracket",
        K::BracketRight => "rightbracket",
        K::Backslash => "backslash",
        K::Semicolon => "semicolon",
        K::Quote => "quote",
        K::Comma => "comma",
        K::Period => "period",
        K::Slash => "slash",
        // ISO key next to left Shift (`<>` on AZERTY): RetroArch reports it as RETROK_OEM_102.
        K::IntlBackslash => "oem_102",
        K::Space => "space",
        K::Enter => "return",
        K::Tab => "tab",
        K::Backspace => "backspace",
        K::Escape => "escape",
        K::Delete => "delete",
        K::Insert => "insert",
        K::Home => "home",
        K::End => "end",
        K::PageUp => "pageup",
        K::PageDown => "pagedown",
        K::ArrowUp => "up",
        K::ArrowDown => "down",
        K::ArrowLeft => "left",
        K::ArrowRight => "right",
        K::F1 => "f1",
        K::F2 => "f2",
        K::F3 => "f3",
        K::F4 => "f4",
        K::F5 => "f5",
        K::F6 => "f6",
        K::F7 => "f7",
        K::F8 => "f8",
        K::F9 => "f9",
        K::F10 => "f10",
        K::F11 => "f11",
        K::F12 => "f12",
        K::F13 => "f13",
        K::F14 => "f14",
        K::F15 => "f15",
        K::Numpad0 => "kp0",
        K::Numpad1 => "kp1",
        K::Numpad2 => "kp2",
        K::Numpad3 => "kp3",
        K::Numpad4 => "kp4",
        K::Numpad5 => "kp5",
        K::Numpad6 => "kp6",
        K::Numpad7 => "kp7",
        K::Numpad8 => "kp8",
        K::Numpad9 => "kp9",
        K::NumpadDecimal => "kp_period",
        K::NumpadDivide => "kp_divide",
        K::NumpadMultiply => "kp_multiply",
        K::NumpadSubtract => "kp_minus",
        K::NumpadAdd => "kp_plus",
        K::NumpadEnter => "kp_enter",
        K::NumpadEqual => "kp_equals",
        K::ShiftLeft => "lshift",
        K::ShiftRight => "rshift",
        K::ControlLeft => "lctrl",
        K::ControlRight => "rctrl",
        K::AltLeft => "lalt",
        K::AltRight => "ralt",
        K::SuperLeft => "lsuper",
        K::SuperRight => "rsuper",
        K::CapsLock => "capslock",
        K::NumLock => "numlock",
        K::ScrollLock => "scrollock",
        K::Pause => "pause",
        K::PrintScreen => "print",
        K::ContextMenu => "menu",
        K::Help => "help",
        _ => return None,
    })
}

/// Physical key → libretro key; `None` when the key has no libretro equivalent (media keys, IME…).
pub(crate) fn retro_key(code: KeyCode) -> Option<RetroKey> {
    name(code).and_then(RetroKey::from_name)
}

/// State of the lock keys, which winit does not report in `ModifiersState`: tracked by the caller from key events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LockState {
    pub caps: bool,
    pub num: bool,
    pub scroll: bool,
}

impl LockState {
    /// Toggles on the press of a lock key (never on repeat or release); other keys are ignored.
    pub fn on_key(&mut self, code: KeyCode, pressed: bool, repeat: bool) {
        if !pressed || repeat {
            return;
        }
        match code {
            KeyCode::CapsLock => self.caps = !self.caps,
            KeyCode::NumLock => self.num = !self.num,
            KeyCode::ScrollLock => self.scroll = !self.scroll,
            _ => {}
        }
    }
}

/// `RETROKMOD_*` set for the current modifiers and locks.
pub(crate) fn modifiers(state: ModifiersState, locks: LockState) -> KeyModifiers {
    let mut bits = 0;
    let mut add = |on: bool, flag: u16| {
        if on {
            bits |= flag;
        }
    };
    add(state.shift_key(), KeyModifiers::SHIFT);
    add(state.control_key(), KeyModifiers::CTRL);
    add(state.alt_key(), KeyModifiers::ALT);
    add(state.super_key(), KeyModifiers::META);
    add(locks.num, KeyModifiers::NUMLOCK);
    add(locks.caps, KeyModifiers::CAPSLOCK);
    add(locks.scroll, KeyModifiers::SCROLLOCK);
    KeyModifiers(bits)
}

/// Removes the fake Left Ctrl Windows sends before every Right Alt (AltGr), which would otherwise make AltGr
/// characters arrive as Ctrl+Alt+key. The Left Ctrl press is held back until the next event: followed by Right Alt
/// it is dropped (and so is its release), otherwise it is replayed. Feed it only on Windows (`ALTGR_QUIRK`);
/// call [`AltGrFilter::flush`] at the end of each event batch so a real Ctrl press is never delayed past it.
#[derive(Debug, Default)]
pub(crate) struct AltGrFilter {
    pending_ctrl: bool,
    ghost_ctrl: bool,
}

/// Windows is the only platform with the AltGr quirk.
pub(crate) const ALTGR_QUIRK: bool = cfg!(windows);

impl AltGrFilter {
    /// Key transitions to process, in order.
    pub fn on_key(&mut self, code: KeyCode, pressed: bool) -> Vec<(KeyCode, bool)> {
        let mut out = Vec::new();
        if code == KeyCode::ControlLeft {
            if pressed {
                if self.pending_ctrl {
                    out.push((code, true));
                }
                self.pending_ctrl = true;
            } else if self.ghost_ctrl {
                self.ghost_ctrl = false;
            } else {
                self.flush_into(&mut out);
                out.push((code, false));
            }
            return out;
        }
        if code == KeyCode::AltRight && pressed && self.pending_ctrl {
            self.pending_ctrl = false;
            self.ghost_ctrl = true;
        } else {
            self.flush_into(&mut out);
        }
        out.push((code, pressed));
        out
    }

    /// The held-back Left Ctrl press, if any: it was a real one.
    pub fn flush(&mut self) -> Option<(KeyCode, bool)> {
        std::mem::take(&mut self.pending_ctrl).then_some((KeyCode::ControlLeft, true))
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn flush_into(&mut self, out: &mut Vec<(KeyCode, bool)>) {
        out.extend(self.flush());
    }
}

#[cfg(test)]
/// Keys currently reported as held to the core, to drop duplicates and release everything on focus loss.
#[derive(Debug, Default)]
pub(crate) struct HeldKeys(HashSet<KeyCode>);

#[cfg(test)]
impl HeldKeys {
    /// `true` when this changes the state (a repeat or a duplicate does not).
    pub fn update(&mut self, code: KeyCode, pressed: bool) -> bool {
        if pressed {
            self.0.insert(code)
        } else {
            self.0.remove(&code)
        }
    }

    pub fn drain(&mut self) -> Vec<KeyCode> {
        let mut keys: Vec<_> = self.0.drain().collect();
        keys.sort_by_key(|k| *k as u32);
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `KeyCode` the table claims must resolve to a distinct libretro key.
    #[test]
    fn table_resolves_without_duplicates() {
        let codes = all_codes();
        let mut seen = HashSet::new();
        for code in codes {
            let Some(n) = name(code) else { continue };
            let key = retro_key(code).unwrap_or_else(|| panic!("{code:?}: unknown name {n}"));
            assert_eq!(key.name(), Some(n));
            assert!(seen.insert(key), "duplicate key for {code:?}");
        }
        assert!(seen.len() >= 100, "{}", seen.len());
    }

    fn all_codes() -> Vec<KeyCode> {
        use KeyCode::*;
        vec![
            KeyA,
            KeyB,
            KeyC,
            KeyD,
            KeyE,
            KeyF,
            KeyG,
            KeyH,
            KeyI,
            KeyJ,
            KeyK,
            KeyL,
            KeyM,
            KeyN,
            KeyO,
            KeyP,
            KeyQ,
            KeyR,
            KeyS,
            KeyT,
            KeyU,
            KeyV,
            KeyW,
            KeyX,
            KeyY,
            KeyZ,
            Digit0,
            Digit1,
            Digit2,
            Digit3,
            Digit4,
            Digit5,
            Digit6,
            Digit7,
            Digit8,
            Digit9,
            Backquote,
            Minus,
            Equal,
            BracketLeft,
            BracketRight,
            Backslash,
            Semicolon,
            Quote,
            Comma,
            Period,
            Slash,
            IntlBackslash,
            Space,
            Enter,
            Tab,
            Backspace,
            Escape,
            Delete,
            Insert,
            Home,
            End,
            PageUp,
            PageDown,
            ArrowUp,
            ArrowDown,
            ArrowLeft,
            ArrowRight,
            F1,
            F2,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            F10,
            F11,
            F12,
            F13,
            F14,
            F15,
            Numpad0,
            Numpad1,
            Numpad2,
            Numpad3,
            Numpad4,
            Numpad5,
            Numpad6,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadDecimal,
            NumpadDivide,
            NumpadMultiply,
            NumpadSubtract,
            NumpadAdd,
            NumpadEnter,
            NumpadEqual,
            ShiftLeft,
            ShiftRight,
            ControlLeft,
            ControlRight,
            AltLeft,
            AltRight,
            SuperLeft,
            SuperRight,
            CapsLock,
            NumLock,
            ScrollLock,
            Pause,
            PrintScreen,
            ContextMenu,
            Help,
        ]
    }

    #[test]
    fn positional_not_layout_dependent() {
        // The key at the US "Q" place is "a" on AZERTY: the position wins.
        assert_eq!(retro_key(KeyCode::KeyQ).and_then(RetroKey::name), Some("q"));
        assert_eq!(
            retro_key(KeyCode::Enter).and_then(RetroKey::name),
            Some("return")
        );
        assert_eq!(
            retro_key(KeyCode::ControlRight).and_then(RetroKey::name),
            Some("rctrl")
        );
        assert_eq!(retro_key(KeyCode::MediaPlayPause), None);
        assert_eq!(retro_key(KeyCode::BrowserBack), None);
    }

    #[test]
    fn modifier_bits() {
        let m = modifiers(
            ModifiersState::SHIFT | ModifiersState::ALT,
            LockState {
                caps: true,
                ..LockState::default()
            },
        );
        assert!(m.contains(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CAPSLOCK));
        assert!(!m.contains(KeyModifiers::CTRL));
        assert!(!m.contains(KeyModifiers::NUMLOCK));
        assert_eq!(
            modifiers(ModifiersState::empty(), LockState::default()),
            KeyModifiers::default()
        );
    }

    #[test]
    fn locks_toggle_on_press_only() {
        let mut l = LockState::default();
        l.on_key(KeyCode::CapsLock, true, false);
        l.on_key(KeyCode::CapsLock, true, true);
        l.on_key(KeyCode::CapsLock, false, false);
        assert!(l.caps);
        l.on_key(KeyCode::CapsLock, true, false);
        assert!(!l.caps);
        l.on_key(KeyCode::NumLock, true, false);
        assert!(l.num && !l.scroll);
    }

    #[test]
    fn altgr_fake_ctrl_is_dropped() {
        let mut f = AltGrFilter::default();
        assert!(f.on_key(KeyCode::ControlLeft, true).is_empty());
        assert_eq!(
            f.on_key(KeyCode::AltRight, true),
            vec![(KeyCode::AltRight, true)]
        );
        assert_eq!(f.flush(), None);
        assert_eq!(f.on_key(KeyCode::KeyE, true), vec![(KeyCode::KeyE, true)]);
        // Release order on Windows: fake Ctrl first, then AltGr.
        assert!(f.on_key(KeyCode::ControlLeft, false).is_empty());
        assert_eq!(
            f.on_key(KeyCode::AltRight, false),
            vec![(KeyCode::AltRight, false)]
        );
        // The next real Ctrl works.
        assert!(f.on_key(KeyCode::ControlLeft, true).is_empty());
        assert_eq!(f.flush(), Some((KeyCode::ControlLeft, true)));
        assert_eq!(
            f.on_key(KeyCode::ControlLeft, false),
            vec![(KeyCode::ControlLeft, false)]
        );
    }

    #[test]
    fn real_ctrl_is_replayed_in_order() {
        let mut f = AltGrFilter::default();
        assert!(f.on_key(KeyCode::ControlLeft, true).is_empty());
        assert_eq!(
            f.on_key(KeyCode::KeyC, true),
            vec![(KeyCode::ControlLeft, true), (KeyCode::KeyC, true)]
        );
        // Plain Right Alt without Ctrl passes.
        assert_eq!(
            f.on_key(KeyCode::AltRight, true),
            vec![(KeyCode::AltRight, true)]
        );
    }

    #[test]
    fn held_keys_ignore_duplicates_and_drain() {
        let mut h = HeldKeys::default();
        assert!(h.update(KeyCode::KeyA, true));
        assert!(!h.update(KeyCode::KeyA, true));
        assert!(h.update(KeyCode::KeyB, true));
        assert!(h.update(KeyCode::KeyB, false));
        assert!(!h.update(KeyCode::KeyB, false));
        assert_eq!(h.drain(), vec![KeyCode::KeyA]);
        assert!(h.drain().is_empty());
    }
}
