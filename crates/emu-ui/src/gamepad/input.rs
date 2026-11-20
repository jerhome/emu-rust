//! Pad state -> `InputFrame`: ports, hotplug, deadzone, hotkeys.

use std::collections::{BTreeMap, HashSet};

use emu_runtime::{InputBackend, InputFrame, JoypadButton, MAX_PORTS};

use super::mapping::{PadButton, PadMap};
use super::source::{GamepadSource, PadAxis, PadEvent, PadEventKind, PadId};
use crate::config_gamepad::{GamepadConfig, GamepadDeviceConfig, HotkeyButton};
use crate::hotkeys::HotkeyAction;

const DEADZONE_MIN: f32 = 0.05;
const DEADZONE_MAX: f32 = 0.60;
/// Stick -> D-pad: pressed above `STICK_ON`, released below `STICK_OFF` (hysteresis 0.1).
const STICK_ON: f32 = 0.5;
const STICK_OFF: f32 = 0.4;
const TRIGGER_HYSTERESIS: f32 = 0.1;

/// Runtime settings extracted from `GamepadConfig` for one launch.
#[derive(Debug, Clone, PartialEq)]
pub struct GamepadSettings {
    pub enabled: bool,
    /// Radial stick deadzone, clamped to 0.05..=0.60.
    pub deadzone: f32,
    /// Analog trigger value above which L2/R2 count as pressed.
    pub trigger_threshold: f32,
    pub hotkey_button: HotkeyButton,
    pub pause_on_disconnect: bool,
    /// Per-model overrides (forced port, deadzone), keyed by lowercase hex GUID.
    pub devices: BTreeMap<String, GamepadDeviceConfig>,
}

fn clamp_deadzone(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(DEADZONE_MIN, DEADZONE_MAX)
    } else {
        fallback
    }
}

impl GamepadSettings {
    pub fn from_config(config: &GamepadConfig) -> Self {
        let trigger = if config.trigger_threshold.is_finite() {
            config.trigger_threshold.clamp(0.1, 0.95)
        } else {
            0.5
        };
        Self {
            enabled: config.enabled,
            deadzone: clamp_deadzone(config.deadzone, 0.25),
            trigger_threshold: trigger,
            hotkey_button: config.hotkey_button,
            pause_on_disconnect: config.pause_on_disconnect,
            devices: config.devices.clone(),
        }
    }
}

impl Default for GamepadSettings {
    fn default() -> Self {
        Self::from_config(&GamepadConfig::default())
    }
}

/// One-shot facts for the UI (toasts). `port` is the 0-based RetroPad port.
#[derive(Debug, Clone, PartialEq)]
pub enum GamepadNotice {
    Connected { name: String, port: usize },
    Disconnected { port: usize },
}

/// Menu / virtual keyboard navigation from the pad (`GamepadInput::set_ui_capture`): the app turns these into
/// egui arrow / Enter presses and « back ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    /// Cross (South): activate the focused widget.
    Activate,
    /// Circle (East): back / close.
    Back,
}

impl NavKey {
    fn from_button(button: PadButton) -> Option<Self> {
        match button {
            PadButton::DPadUp => Some(Self::Up),
            PadButton::DPadDown => Some(Self::Down),
            PadButton::DPadLeft => Some(Self::Left),
            PadButton::DPadRight => Some(Self::Right),
            PadButton::South => Some(Self::Activate),
            PadButton::East => Some(Self::Back),
            _ => None,
        }
    }
}

/// Live state of one attached pad.
#[derive(Debug)]
struct Pad {
    id: PadId,
    port: usize,
    deadzone: f32,
    held: HashSet<PadButton>,
    stick: (f32, f32),
    /// Stick directions currently counted as pressed (hysteresis memory): up, down, left, right.
    stick_dirs: [bool; 4],
    trigger_l2: bool,
    trigger_r2: bool,
}

