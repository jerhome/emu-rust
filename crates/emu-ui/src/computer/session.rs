//! Input state of one computer game (ADR 0015 §3-7): routes winit key and mouse events between the host key,
//! the joystick keymap, the machine's keyboard, its mouse and the virtual keyboard. No window here: grabbing the
//! pointer is ordered through [`CaptureEffect`] and done by `app.rs`.

use emu_runtime::{InputBackend, InputFrame, MouseButton};
use winit::keyboard::{KeyCode, ModifiersState};

use super::host::{HostKey, HostOutcome};
use super::input::{
    CaptureEffect, CaptureEvent, CaptureState, ComputerKeyboard, ComputerMouse, KeyTarget,
    KeyboardMode,
};
use super::keymap::{ALTGR_QUIRK, AltGrFilter, LockState, modifiers};
use super::vkbd::VirtualKeyboard;
use crate::hotkeys::HotkeyAction;
use crate::keyboard::KeyboardInput;

/// A key transition the session did not keep for the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyOutcome {
    /// Host key action; `bool` = pressed (only held actions get a release).
    Action(HotkeyAction, bool),
    /// The virtual keyboard has the focus: egui gets the key.
    ToUi,
}

pub(crate) struct ComputerSession {
    pub host: HostKey,
    pub keyboard: ComputerKeyboard,
    pub mouse: ComputerMouse,
    pub capture: CaptureState,
    pub mode: KeyboardMode,
    pub vkbd: VirtualKeyboard,
    pub auto_capture: bool,
    /// `false`: never grab, always the fallback (`[computer] grab_pointer = false`, noVNC).
    pub grab_allowed: bool,
    locks: LockState,
    altgr: AltGrFilter,
    modifiers: ModifiersState,
    /// Raw device motion arrived since the capture started (otherwise `CursorMoved` deltas are used).
    device_motion_seen: bool,
}

impl ComputerSession {
    pub fn new(host_key: KeyCode, mode: KeyboardMode, vkbd: VirtualKeyboard) -> Self {
        Self {
            host: HostKey::new(host_key),
            keyboard: ComputerKeyboard::default(),
            mouse: ComputerMouse::default(),
            capture: CaptureState::Released,
            mode,
            vkbd,
            auto_capture: true,
            grab_allowed: true,
            locks: LockState::default(),
            altgr: AltGrFilter::default(),
            modifiers: ModifiersState::empty(),
            device_motion_seen: false,
        }
    }

    pub fn set_modifiers(&mut self, state: ModifiersState) {
        self.modifiers = state;
    }

    /// One physical key transition. Keys for the machine go to `pad` (joystick mode, bound keys) or to the
    /// machine's keyboard; the rest is returned.
    pub fn on_key(
        &mut self,
        code: KeyCode,
        pressed: bool,
        repeat: bool,
        text: Option<&str>,
        pad: &mut KeyboardInput,
    ) -> Vec<KeyOutcome> {
        let transitions = if ALTGR_QUIRK && !repeat {
            self.altgr.on_key(code, pressed)
        } else {
            vec![(code, pressed)]
        };
        let mut out = Vec::new();
        for (code, pressed) in transitions {
            self.locks.on_key(code, pressed, repeat);
            match self.host.on_key(code, pressed, repeat) {
                HostOutcome::PassThrough => {}
                HostOutcome::Swallowed => continue,
                HostOutcome::Action(a) => {
                    out.push(KeyOutcome::Action(a, true));
                    continue;
                }
                HostOutcome::ActionHeld(a, down) => {
                    out.push(KeyOutcome::Action(a, down));
                    continue;
                }
            }
            if self.vkbd.is_open() {
                out.push(KeyOutcome::ToUi);
                continue;
            }
            match self.mode.route(pad.binds(code)) {
                KeyTarget::Pad => {
                    pad.handle_key(code, pressed);
                }
                KeyTarget::Keyboard => {
                    self.keyboard
                        .set_modifiers(modifiers(self.modifiers, self.locks));
                    self.keyboard.handle_key(code, pressed, text);
                }
            }
        }
        out
    }

    /// End of an event batch: a real Left Ctrl held back by the AltGr filter goes out now.
    pub fn flush_keys(&mut self) {
        if let Some((code, pressed)) = self.altgr.flush() {
            self.keyboard
                .set_modifiers(modifiers(self.modifiers, self.locks));
            self.keyboard.handle_key(code, pressed, None);
        }
    }

    /// Everything up (focus lost, menu opened, mode switch). Returns held host actions to end.
    pub fn release_all(&mut self, pad: &mut KeyboardInput) -> Vec<HotkeyAction> {
        self.keyboard.release_all();
        self.mouse.release_all();
        self.altgr.reset();
        pad.release_all();
        self.host.reset()
    }

