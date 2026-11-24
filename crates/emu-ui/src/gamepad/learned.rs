//! Learned pad maps (7.2, docs/design/v7.2-manette-snes.md): raw native inputs -> positional `PadButton`s.
//! Pure logic, no I/O.
// Not called outside tests until G3 wires it into `GamepadInput`; remove then.
#![allow(dead_code)]
use std::collections::{BTreeMap, HashSet};

use super::mapping::PadButton;
use super::source::{PadEventKind, RawInput};

/// One physical input bound to a button: a button, or one direction of an axis (hat / D-pad on axes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawBinding {
    Button(u32),
    AxisPos(u32),
    AxisNeg(u32),
}

impl RawBinding {
    /// Config token: `b12`, `+a3`, `-a3`.
    pub fn to_token(self) -> String {
        match self {
            Self::Button(c) => format!("b{c}"),
            Self::AxisPos(c) => format!("+a{c}"),
            Self::AxisNeg(c) => format!("-a{c}"),
        }
    }

    /// Inverse of `to_token`; `None` for anything else.
    pub fn from_token(token: &str) -> Option<Self> {
        // Digits only: `u32::from_str` would accept a leading `+`.
        fn code(s: &str) -> Option<u32> {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            s.parse().ok()
        }
        let token = token.trim();
        if let Some(rest) = token.strip_prefix("+a") {
            code(rest).map(Self::AxisPos)
        } else if let Some(rest) = token.strip_prefix("-a") {
            code(rest).map(Self::AxisNeg)
        } else {
            code(token.strip_prefix('b')?).map(Self::Button)
        }
    }
}

/// Shape of the physical pad: only changes the prompts, never the positional result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadLayout {
    Snes,
    Nes,
    MegaDrive3,
    MegaDrive6,
}

/// One prompt of the learning sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LearnStep {
    pub button: PadButton,
    /// French prompt, e.g. « A (bouton de droite) ».
    pub label: &'static str,
}

const fn step(button: PadButton, label: &'static str) -> LearnStep {
    LearnStep { button, label }
}

const DPAD: [LearnStep; 4] = [
    step(PadButton::DPadUp, "Croix : haut"),
    step(PadButton::DPadDown, "Croix : bas"),
    step(PadButton::DPadLeft, "Croix : gauche"),
    step(PadButton::DPadRight, "Croix : droite"),
];

static SNES_STEPS: [LearnStep; 12] = [
    DPAD[0],
    DPAD[1],
    DPAD[2],
    DPAD[3],
    step(PadButton::South, "B (bouton du bas)"),
    step(PadButton::East, "A (bouton de droite)"),
    step(PadButton::West, "Y (bouton de gauche)"),
    step(PadButton::North, "X (bouton du haut)"),
    step(PadButton::L1, "L (gâchette gauche)"),
    step(PadButton::R1, "R (gâchette droite)"),
    step(PadButton::Select, "Select"),
    step(PadButton::Start, "Start"),
];

static NES_STEPS: [LearnStep; 8] = [
    DPAD[0],
    DPAD[1],
    DPAD[2],
    DPAD[3],
    step(PadButton::South, "B (bouton de gauche)"),
    step(PadButton::East, "A (bouton de droite)"),
    step(PadButton::Select, "Select"),
    step(PadButton::Start, "Start"),
];

static MD3_STEPS: [LearnStep; 8] = [
    DPAD[0],
    DPAD[1],
    DPAD[2],
    DPAD[3],
    step(PadButton::West, "A (bouton de gauche)"),
    step(PadButton::South, "B (bouton du milieu)"),
    step(PadButton::East, "C (bouton de droite)"),
    step(PadButton::Start, "Start"),
];

static MD6_STEPS: [LearnStep; 12] = [
    DPAD[0],
    DPAD[1],
    DPAD[2],
    DPAD[3],
    step(PadButton::West, "A (bas, à gauche)"),
    step(PadButton::South, "B (bas, au milieu)"),
    step(PadButton::East, "C (bas, à droite)"),
    step(PadButton::L1, "X (haut, à gauche)"),
    step(PadButton::North, "Y (haut, au milieu)"),
    step(PadButton::R1, "Z (haut, à droite)"),
    step(PadButton::Select, "Mode"),
    step(PadButton::Start, "Start"),
];