impl Pad {
    fn is_down(&self, button: PadButton) -> bool {
        self.held.contains(&button)
            || match button {
                PadButton::L2 => self.trigger_l2,
                PadButton::R2 => self.trigger_r2,
                _ => false,
            }
    }

    fn update_stick_dirs(&mut self) {
        let (mut x, mut y) = self.stick;
        // Radial deadzone: the whole vector is cut, not each axis.
        if (x * x + y * y).sqrt() < self.deadzone {
            x = 0.0;
            y = 0.0;
        }
        let dirs = [y, -y, -x, x];
        for (state, value) in self.stick_dirs.iter_mut().zip(dirs) {
            *state = if *state {
                value > STICK_OFF
            } else {
                value >= STICK_ON
            };
        }
    }
}

/// Gamepad `InputBackend`. Port assignment: first connected pad = port 0, second = port 1, others ignored; a freed
/// port is reused. While the hotkey button is held, other buttons become hotkeys instead of reaching the core
/// (the hotkey button itself still follows the map when pressed alone, e.g. Share as modifier = Select).
pub struct GamepadInput<S: GamepadSource> {
    source: S,
    map: PadMap,
    settings: GamepadSettings,
    pads: Vec<Pad>,
    hotkeys: Vec<HotkeyAction>,
    notices: Vec<GamepadNotice>,
    nav: Vec<NavKey>,
    /// A menu or the virtual keyboard has the focus: buttons navigate instead of reaching the core.
    ui_capture: bool,
    scratch: Vec<PadEvent>,
}

impl<S: GamepadSource> GamepadInput<S> {
    pub fn new(source: S, map: PadMap, settings: GamepadSettings) -> Self {
        Self {
            source,
            map,
            settings,
            pads: Vec::new(),
            hotkeys: Vec::new(),
            notices: Vec::new(),
            nav: Vec::new(),
            ui_capture: false,
            scratch: Vec::new(),
        }
    }

    /// New settings (configuration edited between two games). Attached pads keep their port and deadzone.
    pub fn set_settings(&mut self, settings: GamepadSettings) {
        self.settings = settings;
    }

    pub fn settings(&self) -> &GamepadSettings {
        &self.settings
    }

    /// Drains the source and updates the pad state, hotkeys, notices and navigation, without touching any
    /// frame. The app calls it on every loop turn, so hotkeys work while the game is paused or a menu is open.
    pub fn pump(&mut self) {
        let mut events = std::mem::take(&mut self.scratch);
        events.clear();
        self.source.poll_events(&mut events);
        if self.settings.enabled {
            for event in events.drain(..) {
                self.handle(event);
            }
        }
        events.clear();
        self.scratch = events;
    }

    /// While `true` (menu or virtual keyboard open), no pad button reaches the core; presses of the D-pad, the
    /// left stick, Cross and Circle become `NavKey`s (`take_nav`). Hotkeys keep working.
    pub fn set_ui_capture(&mut self, capture: bool) {
        if capture != self.ui_capture {
            self.ui_capture = capture;
            self.nav.clear();
        }
    }

    /// Navigation presses since the last call (empty unless `set_ui_capture(true)`).
    pub fn take_nav(&mut self) -> Vec<NavKey> {
        std::mem::take(&mut self.nav)
    }

    /// Replaces the button map (new game launched, or remap screen).
    pub fn set_map(&mut self, map: PadMap) {
        self.map = map;
    }

    /// Hotkey actions triggered since the last call (one entry per press; `FastForward` excluded, see below).
    pub fn take_hotkeys(&mut self) -> Vec<HotkeyAction> {
        std::mem::take(&mut self.hotkeys)
    }

    /// `true` while hotkey button + R2 is held (fast-forward is a held action, like the keyboard Space).
    pub fn fast_forward_held(&self) -> bool {
        self.settings.enabled
            && self
                .pads
                .iter()
                .any(|p| self.modifier_held(p) && p.is_down(PadButton::R2))
    }

