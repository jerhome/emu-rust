//! Pure shortcut logic (testable without a window).

use emu_runtime::{STATE_SLOTS, Speed};
use winit::keyboard::KeyCode;

use crate::config::Hotkeys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Menu,
    TogglePause,
    Reset,
    SaveState,
    LoadState,
    PrevSlot,
    NextSlot,
    FastForward,
    ToggleFullscreen,
    ToggleStats,
    /// Computer systems (ADR 0015 §4), host key + K.
    ToggleVirtualKeyboard,
    /// Host key + M.
    ToggleMouseCapture,
    /// Host key + J: joystick / keyboard.
    ToggleKeyboardMode,
    /// Host key + D: next floppy of the set.
    NextDisk,
}

impl HotkeyAction {
    /// Fixed Ctrl+key alternatives for laptops where F-keys need Fn (physical keys, any layout).
    pub fn from_ctrl_key(key: KeyCode) -> Option<Self> {
        match key {
            KeyCode::KeyS => Some(Self::SaveState),
            KeyCode::KeyL => Some(Self::LoadState),
            KeyCode::Comma => Some(Self::PrevSlot),
            KeyCode::Period => Some(Self::NextSlot),
            KeyCode::KeyF => Some(Self::ToggleFullscreen),
            KeyCode::KeyR => Some(Self::Reset),
            KeyCode::KeyI => Some(Self::ToggleStats),
            _ => None,
        }
    }

    /// `host key + key` on a computer system (physical keys, any layout): the Ctrl alternatives plus pause,
    /// held fast-forward and the computer actions.
    pub fn from_host_key(key: KeyCode) -> Option<Self> {
        match key {
            KeyCode::KeyK => Some(Self::ToggleVirtualKeyboard),
            KeyCode::KeyM => Some(Self::ToggleMouseCapture),
            KeyCode::KeyJ => Some(Self::ToggleKeyboardMode),
            KeyCode::KeyD => Some(Self::NextDisk),
            KeyCode::KeyP => Some(Self::TogglePause),
            KeyCode::Space => Some(Self::FastForward),
            _ => Self::from_ctrl_key(key),
        }
    }

    /// Held actions get a press and a release outcome (fast-forward).
    pub fn is_held(self) -> bool {
        self == Self::FastForward
    }
}

impl Hotkeys {
    pub fn action_for(&self, key: KeyCode) -> Option<HotkeyAction> {
        use HotkeyAction as A;
        [
            (self.menu, A::Menu),
            (self.pause, A::TogglePause),
            (self.reset, A::Reset),
            (self.save_state, A::SaveState),
            (self.load_state, A::LoadState),
            (self.prev_slot, A::PrevSlot),
            (self.next_slot, A::NextSlot),
            (self.fast_forward, A::FastForward),
            (self.fullscreen, A::ToggleFullscreen),
            (self.stats, A::ToggleStats),
        ]
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, a)| a)
    }
}

/// Slot after moving by `delta` (wraps around 0..STATE_SLOTS).
pub fn step_slot(current: u8, delta: i8) -> u8 {
    let n = i16::from(STATE_SLOTS);
    let next = (i16::from(current) + i16::from(delta)).rem_euclid(n);
    u8::try_from(next).unwrap_or(0)
}

/// Pause and fast-forward flags → pacer speed and audio state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpeedState {
    pub paused: bool,
    /// Held by the user (fast-forward key, PS + R2).
    pub fast_forward: bool,
    /// Requested by the core while it loads (env 64, design v5.0 §2.4): the ratio to run at.
    pub core_fast_forward_ratio: Option<f64>,
}

impl SpeedState {
    /// Pause wins over fast-forward; the user's fast-forward (at `ratio`) wins over the core's request.
    pub fn speed(self, ratio: f64) -> Speed {
        if self.paused {
            Speed::Paused
        } else if self.fast_forward {
            Speed::FastForward { ratio }
        } else if let Some(ratio) = self.core_fast_forward_ratio {
            Speed::FastForward { ratio }
        } else {
            Speed::Normal
        }
    }

    fn fast(self) -> bool {
        self.fast_forward || self.core_fast_forward_ratio.is_some()
    }

    /// Audio is silent while paused or fast-forwarding (ADR 0010).
    pub fn audio_paused(self) -> bool {
        self.paused || self.fast()
    }

    /// What `Core::set_fast_forwarding` should be told (`GET_FASTFORWARDING`: the effective state).
    pub fn core_fast_forward(self) -> bool {
        self.fast() && !self.paused
    }
}

