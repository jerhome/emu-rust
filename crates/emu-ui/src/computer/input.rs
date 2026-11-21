//! Keyboard and mouse backends of computer systems, and the pointer-capture state machine. Pure: winit events are
//! translated by `app.rs` (U2). Everything runs on the UI thread (ADR 0010).

use std::collections::BTreeSet;

use emu_runtime::{InputBackend, InputFrame, KeyEvent, KeyModifiers, MouseButton, RetroKey};
use winit::keyboard::KeyCode;

use crate::config_computer::KeyboardModeChoice;

use super::keymap::KeyRemap;

/// What the physical keyboard drives (host key + J).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum KeyboardMode {
    /// Every key goes to the machine's keyboard.
    #[default]
    Keyboard,
    /// Keys of the 1.0 `KeyMap` drive the joystick (pad); the others still reach the keyboard.
    Joystick,
}

/// Where a key press goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyTarget {
    Keyboard,
    Pad,
}

impl KeyboardMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Keyboard => Self::Joystick,
            Self::Joystick => Self::Keyboard,
        }
    }

    /// `in_keymap`: the key is bound in the joystick `KeyMap`.
    pub fn route(self, in_keymap: bool) -> KeyTarget {
        if self == Self::Joystick && in_keymap {
            KeyTarget::Pad
        } else {
            KeyTarget::Keyboard
        }
    }
}

impl From<KeyboardModeChoice> for KeyboardMode {
    fn from(c: KeyboardModeChoice) -> Self {
        match c {
            KeyboardModeChoice::Keyboard => Self::Keyboard,
            KeyboardModeChoice::Joystick => Self::Joystick,
        }
    }
}

/// Keyboard of the emulated machine: held keys (polling) and ordered transitions (callback).
#[derive(Debug, Default)]
pub(crate) struct ComputerKeyboard {
    /// Physical key → machine key: positional default plus the layout's and the user's overrides.
    remap: KeyRemap,
    held: BTreeSet<RetroKey>,
    events: Vec<KeyEvent>,
    modifiers: KeyModifiers,
    paused: bool,
}

/// Safety bound of the transition queue (a poll drains it every frame).
const MAX_PENDING_EVENTS: usize = 256;

impl ComputerKeyboard {
    /// Overrides of the machine's layout (`KeyRemap::for_layout`); the default is the positional map alone.
    pub fn set_remap(&mut self, remap: KeyRemap) {
        self.remap = remap;
    }

    /// While paused, new key presses are dropped (they would all reach the machine at once on resume);
    /// releases still go through so no key stays stuck.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Modifiers sent with the next events (`keymap::modifiers`).
    pub fn set_modifiers(&mut self, modifiers: KeyModifiers) {
        self.modifiers = modifiers;
    }

    /// One non-repeat transition. `text`: characters winit produced for the press (ignored when empty or control
    /// characters: the machine's own layout turns the position into a character). Keys without libretro
    /// equivalent and duplicate presses / releases are ignored.
    pub fn handle_key(&mut self, code: KeyCode, pressed: bool, text: Option<&str>) {
        let Some(key) = self.remap.map(code) else {
            return;
        };
        if self.paused && pressed {
            return;
        }
        if self.events.len() >= MAX_PENDING_EVENTS && pressed {
            return;
        }
        let changed = if pressed {
            self.held.insert(key)
        } else {
            self.held.remove(&key)
        };
        if !changed {
            return;
        }
        let character = if pressed {
            text.and_then(|t| t.chars().next())
                .filter(|c| !c.is_control())
                .map_or(0, u32::from)
        } else {
            0
        };
        self.events.push(KeyEvent {
            down: pressed,
            key,
            character,
            modifiers: self.modifiers,
        });
    }

    /// Releases every held key (focus loss, mode switch, menu opening).
    pub fn release_all(&mut self) {
        for key in std::mem::take(&mut self.held) {
            self.events.push(KeyEvent {
                down: false,
                key,
                character: 0,
                modifiers: self.modifiers,
            });
        }
    }

