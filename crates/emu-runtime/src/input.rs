use std::ops::Range;

use emu_libretro::JoypadButton;

/// Number of virtual controller ports exposed to cores in phase 1.
pub const MAX_PORTS: usize = 2;

/// Digital state of one libretro joypad (bit n = `JoypadButton` with id n).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadState {
    buttons: u16,
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
}

/// Snapshot of every virtual device for one emulated frame. Will grow (analog sticks, keyboard, mouse) without
/// breaking backends, hence `non_exhaustive`: build it with `Default`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct InputFrame {
    pub pads: [PadState; MAX_PORTS],
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