/// Ratio of a fast-forward requested by the core: its own when it asks for more than real time, else the
/// frontend's (`[speed] fast_forward_ratio`); a ratio of 0 means « the frontend's pace », and the pacer would
/// read 0 or 1 as unlimited.
pub fn core_fast_forward_ratio(requested: f32, frontend: f64) -> f64 {
    let requested = f64::from(requested);
    if requested.is_finite() && requested > 1.0 {
        requested
    } else {
        frontend
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_resolve() {
        let h = Hotkeys::default();
        assert_eq!(h.action_for(KeyCode::Escape), Some(HotkeyAction::Menu));
        assert_eq!(
            h.action_for(KeyCode::Space),
            Some(HotkeyAction::FastForward)
        );
        assert_eq!(h.action_for(KeyCode::F2), Some(HotkeyAction::SaveState));
        assert_eq!(h.action_for(KeyCode::KeyZ), None);
        assert_eq!(h.action_for(KeyCode::KeyH), None);
        assert_eq!(h.action_for(KeyCode::F5), Some(HotkeyAction::Reset));
    }

    #[test]
    fn ctrl_alternatives() {
        assert_eq!(
            HotkeyAction::from_ctrl_key(KeyCode::KeyS),
            Some(HotkeyAction::SaveState)
        );
        assert_eq!(
            HotkeyAction::from_ctrl_key(KeyCode::KeyI),
            Some(HotkeyAction::ToggleStats)
        );
        assert_eq!(HotkeyAction::from_ctrl_key(KeyCode::KeyW), None);
    }

    #[test]
    fn host_chords() {
        use HotkeyAction as H;
        for (key, action) in [
            (KeyCode::KeyK, H::ToggleVirtualKeyboard),
            (KeyCode::KeyM, H::ToggleMouseCapture),
            (KeyCode::KeyJ, H::ToggleKeyboardMode),
            (KeyCode::KeyD, H::NextDisk),
            (KeyCode::KeyS, H::SaveState),
            (KeyCode::KeyL, H::LoadState),
            (KeyCode::KeyR, H::Reset),
            (KeyCode::KeyP, H::TogglePause),
            (KeyCode::KeyF, H::ToggleFullscreen),
            (KeyCode::Space, H::FastForward),
        ] {
            assert_eq!(H::from_host_key(key), Some(action), "{key:?}");
        }
        assert_eq!(H::from_host_key(KeyCode::KeyW), None);
        assert!(H::from_host_key(KeyCode::Space).unwrap().is_held());
        assert_eq!(HotkeyAction::from_ctrl_key(KeyCode::KeyK), None);
    }

    #[test]
    fn slots_wrap() {
        assert_eq!(step_slot(0, -1), 9);
        assert_eq!(step_slot(9, 1), 0);
        assert_eq!(step_slot(3, 1), 4);
    }

    #[test]
    fn speed_state() {
        let mut s = SpeedState::default();
        assert_eq!(s.speed(4.0), Speed::Normal);
        assert!(!s.audio_paused());
        s.fast_forward = true;
        assert_eq!(s.speed(4.0), Speed::FastForward { ratio: 4.0 });
        assert!(s.audio_paused() && s.core_fast_forward());
        s.paused = true;
        assert_eq!(s.speed(4.0), Speed::Paused);
        assert!(!s.core_fast_forward());
    }

    /// Design v5.0 §2.4: the core's request speeds up and mutes; the user's held key wins; pause wins over both.
    #[test]
    fn core_requested_fast_forward() {
        let mut s = SpeedState {
            core_fast_forward_ratio: Some(core_fast_forward_ratio(0.0, 4.0)),
            ..SpeedState::default()
        };
        assert_eq!(s.speed(3.0), Speed::FastForward { ratio: 4.0 });
        assert!(s.audio_paused() && s.core_fast_forward());
        s.fast_forward = true;
        assert_eq!(s.speed(3.0), Speed::FastForward { ratio: 3.0 });
        s.fast_forward = false;
        s.paused = true;
        assert_eq!(s.speed(3.0), Speed::Paused);
        assert!(!s.core_fast_forward());
        assert_eq!(core_fast_forward_ratio(8.0, 4.0), 8.0);
        assert_eq!(core_fast_forward_ratio(1.0, 4.0), 4.0);
        assert_eq!(core_fast_forward_ratio(f32::NAN, 4.0), 4.0);
    }
}