impl PadLayout {
    /// Prompts in order: D-pad first, then buttons.
    pub fn steps(self) -> &'static [LearnStep] {
        match self {
            Self::Snes => &SNES_STEPS,
            Self::Nes => &NES_STEPS,
            Self::MegaDrive3 => &MD3_STEPS,
            Self::MegaDrive6 => &MD6_STEPS,
        }
    }

    /// `snes` | `nes` | `megadrive3` | `megadrive6`.
    pub fn config_name(self) -> &'static str {
        match self {
            Self::Snes => "snes",
            Self::Nes => "nes",
            Self::MegaDrive3 => "megadrive3",
            Self::MegaDrive6 => "megadrive6",
        }
    }

    pub fn from_config_name(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        [Self::Snes, Self::Nes, Self::MegaDrive3, Self::MegaDrive6]
            .into_iter()
            .find(|l| l.config_name() == s)
    }
}

/// Positional button -> raw binding, as stored in `[input.gamepad.devices."<guid>".map]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LearnedMap {
    bindings: Vec<(PadButton, RawBinding)>,
}

impl LearnedMap {
    /// Unknown keys / invalid tokens are skipped with `warn!`.
    pub fn from_config(map: &BTreeMap<String, String>) -> Self {
        let mut bindings: Vec<(PadButton, RawBinding)> = Vec::new();
        for (key, value) in map {
            let (Some(button), Some(binding)) = (
                PadButton::from_config_name(key),
                RawBinding::from_token(value),
            ) else {
                tracing::warn!("entrée de manette apprise ignorée : {key}");
                continue;
            };
            if bindings.iter().any(|(b, _)| *b == button) {
                continue;
            }
            bindings.push((button, binding));
        }
        Self { bindings }
    }

    pub fn to_config(&self) -> BTreeMap<String, String> {
        self.bindings
            .iter()
            .map(|(b, r)| (b.config_name().to_owned(), r.to_token()))
            .collect()
    }

    pub fn bindings(&self) -> &[(PadButton, RawBinding)] {
        &self.bindings
    }

    pub fn has(&self, button: PadButton) -> bool {
        self.bindings.iter().any(|(b, _)| *b == button)
    }
}

/// Threshold to press and (lower) threshold to release an axis, so a noisy axis does not flutter.
const PRESS: f32 = 0.5;
const RELEASE: f32 = 0.4;

/// Tracks axes whose rest position is -1 (analog triggers): their first reported value is <= -0.9.
/// Such an axis is read as `(v + 1) / 2`, so it only ever goes "positive". A D-pad axis (rest 0) is untouched.
/// Limit: a D-pad axis whose very first event is a full negative press would be mistaken for a trigger.
#[derive(Debug, Default)]
struct AxisRest {
    seen: HashSet<u32>,
    low: HashSet<u32>,
}

impl AxisRest {
    fn normalize(&mut self, code: u32, value: f32, allow_low: bool) -> f32 {
        if self.seen.insert(code) && allow_low && value <= -0.9 {
            self.low.insert(code);
        }
        if self.low.contains(&code) {
            (value + 1.0) / 2.0
        } else {
            value
        }
    }
}

/// Edge detection with hysteresis for one binding: `held` is the previous state.
fn pressed_now(binding: RawBinding, value: f32, held: bool) -> bool {
    let v = match binding {
        RawBinding::Button(_) | RawBinding::AxisPos(_) => value,
        RawBinding::AxisNeg(_) => -value,
    };
    if held { v >= RELEASE } else { v >= PRESS }
}

/// The binding(s) an input can drive, and whether `input` is the right kind for it.
fn binding_matches(binding: RawBinding, input: RawInput) -> bool {
    matches!(
        (binding, input),
        (RawBinding::Button(a), RawInput::Button(b))
            | (RawBinding::AxisPos(a) | RawBinding::AxisNeg(a), RawInput::Axis(b)) if a == b
    )
}

/// Per-pad state turning `Raw` events into `Button` events (axes: threshold 0.5, release below 0.4).
#[derive(Debug, Default)]
pub struct RawTranslator {
    held: HashSet<RawBinding>,
    rest: AxisRest,
}