    pub fn toggle_mode(&mut self, pad: &mut KeyboardInput) -> KeyboardMode {
        self.keyboard.release_all();
        pad.release_all();
        self.mode = self.mode.toggled();
        self.mode
    }

    /// Capture transition; the caller performs the effect and reports a grab with [`Self::grab_result`].
    pub fn capture_event(&mut self, event: CaptureEvent) -> CaptureEffect {
        let (next, effect) = self.capture.next(event, self.auto_capture);
        if next != self.capture {
            self.mouse.release_all();
        }
        self.capture = next;
        self.device_motion_seen = false;
        effect
    }

    pub fn grab_result(&mut self, locked: Option<bool>) {
        let result = if self.grab_allowed { locked } else { None };
        self.capture_event(CaptureEvent::GrabResult(result));
    }

    /// A button over the game area. `core_uses_mouse`: the core has read the mouse (else no capture).
    pub fn on_button(
        &mut self,
        button: MouseButton,
        pressed: bool,
        core_uses_mouse: bool,
    ) -> CaptureEffect {
        if self.capture.is_captured() {
            self.mouse.button(button, pressed);
            return CaptureEffect::None;
        }
        if pressed && core_uses_mouse {
            // The capturing click is not given to the machine (it would click wherever the pointer was).
            return self.capture_event(CaptureEvent::ClickInGame);
        }
        CaptureEffect::None
    }

    pub fn on_device_motion(&mut self, dx: f64, dy: f64) {
        if self.capture.uses_device_motion() {
            self.device_motion_seen = true;
            self.mouse.motion(dx, dy);
        }
    }

    /// `CursorMoved` in physical pixels: used in the fallback, and while confined until raw motion shows up.
    pub fn on_cursor_moved(&mut self, x: f64, y: f64) {
        let confined_without_raw = matches!(self.capture, CaptureState::Captured { locked: false })
            && !self.device_motion_seen;
        if self.capture.uses_cursor_motion() || confined_without_raw {
            self.mouse.cursor_moved(x, y);
        } else {
            self.mouse.cursor_left();
        }
    }

    pub fn on_wheel(&mut self, vertical: f64, horizontal: f64) {
        if self.capture.is_captured() {
            self.mouse.wheel(vertical, horizontal);
        }
    }
}

/// Every backend of a computer frame: joystick keys, machine keyboard, mouse, virtual keyboard.
pub(crate) struct ComputerInputs<'a> {
    pub pad: &'a mut KeyboardInput,
    pub session: &'a mut ComputerSession,
}

impl InputBackend for ComputerInputs<'_> {
    fn poll(&mut self, frame_index: u64, state: &mut InputFrame) {
        self.pad.poll(frame_index, state);
        self.session.keyboard.poll(frame_index, state);
        self.session.mouse.poll(frame_index, state);
        self.session.vkbd.poll(frame_index, state);
    }
}

#[cfg(test)]
mod tests {
    use emu_runtime::RetroKey;

    use super::*;
    use crate::keyboard::KeyMap;

    fn session(mode: KeyboardMode) -> (ComputerSession, KeyboardInput) {
        (
            ComputerSession::new(
                KeyCode::ControlRight,
                mode,
                VirtualKeyboard::new("atari_st", "us"),
            ),
            KeyboardInput::new(KeyMap::computer_default()),
        )
    }

    fn frame(s: &mut ComputerSession, pad: &mut KeyboardInput) -> InputFrame {
        let mut f = InputFrame::default();
        ComputerInputs { pad, session: s }.poll(0, &mut f);
        f
    }

    fn key(name: &str) -> RetroKey {
        RetroKey::from_name(name).unwrap()
    }

    #[test]
    fn keyboard_mode_sends_everything_to_the_machine() {
        let (mut s, mut pad) = session(KeyboardMode::Keyboard);
        assert!(
            s.on_key(KeyCode::KeyW, true, false, Some("z"), &mut pad)
                .is_empty()
        );
        assert!(
            s.on_key(KeyCode::Escape, true, false, None, &mut pad)
                .is_empty()
        );
        let f = frame(&mut s, &mut pad);
        assert!(f.keyboard.is_pressed(key("w")) && f.keyboard.is_pressed(key("escape")));
        assert_eq!(f.key_events[0].character, u32::from('z'));
        assert_eq!(f.pads[0].bits(), 0);
    }

