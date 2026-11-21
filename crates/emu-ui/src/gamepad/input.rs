//! Pad state -> `InputFrame`: ports, hotplug, deadzone, hotkeys.

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use emu_runtime::{AnalogStick, InputBackend, InputFrame, JoypadButton, MAX_PORTS};

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
/// Held direction in a menu: first repeat after `REPEAT_DELAY`, then one every `REPEAT_INTERVAL`.
pub const REPEAT_DELAY: Duration = Duration::from_millis(400);
pub const REPEAT_INTERVAL: Duration = Duration::from_millis(80);

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
    fn is_direction(self) -> bool {
        matches!(self, Self::Up | Self::Down | Self::Left | Self::Right)
    }

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
    name: String,
    port: usize,
    deadzone: f32,
    held: HashSet<PadButton>,
    stick: (f32, f32),
    /// Right stick (cores with `analog = true` only).
    right: (f32, f32),
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

/// Stick position for `RETRO_DEVICE_ANALOG` (ADR 0016 amended, design v4.0 §3.2): radial deadzone of the pad, then
/// the remaining range rescaled to the full `i16` range; pad Y up (+) becomes libretro Y up (−).
fn analog_axes((x, y): (f32, f32), deadzone: f32) -> (i16, i16) {
    let x = x.clamp(-1.0, 1.0);
    let y = y.clamp(-1.0, 1.0);
    let magnitude = (x * x + y * y).sqrt();
    if magnitude < deadzone || magnitude <= f32::EPSILON {
        return (0, 0);
    }
    let scaled = ((magnitude - deadzone) / (1.0 - deadzone).max(f32::EPSILON)).min(1.0);
    let k = scaled / magnitude;
    #[allow(clippy::cast_possible_truncation)] // clamped to ±32767 just before
    let to_i16 = |v: f32| (v * k * 32767.0).round().clamp(-32767.0, 32767.0) as i16;
    (to_i16(x), to_i16(-y))
}

