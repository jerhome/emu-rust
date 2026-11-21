use std::ops::Range;

use emu_libretro::{JoypadButton, KeyEvent, RetroKey};

/// Number of virtual controller ports exposed to cores in phase 1.
pub const MAX_PORTS: usize = 2;

/// Frames a simulated key stays pressed for `Emulator::pulse_key` (cassette actions, design v5.0 §2.3).
pub const PULSE_FRAMES: u32 = 4;

/// Digital state of one libretro joypad (bit n = `JoypadButton` with id n).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadState {
    buttons: u16,
    /// Left X, left Y, right X, right Y (`RETRO_DEVICE_ANALOG`, -0x8000..=0x7FFF, 0 = centred).
    analog: [i16; 4],
}

impl PadState {
    pub fn set(&mut self, button: JoypadButton, pressed: bool) {
        let bit = 1u16 << button.id();
        if pressed {
            self.buttons |= bit;
        } else {
            self.buttons &= !bit;
        }
    }

    pub fn is_pressed(&self, button: JoypadButton) -> bool {
        self.buttons & (1u16 << button.id()) != 0
    }

    /// Bitmask answered to `RETRO_DEVICE_ID_JOYPAD_MASK`.
    pub fn bits(&self) -> u16 {
        self.buttons
    }

    /// Stick position answered to `input_state(ANALOG, index, X / Y)` (cores with `analog = true` only; others
    /// keep 0 and the stick → D-pad conversion, ADR 0016 amended). Design v4.0 §3.2.
    pub fn set_analog(&mut self, stick: AnalogStick, x: i16, y: i16) {
        let i = stick.offset();
        self.analog[i] = x;
        self.analog[i + 1] = y;
    }

    /// `(x, y)`, `(0, 0)` unless set.
    pub fn analog(&self, stick: AnalogStick) -> (i16, i16) {
        let i = stick.offset();
        (self.analog[i], self.analog[i + 1])
    }
}

/// `RETRO_DEVICE_INDEX_ANALOG_LEFT` / `_RIGHT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalogStick {
    Left,
    Right,
}

impl AnalogStick {
    /// `RETRO_DEVICE_INDEX_ANALOG_LEFT` (0) / `_RIGHT` (1); `None` for any other index (2 = analog buttons).
    pub(crate) fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            _ => None,
        }
    }

    /// Index of the X axis in `PadState::analog`.
    fn offset(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 2,
        }
    }
}

/// Snapshot of every virtual device for one emulated frame. Will grow (analog sticks, keyboard, mouse) without
/// breaking backends, hence `non_exhaustive`: build it with `Default`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct InputFrame {
    pub pads: [PadState; MAX_PORTS],
    /// Keys held during this frame (`RETRO_DEVICE_KEYBOARD` polling). ADR 0015.
    pub keyboard: KeyboardState,
    /// Transitions since the previous frame, in order, for cores with a keyboard callback. Backends append.
    pub key_events: Vec<KeyEvent>,
    /// Mouse of port 0 (`RETRO_DEVICE_MOUSE`); computers have a single mouse.
    pub mouse: MouseState,
}

/// Set of held `RetroKey`s (bit n = key n, `RetroKey::LAST` bits).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardState {
    bits: [u64; KEYBOARD_WORDS],
}

const KEYBOARD_WORDS: usize = (RetroKey::LAST as usize).div_ceil(64);

impl KeyboardState {
    pub fn set(&mut self, key: RetroKey, pressed: bool) {
        let raw = usize::from(key.raw());
        // `RetroKey` guarantees `raw < LAST`; the `get_mut` only keeps this panic-free.
        if let Some(word) = self.bits.get_mut(raw / 64) {
            let bit = 1u64 << (raw % 64);
            if pressed {
                *word |= bit;
            } else {
                *word &= !bit;
            }
        }
    }

    pub fn is_pressed(&self, key: RetroKey) -> bool {
        let raw = usize::from(key.raw());
        self.bits
            .get(raw / 64)
            .is_some_and(|word| word & (1u64 << (raw % 64)) != 0)
    }

    /// No key held.
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    /// Held keys in ascending `RetroKey` order.
    pub fn pressed(&self) -> impl Iterator<Item = RetroKey> + '_ {
        (0..RetroKey::LAST)
            .filter_map(RetroKey::new)
            .filter(|&k| self.is_pressed(k))
    }
}

/// Mouse state for one frame. Backends combine by adding deltas and OR-ing buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseState {
    /// Relative motion accumulated since the previous frame, already scaled by the sensitivity setting.
    pub dx: i32,
    pub dy: i32,
    /// Wheel notches since the previous frame (positive = up / right).
    pub wheel: i32,
    pub hwheel: i32,
    /// Bit n = `MouseButton` n.
    pub buttons: u8,
}

impl MouseState {
    pub fn set_button(&mut self, button: MouseButton, pressed: bool) {
        if pressed {
            self.buttons |= button.bit();
        } else {
            self.buttons &= !button.bit();
        }
    }