    #[test]
    fn joystick_mode_routes_keymap_keys_to_the_pad() {
        let (mut s, mut pad) = session(KeyboardMode::Joystick);
        s.on_key(KeyCode::KeyW, true, false, None, &mut pad);
        s.on_key(KeyCode::F1, true, false, None, &mut pad);
        let f = frame(&mut s, &mut pad);
        assert!(f.pads[0].is_pressed(emu_runtime::JoypadButton::Up));
        assert!(!f.keyboard.is_pressed(key("w")));
        assert!(f.keyboard.is_pressed(key("f1")));
        // Switching releases what was held on both sides.
        assert_eq!(s.toggle_mode(&mut pad), KeyboardMode::Keyboard);
        let f = frame(&mut s, &mut pad);
        assert_eq!(f.pads[0].bits(), 0);
        assert!(!f.keyboard.is_pressed(key("f1")));
    }

    #[test]
    fn host_key_never_reaches_the_machine() {
        let (mut s, mut pad) = session(KeyboardMode::Keyboard);
        assert!(
            s.on_key(KeyCode::ControlRight, true, false, None, &mut pad)
                .is_empty()
        );
        assert_eq!(
            s.on_key(KeyCode::KeyJ, true, false, None, &mut pad),
            vec![KeyOutcome::Action(HotkeyAction::ToggleKeyboardMode, true)]
        );
        s.on_key(KeyCode::KeyJ, false, false, None, &mut pad);
        s.on_key(KeyCode::ControlRight, false, false, None, &mut pad);
        assert_eq!(
            {
                s.on_key(KeyCode::ControlRight, true, false, None, &mut pad);
                s.on_key(KeyCode::ControlRight, false, false, None, &mut pad)
            },
            vec![KeyOutcome::Action(HotkeyAction::Menu, true)]
        );
        let f = frame(&mut s, &mut pad);
        assert!(f.key_events.is_empty() && f.keyboard.is_empty());
    }

    #[test]
    fn virtual_keyboard_takes_the_keys_while_open() {
        let (mut s, mut pad) = session(KeyboardMode::Keyboard);
        s.vkbd.set_open(true);
        assert_eq!(
            s.on_key(KeyCode::ArrowLeft, true, false, None, &mut pad),
            vec![KeyOutcome::ToUi]
        );
        // The host key still works.
        s.on_key(KeyCode::ControlRight, true, false, None, &mut pad);
        assert_eq!(
            s.on_key(KeyCode::KeyK, true, false, None, &mut pad),
            vec![KeyOutcome::Action(
                HotkeyAction::ToggleVirtualKeyboard,
                true
            )]
        );
    }

    #[test]
    fn click_captures_then_buttons_and_motion_reach_the_machine() {
        let (mut s, mut pad) = session(KeyboardMode::Keyboard);
        // The core never read the mouse: no capture.
        assert_eq!(
            s.on_button(MouseButton::Left, true, false),
            CaptureEffect::None
        );
        assert_eq!(
            s.on_button(MouseButton::Left, true, true),
            CaptureEffect::Grab
        );
        s.grab_result(Some(true));
        assert_eq!(s.capture, CaptureState::Captured { locked: true });
        s.on_device_motion(5.0, -3.0);
        s.on_cursor_moved(100.0, 100.0);
        s.on_button(MouseButton::Left, true, true);
        let f = frame(&mut s, &mut pad);
        assert_eq!((f.mouse.dx, f.mouse.dy), (5, -3));
        assert!(f.mouse.is_pressed(MouseButton::Left));
        assert_eq!(
            s.capture_event(CaptureEvent::MenuOpened),
            CaptureEffect::Ungrab
        );
        assert_eq!(frame(&mut s, &mut pad).mouse.buttons, 0);
    }

    #[test]
    fn fallback_uses_cursor_positions() {
        let (mut s, mut pad) = session(KeyboardMode::Keyboard);
        s.grab_allowed = false;
        s.capture_event(CaptureEvent::Toggle);
        s.grab_result(Some(true));
        assert_eq!(s.capture, CaptureState::Fallback);
        s.on_cursor_moved(10.0, 10.0);
        s.on_cursor_moved(14.0, 8.0);
        s.on_device_motion(50.0, 50.0);
        let f = frame(&mut s, &mut pad);
        assert_eq!((f.mouse.dx, f.mouse.dy), (4, -2));
    }

    #[test]
    fn confined_grab_falls_back_to_cursor_until_raw_motion() {
        let (mut s, mut pad) = session(KeyboardMode::Keyboard);
        s.capture_event(CaptureEvent::Toggle);
        s.grab_result(Some(false));
        s.on_cursor_moved(0.0, 0.0);
        s.on_cursor_moved(3.0, 0.0);
        assert_eq!(frame(&mut s, &mut pad).mouse.dx, 3);
        s.on_device_motion(2.0, 0.0);
        s.on_cursor_moved(9.0, 0.0);
        assert_eq!(frame(&mut s, &mut pad).mouse.dx, 2);
    }
}
