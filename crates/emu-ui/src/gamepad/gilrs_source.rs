//! The only file that touches gilrs (ADR 0016 §2: replaceable by an SDL3 source without other changes).
//!
//! `Gilrs` itself cannot be created in tests (no `/dev/input` in the dev container): the conversion lives in
//! `Translator`, fed with `Raw` values built from gilrs events, and is tested on its own.
//!
//! Raw mode (ADR 0016, 2026-11-23 amendment): what gilrs 0.11.2 really does with a pad absent from its SDL database
//! (read in its sources): `mapping_source()` never returns `MappingSource::None`; such a pad gets a *default*
//! mapping and reports `Driver`. That default mapping names native codes that happen to match the platform's
//! "gamepad" codes (evdev `BTN_SOUTH`..., HID button usages 1..15 on macOS, WGI button indices), everything else
//! comes as `Button::Unknown` / `Axis::Unknown`, always with its native `Code`. A hat whose codes match the default
//! `DPadX/DPadY` is turned into `DPad*` button events by gilrs' default `axis_dpad_to_button` filter (with the
//! `BTN_DPAD_*` codes); otherwise it arrives as an axis in `[-1, 1]`. In raw mode we therefore ignore gilrs' names
//! and only keep `Code::into_u32`: buttons, axes and hats all become `RawInput` whatever gilrs called them.
//!
//! Exception (user decision of 2026-11-24, "guess and offer"): on macOS a `Driver` pad keeps gilrs' guessed names
//! (as in phase 6, so nothing regresses) and is announced `guessed`; the UI offers to learn it.

use std::collections::{HashMap, HashSet};

use gilrs::{Axis, Button, EventType, Gilrs, MappingSource};

use super::mapping::PadButton;
use super::source::{
    GamepadError, GamepadSource, PadAxis, PadEvent, PadEventKind, PadId, RawInput,
};

/// Real pads through gilrs 0.11. Pads without usable names, and models listed by `set_raw_guids`, are read raw
/// (`Connected { raw: true }`, then `PadEventKind::Raw`); macOS guesses are announced `guessed: true`.
pub struct GilrsSource {
    gilrs: Gilrs,
    translator: Translator,
    /// Pads plugged in before creation are announced by the first `poll_events` (gilrs sends no event for them).
    initial_done: bool,
    /// Re-announcements produced by `set_raw_guids`, delivered by the next `poll_events`.
    pending: Vec<PadEvent>,
}

impl GilrsSource {
    /// Fails (never panics) when the platform backend cannot start, e.g. no `/dev/input` in a container.
    pub fn new() -> Result<Self, GamepadError> {
        match Gilrs::new() {
            Ok(gilrs) => Ok(Self {
                gilrs,
                translator: Translator::default(),
                initial_done: false,
                pending: Vec::new(),
            }),
            // `NotImplemented` carries a dummy backend that never reports a pad: treated as unavailable.
            Err(e) => Err(GamepadError::Backend(e.to_string())),
        }
    }

    fn connected(&self, id: gilrs::GamepadId) -> Raw {
        let pad = self.gilrs.gamepad(id);
        let uuid = pad.uuid();
        Raw::Connected {
            name: pad.name().to_owned(),
            guid: guid_hex(uuid),
            trust: mapping_trust(
                pad.mapping_source(),
                Platform::CURRENT,
                pad.button_code(Button::South).is_some(),
                uuid == [0; 16],
            ),
        }
    }
}