    #[cfg(test)]
    pub fn is_held(&self, key: RetroKey) -> bool {
        self.held.contains(&key)
    }

    #[cfg(test)]
    /// Events queued since the last poll (tests, virtual keyboard merge).
    pub fn pending_events(&self) -> &[KeyEvent] {
        &self.events
    }

    #[cfg(test)]
    /// Key events from another source (virtual keyboard), appended in order.
    pub fn push_events(&mut self, events: impl IntoIterator<Item = KeyEvent>) {
        for e in events {
            if e.key != RetroKey::UNKNOWN {
                if e.down {
                    self.held.insert(e.key);
                } else {
                    self.held.remove(&e.key);
                }
            }
            self.events.push(e);
        }
    }
}

impl InputBackend for ComputerKeyboard {
    fn poll(&mut self, _frame_index: u64, state: &mut InputFrame) {
        for key in &self.held {
            state.keyboard.set(*key, true);
        }
        state.key_events.append(&mut self.events);
    }
}

/// Relative mouse of port 0. Sub-pixel motion and fractional wheel steps are carried over to the next frame.
#[derive(Debug)]
pub(crate) struct ComputerMouse {
    sensitivity: f64,
    dx: f64,
    dy: f64,
    wheel: f64,
    hwheel: f64,
    held: u8,
    /// Buttons pressed since the last poll: a click shorter than a frame still registers once.
    latched: u8,
    last_cursor: Option<(f64, f64)>,
}

pub(crate) const MIN_SENSITIVITY: f32 = 0.1;
pub(crate) const MAX_SENSITIVITY: f32 = 5.0;

impl Default for ComputerMouse {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl ComputerMouse {
    pub fn new(sensitivity: f32) -> Self {
        let mut m = Self {
            sensitivity: 1.0,
            dx: 0.0,
            dy: 0.0,
            wheel: 0.0,
            hwheel: 0.0,
            held: 0,
            latched: 0,
            last_cursor: None,
        };
        m.set_sensitivity(sensitivity);
        m
    }

    /// Clamped to `MIN_SENSITIVITY..=MAX_SENSITIVITY`; NaN gives 1.0.
    pub fn set_sensitivity(&mut self, sensitivity: f32) {
        self.sensitivity = if sensitivity.is_nan() {
            1.0
        } else {
            f64::from(sensitivity.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY))
        };
    }

    /// Raw relative motion (`DeviceEvent::MouseMotion`) in host pixels.
    pub fn motion(&mut self, dx: f64, dy: f64) {
        if dx.is_finite() && dy.is_finite() {
            self.dx += dx * self.sensitivity;
            self.dy += dy * self.sensitivity;
        }
    }

    /// Fallback without grab (noVNC, WSLg): deltas from successive `CursorMoved` positions.
    pub fn cursor_moved(&mut self, x: f64, y: f64) {
        if let Some((px, py)) = self.last_cursor {
            self.motion(x - px, y - py);
        }
        self.last_cursor = Some((x, y));
    }

    /// Cursor left the window or capture changed: the next `cursor_moved` starts a new reference.
    pub fn cursor_left(&mut self) {
        self.last_cursor = None;
    }

    pub fn button(&mut self, button: MouseButton, pressed: bool) {
        let bit = 1u8 << (button as u8);
        if pressed {
            self.held |= bit;
            self.latched |= bit;
        } else {
            self.held &= !bit;
        }
    }

    /// Wheel in notches (lines); pixel deltas must be converted by the caller.
    pub fn wheel(&mut self, vertical: f64, horizontal: f64) {
        if vertical.is_finite() && horizontal.is_finite() {
            self.wheel += vertical;
            self.hwheel += horizontal;
        }
    }