    /// Connection / disconnection notices since the last call.
    pub fn take_notices(&mut self) -> Vec<GamepadNotice> {
        std::mem::take(&mut self.notices)
    }

    /// Ports currently attached, `(port, pad id)`, for the settings screen.
    pub fn attached(&self) -> Vec<(usize, PadId)> {
        let mut v: Vec<_> = self.pads.iter().map(|p| (p.port, p.id)).collect();
        v.sort_unstable();
        v
    }

    fn modifier_button(&self) -> PadButton {
        match self.settings.hotkey_button {
            HotkeyButton::Mode => PadButton::Mode,
            HotkeyButton::Select => PadButton::Select,
        }
    }

    fn modifier_held(&self, pad: &Pad) -> bool {
        pad.held.contains(&self.modifier_button())
    }

    fn hotkey_for(&self, button: PadButton) -> Option<HotkeyAction> {
        use HotkeyAction as H;
        use PadButton as P;
        match button {
            P::Start => Some(H::Menu),
            P::Select if self.modifier_button() != P::Select => Some(H::TogglePause),
            // With Share as the modifier, PS takes over the pause action.
            P::Mode if self.modifier_button() == P::Select => Some(H::TogglePause),
            P::R1 => Some(H::SaveState),
            P::L1 => Some(H::LoadState),
            P::DPadRight => Some(H::NextSlot),
            P::DPadLeft => Some(H::PrevSlot),
            P::North => Some(H::ToggleFullscreen),
            P::West => Some(H::Reset),
            P::South => Some(H::ToggleStats),
            // Computer systems (ADR 0016 amendment, point 3); the app ignores them on consoles.
            P::R3 => Some(H::ToggleVirtualKeyboard),
            P::L3 => Some(H::NextDisk),
            _ => None,
        }
    }

    fn handle(&mut self, event: PadEvent) {
        match event.kind {
            PadEventKind::Connected { name, guid } => self.connect(event.id, name, &guid),
            PadEventKind::Disconnected => self.disconnect(event.id),
            PadEventKind::Button { button, pressed } => self.button(event.id, button, pressed),
            PadEventKind::Axis { axis, value } => self.axis(event.id, axis, value),
        }
    }

    fn connect(&mut self, id: PadId, name: String, guid: &str) {
        if self.pads.iter().any(|p| p.id == id) {
            return;
        }
        let device = self.settings.devices.get(&guid.to_ascii_lowercase());
        let forced = device
            .and_then(|d| d.port)
            .filter(|p| *p < MAX_PORTS && self.pads.iter().all(|q| q.port != *p));
        let Some(port) =
            forced.or_else(|| (0..MAX_PORTS).find(|p| self.pads.iter().all(|q| q.port != *p)))
        else {
            return; // More pads than ports: ignored.
        };
        let deadzone = device
            .and_then(|d| d.deadzone)
            .map_or(self.settings.deadzone, |d| {
                clamp_deadzone(d, self.settings.deadzone)
            });
        self.pads.push(Pad {
            id,
            port,
            deadzone,
            held: HashSet::new(),
            stick: (0.0, 0.0),
            stick_dirs: [false; 4],
            trigger_l2: false,
            trigger_r2: false,
        });
        self.notices.push(GamepadNotice::Connected { name, port });
    }

    fn disconnect(&mut self, id: PadId) {
        if let Some(pos) = self.pads.iter().position(|p| p.id == id) {
            let pad = self.pads.remove(pos);
            self.notices
                .push(GamepadNotice::Disconnected { port: pad.port });
        }
    }

    fn button(&mut self, id: PadId, button: PadButton, pressed: bool) {
        let modifier = self.modifier_button();
        let Some(idx) = self.pads.iter().position(|p| p.id == id) else {
            return;
        };
        let was_down = self.pads[idx].held.contains(&button);
        if pressed {
            self.pads[idx].held.insert(button);
        } else {
            self.pads[idx].held.remove(&button);
        }
        let held_modifier = self.pads[idx].held.contains(&modifier);
        if !pressed || was_down || button == modifier {
            return;
        }
        if held_modifier {
            if let Some(action) = self.hotkey_for(button) {
                self.hotkeys.push(action);
            }
        } else if self.ui_capture
            && let Some(nav) = NavKey::from_button(button)
        {
            self.nav.push(nav);
        }
    }