impl GamepadSource for GilrsSource {
    fn poll_events(&mut self, out: &mut Vec<PadEvent>) {
        out.append(&mut self.pending);
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

    fn set_raw_guids(&mut self, guids: &[String]) {
        self.translator.set_raw_guids(guids, &mut self.pending);
    }

    fn set_raw_logging(&mut self, pad: Option<PadId>) {
        self.translator.raw_log = pad;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl Platform {
    const CURRENT: Platform = if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Other
    };
}

/// How far gilrs' button names can be trusted for one pad (before learned models are applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trust {
    /// SDL database, or a platform that guarantees a gamepad layout.
    Mapped,
    /// Names guessed by gilrs (macOS): used as is, the UI offers to learn the pad.
    Guessed,
    /// No useful names: raw mode.
    Unmapped,
}

/// gilrs reports `Driver` for every pad missing from its database (`None` is never returned in 0.11.2), so the
/// default mapping is only trusted where the platform itself guarantees a gamepad layout: on Linux when the
/// kernel driver exposes `BTN_SOUTH` (xpad, hid-playstation...: generic HID pads use the joystick range
/// `BTN_TRIGGER..`), on Windows for WGI `Gamepad`-class devices (nil GUID). On macOS the default mapping is a
/// guess on HID button numbers (usages 1..15): kept as a guess rather than going raw, so the pad works at once.
fn mapping_trust(
    source: MappingSource,
    platform: Platform,
    has_native_south: bool,
    nil_uuid: bool,
) -> Trust {
    let trusted = |ok: bool| if ok { Trust::Mapped } else { Trust::Unmapped };
    match source {
        MappingSource::SdlMappings => Trust::Mapped,
        MappingSource::None => Trust::Unmapped,
        MappingSource::Driver => match platform {
            Platform::Linux => trusted(has_native_south),
            Platform::Windows => trusted(nil_uuid),
            Platform::MacOs => Trust::Guessed,
            Platform::Other => Trust::Unmapped,
        },
    }
}

/// A gilrs event reduced to what the translation needs. `u32` fields are `Code::into_u32` (raw mode).
#[derive(Debug, Clone, PartialEq)]
enum Raw {
    Connected {
        name: String,
        guid: String,
        trust: Trust,
    },
    Disconnected,
    Button(Button, bool, u32),
    /// Analog value of a button, in `[0, 1]` (gilrs reports L2/R2 this way).
    ButtonValue(Button, f32),
    Axis(Axis, f32, u32),
    Ignored,
}

impl Raw {
    fn from_event(event: EventType) -> Self {
        match event {
            EventType::ButtonPressed(b, c) => Raw::Button(b, true, c.into_u32()),
            EventType::ButtonReleased(b, c) => Raw::Button(b, false, c.into_u32()),
            EventType::ButtonChanged(b, v, _) => Raw::ButtonValue(b, v),
            EventType::AxisChanged(a, v, c) => Raw::Axis(a, v, c.into_u32()),
            EventType::Disconnected => Raw::Disconnected,
            // Connected needs the gamepad handle (see `GilrsSource::connected`); the rest carries no state.
            _ => Raw::Ignored,
        }
    }
}

#[derive(Debug, Clone)]
struct KnownPad {
    name: String,
    guid: String,
    /// What the database says, before `raw_guids`.
    trust: Trust,
    /// Mode announced in the last `Connected`.
    raw: bool,
}

impl KnownPad {
    fn is_raw(&self, raw_guids: &HashSet<String>) -> bool {
        self.trust == Trust::Unmapped || raw_guids.contains(&self.guid)
    }

    fn connected(&self) -> PadEventKind {
        PadEventKind::Connected {
            name: self.name.clone(),
            guid: self.guid.clone(),
            raw: self.raw,
            guessed: !self.raw && self.trust == Trust::Guessed,
        }
    }
}

/// Keeps track of announced pads; turns `Raw` into normalised events.
#[derive(Debug, Default)]
struct Translator {
    known: HashMap<PadId, KnownPad>,
    /// Learned models: read raw even when the database maps them.
    raw_guids: HashSet<String>,
    /// Pad whose raw codes are logged at `debug` (learning screen only).
    raw_log: Option<PadId>,
}

impl Translator {
    fn translate(&mut self, id: PadId, raw: Raw, out: &mut Vec<PadEvent>) {
        let kind = match raw {
            Raw::Connected { name, guid, trust } => {
                if self.known.contains_key(&id) {
                    return; // duplicate announcement (initial scan + event)
                }
                let mut pad = KnownPad {
                    name,
                    guid,
                    trust,
                    raw: false,
                };
                pad.raw = pad.is_raw(&self.raw_guids);
                let mode = match (pad.raw, trust) {
                    (true, _) => " (mode brut)",
                    (false, Trust::Guessed) => " (noms devinés)",
                    _ => "",
                };
                tracing::info!("manette détectée{mode} : {:?} ({})", pad.name, pad.guid);
                let kind = pad.connected();
                self.known.insert(id, pad);
                kind
            }
            Raw::Disconnected => {
                if self.known.remove(&id).is_none() {
                    return;
                }
                PadEventKind::Disconnected
            }
            Raw::Ignored => return,
            other => {
                let Some(pad) = self.known.get(&id) else {
                    return;
                };
                let converted = if pad.raw {
                    self.convert_raw(id, other)
                } else {
                    convert_mapped(other)
                };
                let Some(kind) = converted else {
                    return;
                };
                kind
            }
        };
        out.push(PadEvent { id, kind });
    }

