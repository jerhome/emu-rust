use std::collections::HashSet;

use emu_runtime::{InputBackend, InputFrame, JoypadButton, MAX_PORTS};
use winit::keyboard::KeyCode;

/// Physical key → (port, libretro button). Configurable from the UI in phase 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMap {
    bindings: Vec<(KeyCode, usize, JoypadButton)>,
}

impl KeyMap {
    pub fn new(bindings: Vec<(KeyCode, usize, JoypadButton)>) -> Self {
        Self { bindings }
    }

    /// Port 0, genesis_plus_gx layout: arrows = D-pad, Enter = Start, Right Shift = Select (Mode),
    /// Z/X/C = Y/B/A (MD A/B/C), A/S/D = L/X/R (MD X/Y/Z).
    pub fn genesis_default() -> Self {
        use JoypadButton as B;
        use KeyCode as K;
        Self::new(vec![
            (K::ArrowUp, 0, B::Up),
            (K::ArrowDown, 0, B::Down),
            (K::ArrowLeft, 0, B::Left),
            (K::ArrowRight, 0, B::Right),
            (K::Enter, 0, B::Start),
            (K::ShiftRight, 0, B::Select),
            (K::KeyZ, 0, B::Y),
            (K::KeyX, 0, B::B),
            (K::KeyC, 0, B::A),
            (K::KeyA, 0, B::L),
            (K::KeyS, 0, B::X),
            (K::KeyD, 0, B::R),
        ])
    }

    pub fn bindings(&self) -> &[(KeyCode, usize, JoypadButton)] {
        &self.bindings
    }
}

/// Keyboard `InputBackend` fed with winit key events.
#[derive(Debug, Clone)]
pub struct KeyboardInput {
    map: KeyMap,
    held: HashSet<KeyCode>,
}

impl KeyboardInput {
    pub fn new(map: KeyMap) -> Self {
        Self {
            map,
            held: HashSet::new(),
        }
    }

    /// Records a key transition; returns `true` if the key is bound (event consumed).
    pub fn handle_key(&mut self, key: KeyCode, pressed: bool) -> bool {
        if !self.map.bindings.iter().any(|(k, _, _)| *k == key) {
            return false;
        }
        if pressed {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
        true
    }

    /// Releases every key (window focus lost).
    pub fn release_all(&mut self) {
        self.held.clear();
    }
}

impl InputBackend for KeyboardInput {
    fn poll(&mut self, _frame_index: u64, state: &mut InputFrame) {
        for (key, port, button) in &self.map.bindings {
            if *port < MAX_PORTS && self.held.contains(key) {
                state.pads[*port].set(*button, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_map_has_no_duplicate_keys() {
        let map = KeyMap::genesis_default();
        let keys: HashSet<_> = map.bindings().iter().map(|b| b.0).collect();
        assert_eq!(keys.len(), map.bindings().len());
        assert!(map.bindings().iter().all(|b| b.1 < MAX_PORTS));
    }

    #[test]
    fn bound_key_is_consumed_and_polled() {
        let mut kb = KeyboardInput::new(KeyMap::genesis_default());
        assert!(kb.handle_key(KeyCode::ArrowUp, true));
        assert!(!kb.handle_key(KeyCode::KeyQ, true));
        let mut frame = InputFrame::default();
        kb.poll(0, &mut frame);
        assert!(frame.pads[0].is_pressed(JoypadButton::Up));
        assert!(!frame.pads[0].is_pressed(JoypadButton::Down));
        assert!(!frame.pads[1].is_pressed(JoypadButton::Up));
    }

    #[test]
    fn release_and_release_all() {
        let mut kb = KeyboardInput::new(KeyMap::genesis_default());
        kb.handle_key(KeyCode::KeyZ, true);
        kb.handle_key(KeyCode::KeyX, true);
        kb.handle_key(KeyCode::KeyZ, false);
        let mut frame = InputFrame::default();
        kb.poll(0, &mut frame);
        assert!(!frame.pads[0].is_pressed(JoypadButton::Y));
        assert!(frame.pads[0].is_pressed(JoypadButton::B));
        kb.release_all();
        let mut frame = InputFrame::default();
        kb.poll(1, &mut frame);
        assert_eq!(frame.pads[0].bits(), 0);
    }

    #[test]
    fn out_of_range_port_is_ignored() {
        let mut kb = KeyboardInput::new(KeyMap::new(vec![(KeyCode::KeyA, 9, JoypadButton::A)]));
        kb.handle_key(KeyCode::KeyA, true);
        let mut frame = InputFrame::default();
        kb.poll(0, &mut frame);
        assert_eq!(frame.pads[0].bits(), 0);
    }
}