    fn axis(&mut self, id: PadId, axis: PadAxis, value: f32) {
        let threshold = self.settings.trigger_threshold;
        let modifier = self.modifier_button();
        let ui_capture = self.ui_capture;
        let Some(pad) = self.pads.iter_mut().find(|p| p.id == id) else {
            return;
        };
        if !value.is_finite() {
            return;
        }
        let trigger = |on: bool| {
            if on {
                value > threshold - TRIGGER_HYSTERESIS
            } else {
                value >= threshold
            }
        };
        match axis {
            PadAxis::LeftX => pad.stick.0 = value,
            PadAxis::LeftY => pad.stick.1 = value,
            PadAxis::L2 => pad.trigger_l2 = trigger(pad.trigger_l2),
            PadAxis::R2 => pad.trigger_r2 = trigger(pad.trigger_r2),
            PadAxis::RightX | PadAxis::RightY => {}
        }
        let before = pad.stick_dirs;
        pad.update_stick_dirs();
        if ui_capture && !pad.held.contains(&modifier) {
            let dirs = [NavKey::Up, NavKey::Down, NavKey::Left, NavKey::Right];
            for ((was, now), nav) in before.iter().zip(pad.stick_dirs).zip(dirs) {
                if now && !was {
                    self.nav.push(nav);
                }
            }
        }
    }
}