    /// Raw mode: gilrs' names are ignored, only native codes remain. `ButtonValue` is skipped: it duplicates
    /// `Pressed`/`Released` and carries no code.
    fn convert_raw(&self, id: PadId, raw: Raw) -> Option<PadEventKind> {
        let (input, value) = match raw {
            Raw::Button(_, pressed, code) => {
                (RawInput::Button(code), if pressed { 1.0 } else { 0.0 })
            }
            Raw::Axis(_, value, code) => (RawInput::Axis(code), value),
            _ => return None,
        };
        if self.raw_log == Some(id) {
            tracing::debug!("entrée brute : {input:?} = {value}");
        }
        Some(PadEventKind::Raw { input, value })
    }

    /// Applies the learned models; pads whose mode changes are re-announced (`Disconnected` then `Connected`).
    fn set_raw_guids(&mut self, guids: &[String], out: &mut Vec<PadEvent>) {
        self.raw_guids = guids.iter().cloned().collect();
        let mut ids: Vec<PadId> = self.known.keys().copied().collect();
        ids.sort();
        for id in ids {
            let Some(pad) = self.known.get_mut(&id) else {
                continue;
            };
            let raw = pad.is_raw(&self.raw_guids);
            if raw == pad.raw {
                continue;
            }
            pad.raw = raw;
            tracing::info!(
                "manette réannoncée ({}) : {:?} ({})",
                if raw { "mode brut" } else { "noms de gilrs" },
                pad.name,
                pad.guid
            );
            out.push(PadEvent {
                id,
                kind: PadEventKind::Disconnected,
            });
            out.push(PadEvent {
                id,
                kind: pad.connected(),
            });
        }
    }
}

fn convert_mapped(raw: Raw) -> Option<PadEventKind> {
    match raw {
        Raw::Button(b, pressed, _) => {
            // Triggers are analog: their value (`ButtonValue`) decides, with our threshold.
            if matches!(b, Button::LeftTrigger2 | Button::RightTrigger2) {
                return None;
            }
            let button = convert_button(b)?;
            Some(PadEventKind::Button { button, pressed })
        }
        Raw::ButtonValue(b, value) => {
            let axis = match b {
                Button::LeftTrigger2 => PadAxis::L2,
                Button::RightTrigger2 => PadAxis::R2,
                _ => return None,
            };
            Some(PadEventKind::Axis { axis, value })
        }
        Raw::Axis(a, value, _) => {
            let axis = convert_axis(a)?;
            Some(PadEventKind::Axis { axis, value })
        }
        _ => None,
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
    const DS4_GUID: &str = "030000004c050000c405000000000000";
    const SNES_GUID: &str = "03000000790000001100000000000000";

    fn connect(mapped: bool) -> Raw {
        Raw::Connected {
            name: "Sony DualShock 4".into(),
            guid: DS4_GUID.into(),
            trust: if mapped {
                Trust::Mapped
            } else {
                Trust::Unmapped
            },
        }
    }

    /// Same SNES pad seen on macOS: absent from the database, gilrs guesses the names.
    fn snes_mac() -> Raw {
        Raw::Connected {
            name: "USB Gamepad".into(),
            guid: SNES_GUID.into(),
            trust: Trust::Guessed,
        }
    }

    fn snes() -> Raw {
        Raw::Connected {
            name: "USB Gamepad".into(),
            guid: SNES_GUID.into(),
            trust: Trust::Unmapped,
        }
    }

    fn feed(t: &mut Translator, raws: Vec<(PadId, Raw)>) -> Vec<PadEvent> {
        let mut out = Vec::new();
        for (id, raw) in raws {
            t.translate(id, raw, &mut out);
        }
        out
    }

    fn run(raws: Vec<(PadId, Raw)>) -> Vec<PadEvent> {
        feed(&mut Translator::default(), raws)
    }

    fn kinds(events: &[PadEvent]) -> Vec<PadEventKind> {
        events.iter().map(|e| e.kind.clone()).collect()
    }

    fn raw_ev(input: RawInput, value: f32) -> PadEventKind {
        PadEventKind::Raw { input, value }
    }

    fn connected(name: &str, guid: &str, raw: bool) -> PadEventKind {
        PadEventKind::Connected {
            name: name.into(),
            guid: guid.into(),
            raw,
            guessed: false,
        }
    }

    fn guessed(name: &str, guid: &str) -> PadEventKind {
        PadEventKind::Connected {
            name: name.into(),
            guid: guid.into(),
            raw: false,
            guessed: true,
        }
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
    fn unmapped_pad_is_announced_raw_and_sends_codes() {
        // DragonRise-like: cross on two axes, hat turned into D-pad buttons by gilrs, unknown buttons.
        let out = run(vec![
            (A, snes()),
            (A, Raw::Axis(Axis::LeftStickX, -1.0, 0x3_0000)),
            (A, Raw::Axis(Axis::Unknown, 0.004, 0x3_0001)),
            (A, Raw::Button(Button::Unknown, true, 0x1_0122)),
            (A, Raw::ButtonValue(Button::Unknown, 1.0)), // duplicate of the press, no code: dropped
            (A, Raw::Button(Button::DPadUp, true, 0x1_0220)),
            (A, Raw::Button(Button::LeftTrigger2, false, 7)), // raw: no trigger special case
            (A, Raw::Ignored),
            (A, Raw::Disconnected),
            (A, Raw::Axis(Axis::LeftStickX, 1.0, 0x3_0000)), // after disconnection: dropped
        ]);
        assert_eq!(
            kinds(&out),
            vec![
                connected("USB Gamepad", SNES_GUID, true),
                raw_ev(RawInput::Axis(0x3_0000), -1.0),
                raw_ev(RawInput::Axis(0x3_0001), 0.004),
                raw_ev(RawInput::Button(0x1_0122), 1.0),
                raw_ev(RawInput::Button(0x1_0220), 1.0),
                raw_ev(RawInput::Button(7), 0.0),
                PadEventKind::Disconnected,
            ]
        );
    }

    #[test]
    fn mapped_pad_events_are_converted() {
        let out = run(vec![
            (A, connect(true)),
            (A, connect(true)), // duplicate announcement (initial scan + event) is dropped
            (A, Raw::Button(Button::South, true, 1)),
            (A, Raw::Button(Button::Mode, false, 2)),
            (A, Raw::Button(Button::RightThumb, true, 3)),
            (A, Raw::Button(Button::C, true, 4)),
            (A, Raw::Button(Button::LeftTrigger2, true, 5)),
            (A, Raw::ButtonValue(Button::LeftTrigger2, 0.7)),
            (A, Raw::ButtonValue(Button::South, 1.0)),
            (A, Raw::Axis(Axis::LeftStickY, -0.8, 6)),
            (A, Raw::Axis(Axis::DPadX, 1.0, 7)),
            (A, Raw::Ignored),
            (A, Raw::Disconnected),
            (A, Raw::Disconnected),
        ]);
        assert_eq!(
            kinds(&out),
            vec![
                connected("Sony DualShock 4", DS4_GUID, false),
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
            (A, Raw::Button(Button::South, true, 1)),
            (PadId(1), connect(true)),
            (A, Raw::Axis(Axis::LeftStickX, 1.0, 2)),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, PadId(1));
    }

    #[test]
    fn learned_guid_switches_a_mapped_pad_to_raw_and_back() {
        let b = PadId(1);
        let mut t = Translator::default();
        let mut out = feed(&mut t, vec![(A, connect(true)), (b, snes())]);
        out.clear();

        t.set_raw_guids(&[DS4_GUID.into(), SNES_GUID.into()], &mut out);
        // Only the DS4 changes mode; the SNES pad was already raw.
        assert_eq!(
            kinds(&out),
            vec![
                PadEventKind::Disconnected,
                connected("Sony DualShock 4", DS4_GUID, true),
            ]
        );
        assert!(out.iter().all(|e| e.id == A));

        let out = feed(&mut t, vec![(A, Raw::Button(Button::South, true, 0x130))]);
        assert_eq!(kinds(&out), vec![raw_ev(RawInput::Button(0x130), 1.0)]);

        let mut out = Vec::new();
        t.set_raw_guids(&[DS4_GUID.into()], &mut out);
        assert!(out.is_empty(), "same mode, no re-announcement: {out:?}");

        t.set_raw_guids(&[], &mut out);
        assert_eq!(
            kinds(&out),
            vec![
                PadEventKind::Disconnected,
                connected("Sony DualShock 4", DS4_GUID, false),
            ]
        );
    }

    #[test]
    fn learned_guid_applies_at_connection() {
        let mut t = Translator::default();
        let mut out = Vec::new();
        t.set_raw_guids(&[DS4_GUID.into()], &mut out);
        assert!(out.is_empty());
        let out = feed(&mut t, vec![(A, connect(true))]);
        assert_eq!(
            kinds(&out),
            vec![connected("Sony DualShock 4", DS4_GUID, true)]
        );
    }

    #[test]
    fn mapping_trust_per_platform() {
        use MappingSource::{Driver, None as NoMap, SdlMappings};
        use Trust::{Guessed, Mapped, Unmapped};
        for p in [
            Platform::Linux,
            Platform::Windows,
            Platform::MacOs,
            Platform::Other,
        ] {
            assert_eq!(mapping_trust(SdlMappings, p, false, false), Mapped);
            assert_eq!(mapping_trust(NoMap, p, true, true), Unmapped);
        }
        assert_eq!(mapping_trust(Driver, Platform::Linux, true, false), Mapped);
        assert_eq!(
            mapping_trust(Driver, Platform::Linux, false, false),
            Unmapped
        );
        assert_eq!(
            mapping_trust(Driver, Platform::Windows, false, true),
            Mapped
        );
        assert_eq!(
            mapping_trust(Driver, Platform::Windows, true, false),
            Unmapped
        );
        assert_eq!(mapping_trust(Driver, Platform::MacOs, true, true), Guessed);
        assert_eq!(
            mapping_trust(Driver, Platform::MacOs, false, false),
            Guessed
        );
        assert_eq!(mapping_trust(Driver, Platform::Other, true, true), Unmapped);
    }

    #[test]
    fn guessed_pad_keeps_names_until_learned() {
        let mut t = Translator::default();
        let out = feed(
            &mut t,
            vec![
                (A, snes_mac()),
                (A, Raw::Button(Button::South, true, 0x9_0002)),
            ],
        );
        assert_eq!(
            kinds(&out),
            vec![
                guessed("USB Gamepad", SNES_GUID),
                PadEventKind::Button {
                    button: PadButton::South,
                    pressed: true
                },
            ]
        );

        let mut out = Vec::new();
        t.set_raw_guids(&[SNES_GUID.into()], &mut out);
        assert_eq!(
            kinds(&out),
            vec![
                PadEventKind::Disconnected,
                connected("USB Gamepad", SNES_GUID, true),
            ]
        );
        let out = feed(
            &mut t,
            vec![(A, Raw::Button(Button::South, false, 0x9_0002))],
        );
        assert_eq!(kinds(&out), vec![raw_ev(RawInput::Button(0x9_0002), 0.0)]);

        let mut out = Vec::new();
        t.set_raw_guids(&[], &mut out);
        assert_eq!(
            kinds(&out),
            vec![
                PadEventKind::Disconnected,
                guessed("USB Gamepad", SNES_GUID)
            ]
        );
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