    pub fn is_pressed(&self, button: MouseButton) -> bool {
        self.buttons & button.bit() != 0
    }
}

/// Buttons of [`MouseState::buttons`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    Button4 = 3,
    Button5 = 4,
}

impl MouseButton {
    /// Mask of this button in [`MouseState::buttons`].
    pub const fn bit(self) -> u8 {
        1 << self as u8
    }
}

impl InputFrame {
    /// Back to the default state, keeping the capacity of `key_events` (no allocation per frame).
    pub(crate) fn clear(&mut self) {
        self.pads = Default::default();
        self.keyboard = KeyboardState::default();
        self.key_events.clear();
        self.mouse = MouseState::default();
    }
}

/// Source of input (keyboard, scripted, gamepad in phase 6).
///
/// Contract: the runtime clears `state` before each frame, then calls every active backend in turn; a backend
/// only *sets* what it holds pressed, so several backends combine by OR.
pub trait InputBackend {
    fn poll(&mut self, frame_index: u64, state: &mut InputFrame);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NullInput;

impl InputBackend for NullInput {
    fn poll(&mut self, _frame_index: u64, _state: &mut InputFrame) {}
}

/// Deterministic input for headless tests: button held during a range of frame indices.
#[derive(Debug, Default, Clone)]
pub struct ScriptedInput {
    presses: Vec<(usize, JoypadButton, Range<u64>)>,
    keys: Vec<(RetroKey, Range<u64>)>,
    mouse: Vec<(i32, i32, u8, Range<u64>)>,
}

impl ScriptedInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds `button` on `port` for every frame index in `frames`.
    pub fn press(mut self, port: usize, button: JoypadButton, frames: Range<u64>) -> Self {
        self.presses.push((port, button, frames));
        self
    }

    /// Holds `key` (keyboard state) for every frame in `frames`, with a key-down event at `frames.start` and a
    /// key-up event at `frames.end` (no character, no modifier). An empty range produces nothing.
    pub fn key(mut self, key: RetroKey, frames: Range<u64>) -> Self {
        self.keys.push((key, frames));
        self
    }

    /// Moves the mouse by (`dx`, `dy`) and holds `buttons` (bit n = [`MouseButton`] n) on every frame in `frames`.
    pub fn mouse(mut self, dx: i32, dy: i32, buttons: u8, frames: Range<u64>) -> Self {
        self.mouse.push((dx, dy, buttons, frames));
        self
    }
}

fn key_event(key: RetroKey, down: bool) -> KeyEvent {
    KeyEvent {
        down,
        key,
        character: 0,
        modifiers: emu_libretro::KeyModifiers(0),
    }
}