impl RawTranslator {
    /// Appends the `PadEventKind::Button` transitions caused by `input` moving to `value`.
    pub fn feed(
        &mut self,
        map: &LearnedMap,
        input: RawInput,
        value: f32,
        out: &mut Vec<PadEventKind>,
    ) {
        let value = match input {
            RawInput::Axis(code) => {
                let has_neg = map
                    .bindings
                    .iter()
                    .any(|(_, b)| *b == RawBinding::AxisNeg(code));
                self.rest.normalize(code, value, !has_neg)
            }
            RawInput::Button(_) => value,
        };
        for &(button, binding) in &map.bindings {
            if !binding_matches(binding, input) {
                continue;
            }
            let was = self.held.contains(&binding);
            let now = pressed_now(binding, value, was);
            if now == was {
                continue;
            }
            if now {
                self.held.insert(binding);
            } else {
                self.held.remove(&binding);
            }
            out.push(PadEventKind::Button {
                button,
                pressed: now,
            });
        }
    }
}

/// Result of feeding one raw input to a `LearnSession`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnProgress {
    Waiting,
    Recorded,
    /// Input already bound to this button: refused.
    AlreadyUsed(PadButton),
    Done,
}

/// Step-by-step learning of one pad; the UI view stays thin on top of it.
#[derive(Debug)]
pub struct LearnSession {
    layout: PadLayout,
    position: usize,
    recorded: Vec<Option<RawBinding>>,
    held: HashSet<RawBinding>,
    /// True once every input has been released since the last capture / refusal.
    armed: bool,
    rest: AxisRest,
}

impl LearnSession {
    pub fn new(layout: PadLayout) -> Self {
        Self {
            layout,
            position: 0,
            recorded: vec![None; layout.steps().len()],
            held: HashSet::new(),
            armed: true,
            rest: AxisRest::default(),
        }
    }

    /// Current prompt; `None` once every step is done.
    pub fn current(&self) -> Option<&LearnStep> {
        self.layout.steps().get(self.position)
    }

    /// Records on the first input crossing 0.5 once every input is released (no double capture).
    pub fn feed(&mut self, input: RawInput, value: f32) -> LearnProgress {
        // Every direction the input can express: a button, or the positive / negative side of an axis.
        let (value, candidates) = match input {
            RawInput::Button(c) => (value, [RawBinding::Button(c), RawBinding::Button(c)]),
            RawInput::Axis(c) => (
                self.rest.normalize(c, value, true),
                [RawBinding::AxisPos(c), RawBinding::AxisNeg(c)],
            ),
        };
        let mut new_press = None;
        for binding in candidates {
            let was = self.held.contains(&binding);
            let now = pressed_now(binding, value, was);
            if now && !was {
                self.held.insert(binding);
                new_press.get_or_insert(binding);
            } else if !now && was {
                self.held.remove(&binding);
            }
        }
        let was_armed = self.armed;
        if self.held.is_empty() {
            self.armed = true;
        }
        if self.current().is_none() {
            return LearnProgress::Done;
        }
        let Some(binding) = new_press.filter(|_| was_armed) else {
            return LearnProgress::Waiting;
        };
        self.armed = false;
        let used = self
            .recorded
            .iter()
            .position(|r| *r == Some(binding))
            .map(|i| self.layout.steps()[i].button);
        if let Some(button) = used {
            return LearnProgress::AlreadyUsed(button);
        }
        self.recorded[self.position] = Some(binding);
        self.position += 1;
        if self.current().is_none() {
            LearnProgress::Done
        } else {
            LearnProgress::Recorded
        }
    }

    /// Leaves the current step unbound.
    pub fn skip(&mut self) {
        if self.current().is_some() {
            self.recorded[self.position] = None;
            self.position += 1;
        }
    }

    /// Goes back one step and drops its binding.
    pub fn back(&mut self) {
        if self.position > 0 {
            self.position -= 1;
            self.recorded[self.position] = None;
        }
    }