    /// Releases buttons and drops pending motion (focus loss, menu).
    pub fn release_all(&mut self) {
        self.held = 0;
        self.latched = 0;
        self.dx = 0.0;
        self.dy = 0.0;
        self.wheel = 0.0;
        self.hwheel = 0.0;
        self.last_cursor = None;
    }
}

/// Whole part of `v` (clamped to `i16`, as libretro mouse axes are), remainder left in `v`.
fn take_whole(v: &mut f64) -> i32 {
    let whole = v.trunc().clamp(f64::from(i16::MIN), f64::from(i16::MAX));
    // Over-range motion is dropped, not carried over.
    *v = if v.abs() > f64::from(i16::MAX) {
        0.0
    } else {
        *v - whole
    };
    whole as i32
}

impl InputBackend for ComputerMouse {
    fn poll(&mut self, _frame_index: u64, state: &mut InputFrame) {
        let m = &mut state.mouse;
        m.dx += take_whole(&mut self.dx);
        m.dy += take_whole(&mut self.dy);
        m.wheel += take_whole(&mut self.wheel);
        m.hwheel += take_whole(&mut self.hwheel);
        m.buttons |= self.held | self.latched;
        self.latched = 0;
    }
}

/// winit button → libretro button; `None` for unknown buttons.
pub(crate) fn map_button(button: winit::event::MouseButton) -> Option<MouseButton> {
    use winit::event::MouseButton as W;
    match button {
        W::Left => Some(MouseButton::Left),
        W::Right => Some(MouseButton::Right),
        W::Middle => Some(MouseButton::Middle),
        W::Back => Some(MouseButton::Button4),
        W::Forward => Some(MouseButton::Button5),
        W::Other(_) => None,
    }
}