impl<S: GamepadSource> InputBackend for GamepadInput<S> {
    /// Drains the source (`pump`), then ORs the held buttons of every attached pad into `state.pads[port]`.
    fn poll(&mut self, _frame_index: u64, state: &mut InputFrame) {
        self.pump();
        if !self.settings.enabled || self.ui_capture {
            return;
        }
        for pad in &self.pads {
            if self.modifier_held(pad) {
                continue; // Hotkey mode: the core sees nothing from this pad.
            }
            let Some(target) = state.pads.get_mut(pad.port) else {
                continue;
            };
            for (button, retro) in self.map.bindings() {
                if pad.is_down(*button) {
                    target.set(*retro, true);
                }
            }
            for (down, retro) in pad.stick_dirs.iter().zip([
                JoypadButton::Up,
                JoypadButton::Down,
                JoypadButton::Left,
                JoypadButton::Right,
            ]) {
                if *down {
                    target.set(retro, true);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::source::ScriptedPadSource;
    use crate::keyboard::PadStyle;

    const A: PadId = PadId(1);
    const B: PadId = PadId(2);
    const C: PadId = PadId(3);

    fn connected(id: PadId, guid: &str) -> PadEvent {
        PadEvent {
            id,
            kind: PadEventKind::Connected {
                name: "DS4".into(),
                guid: guid.into(),
            },
        }
    }
    fn disc(id: PadId) -> PadEvent {
        PadEvent {
            id,
            kind: PadEventKind::Disconnected,
        }
    }
    fn btn(id: PadId, button: PadButton, pressed: bool) -> PadEvent {
        PadEvent {
            id,
            kind: PadEventKind::Button { button, pressed },
        }
    }
    fn axis(id: PadId, axis: PadAxis, value: f32) -> PadEvent {
        PadEvent {
            id,
            kind: PadEventKind::Axis { axis, value },
        }
    }

    fn input(style: PadStyle, batches: Vec<Vec<PadEvent>>) -> GamepadInput<ScriptedPadSource> {
        input_with(style, GamepadSettings::default(), batches)
    }

    fn input_with(
        style: PadStyle,
        settings: GamepadSettings,
        batches: Vec<Vec<PadEvent>>,
    ) -> GamepadInput<ScriptedPadSource> {
        let mut src = ScriptedPadSource::new();
        for b in batches {
            src.push_batch(b);
        }
        GamepadInput::new(src, PadMap::default_for(style), settings)
    }

    fn poll(i: &mut GamepadInput<ScriptedPadSource>) -> InputFrame {
        let mut f = InputFrame::default();
        i.poll(0, &mut f);
        f
    }

    #[test]
    fn mega_drive_buttons_reach_port_zero() {
        let mut i = input(
            PadStyle::MegaDrive,
            vec![vec![
                connected(A, "g"),
                btn(A, PadButton::South, true),
                btn(A, PadButton::East, true),
                btn(A, PadButton::Start, true),
            ]],
        );
        let f = poll(&mut i);
        assert!(f.pads[0].is_pressed(JoypadButton::B));
        assert!(f.pads[0].is_pressed(JoypadButton::A));
        assert!(f.pads[0].is_pressed(JoypadButton::Start));
        assert!(!f.pads[0].is_pressed(JoypadButton::Y));
        assert_eq!(f.pads[1].bits(), 0);
        assert_eq!(
            i.take_notices(),
            vec![GamepadNotice::Connected {
                name: "DS4".into(),
                port: 0
            }]
        );
        assert!(i.take_notices().is_empty());
    }

    #[test]
    fn master_system_ignores_unmapped_buttons() {
        let mut i = input(
            PadStyle::MasterSystem,
            vec![vec![
                connected(A, "g"),
                btn(A, PadButton::West, true),
                btn(A, PadButton::Select, true),
                btn(A, PadButton::South, true),
            ]],
        );
        let f = poll(&mut i);
        assert_eq!(f.pads[0].bits(), 1 << JoypadButton::B.id());
    }

    #[test]
    fn release_clears_the_button() {
        let mut i = input(
            PadStyle::Snes,
            vec![
                vec![connected(A, "g"), btn(A, PadButton::L1, true)],
                vec![btn(A, PadButton::L1, false)],
            ],
        );
        assert!(poll(&mut i).pads[0].is_pressed(JoypadButton::L));
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
    }

    #[test]
    fn ports_follow_connection_order_and_are_reused() {
        let mut i = input(
            PadStyle::Nes,
            vec![
                vec![connected(A, "g"), connected(B, "g"), connected(C, "g")],
                vec![disc(A)],
                vec![connected(C, "g"), btn(C, PadButton::South, true)],
            ],
        );
        poll(&mut i);
        assert_eq!(i.attached(), vec![(0, A), (1, B)]);
        assert_eq!(i.take_notices().len(), 2); // third pad: no port, no notice
        poll(&mut i);
        assert_eq!(i.attached(), vec![(1, B)]);
        assert_eq!(
            i.take_notices(),
            vec![GamepadNotice::Disconnected { port: 0 }]
        );
        let f = poll(&mut i);
        assert_eq!(i.attached(), vec![(0, C), (1, B)]);
        assert!(f.pads[0].is_pressed(JoypadButton::B));
        assert_eq!(f.pads[1].bits(), 0);
    }

    #[test]
    fn disconnect_releases_buttons_and_survives_unknown_pads() {
        let mut i = input(
            PadStyle::Nes,
            vec![
                vec![connected(A, "g"), btn(A, PadButton::South, true)],
                vec![
                    disc(A),
                    disc(A),
                    btn(A, PadButton::East, true),
                    axis(B, PadAxis::LeftX, 1.0),
                ],
            ],
        );
        assert_ne!(poll(&mut i).pads[0].bits(), 0);
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
        assert!(i.attached().is_empty());
    }

    #[test]
    fn reconnecting_starts_with_clean_state() {
        let mut i = input(
            PadStyle::Nes,
            vec![
                vec![connected(A, "g"), btn(A, PadButton::South, true), disc(A)],
                vec![connected(A, "g")],
            ],
        );
        poll(&mut i);
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
    }

    #[test]
    fn stick_deadzone_and_hysteresis() {
        let mut i = input(
            PadStyle::Nes,
            vec![
                vec![connected(A, "g"), axis(A, PadAxis::LeftX, 0.3)],
                vec![axis(A, PadAxis::LeftX, 0.6)],
                vec![axis(A, PadAxis::LeftX, 0.45)],
                vec![axis(A, PadAxis::LeftX, 0.35)],
            ],
        );
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
        assert!(poll(&mut i).pads[0].is_pressed(JoypadButton::Right));
        assert!(poll(&mut i).pads[0].is_pressed(JoypadButton::Right)); // hysteresis keeps it
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
    }

    #[test]
    fn stick_is_radial_and_y_positive_is_up() {
        // 0.5/0.5 has magnitude 0.707, but each axis is only 0.5 -> both directions on.
        let mut i = input(
            PadStyle::Nes,
            vec![vec![
                connected(A, "g"),
                axis(A, PadAxis::LeftX, -0.5),
                axis(A, PadAxis::LeftY, 0.5),
            ]],
        );
        let f = poll(&mut i);
        assert!(f.pads[0].is_pressed(JoypadButton::Left));
        assert!(f.pads[0].is_pressed(JoypadButton::Up));
        assert!(!f.pads[0].is_pressed(JoypadButton::Down));

        // Large deadzone cuts the diagonal (magnitude 0.42 < 0.5 anyway); here 0.6 / 0.0 under dz 0.6.
        let settings = GamepadSettings {
            deadzone: 0.6,
            ..GamepadSettings::default()
        };
        let mut i = input_with(
            PadStyle::Nes,
            settings,
            vec![vec![connected(A, "g"), axis(A, PadAxis::LeftX, 0.55)]],
        );
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
    }

    #[test]
    fn device_override_sets_port_and_deadzone() {
        let mut cfg = GamepadConfig::default();
        cfg.devices.insert(
            "abcd".into(),
            GamepadDeviceConfig {
                port: Some(1),
                deadzone: Some(0.6),
                ..Default::default()
            },
        );
        let mut i = input_with(
            PadStyle::Nes,
            GamepadSettings::from_config(&cfg),
            vec![vec![
                connected(A, "ABCD"),
                axis(A, PadAxis::LeftX, 0.55),
                btn(A, PadButton::South, true),
            ]],
        );
        let f = poll(&mut i);
        assert_eq!(i.attached(), vec![(1, A)]);
        assert_eq!(f.pads[0].bits(), 0);
        assert_eq!(f.pads[1].bits(), 1 << JoypadButton::B.id()); // stick inside the override deadzone
    }

    #[test]
    fn triggers_use_threshold_only_where_mapped() {
        let mut i = input(
            PadStyle::Generic,
            vec![
                vec![connected(A, "g"), axis(A, PadAxis::R2, 0.4)],
                vec![axis(A, PadAxis::R2, 0.6)],
                vec![axis(A, PadAxis::R2, 0.45)],
                vec![axis(A, PadAxis::R2, 0.1)],
            ],
        );
        assert!(!poll(&mut i).pads[0].is_pressed(JoypadButton::R2));
        assert!(poll(&mut i).pads[0].is_pressed(JoypadButton::R2));
        assert!(poll(&mut i).pads[0].is_pressed(JoypadButton::R2));
        assert!(!poll(&mut i).pads[0].is_pressed(JoypadButton::R2));
        // SNES maps no trigger.
        let mut i = input(
            PadStyle::Snes,
            vec![vec![connected(A, "g"), axis(A, PadAxis::R2, 1.0)]],
        );
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
    }

    #[test]
    fn hotkeys_hide_buttons_from_the_core() {
        let mut i = input(
            PadStyle::Snes,
            vec![
                vec![
                    connected(A, "g"),
                    btn(A, PadButton::Mode, true),
                    btn(A, PadButton::R1, true),
                    btn(A, PadButton::Start, true),
                ],
                vec![
                    btn(A, PadButton::R1, false),
                    btn(A, PadButton::Start, false),
                ],
                vec![
                    btn(A, PadButton::Mode, false),
                    btn(A, PadButton::South, true),
                ],
            ],
        );
        let f = poll(&mut i);
        assert_eq!(f.pads[0].bits(), 0);
        assert_eq!(
            i.take_hotkeys(),
            vec![HotkeyAction::SaveState, HotkeyAction::Menu]
        );
        poll(&mut i);
        let f = poll(&mut i);
        assert!(f.pads[0].is_pressed(JoypadButton::B));
        assert!(i.take_hotkeys().is_empty());
    }

    #[test]
    fn full_hotkey_table() {
        use HotkeyAction as H;
        use PadButton as P;
        let table = [
            (P::Start, H::Menu),
            (P::Select, H::TogglePause),
            (P::R1, H::SaveState),
            (P::L1, H::LoadState),
            (P::DPadRight, H::NextSlot),
            (P::DPadLeft, H::PrevSlot),
            (P::North, H::ToggleFullscreen),
            (P::West, H::Reset),
            (P::South, H::ToggleStats),
            (P::R3, H::ToggleVirtualKeyboard),
            (P::L3, H::NextDisk),
        ];
        let mut events = vec![connected(A, "g"), btn(A, P::Mode, true)];
        events.extend(table.iter().map(|(b, _)| btn(A, *b, true)));
        let mut i = input(PadStyle::Nes, vec![events]);
        poll(&mut i);
        let expected: Vec<_> = table.iter().map(|(_, a)| *a).collect();
        assert_eq!(i.take_hotkeys(), expected);
    }

    #[test]
    fn ps_alone_does_nothing() {
        let mut i = input(
            PadStyle::Nes,
            vec![vec![
                connected(A, "g"),
                btn(A, PadButton::Mode, true),
                btn(A, PadButton::Mode, false),
            ]],
        );
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
        assert!(i.take_hotkeys().is_empty());
    }

    #[test]
    fn holding_a_hotkey_fires_once() {
        let mut i = input(
            PadStyle::Nes,
            vec![vec![
                connected(A, "g"),
                btn(A, PadButton::Mode, true),
                btn(A, PadButton::R1, true),
                btn(A, PadButton::R1, true),
            ]],
        );
        poll(&mut i);
        assert_eq!(i.take_hotkeys(), vec![HotkeyAction::SaveState]);
    }

    #[test]
    fn fast_forward_is_ps_plus_r2_held() {
        let mut i = input(
            PadStyle::Nes,
            vec![
                vec![connected(A, "g"), btn(A, PadButton::Mode, true)],
                vec![axis(A, PadAxis::R2, 0.9)],
                vec![axis(A, PadAxis::R2, 0.0)],
                vec![btn(A, PadButton::R2, true)],
                vec![btn(A, PadButton::Mode, false)],
            ],
        );
        poll(&mut i);
        assert!(!i.fast_forward_held());
        poll(&mut i);
        assert!(i.fast_forward_held());
        poll(&mut i);
        assert!(!i.fast_forward_held());
        poll(&mut i);
        assert!(i.fast_forward_held());
        assert!(i.take_hotkeys().is_empty());
        poll(&mut i);
        assert!(!i.fast_forward_held());
    }

    #[test]
    fn share_fallback_modifier() {
        let settings = GamepadSettings {
            hotkey_button: HotkeyButton::Select,
            ..GamepadSettings::default()
        };
        let mut i = input_with(
            PadStyle::Snes,
            settings,
            vec![vec![
                connected(A, "g"),
                btn(A, PadButton::Select, true),
                btn(A, PadButton::L1, true),
                btn(A, PadButton::Mode, true),
            ]],
        );
        let f = poll(&mut i);
        assert_eq!(f.pads[0].bits(), 0);
        assert_eq!(
            i.take_hotkeys(),
            vec![HotkeyAction::LoadState, HotkeyAction::TogglePause]
        );
    }

    #[test]
    fn disabled_gamepad_does_nothing() {
        let settings = GamepadSettings {
            enabled: false,
            ..GamepadSettings::default()
        };
        let mut i = input_with(
            PadStyle::Nes,
            settings,
            vec![vec![connected(A, "g"), btn(A, PadButton::South, true)]],
        );
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
        assert!(i.attached().is_empty());
    }

    #[test]
    fn ors_with_existing_frame_content() {
        let mut i = input(
            PadStyle::Nes,
            vec![vec![connected(A, "g"), btn(A, PadButton::South, true)]],
        );
        let mut f = InputFrame::default();
        f.pads[0].set(JoypadButton::Start, true);
        i.poll(0, &mut f);
        assert!(f.pads[0].is_pressed(JoypadButton::Start));
        assert!(f.pads[0].is_pressed(JoypadButton::B));
    }

    #[test]
    fn ui_capture_turns_buttons_into_navigation() {
        let mut i = input(
            PadStyle::Nes,
            vec![
                vec![connected(A, "g")],
                vec![
                    btn(A, PadButton::DPadDown, true),
                    btn(A, PadButton::DPadDown, true),
                    btn(A, PadButton::South, true),
                    btn(A, PadButton::East, true),
                    btn(A, PadButton::Start, true),
                    axis(A, PadAxis::LeftX, -0.9),
                    axis(A, PadAxis::LeftX, -0.95),
                ],
                vec![
                    btn(A, PadButton::Mode, true),
                    btn(A, PadButton::R3, true),
                    btn(A, PadButton::DPadUp, true),
                ],
            ],
        );
        i.pump();
        i.set_ui_capture(true);
        let f = poll(&mut i);
        assert_eq!(
            f.pads[0].bits(),
            0,
            "the core sees nothing while a menu is open"
        );
        assert_eq!(
            i.take_nav(),
            vec![NavKey::Down, NavKey::Activate, NavKey::Back, NavKey::Left]
        );
        i.pump();
        assert!(i.take_nav().is_empty(), "PS held: hotkeys, not navigation");
        assert_eq!(i.take_hotkeys(), vec![HotkeyAction::ToggleVirtualKeyboard]);
        i.set_ui_capture(false);
        assert!(i.take_nav().is_empty());
    }

    #[test]
    fn navigation_is_off_without_ui_capture() {
        let mut i = input(
            PadStyle::Nes,
            vec![vec![connected(A, "g"), btn(A, PadButton::DPadUp, true)]],
        );
        i.pump();
        assert!(i.take_nav().is_empty());
        assert!(poll(&mut i).pads[0].is_pressed(JoypadButton::Up));
    }

    #[test]
    fn pump_updates_state_without_a_frame() {
        let mut i = input(
            PadStyle::Nes,
            vec![vec![
                connected(A, "g"),
                btn(A, PadButton::Mode, true),
                btn(A, PadButton::L3, true),
            ]],
        );
        i.pump();
        assert_eq!(i.take_hotkeys(), vec![HotkeyAction::NextDisk]);
        assert_eq!(i.attached(), vec![(0, A)]);
        i.set_settings(GamepadSettings {
            enabled: false,
            ..GamepadSettings::default()
        });
        assert!(!i.settings().enabled);
        assert_eq!(poll(&mut i).pads[0].bits(), 0);
    }

    #[test]
    fn settings_are_clamped() {
        let cfg = GamepadConfig {
            deadzone: 0.9,
            trigger_threshold: f32::NAN,
            ..GamepadConfig::default()
        };
        let s = GamepadSettings::from_config(&cfg);
        assert_eq!(s.deadzone, 0.60);
        assert_eq!(s.trigger_threshold, 0.5);
        let cfg = GamepadConfig {
            deadzone: 0.0,
            ..GamepadConfig::default()
        };
        assert_eq!(GamepadSettings::from_config(&cfg).deadzone, 0.05);
        assert_eq!(GamepadSettings::default().deadzone, 0.25);
    }
}
