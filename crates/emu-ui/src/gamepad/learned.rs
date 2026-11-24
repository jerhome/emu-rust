//! Learned pad maps (7.2, docs/design/v7.2-manette-snes.md): raw native inputs -> positional `PadButton`s.
//! Pure logic, no I/O. A0: signatures only; bodies come with G1.
// A0: stubs are not called yet; remove once G3/G4 use them.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;

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
        todo!("G1")
    }

    /// Inverse of `to_token`; `None` for anything else.
    pub fn from_token(token: &str) -> Option<Self> {
        todo!("G1")
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

impl PadLayout {
    /// Prompts in order: D-pad first, then buttons.
    pub fn steps(self) -> &'static [LearnStep] {
        todo!("G1")
    }

    /// `snes` | `nes` | `megadrive3` | `megadrive6`.
    pub fn config_name(self) -> &'static str {
        todo!("G1")
    }

    pub fn from_config_name(s: &str) -> Option<Self> {
        todo!("G1")
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
        todo!("G1")
    }

    pub fn to_config(&self) -> BTreeMap<String, String> {
        todo!("G1")
    }

    pub fn bindings(&self) -> &[(PadButton, RawBinding)] {
        &self.bindings
    }

    pub fn has(&self, button: PadButton) -> bool {
        self.bindings.iter().any(|(b, _)| *b == button)
    }
}

/// Per-pad state turning `Raw` events into `Button` events (axes: threshold 0.5, release below 0.4).
#[derive(Debug, Default)]
pub struct RawTranslator {
    // A0: G1 adds the held-state fields.
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
        todo!("G1")
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
    // A0: G1 adds position, recorded bindings and the released-since-last-capture flag.
}

impl LearnSession {
    pub fn new(layout: PadLayout) -> Self {
        Self { layout }
    }

    /// Current prompt; `None` once every step is done.
    pub fn current(&self) -> Option<&LearnStep> {
        todo!("G1")
    }

    /// Records on the first input crossing 0.5 once every input is released (no double capture).
    pub fn feed(&mut self, input: RawInput, value: f32) -> LearnProgress {
        todo!("G1")
    }

    /// Leaves the current step unbound.
    pub fn skip(&mut self) {
        todo!("G1")
    }

    /// Goes back one step and drops its binding.
    pub fn back(&mut self) {
        todo!("G1")
    }

    pub fn finish(self) -> LearnedMap {
        todo!("G1")
    }
}
