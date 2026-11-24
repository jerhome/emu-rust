//! The only file that touches gilrs (ADR 0016 §2: replaceable by an SDL3 source without other changes).
//!
//! `Gilrs` itself cannot be created in tests (no `/dev/input` in the dev container): the conversion lives in
//! `Translator`, fed with `Raw` values built from gilrs events, and is tested on its own.

use std::collections::HashSet;

use gilrs::{Axis, Button, EventType, Gilrs, MappingSource};

use super::mapping::PadButton;
use super::source::{GamepadError, GamepadSource, PadAxis, PadEvent, PadEventKind, PadId};

/// Real pads through gilrs 0.11. Pads with `MappingSource::None` are ignored with a `warn!`.
pub struct GilrsSource {
    gilrs: Gilrs,
    translator: Translator,
    /// Pads plugged in before creation are announced by the first `poll_events` (gilrs sends no event for them).
    initial_done: bool,
}

impl GilrsSource {
    /// Fails (never panics) when the platform backend cannot start, e.g. no `/dev/input` in a container.
    pub fn new() -> Result<Self, GamepadError> {
        match Gilrs::new() {
            Ok(gilrs) => Ok(Self {
                gilrs,
                translator: Translator::default(),
                initial_done: false,
            }),
            // `NotImplemented` carries a dummy backend that never reports a pad: treated as unavailable.
            Err(e) => Err(GamepadError::Backend(e.to_string())),
        }
    }

    fn connected(&self, id: gilrs::GamepadId) -> Raw {
        let pad = self.gilrs.gamepad(id);
        Raw::Connected {
            name: pad.name().to_owned(),
            guid: guid_hex(pad.uuid()),
            mapped: pad.mapping_source() != MappingSource::None,
        }
    }
}

impl GamepadSource for GilrsSource {
    fn poll_events(&mut self, out: &mut Vec<PadEvent>) {
        if !self.initial_done {
            self.initial_done = true;
            let present: Vec<_> = self.gilrs.gamepads().map(|(id, _)| id).collect();
            for id in present {
                let raw = self.connected(id);
                self.translator.translate(pad_id(id), raw, out);
            }
        }
        while let Some(event) = self.gilrs.next_event() {
            let raw = match event.event {
                EventType::Connected => self.connected(event.id),
                other => Raw::from_event(other),
            };
            self.translator.translate(pad_id(event.id), raw, out);
        }
    }
}

fn pad_id(id: gilrs::GamepadId) -> PadId {
    PadId(u32::try_from(usize::from(id)).unwrap_or(u32::MAX))
}