    pub fn finish(self) -> LearnedMap {
        let bindings = self
            .layout
            .steps()
            .iter()
            .zip(self.recorded)
            .filter_map(|(step, r)| r.map(|r| (step.button, r)))
            .collect();
        LearnedMap { bindings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(s: &mut LearnSession, input: RawInput) -> LearnProgress {
        let p = s.feed(input, 1.0);
        s.feed(input, 0.0);
        p
    }

    fn map_of(pairs: &[(PadButton, RawBinding)]) -> LearnedMap {
        LearnedMap {
            bindings: pairs.to_vec(),
        }
    }

    fn feed_all(
        t: &mut RawTranslator,
        m: &LearnedMap,
        input: RawInput,
        v: f32,
    ) -> Vec<PadEventKind> {
        let mut out = Vec::new();
        t.feed(m, input, v, &mut out);
        out
    }

    fn ev(button: PadButton, pressed: bool) -> PadEventKind {
        PadEventKind::Button { button, pressed }
    }

    #[test]
    fn tokens_round_trip_and_reject_garbage() {
        for b in [
            RawBinding::Button(12),
            RawBinding::AxisPos(3),
            RawBinding::AxisNeg(0),
        ] {
            assert_eq!(RawBinding::from_token(&b.to_token()), Some(b));
        }
        assert_eq!(RawBinding::Button(12).to_token(), "b12");
        assert_eq!(RawBinding::AxisPos(3).to_token(), "+a3");
        assert_eq!(RawBinding::AxisNeg(3).to_token(), "-a3");
        for bad in [
            "",
            "b",
            "a3",
            "+a",
            "b+1",
            "b-1",
            "x1",
            "++a1",
            "b1x",
            "b99999999999",
        ] {
            assert_eq!(RawBinding::from_token(bad), None, "{bad}");
        }
    }

    #[test]
    fn layouts_steps_and_names() {
        let buttons = |l: PadLayout| l.steps().iter().map(|s| s.button).collect::<Vec<_>>();
        use PadButton as P;
        assert_eq!(
            &buttons(PadLayout::Snes)[..4],
            &[P::DPadUp, P::DPadDown, P::DPadLeft, P::DPadRight]
        );
        assert_eq!(
            &buttons(PadLayout::Snes)[4..],
            &[
                P::South,
                P::East,
                P::West,
                P::North,
                P::L1,
                P::R1,
                P::Select,
                P::Start
            ]
        );
        assert_eq!(
            &buttons(PadLayout::Nes)[4..],
            &[P::South, P::East, P::Select, P::Start]
        );
        assert_eq!(
            &buttons(PadLayout::MegaDrive3)[4..],
            &[P::West, P::South, P::East, P::Start]
        );
        assert_eq!(
            &buttons(PadLayout::MegaDrive6)[4..],
            &[
                P::West,
                P::South,
                P::East,
                P::L1,
                P::North,
                P::R1,
                P::Select,
                P::Start
            ]
        );
        for l in [
            PadLayout::Snes,
            PadLayout::Nes,
            PadLayout::MegaDrive3,
            PadLayout::MegaDrive6,
        ] {
            assert_eq!(PadLayout::from_config_name(l.config_name()), Some(l));
            let b = buttons(l);
            let unique: HashSet<_> = b.iter().collect();
            assert_eq!(unique.len(), b.len());
            assert!(l.steps().iter().all(|s| !s.label.is_empty()));
        }
        assert_eq!(PadLayout::from_config_name("n64"), None);
    }

    #[test]
    fn config_round_trip_skips_invalid() {
        let m = map_of(&[
            (PadButton::South, RawBinding::Button(2)),
            (PadButton::DPadLeft, RawBinding::AxisNeg(0)),
        ]);
        assert_eq!(
            LearnedMap::from_config(&m.to_config()).to_config(),
            m.to_config()
        );
        let mut cfg = m.to_config();
        cfg.insert("bogus".into(), "b1".into());
        cfg.insert("east".into(), "zzz".into());
        let back = LearnedMap::from_config(&cfg);
        assert_eq!(back.bindings().len(), 2);
        assert!(back.has(PadButton::South));
        assert!(!back.has(PadButton::East));
    }

    #[test]
    fn translator_buttons() {
        let m = map_of(&[(PadButton::South, RawBinding::Button(2))]);
        let mut t = RawTranslator::default();
        assert_eq!(
            feed_all(&mut t, &m, RawInput::Button(2), 1.0),
            [ev(PadButton::South, true)]
        );
        assert!(feed_all(&mut t, &m, RawInput::Button(2), 1.0).is_empty());
        assert_eq!(
            feed_all(&mut t, &m, RawInput::Button(2), 0.0),
            [ev(PadButton::South, false)]
        );
        assert!(feed_all(&mut t, &m, RawInput::Button(3), 1.0).is_empty());
        assert!(feed_all(&mut t, &m, RawInput::Axis(2), 1.0).is_empty());
    }

    #[test]
    fn translator_dpad_axes_with_hysteresis() {
        let m = map_of(&[
            (PadButton::DPadLeft, RawBinding::AxisNeg(0)),
            (PadButton::DPadRight, RawBinding::AxisPos(0)),
        ]);
        let mut t = RawTranslator::default();
        let a = RawInput::Axis(0);
        assert!(feed_all(&mut t, &m, a, 0.004).is_empty());
        assert!(feed_all(&mut t, &m, a, 0.45).is_empty());
        assert_eq!(
            feed_all(&mut t, &m, a, 0.6),
            [ev(PadButton::DPadRight, true)]
        );
        // Between 0.4 and 0.5: still held.
        assert!(feed_all(&mut t, &m, a, 0.45).is_empty());
        assert_eq!(
            feed_all(&mut t, &m, a, 0.3),
            [ev(PadButton::DPadRight, false)]
        );
        assert_eq!(
            feed_all(&mut t, &m, a, -1.0),
            [ev(PadButton::DPadLeft, true)]
        );
        // Straight from left to right releases left and presses right.
        assert_eq!(
            feed_all(&mut t, &m, a, 1.0),
            [
                ev(PadButton::DPadLeft, false),
                ev(PadButton::DPadRight, true)
            ]
        );
    }

    #[test]
    fn translator_axis_resting_at_minus_one() {
        let m = map_of(&[(PadButton::R1, RawBinding::AxisPos(5))]);
        let mut t = RawTranslator::default();
        let a = RawInput::Axis(5);
        assert!(feed_all(&mut t, &m, a, -1.0).is_empty());
        assert!(feed_all(&mut t, &m, a, -0.2).is_empty());
        assert_eq!(feed_all(&mut t, &m, a, 0.8), [ev(PadButton::R1, true)]);
        assert_eq!(feed_all(&mut t, &m, a, -1.0), [ev(PadButton::R1, false)]);
    }

    #[test]
    fn session_records_full_snes_sequence() {
        let mut s = LearnSession::new(PadLayout::Snes);
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::DPadUp));
        // D-pad on axes: up/down on axis 1, left/right on axis 0, rest at 0.
        assert_eq!(s.feed(RawInput::Axis(1), 0.004), LearnProgress::Waiting);
        assert_eq!(s.feed(RawInput::Axis(1), -1.0), LearnProgress::Recorded);
        s.feed(RawInput::Axis(1), 0.0);
        assert_eq!(press(&mut s, RawInput::Axis(1)), LearnProgress::Recorded);
        // press() sends +1.0, so axis 1 positive = down.
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::DPadLeft));
        // The D-pad axis rests at 0 before the first press.
        s.feed(RawInput::Axis(0), 0.0);
        let mut last = LearnProgress::Waiting;
        for (i, input) in [
            RawInput::Axis(0),
            RawInput::Axis(0),
            RawInput::Button(2),
            RawInput::Button(1),
            RawInput::Button(3),
            RawInput::Button(0),
            RawInput::Button(4),
            RawInput::Button(5),
            RawInput::Button(8),
            RawInput::Button(9),
        ]
        .into_iter()
        .enumerate()
        {
            let v = if i == 0 { -1.0 } else { 1.0 };
            last = s.feed(input, v);
            s.feed(input, 0.0);
        }
        assert_eq!(last, LearnProgress::Done);
        assert!(s.current().is_none());
        let m = s.finish();
        let get = |b| m.bindings().iter().find(|(x, _)| *x == b).map(|(_, r)| *r);
        assert_eq!(get(PadButton::DPadUp), Some(RawBinding::AxisNeg(1)));
        assert_eq!(get(PadButton::DPadDown), Some(RawBinding::AxisPos(1)));
        assert_eq!(get(PadButton::DPadLeft), Some(RawBinding::AxisNeg(0)));
        assert_eq!(get(PadButton::DPadRight), Some(RawBinding::AxisPos(0)));
        assert_eq!(get(PadButton::South), Some(RawBinding::Button(2)));
        assert_eq!(get(PadButton::Start), Some(RawBinding::Button(9)));
        assert_eq!(m.bindings().len(), 12);
    }

    #[test]
    fn session_needs_release_between_captures() {
        let mut s = LearnSession::new(PadLayout::Nes);
        assert_eq!(s.feed(RawInput::Button(1), 1.0), LearnProgress::Recorded);
        // Another input pressed while the first is still held: ignored.
        assert_eq!(s.feed(RawInput::Button(2), 1.0), LearnProgress::Waiting);
        s.feed(RawInput::Button(1), 0.0);
        s.feed(RawInput::Button(2), 0.0);
        assert_eq!(s.feed(RawInput::Button(2), 1.0), LearnProgress::Recorded);
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::DPadLeft));
    }

    #[test]
    fn session_refuses_duplicates_until_release() {
        let mut s = LearnSession::new(PadLayout::Nes);
        press(&mut s, RawInput::Button(1));
        assert_eq!(
            s.feed(RawInput::Button(1), 1.0),
            LearnProgress::AlreadyUsed(PadButton::DPadUp)
        );
        // Still held: no repeated refusal, nothing recorded.
        assert_eq!(s.feed(RawInput::Button(1), 1.0), LearnProgress::Waiting);
        s.feed(RawInput::Button(1), 0.0);
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::DPadDown));
        assert_eq!(press(&mut s, RawInput::Button(7)), LearnProgress::Recorded);
    }

    #[test]
    fn session_skip_and_back() {
        let mut s = LearnSession::new(PadLayout::Nes);
        press(&mut s, RawInput::Button(1));
        s.skip();
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::DPadLeft));
        s.back();
        s.back();
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::DPadUp));
        s.back(); // no-op at the start
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::DPadUp));
        // Binding was dropped, so the button can be reused.
        assert_eq!(press(&mut s, RawInput::Button(1)), LearnProgress::Recorded);
        for _ in 0..7 {
            s.skip();
        }
        assert!(s.current().is_none());
        s.skip();
        s.back();
        assert_eq!(s.current().map(|c| c.button), Some(PadButton::Start));
        assert_eq!(
            s.finish().bindings(),
            &[(PadButton::DPadUp, RawBinding::Button(1))]
        );
    }

    #[test]
    fn session_trigger_axis_resting_at_minus_one() {
        let mut s = LearnSession::new(PadLayout::MegaDrive3);
        for _ in 0..4 {
            s.skip();
        }
        // Analog trigger on axis 4, rest at -1: not a press until it moves.
        assert_eq!(s.feed(RawInput::Axis(4), -1.0), LearnProgress::Waiting);
        assert_eq!(s.feed(RawInput::Axis(4), -0.9), LearnProgress::Waiting);
        assert_eq!(s.feed(RawInput::Axis(4), 1.0), LearnProgress::Recorded);
        s.feed(RawInput::Axis(4), -1.0);
        assert_eq!(s.feed(RawInput::Axis(4), -1.0), LearnProgress::Waiting);
        s.skip();
        s.skip();
        s.skip();
        let m = s.finish();
        assert_eq!(m.bindings(), &[(PadButton::West, RawBinding::AxisPos(4))]);
    }

    #[test]
    fn session_hat_axis_rest_zero() {
        let mut s = LearnSession::new(PadLayout::Snes);
        assert_eq!(s.feed(RawInput::Axis(6), 0.0), LearnProgress::Waiting);
        assert_eq!(s.feed(RawInput::Axis(6), -1.0), LearnProgress::Recorded);
        s.feed(RawInput::Axis(6), 0.0);
        assert_eq!(s.feed(RawInput::Axis(6), 1.0), LearnProgress::Recorded);
    }
}