/// Pointer capture (host key + M). The UI lives in the same window: any menu releases the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CaptureState {
    #[default]
    Released,
    /// Cursor grabbed and hidden; `locked`: `CursorGrabMode::Locked` (else `Confined`).
    Captured { locked: bool },
    /// No grab possible (noVNC, some WSLg setups): cursor hidden over the window, motion from `CursorMoved`.
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureEvent {
    /// Host key + M.
    Toggle,
    /// Click in the game area (only captures when `auto_capture`).
    ClickInGame,
    FocusLost,
    /// A menu or dialog opened.
    MenuOpened,
    /// Result of the grab ordered by [`CaptureEffect::Grab`]: `Some(locked)` or `None` when both modes failed.
    GrabResult(Option<bool>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureEffect {
    None,
    /// Try `Locked` then `Confined`, hide the cursor, then report `GrabResult`.
    Grab,
    /// Release the grab and show the cursor.
    Ungrab,
}

impl CaptureState {
    pub fn next(self, event: CaptureEvent, auto_capture: bool) -> (Self, CaptureEffect) {
        use CaptureEffect as E;
        use CaptureEvent as V;
        match (self, event) {
            (Self::Released, V::Toggle) => (self, E::Grab),
            (Self::Released, V::ClickInGame) if auto_capture => (self, E::Grab),
            (Self::Released, V::GrabResult(r)) => (
                r.map_or(Self::Fallback, |locked| Self::Captured { locked }),
                E::None,
            ),
            (Self::Captured { .. } | Self::Fallback, V::Toggle | V::FocusLost | V::MenuOpened) => {
                (Self::Released, E::Ungrab)
            }
            _ => (self, E::None),
        }
    }

    pub fn is_captured(self) -> bool {
        self != Self::Released
    }

    /// Raw device motion is used when grabbed; `CursorMoved` deltas in the fallback.
    pub fn uses_device_motion(self) -> bool {
        matches!(self, Self::Captured { .. })
    }

    pub fn uses_cursor_motion(self) -> bool {
        self == Self::Fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_runtime::{KeyModifiers, MouseState};

    fn poll(b: &mut impl InputBackend) -> InputFrame {
        let mut f = InputFrame::default();
        b.poll(0, &mut f);
        f
    }

    #[test]
    fn mode_routing_and_toggle() {
        assert_eq!(KeyboardMode::default(), KeyboardMode::Keyboard);
        assert_eq!(KeyboardMode::Keyboard.route(true), KeyTarget::Keyboard);
        assert_eq!(KeyboardMode::Joystick.route(true), KeyTarget::Pad);
        assert_eq!(KeyboardMode::Joystick.route(false), KeyTarget::Keyboard);
        assert_eq!(KeyboardMode::Keyboard.toggled(), KeyboardMode::Joystick);
        assert_eq!(
            KeyboardMode::from(KeyboardModeChoice::Joystick),
            KeyboardMode::Joystick
        );
    }

    #[test]
    fn key_events_in_order_with_text_and_modifiers() {
        let mut k = ComputerKeyboard::default();
        k.set_modifiers(KeyModifiers(KeyModifiers::SHIFT));
        k.handle_key(KeyCode::KeyQ, true, Some("A"));
        k.handle_key(KeyCode::Enter, true, Some("\r"));
        k.handle_key(KeyCode::Enter, true, Some("\r")); // duplicate
        k.handle_key(KeyCode::KeyQ, false, None);
        k.handle_key(KeyCode::MediaPlayPause, true, None);
        let ev = k.pending_events().to_vec();
        assert_eq!(ev.len(), 3);
        assert!(ev[0].down && ev[0].character == u32::from('A'));
        assert_eq!(ev[0].key.name(), Some("q"));
        assert!(ev[0].modifiers.contains(KeyModifiers::SHIFT));
        assert_eq!(ev[1].character, 0);
        assert!(!ev[2].down && ev[2].character == 0);
        assert!(k.is_held(RetroKey::from_name("return").unwrap()));
        assert!(!k.is_held(RetroKey::from_name("q").unwrap()));
    }

    #[test]
    fn presses_are_ignored_while_paused_but_releases_pass() {
        let mut k = ComputerKeyboard::default();
        k.handle_key(KeyCode::KeyA, true, None);
        k.set_paused(true);
        k.handle_key(KeyCode::KeyB, true, None);
        k.handle_key(KeyCode::KeyA, false, None);
        let ev = k.pending_events();
        assert_eq!(ev.len(), 2);
        assert!(!k.is_held(RetroKey::from_name("a").unwrap()));
        assert!(!k.is_held(RetroKey::from_name("b").unwrap()));
        k.set_paused(false);
        k.handle_key(KeyCode::KeyB, true, None);
        assert_eq!(k.pending_events().len(), 3);
    }

    #[test]
    fn release_all_emits_releases() {
        let mut k = ComputerKeyboard::default();
        k.handle_key(KeyCode::KeyA, true, None);
        k.handle_key(KeyCode::KeyB, true, None);
        k.release_all();
        let ups = k.pending_events().iter().filter(|e| !e.down).count();
        assert_eq!(ups, 2);
        k.release_all();
        assert_eq!(k.pending_events().len(), 4);
        // Poll drains the queue; nothing is held, so the (not yet implemented) bitset is untouched.
        let f = poll(&mut k);
        assert_eq!(f.key_events.len(), 4);
        assert!(k.pending_events().is_empty());
    }

    #[test]
    fn virtual_keyboard_events_are_tracked() {
        let mut k = ComputerKeyboard::default();
        let key = RetroKey::from_name("f1").unwrap();
        k.push_events([KeyEvent {
            down: true,
            key,
            character: 0,
            modifiers: KeyModifiers::default(),
        }]);
        assert!(k.is_held(key));
        k.release_all();
        assert!(!k.is_held(key));
    }

    #[test]
    fn mouse_scaling_and_remainder() {
        let mut m = ComputerMouse::new(0.5);
        m.motion(3.0, -3.0);
        let f = poll(&mut m);
        assert_eq!((f.mouse.dx, f.mouse.dy), (1, -1));
        m.motion(1.0, 1.0); // 0.5 + 0.5 carried = 1
        let f = poll(&mut m);
        assert_eq!((f.mouse.dx, f.mouse.dy), (1, 0));
        assert_eq!(poll(&mut m).mouse, MouseState::default());
    }

    #[test]
    fn mouse_motion_is_clamped_and_finite() {
        let mut m = ComputerMouse::new(1.0);
        m.motion(1.0e9, 0.0);
        m.motion(f64::NAN, 5.0);
        assert_eq!(poll(&mut m).mouse.dx, i32::from(i16::MAX));
        m.set_sensitivity(f32::NAN);
        m.set_sensitivity(100.0);
        m.motion(1.0, 0.0);
        assert_eq!(poll(&mut m).mouse.dx, 5);
    }

    #[test]
    fn short_click_registers_once() {
        let mut m = ComputerMouse::default();
        m.button(MouseButton::Left, true);
        m.button(MouseButton::Left, false);
        assert_eq!(poll(&mut m).mouse.buttons, 1);
        assert_eq!(poll(&mut m).mouse.buttons, 0);
        m.button(MouseButton::Right, true);
        assert_eq!(poll(&mut m).mouse.buttons, 2);
        assert_eq!(poll(&mut m).mouse.buttons, 2);
        m.release_all();
        assert_eq!(poll(&mut m).mouse.buttons, 0);
    }

    #[test]
    fn wheel_and_cursor_fallback() {
        let mut m = ComputerMouse::default();
        m.wheel(1.5, 0.0);
        assert_eq!(poll(&mut m).mouse.wheel, 1);
        m.wheel(0.5, -2.0);
        let f = poll(&mut m);
        assert_eq!((f.mouse.wheel, f.mouse.hwheel), (1, -2));
        m.cursor_moved(10.0, 10.0); // reference only
        m.cursor_moved(14.0, 7.0);
        let f = poll(&mut m);
        assert_eq!((f.mouse.dx, f.mouse.dy), (4, -3));
        m.cursor_left();
        m.cursor_moved(100.0, 100.0);
        assert_eq!(poll(&mut m).mouse.dx, 0);
    }

    #[test]
    fn winit_buttons() {
        use winit::event::MouseButton as W;
        assert_eq!(map_button(W::Left), Some(MouseButton::Left));
        assert_eq!(map_button(W::Back), Some(MouseButton::Button4));
        assert_eq!(map_button(W::Other(9)), None);
    }

    #[test]
    fn capture_transitions() {
        use CaptureEffect as E;
        use CaptureEvent as V;
        let s = CaptureState::Released;
        assert_eq!(s.next(V::Toggle, false), (s, E::Grab));
        assert_eq!(s.next(V::ClickInGame, false), (s, E::None));
        assert_eq!(s.next(V::ClickInGame, true), (s, E::Grab));
        let (c, e) = s.next(V::GrabResult(Some(true)), false);
        assert_eq!((c, e), (CaptureState::Captured { locked: true }, E::None));
        assert!(c.uses_device_motion() && !c.uses_cursor_motion());
        let (f, _) = s.next(V::GrabResult(None), false);
        assert_eq!(f, CaptureState::Fallback);
        assert!(f.uses_cursor_motion() && f.is_captured());
        for st in [c, f] {
            for ev in [V::Toggle, V::FocusLost, V::MenuOpened] {
                assert_eq!(st.next(ev, true), (CaptureState::Released, E::Ungrab));
            }
            assert_eq!(st.next(V::ClickInGame, true), (st, E::None));
        }
        assert_eq!(s.next(V::FocusLost, true), (s, E::None));
        // A stray grab result while captured changes nothing.
        assert_eq!(c.next(V::GrabResult(None), true), (c, E::None));
    }
}