/// Lowercase hex of the mapping GUID (same text as SDL_GameControllerDB and `[input.gamepad.devices]`).
fn guid_hex(uuid: [u8; 16]) -> String {
    use std::fmt::Write;
    uuid.iter().fold(String::with_capacity(32), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// A gilrs event reduced to what the translation needs.
#[derive(Debug, Clone, PartialEq)]
enum Raw {
    Connected {
        name: String,
        guid: String,
        /// `false` for `MappingSource::None`: raw indices, no reliable button names.
        mapped: bool,
    },
    Disconnected,
    Button(Button, bool),
    /// Analog value of a button, in `[0, 1]` (gilrs reports L2/R2 this way).
    ButtonValue(Button, f32),
    Axis(Axis, f32),
    Ignored,
}

impl Raw {
    fn from_event(event: EventType) -> Self {
        match event {
            EventType::ButtonPressed(b, _) => Raw::Button(b, true),
            EventType::ButtonReleased(b, _) => Raw::Button(b, false),
            EventType::ButtonChanged(b, v, _) => Raw::ButtonValue(b, v),
            EventType::AxisChanged(a, v, _) => Raw::Axis(a, v),
            EventType::Disconnected => Raw::Disconnected,
            // Connected needs the gamepad handle (see `GilrsSource::connected`); the rest carries no state.
            _ => Raw::Ignored,
        }
    }
}

/// Keeps track of announced and ignored pads; turns `Raw` into normalised events.
#[derive(Debug, Default)]
struct Translator {
    known: HashSet<PadId>,
    ignored: HashSet<PadId>,
}

impl Translator {
    fn translate(&mut self, id: PadId, raw: Raw, out: &mut Vec<PadEvent>) {
        let kind = match raw {
            Raw::Connected { name, guid, mapped } => {
                if !mapped {
                    if self.ignored.insert(id) {
                        tracing::warn!(
                            "manette sans correspondance de boutons ignorée : {name:?} ({guid})"
                        );
                    }
                    return;
                }
                self.ignored.remove(&id);
                if !self.known.insert(id) {
                    return;
                }
                tracing::info!("manette détectée : {name:?} ({guid})");
                PadEventKind::Connected {
                    name,
                    guid,
                    raw: false,
                }
            }
            Raw::Disconnected => {
                self.ignored.remove(&id);
                if !self.known.remove(&id) {
                    return;
                }
                PadEventKind::Disconnected
            }
            _ if !self.known.contains(&id) => return,
            Raw::Button(b, pressed) => {
                // Triggers are analog: their value (`ButtonValue`) decides, with our threshold.
                if matches!(b, Button::LeftTrigger2 | Button::RightTrigger2) {
                    return;
                }
                let Some(button) = convert_button(b) else {
                    return;
                };
                PadEventKind::Button { button, pressed }
            }
            Raw::ButtonValue(b, value) => {
                let axis = match b {
                    Button::LeftTrigger2 => PadAxis::L2,
                    Button::RightTrigger2 => PadAxis::R2,
                    _ => return,
                };
                PadEventKind::Axis { axis, value }
            }
            Raw::Axis(a, value) => {
                let Some(axis) = convert_axis(a) else {
                    return;
                };
                PadEventKind::Axis { axis, value }
            }
            Raw::Ignored => return,
        };
        out.push(PadEvent { id, kind });
    }
}

fn convert_button(b: Button) -> Option<PadButton> {
    Some(match b {
        Button::South => PadButton::South,
        Button::East => PadButton::East,
        Button::West => PadButton::West,
        Button::North => PadButton::North,
        Button::LeftTrigger => PadButton::L1,
        Button::RightTrigger => PadButton::R1,
        Button::LeftTrigger2 => PadButton::L2,
        Button::RightTrigger2 => PadButton::R2,
        Button::Select => PadButton::Select,
        Button::Start => PadButton::Start,
        Button::Mode => PadButton::Mode,
        Button::LeftThumb => PadButton::L3,
        Button::RightThumb => PadButton::R3,
        Button::DPadUp => PadButton::DPadUp,
        Button::DPadDown => PadButton::DPadDown,
        Button::DPadLeft => PadButton::DPadLeft,
        Button::DPadRight => PadButton::DPadRight,
        _ => return None,
    })
}

/// gilrs sticks already have Y positive = up, like `PadAxis`. Hat axes become D-pad buttons in gilrs' default
/// filters, so `DPadX/DPadY` are not needed here.
fn convert_axis(a: Axis) -> Option<PadAxis> {
    Some(match a {
        Axis::LeftStickX => PadAxis::LeftX,
        Axis::LeftStickY => PadAxis::LeftY,
        Axis::RightStickX => PadAxis::RightX,
        Axis::RightStickY => PadAxis::RightY,
        Axis::LeftZ => PadAxis::L2,
        Axis::RightZ => PadAxis::R2,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PadId = PadId(0);

    fn connect(mapped: bool) -> Raw {
        Raw::Connected {
            name: "Sony DualShock 4".into(),
            guid: "030000004c050000c405000000000000".into(),
            mapped,
        }
    }

    fn run(raws: Vec<(PadId, Raw)>) -> Vec<PadEvent> {
        let mut t = Translator::default();
        let mut out = Vec::new();
        for (id, raw) in raws {
            t.translate(id, raw, &mut out);
        }
        out
    }

    fn kinds(events: &[PadEvent]) -> Vec<PadEventKind> {
        events.iter().map(|e| e.kind.clone()).collect()
    }

    #[test]
    fn guid_is_lowercase_hex() {
        let mut uuid = [0u8; 16];
        uuid[0] = 0x03;
        uuid[4] = 0x4c;
        uuid[5] = 0x05;
        uuid[8] = 0xC4;
        assert_eq!(guid_hex(uuid), "030000004c050000c400000000000000");
    }

    #[test]
    fn unmapped_pad_emits_nothing() {
        let out = run(vec![
            (A, connect(false)),
            (A, Raw::Button(Button::South, true)),
            (A, Raw::Axis(Axis::LeftStickX, 1.0)),
            (A, Raw::Disconnected),
        ]);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn mapped_pad_events_are_converted() {
        let out = run(vec![
            (A, connect(true)),
            (A, connect(true)), // duplicate announcement (initial scan + event) is dropped
            (A, Raw::Button(Button::South, true)),
            (A, Raw::Button(Button::Mode, false)),
            (A, Raw::Button(Button::RightThumb, true)),
            (A, Raw::Button(Button::C, true)),
            (A, Raw::Button(Button::LeftTrigger2, true)),
            (A, Raw::ButtonValue(Button::LeftTrigger2, 0.7)),
            (A, Raw::ButtonValue(Button::South, 1.0)),
            (A, Raw::Axis(Axis::LeftStickY, -0.8)),
            (A, Raw::Axis(Axis::DPadX, 1.0)),
            (A, Raw::Ignored),
            (A, Raw::Disconnected),
            (A, Raw::Disconnected),
        ]);
        assert_eq!(
            kinds(&out),
            vec![
                PadEventKind::Connected {
                    name: "Sony DualShock 4".into(),
                    guid: "030000004c050000c405000000000000".into(),
                    raw: false,
                },
                PadEventKind::Button {
                    button: PadButton::South,
                    pressed: true
                },
                PadEventKind::Button {
                    button: PadButton::Mode,
                    pressed: false
                },
                PadEventKind::Button {
                    button: PadButton::R3,
                    pressed: true
                },
                PadEventKind::Axis {
                    axis: PadAxis::L2,
                    value: 0.7
                },
                PadEventKind::Axis {
                    axis: PadAxis::LeftY,
                    value: -0.8
                },
                PadEventKind::Disconnected,
            ]
        );
        assert!(out.iter().all(|e| e.id == A));
    }

    #[test]
    fn events_of_unknown_pads_are_dropped() {
        let out = run(vec![
            (A, Raw::Button(Button::South, true)),
            (PadId(1), connect(true)),
            (A, Raw::Axis(Axis::LeftStickX, 1.0)),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, PadId(1));
    }

    #[test]
    fn every_positional_button_has_a_gilrs_source() {
        let all = [
            Button::South,
            Button::East,
            Button::West,
            Button::North,
            Button::LeftTrigger,
            Button::RightTrigger,
            Button::LeftTrigger2,
            Button::RightTrigger2,
            Button::Select,
            Button::Start,
            Button::Mode,
            Button::LeftThumb,
            Button::RightThumb,
            Button::DPadUp,
            Button::DPadDown,
            Button::DPadLeft,
            Button::DPadRight,
        ];
        let converted: HashSet<_> = all.into_iter().filter_map(convert_button).collect();
        assert_eq!(converted.len(), PadButton::ALL.len());
        assert_eq!(convert_button(Button::Unknown), None);
        assert_eq!(convert_axis(Axis::Unknown), None);
        assert_eq!(convert_axis(Axis::RightZ), Some(PadAxis::R2));
    }
}
