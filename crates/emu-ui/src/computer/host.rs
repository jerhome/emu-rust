//! Host key state machine (ADR 0015 §4, design §2.3): pure, no window. The host key (Right Ctrl on every OS, user
//! decision 2026-11-20; Right Command is the configurable fallback for Mac keyboards without it) is never forwarded to the core. Alone it opens the menu; held, the next keys are chords.

use winit::keyboard::KeyCode;

use crate::hotkeys::HotkeyAction;

/// Default host key, the same on every OS (macOS included).
pub(crate) const DEFAULT_HOST_KEY: KeyCode = KeyCode::ControlRight;

/// What to do with one key transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostOutcome {
    /// Forward to the keyboard backend.
    PassThrough,
    /// Consumed (host key itself, unmapped chord, repeat): nothing reaches the core.
    Swallowed,
    /// Run once.
    Action(HotkeyAction),
    /// Held action (fast-forward): `true` on press, `false` on release.
    ActionHeld(HotkeyAction, bool),
}

#[derive(Debug)]
pub(crate) struct HostKey {
    key: KeyCode,
    held: bool,
    used: bool,
    /// Keys pressed while the host key was held: their release is swallowed too, with the held action to end.
    swallowed: Vec<(KeyCode, Option<HotkeyAction>)>,
}