impl InputBackend for ScriptedInput {
    /// Out-of-range ports are ignored.
    fn poll(&mut self, frame_index: u64, state: &mut InputFrame) {
        for (port, button, frames) in &self.presses {
            if frames.contains(&frame_index)
                && let Some(pad) = state.pads.get_mut(*port)
            {
                pad.set(*button, true);
            }
        }
        for (key, frames) in &self.keys {
            if frames.is_empty() {
                continue;
            }
            if frames.contains(&frame_index) {
                state.keyboard.set(*key, true);
            }
            if frame_index == frames.start {
                state.key_events.push(key_event(*key, true));
            } else if frame_index == frames.end {
                state.key_events.push(key_event(*key, false));
            }
        }
        for (dx, dy, buttons, frames) in &self.mouse {
            if frames.contains(&frame_index) {
                state.mouse.dx = state.mouse.dx.saturating_add(*dx);
                state.mouse.dy = state.mouse.dy.saturating_add(*dy);
                state.mouse.buttons |= buttons;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_bits_follow_libretro_ids() {
        let mut pad = PadState::default();
        pad.set(JoypadButton::B, true);
        pad.set(JoypadButton::A, true);
        pad.set(JoypadButton::R3, true);
        assert_eq!(pad.bits(), 1 | (1 << 8) | (1 << 15));
        assert!(pad.is_pressed(JoypadButton::A));
        assert!(!pad.is_pressed(JoypadButton::Start));
        pad.set(JoypadButton::A, false);
        assert_eq!(pad.bits(), 1 | (1 << 15));
    }

    #[test]
    fn analog_sticks_are_independent_of_buttons() {
        let mut pad = PadState::default();
        assert_eq!(pad.analog(AnalogStick::Left), (0, 0));
        pad.set_analog(AnalogStick::Left, i16::MIN, 1234);
        pad.set_analog(AnalogStick::Right, -1, i16::MAX);
        assert_eq!(pad.analog(AnalogStick::Left), (i16::MIN, 1234));
        assert_eq!(pad.analog(AnalogStick::Right), (-1, i16::MAX));
        assert_eq!(pad.bits(), 0);
        pad.set(JoypadButton::Start, true);
        assert_eq!(pad.analog(AnalogStick::Left), (i16::MIN, 1234));
        let mut frame = InputFrame::default();
        frame.pads[1] = pad;
        frame.clear();
        assert_eq!(frame.pads[1], PadState::default(), "cleared every frame");
        assert_eq!(AnalogStick::from_index(0), Some(AnalogStick::Left));
        assert_eq!(AnalogStick::from_index(1), Some(AnalogStick::Right));
        assert_eq!(AnalogStick::from_index(2), None);
    }

    #[test]
    fn scripted_input_ranges_and_ports() {
        let mut script = ScriptedInput::new()
            .press(0, JoypadButton::Down, 400..410)
            .press(1, JoypadButton::Start, 405..406)
            .press(7, JoypadButton::A, 0..1000);
        let mut at = |i| {
            let mut frame = InputFrame::default();
            script.poll(i, &mut frame);
            frame
        };
        assert_eq!(at(399), InputFrame::default());
        let f = at(400);
        assert!(f.pads[0].is_pressed(JoypadButton::Down));
        assert_eq!(f.pads[1].bits(), 0);
        let f = at(405);
        assert!(f.pads[0].is_pressed(JoypadButton::Down));
        assert!(f.pads[1].is_pressed(JoypadButton::Start));
        assert_eq!(at(410), InputFrame::default());
    }

    fn key(raw: u16) -> RetroKey {
        RetroKey::new(raw).unwrap()
    }

    #[test]
    fn keyboard_state_bits() {
        let mut kb = KeyboardState::default();
        assert!(kb.is_empty());
        for raw in [0, 63, 64, 97, RetroKey::LAST - 1] {
            kb.set(key(raw), true);
            assert!(kb.is_pressed(key(raw)), "{raw}");
        }
        assert!(!kb.is_pressed(key(98)));
        assert_eq!(
            kb.pressed().map(RetroKey::raw).collect::<Vec<_>>(),
            [0, 63, 64, 97, RetroKey::LAST - 1]
        );
        kb.set(key(64), false);
        assert!(!kb.is_pressed(key(64)));
        assert!(kb.is_pressed(key(63)));
        for raw in [0, 63, 97, RetroKey::LAST - 1] {
            kb.set(key(raw), false);
        }
        assert!(kb.is_empty());
    }

    #[test]
    fn mouse_buttons() {
        let mut m = MouseState::default();
        m.set_button(MouseButton::Left, true);
        m.set_button(MouseButton::Button5, true);
        assert_eq!(m.buttons, 0b1_0001);
        assert!(m.is_pressed(MouseButton::Left));
        assert!(!m.is_pressed(MouseButton::Right));
        m.set_button(MouseButton::Left, false);
        assert_eq!(m.buttons, MouseButton::Button5.bit());
    }

    #[test]
    fn scripted_keys_and_mouse() {
        let a = key(97);
        let mut script = ScriptedInput::new()
            .key(a, 2..4)
            .key(key(13), 5..5)
            .mouse(3, -2, MouseButton::Left.bit(), 3..5)
            .mouse(1, 1, MouseButton::Right.bit(), 4..5);
        let mut at = |i| {
            let mut frame = InputFrame::default();
            script.poll(i, &mut frame);
            frame
        };
        assert_eq!(at(1), InputFrame::default());
        let f = at(2);
        assert!(f.keyboard.is_pressed(a));
        assert_eq!(f.key_events, [key_event(a, true)]);
        let f = at(3);
        assert!(f.keyboard.is_pressed(a) && f.key_events.is_empty());
        assert_eq!((f.mouse.dx, f.mouse.dy, f.mouse.buttons), (3, -2, 1));
        let f = at(4);
        assert!(!f.keyboard.is_pressed(a));
        assert_eq!(f.key_events, [key_event(a, false)]);
        assert_eq!((f.mouse.dx, f.mouse.dy, f.mouse.buttons), (4, -1, 3));
        assert_eq!(at(5), InputFrame::default());
    }

    #[test]
    fn clear_keeps_event_capacity() {
        let mut frame = InputFrame::default();
        ScriptedInput::new()
            .key(key(97), 0..1)
            .mouse(1, 1, 1, 0..1)
            .press(0, JoypadButton::A, 0..1)
            .poll(0, &mut frame);
        let capacity = frame.key_events.capacity();
        frame.clear();
        assert_eq!(frame, InputFrame::default());
        assert_eq!(frame.key_events.capacity(), capacity);
    }

    #[test]
    fn backends_combine_by_or() {
        let mut frame = InputFrame::default();
        ScriptedInput::new()
            .press(0, JoypadButton::A, 0..1)
            .poll(0, &mut frame);
        NullInput.poll(0, &mut frame);
        ScriptedInput::new()
            .press(0, JoypadButton::B, 0..1)
            .poll(0, &mut frame);
        assert_eq!(frame.pads[0].bits(), (1 << 8) | 1);
    }
}