/// Of two pads on one port, the stick pushed further wins (OR of the buttons).
fn stronger(a: (i16, i16), b: (i16, i16)) -> (i16, i16) {
    let m = |(x, y): (i16, i16)| i32::from(x).pow(2) + i32::from(y).pow(2);
    if m(b) > m(a) { b } else { a }
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
    /// Direction pressed since the last `pump_at`: starts the auto-repeat.
    fresh_dir: Option<NavKey>,
    /// Direction being repeated and when its next repeat is due.
    repeat: Option<(NavKey, Instant)>,
    /// Remap screen: the next pressed button is reported by `take_learned` instead of being acted on.
    learning: bool,
    learned: Option<PadButton>,
    /// A menu or the virtual keyboard has the focus: buttons navigate instead of reaching the core.
    ui_capture: bool,
    /// The running core reads the sticks (`analog = true`): axes sent, no stick → D-pad conversion.
    analog: bool,
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
            fresh_dir: None,
            repeat: None,
            learning: false,
            learned: None,
            ui_capture: false,
            analog: false,
            scratch: Vec::new(),
        }
    }

    /// Core of the game (`CorePreferences::analog`): `true` sends both sticks as `RETRO_DEVICE_ANALOG` and stops
    /// converting the left stick into the D-pad (ADR 0016 amended). Menus keep navigating with the stick.
    pub fn set_analog(&mut self, analog: bool) {
        self.analog = analog;
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
        self.pump_at(Instant::now());
    }

    /// `pump` with an explicit clock (auto-repeat of held directions while a menu has the focus).
    pub fn pump_at(&mut self, now: Instant) {
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
        self.update_repeat(now);
    }

    fn direction_held(&self, key: NavKey) -> bool {
        let (button, idx) = match key {
            NavKey::Up => (PadButton::DPadUp, 0),
            NavKey::Down => (PadButton::DPadDown, 1),
            NavKey::Left => (PadButton::DPadLeft, 2),
            NavKey::Right => (PadButton::DPadRight, 3),
            _ => return false,
        };
        self.pads
            .iter()
            .any(|p| !self.modifier_held(p) && (p.held.contains(&button) || p.stick_dirs[idx]))
    }

    fn update_repeat(&mut self, now: Instant) {
        if !self.ui_capture || !self.settings.enabled {
            self.repeat = None;
            self.fresh_dir = None;
            return;
        }
        if let Some(key) = self.fresh_dir.take() {
            self.repeat = Some((key, now + REPEAT_DELAY));
        }
        if let Some((key, due)) = self.repeat {
            if !self.direction_held(key) {
                self.repeat = None;
            } else if now >= due {
                self.nav.push(key);
                self.repeat = Some((key, now + REPEAT_INTERVAL));
            }
        }
    }

    /// Remap screen: while `true`, the next button pressed on any pad is kept for `take_learned` and goes
    /// nowhere else (no game input, no hotkey, no navigation).
    pub fn set_learning(&mut self, learning: bool) {
        if learning != self.learning {
            self.learning = learning;
            self.learned = None;
        }
    }

    pub fn take_learned(&mut self) -> Option<PadButton> {
        self.learned.take()
    }

    /// Attached pads, `(port, name)`, for the settings screen.
    pub fn attached_names(&self) -> Vec<(usize, String)> {
        let mut v: Vec<_> = self.pads.iter().map(|p| (p.port, p.name.clone())).collect();
        v.sort();
        v
    }

    /// While `true` (menu or virtual keyboard open), no pad button reaches the core; presses of the D-pad, the
    /// left stick, Cross and Circle become `NavKey`s (`take_nav`). Hotkeys keep working.
    pub fn set_ui_capture(&mut self, capture: bool) {
        if capture != self.ui_capture {
            self.ui_capture = capture;
            self.nav.clear();
            self.repeat = None;
            self.fresh_dir = None;
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
            name: name.clone(),
            port,
            deadzone,
            held: HashSet::new(),
            stick: (0.0, 0.0),
            right: (0.0, 0.0),
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
        if self.learning {
            if pressed && !was_down {
                self.learned = Some(button);
            }
            if pressed {
                self.pads[idx].held.insert(button);
            } else {
                self.pads[idx].held.remove(&button);
            }
            return;
        }
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
            if nav.is_direction() {
                self.fresh_dir = Some(nav);
            }
            self.nav.push(nav);
        }
    }

    fn axis(&mut self, id: PadId, axis: PadAxis, value: f32) {
        let threshold = self.settings.trigger_threshold;
        let modifier = self.modifier_button();
        let ui_capture = self.ui_capture;
        let learning = self.learning;
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
            PadAxis::L2 | PadAxis::R2 => {
                let (state, button) = if axis == PadAxis::L2 {
                    (&mut pad.trigger_l2, PadButton::L2)
                } else {
                    (&mut pad.trigger_r2, PadButton::R2)
                };
                let was = *state;
                *state = trigger(was);
                if learning && *state && !was {
                    self.learned = Some(button);
                }
            }
            PadAxis::RightX => pad.right.0 = value,
            PadAxis::RightY => pad.right.1 = value,
        }
        let before = pad.stick_dirs;
        pad.update_stick_dirs();
        if learning {
            return;
        }
        if ui_capture && !pad.held.contains(&modifier) {
            let dirs = [NavKey::Up, NavKey::Down, NavKey::Left, NavKey::Right];
            for ((was, now), nav) in before.iter().zip(pad.stick_dirs).zip(dirs) {
                if now && !was {
                    self.fresh_dir = Some(nav);
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
            if self.analog {
                for (stick, value) in [
                    (AnalogStick::Left, pad.stick),
                    (AnalogStick::Right, pad.right),
                ] {
                    let (x, y) = stronger(target.analog(stick), analog_axes(value, pad.deadzone));
                    target.set_analog(stick, x, y);
                }
                continue;
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

    /// DG1 (ADR 0016 amended): an `analog = true` core gets both sticks, never the D-pad conversion.
    #[test]
    fn analog_cores_get_the_sticks_instead_of_the_dpad() {
        let batches = vec![vec![
            connected(A, "g"),
            axis(A, PadAxis::LeftX, 1.0),
            axis(A, PadAxis::LeftY, 1.0),
            axis(A, PadAxis::RightX, -0.1),
            axis(A, PadAxis::RightY, -1.0),
        ]];
        let mut i = input(PadStyle::Generic, batches.clone());
        i.set_analog(true);
        let f = poll(&mut i);
        assert_eq!(f.pads[0].bits(), 0, "pas de conversion en croix");
        let (x, y) = f.pads[0].analog(AnalogStick::Left);
        assert!(x > 20_000 && y < -20_000, "{x} {y}"); // up = negative for libretro
        let (rx, ry) = f.pads[0].analog(AnalogStick::Right);
        assert!(rx < 0 && ry > 30_000, "{rx} {ry}");
        // Other cores: D-pad from the stick, axes stay at 0.
        let mut i = input(PadStyle::Generic, batches);
        let f = poll(&mut i);
        assert!(f.pads[0].is_pressed(JoypadButton::Right));
        assert!(f.pads[0].is_pressed(JoypadButton::Up));
        assert_eq!(f.pads[0].analog(AnalogStick::Left), (0, 0));
    }

    #[test]
    fn analog_deadzone_is_radial_and_rescaled() {
        assert_eq!(analog_axes((0.2, 0.0), 0.25), (0, 0));
        assert_eq!(analog_axes((0.0, 0.0), 0.25), (0, 0));
        assert_eq!(analog_axes((1.0, 0.0), 0.25), (32767, 0));
        assert_eq!(analog_axes((0.0, -1.0), 0.25), (0, 32767));
        let (x, _) = analog_axes((0.625, 0.0), 0.25);
        assert!((16_000..=16_800).contains(&x), "{x}");
        // Out-of-range values from a driver are clamped.
        assert_eq!(analog_axes((3.0, 0.0), 0.25), (32767, 0));
        assert_eq!(stronger((100, 0), (0, -200)), (0, -200));
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

    fn nav_input(batches: Vec<Vec<PadEvent>>) -> GamepadInput<ScriptedPadSource> {
        let mut i = input(PadStyle::MegaDrive, batches);
        i.set_ui_capture(true);
        i
    }

    #[test]
    fn held_direction_repeats_after_delay_then_at_interval() {
        let t0 = Instant::now();
        let mut i = nav_input(vec![
            vec![connected(A, "g"), btn(A, PadButton::DPadDown, true)],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![btn(A, PadButton::DPadDown, false)],
            vec![],
        ]);
        i.pump_at(t0);
        assert_eq!(i.take_nav(), vec![NavKey::Down]);
        i.pump_at(t0 + REPEAT_DELAY - Duration::from_millis(1));
        assert!(i.take_nav().is_empty());
        i.pump_at(t0 + REPEAT_DELAY);
        assert_eq!(i.take_nav(), vec![NavKey::Down]);
        i.pump_at(t0 + REPEAT_DELAY + REPEAT_INTERVAL - Duration::from_millis(1));
        assert!(i.take_nav().is_empty());
        i.pump_at(t0 + REPEAT_DELAY + REPEAT_INTERVAL);
        assert_eq!(i.take_nav(), vec![NavKey::Down]);
        // Released: nothing more, however late.
        i.pump_at(t0 + Duration::from_secs(5));
        assert!(i.take_nav().is_empty());
        i.pump_at(t0 + Duration::from_secs(6));
        assert!(i.take_nav().is_empty());
    }

    #[test]
    fn stick_direction_repeats_and_activate_does_not() {
        let t0 = Instant::now();
        let mut i = nav_input(vec![
            vec![
                connected(A, "g"),
                axis(A, PadAxis::LeftX, 1.0),
                btn(A, PadButton::South, true),
            ],
            vec![],
        ]);
        i.pump_at(t0);
        assert_eq!(i.take_nav(), vec![NavKey::Right, NavKey::Activate]);
        i.pump_at(t0 + REPEAT_DELAY);
        assert_eq!(i.take_nav(), vec![NavKey::Right]);
    }

    #[test]
    fn repeat_stops_when_capture_ends() {
        let t0 = Instant::now();
        let mut i = nav_input(vec![
            vec![connected(A, "g"), btn(A, PadButton::DPadUp, true)],
            vec![],
        ]);
        i.pump_at(t0);
        let _ = i.take_nav();
        i.set_ui_capture(false);
        i.pump_at(t0 + Duration::from_secs(1));
        assert!(i.take_nav().is_empty());
    }

    #[test]
    fn learning_keeps_the_next_button_and_nothing_else() {
        let mut i = nav_input(vec![
            vec![connected(A, "g")],
            vec![btn(A, PadButton::North, true)],
            vec![axis(A, PadAxis::R2, 1.0)],
        ]);
        i.pump();
        i.set_learning(true);
        i.pump();
        assert_eq!(i.take_learned(), Some(PadButton::North));
        assert!(i.take_nav().is_empty());
        i.pump();
        assert_eq!(i.take_learned(), Some(PadButton::R2));
        i.set_learning(false);
        assert_eq!(i.take_learned(), None);
    }

    #[test]
    fn attached_names_lists_ports() {
        let mut i = input(PadStyle::MegaDrive, vec![vec![connected(B, "g")]]);
        i.pump();
        assert_eq!(i.attached_names(), vec![(0, "DS4".to_owned())]);
    }
}