impl HostKey {
    pub fn new(key: KeyCode) -> Self {
        Self {
            key,
            held: false,
            used: false,
            swallowed: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// Repeats are always swallowed (the emulated machine has its own key repeat).
    pub fn on_key(&mut self, code: KeyCode, pressed: bool, repeat: bool) -> HostOutcome {
        if repeat {
            return HostOutcome::Swallowed;
        }
        if code == self.key {
            return self.on_host(pressed);
        }
        if pressed {
            if !self.held {
                return HostOutcome::PassThrough;
            }
            self.used = true;
            let action = HotkeyAction::from_host_key(code);
            self.swallowed.push((code, action.filter(|a| a.is_held())));
            return match action {
                Some(a) if a.is_held() => HostOutcome::ActionHeld(a, true),
                Some(a) => HostOutcome::Action(a),
                None => HostOutcome::Swallowed,
            };
        }
        match self.swallowed.iter().position(|(k, _)| *k == code) {
            Some(i) => match self.swallowed.remove(i).1 {
                Some(a) => HostOutcome::ActionHeld(a, false),
                None => HostOutcome::Swallowed,
            },
            None => HostOutcome::PassThrough,
        }
    }

    fn on_host(&mut self, pressed: bool) -> HostOutcome {
        if pressed {
            if !self.held {
                self.held = true;
                self.used = false;
            }
            return HostOutcome::Swallowed;
        }
        let was_tap = self.held && !self.used;
        self.held = false;
        self.used = false;
        if was_tap {
            HostOutcome::Action(HotkeyAction::Menu)
        } else {
            HostOutcome::Swallowed
        }
    }

    /// Focus lost: back to idle. Returns the held actions to end (fast-forward); the caller also releases keys.
    pub fn reset(&mut self) -> Vec<HotkeyAction> {
        self.held = false;
        self.used = false;
        self.swallowed.drain(..).filter_map(|(_, a)| a).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: KeyCode = KeyCode::ControlRight;

    fn host() -> HostKey {
        HostKey::new(HOST)
    }

    #[test]
    fn tap_opens_menu() {
        let mut h = host();
        assert_eq!(h.on_key(HOST, true, false), HostOutcome::Swallowed);
        assert!(h.is_held());
        assert_eq!(
            h.on_key(HOST, false, false),
            HostOutcome::Action(HotkeyAction::Menu)
        );
        assert!(!h.is_held());
    }

    #[test]
    fn chord_runs_action_and_hides_key_and_menu() {
        let mut h = host();
        h.on_key(HOST, true, false);
        assert_eq!(
            h.on_key(KeyCode::KeyK, true, false),
            HostOutcome::Action(HotkeyAction::ToggleVirtualKeyboard)
        );
        assert_eq!(
            h.on_key(KeyCode::KeyK, false, false),
            HostOutcome::Swallowed
        );
        assert_eq!(h.on_key(HOST, false, false), HostOutcome::Swallowed);
    }

    #[test]
    fn unmapped_chord_is_swallowed_and_cancels_menu() {
        let mut h = host();
        h.on_key(HOST, true, false);
        assert_eq!(h.on_key(KeyCode::KeyW, true, false), HostOutcome::Swallowed);
        assert_eq!(
            h.on_key(KeyCode::KeyW, false, false),
            HostOutcome::Swallowed
        );
        assert_eq!(h.on_key(HOST, false, false), HostOutcome::Swallowed);
    }

    #[test]
    fn plain_keys_pass_through() {
        let mut h = host();
        assert_eq!(
            h.on_key(KeyCode::KeyK, true, false),
            HostOutcome::PassThrough
        );
        assert_eq!(
            h.on_key(KeyCode::KeyK, false, false),
            HostOutcome::PassThrough
        );
    }

    #[test]
    fn repeats_are_swallowed() {
        let mut h = host();
        assert_eq!(h.on_key(KeyCode::KeyA, true, true), HostOutcome::Swallowed);
        h.on_key(HOST, true, false);
        assert_eq!(h.on_key(HOST, true, true), HostOutcome::Swallowed);
        h.on_key(KeyCode::KeyS, true, false);
        assert_eq!(h.on_key(KeyCode::KeyS, true, true), HostOutcome::Swallowed);
        // A repeated host press does not turn a chord into a tap.
        assert_eq!(h.on_key(HOST, false, false), HostOutcome::Swallowed);
    }

    #[test]
    fn key_held_before_host_releases_normally() {
        let mut h = host();
        h.on_key(KeyCode::KeyA, true, false);
        h.on_key(HOST, true, false);
        assert_eq!(
            h.on_key(KeyCode::KeyA, false, false),
            HostOutcome::PassThrough
        );
    }

    #[test]
    fn fast_forward_held_survives_host_release() {
        let ff = HotkeyAction::FastForward;
        let mut h = host();
        h.on_key(HOST, true, false);
        assert_eq!(
            h.on_key(KeyCode::Space, true, false),
            HostOutcome::ActionHeld(ff, true)
        );
        assert_eq!(h.on_key(HOST, false, false), HostOutcome::Swallowed);
        assert_eq!(
            h.on_key(KeyCode::Space, false, false),
            HostOutcome::ActionHeld(ff, false)
        );
        // Plain Space afterwards goes to the core.
        assert_eq!(
            h.on_key(KeyCode::Space, true, false),
            HostOutcome::PassThrough
        );
    }

    #[test]
    fn focus_loss_resets() {
        let ff = HotkeyAction::FastForward;
        let mut h = host();
        h.on_key(HOST, true, false);
        h.on_key(KeyCode::Space, true, false);
        assert_eq!(h.reset(), vec![ff]);
        assert!(!h.is_held());
        // Release of the host after focus loss: no menu.
        assert_eq!(h.on_key(HOST, false, false), HostOutcome::Swallowed);
        assert_eq!(
            h.on_key(KeyCode::KeyK, true, false),
            HostOutcome::PassThrough
        );
    }

    #[test]
    fn host_key_is_never_passed_through() {
        let mut h = host();
        for pressed in [true, false, true, false] {
            assert_ne!(h.on_key(HOST, pressed, false), HostOutcome::PassThrough);
        }
        assert_eq!(DEFAULT_HOST_KEY, KeyCode::ControlRight);
    }

    #[test]
    fn custom_host_key() {
        let mut h = HostKey::new(KeyCode::ControlLeft);
        h.on_key(KeyCode::ControlLeft, true, false);
        assert_eq!(
            h.on_key(KeyCode::KeyM, true, false),
            HostOutcome::Action(HotkeyAction::ToggleMouseCapture)
        );
    }
}
