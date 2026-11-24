//! Normalised gamepad events and the injectable source trait.

use std::collections::VecDeque;

use super::mapping::PadButton;

/// Identity of a connected pad, stable while it stays connected (not across reconnections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PadId(pub u32);

/// Analog axes, positional. Sticks in `[-1, 1]` (Y positive = up), triggers in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    L2,
    R2,
}

/// Native input of a pad read without gilrs' names. Codes are opaque, stable for one model on one platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawInput {
    Button(u32),
    Axis(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PadEventKind {
    /// `guid` is the lowercase hex mapping GUID (same for identical models); only name and GUID may be logged.
    /// `raw = true`: this pad's inputs arrive as `Raw` (unmapped model, or a learned map overrides the database).
    Connected {
        name: String,
        guid: String,
        raw: bool,
    },
    Disconnected,
    Button {
        button: PadButton,
        pressed: bool,
    },
    Axis {
        axis: PadAxis,
        value: f32,
    },
    /// Buttons: 0.0 / 1.0; axes and hats: [-1, 1].
    Raw {
        input: RawInput,
        value: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PadEvent {
    pub id: PadId,
    pub kind: PadEventKind,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GamepadError {
    /// The platform backend could not start (no `/dev/input`, no udev...); the app continues without a pad.
    #[error("gamepad backend unavailable: {0}")]
    Backend(String),
}

/// Source of normalised events: real (`GilrsSource`) or scripted (tests). Never blocks, never panics.
pub trait GamepadSource {
    /// Appends the events received since the previous call. A pad without button mapping is announced as
    /// `Connected { raw: true }` and then sends `Raw` events (7.2; it used to be ignored).
    /// A source also reports pads already plugged in at creation as `Connected` on its first call.
    fn poll_events(&mut self, out: &mut Vec<PadEvent>);

    /// Models (GUIDs) to read raw even when the database maps them; a pad already plugged in is re-announced
    /// (`Disconnected` then `Connected { raw }`). Default: ignored (scripted sources).
    fn set_raw_guids(&mut self, _guids: &[String]) {}

    /// Learning screen: native codes of `pad` are logged at `debug` (never otherwise). Default: ignored.
    fn set_raw_logging(&mut self, _pad: Option<PadId>) {}
}

impl<S: GamepadSource + ?Sized> GamepadSource for Box<S> {
    fn poll_events(&mut self, out: &mut Vec<PadEvent>) {
        (**self).poll_events(out);
    }

    fn set_raw_guids(&mut self, guids: &[String]) {
        (**self).set_raw_guids(guids);
    }

    fn set_raw_logging(&mut self, pad: Option<PadId>) {
        (**self).set_raw_logging(pad);
    }
}

/// Deterministic source for tests: each `poll_events` call delivers the next queued batch (empty when exhausted).
#[derive(Debug, Default, Clone)]
pub struct ScriptedPadSource {
    batches: VecDeque<Vec<PadEvent>>,
}

impl ScriptedPadSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one batch, delivered by one `poll_events` call.
    pub fn push_batch(&mut self, events: Vec<PadEvent>) {
        self.batches.push_back(events);
    }
}

impl GamepadSource for ScriptedPadSource {
    fn poll_events(&mut self, out: &mut Vec<PadEvent>) {
        if let Some(batch) = self.batches.pop_front() {
            out.extend(batch);
        }
    }
}
