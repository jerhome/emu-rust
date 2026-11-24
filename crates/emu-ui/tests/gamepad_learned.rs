//! Raw pads and learned maps in `GamepadInput` (7.2, docs/design/v7.2-manette-snes.md §6), public API only.
//! Headless: scripted events, gilrs is never initialised.

use std::collections::BTreeMap;

use emu_runtime::{InputBackend, InputFrame, JoypadButton as J};
use emu_ui::{
    GamepadInput, GamepadNotice, GamepadSettings, HotkeyAction, LearnedMap, NavKey, PadButton as P,
    PadEvent, PadEventKind, PadId, PadMap, PadStatus, PadStyle, RawInput, ScriptedPadSource,
    UnrecognizedPad,
};

const DS4_ID: PadId = PadId(1);
const SNES_ID: PadId = PadId(2);
const DS4: &str = "030000004c050000c405000000000000";
const SNES: &str = "03000000790000001100000000000000";

fn ev(id: PadId, kind: PadEventKind) -> PadEvent {
    PadEvent { id, kind }
}
fn connect(id: PadId, name: &str, guid: &str, raw: bool, guessed: bool) -> PadEvent {
    ev(
        id,
        PadEventKind::Connected {
            name: name.into(),
            guid: guid.into(),
            raw,
            guessed,
        },
    )
}
fn ds4() -> PadEvent {
    connect(DS4_ID, "Wireless Controller", DS4, false, false)
}
fn snes_raw() -> PadEvent {
    connect(SNES_ID, "USB Gamepad", SNES, true, false)
}
fn disconnect(id: PadId) -> PadEvent {
    ev(id, PadEventKind::Disconnected)
}
fn raw(id: PadId, input: RawInput, value: f32) -> PadEvent {
    ev(id, PadEventKind::Raw { input, value })
}
fn btn(id: PadId, button: P, pressed: bool) -> PadEvent {
    ev(id, PadEventKind::Button { button, pressed })
}

/// DragonRise-like SNES pad: cross on axes 0/1, buttons b0..b9.
fn snes_map() -> LearnedMap {
    let cfg: BTreeMap<String, String> = [
        ("dpad_up", "-a1"),
        ("dpad_down", "+a1"),
        ("dpad_left", "-a0"),
        ("dpad_right", "+a0"),
        ("south", "b2"),
        ("east", "b1"),
        ("west", "b3"),
        ("north", "b0"),
        ("l1", "b4"),
        ("r1", "b5"),
        ("select", "b8"),
        ("start", "b9"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    LearnedMap::from_config(&cfg)
}

fn maps() -> BTreeMap<String, LearnedMap> {
    BTreeMap::from([(SNES.to_owned(), snes_map())])
}

fn input(batches: Vec<Vec<PadEvent>>) -> GamepadInput<ScriptedPadSource> {
    let mut source = ScriptedPadSource::new();
    for batch in batches {
        source.push_batch(batch);
    }
    GamepadInput::new(
        source,
        PadMap::default_for(PadStyle::Snes),
        GamepadSettings::default(),
    )
}

fn poll(input: &mut impl InputBackend) -> InputFrame {
    let mut frame = InputFrame::default();
    input.poll(0, &mut frame);
    frame
}

fn pressed(frame: &InputFrame, port: usize) -> Vec<J> {
    J::ALL
        .into_iter()
        .filter(|b| frame.pads[port].is_pressed(*b))
        .collect()
}

#[test]
fn unlearned_raw_pad_takes_no_port_and_reaches_nothing() {
    let mut i = input(vec![
        vec![snes_raw(), raw(SNES_ID, RawInput::Button(2), 1.0)],
        vec![ds4(), raw(SNES_ID, RawInput::Axis(0), 1.0)],
    ]);
    let f = poll(&mut i);
    assert_eq!(f.pads[0].bits(), 0);
    assert!(i.take_notices().is_empty());
    assert_eq!(
        i.take_unrecognized(),
        vec![UnrecognizedPad {
            id: SNES_ID,
            name: "USB Gamepad".into(),
            guid: SNES.into(),
        }]
    );
    // The DS4 plugged in afterwards still gets port 0.
    let f = poll(&mut i);
    assert_eq!(f.pads[0].bits(), 0);
    assert_eq!(
        i.take_notices(),
        vec![GamepadNotice::Connected {
            name: "Wireless Controller".into(),
            port: 0
        }]
    );
    assert!(
        i.take_unrecognized().is_empty(),
        "one notice per connection"
    );
    let pads = i.pads();
    assert_eq!(pads.len(), 2);
    assert_eq!((pads[0].port, pads[0].status), (Some(0), PadStatus::Mapped));
    assert_eq!(
        (pads[1].port, pads[1].status),
        (None, PadStatus::Unrecognized)
    );
    assert_eq!(pads[1].guid, SNES);
}

#[test]
fn learned_pad_is_translated_next_to_the_ds4() {
    let mut i = input(vec![
        vec![ds4(), snes_raw()],
        vec![
            raw(SNES_ID, RawInput::Axis(1), -1.0),
            raw(SNES_ID, RawInput::Axis(0), 0.004),
            raw(SNES_ID, RawInput::Button(2), 1.0),
            raw(SNES_ID, RawInput::Button(1), 1.0),
            raw(SNES_ID, RawInput::Button(7), 1.0), // not learned: ignored
            btn(DS4_ID, P::East, true),
        ],
        vec![
            raw(SNES_ID, RawInput::Axis(1), 0.0),
            raw(SNES_ID, RawInput::Button(2), 0.0),
        ],
    ]);
    i.set_learned_maps(maps());
    poll(&mut i);
    let f = poll(&mut i);
    assert_eq!(pressed(&f, 0), vec![J::A]);
    assert_eq!(pressed(&f, 1), vec![J::B, J::Up, J::A]);
    assert!(
        i.take_unrecognized().is_empty(),
        "learned: not unrecognized"
    );
    let pads = i.pads();
    assert_eq!(
        (pads[1].port, pads[1].status),
        (Some(1), PadStatus::Learned)
    );

    let f = poll(&mut i);
    assert_eq!(pressed(&f, 1), vec![J::A]);
}

#[test]
fn learned_pad_uses_select_as_hotkey_button() {
    let mut i = input(vec![
        vec![
            ds4(),
            snes_raw(),
            raw(SNES_ID, RawInput::Button(8), 1.0),
            raw(SNES_ID, RawInput::Button(9), 1.0),
            raw(SNES_ID, RawInput::Button(5), 1.0),
            btn(DS4_ID, P::Select, true), // Share alone: no hotkey on the DS4 (PS is its modifier)
            btn(DS4_ID, P::Start, true),
        ],
        vec![
            raw(SNES_ID, RawInput::Button(8), 0.0),
            raw(SNES_ID, RawInput::Button(9), 0.0),
            raw(SNES_ID, RawInput::Button(5), 0.0),
            raw(SNES_ID, RawInput::Button(8), 1.0),
        ],
    ]);
    i.set_learned_maps(maps());
    let f = poll(&mut i);
    assert_eq!(
        i.take_hotkeys(),
        vec![HotkeyAction::Menu, HotkeyAction::SaveState]
    );
    assert!(
        pressed(&f, 1).is_empty(),
        "hotkey mode: the core sees nothing"
    );
    // Select alone still plays.
    let f = poll(&mut i);
    assert_eq!(pressed(&f, 1), vec![J::Select]);
}

#[test]
fn learning_while_plugged_attaches_and_forgetting_detaches() {
    let mut i = input(vec![
        vec![snes_raw()],
        vec![],
        vec![raw(SNES_ID, RawInput::Button(9), 1.0)],
        vec![],
    ]);
    poll(&mut i);
    assert_eq!(i.take_unrecognized().len(), 1);
    i.set_learned_maps(maps());
    assert_eq!(
        i.take_notices(),
        vec![GamepadNotice::Connected {
            name: "USB Gamepad".into(),
            port: 0
        }]
    );
    poll(&mut i);
    let f = poll(&mut i);
    assert_eq!(pressed(&f, 0), vec![J::Start]);

    i.set_learned_maps(BTreeMap::new());
    assert_eq!(
        i.take_notices(),
        vec![GamepadNotice::Disconnected { port: 0 }]
    );
    let f = poll(&mut i);
    assert_eq!(f.pads[0].bits(), 0);
    assert_eq!(i.pads()[0].status, PadStatus::Unrecognized);
    assert!(i.take_unrecognized().is_empty());
}

#[test]
fn guessed_pad_plays_at_once_and_is_offered_for_learning() {
    let mut i = input(vec![vec![
        connect(SNES_ID, "USB Gamepad", SNES, false, true),
        btn(SNES_ID, P::South, true),
    ]]);
    let f = poll(&mut i);
    assert_eq!(pressed(&f, 0), vec![J::B]);
    assert_eq!(
        i.take_notices(),
        vec![GamepadNotice::Connected {
            name: "USB Gamepad".into(),
            port: 0
        }]
    );
    assert_eq!(i.take_unrecognized().len(), 1);
    let pads = i.pads();
    assert_eq!(
        (pads[0].port, pads[0].status),
        (Some(0), PadStatus::Unrecognized)
    );
}

#[test]
fn ds4_from_the_database_is_unchanged() {
    let mut i = input(vec![vec![ds4(), btn(DS4_ID, P::South, true)]]);
    i.set_learned_maps(maps());
    let f = poll(&mut i);
    assert_eq!(pressed(&f, 0), vec![J::B]);
    assert!(i.take_unrecognized().is_empty());
    assert_eq!(i.pads()[0].status, PadStatus::Mapped);
}

#[test]
fn learned_named_pad_is_reannounced_silently_on_its_port() {
    let mut i = input(vec![
        vec![ds4(), connect(SNES_ID, "USB Gamepad", SNES, false, true)],
        // What the source sends after `set_raw_guids`.
        vec![
            disconnect(SNES_ID),
            snes_raw(),
            raw(SNES_ID, RawInput::Button(1), 1.0),
        ],
    ]);
    poll(&mut i);
    i.take_notices();
    i.take_unrecognized();
    i.set_learned_maps(maps());
    let f = poll(&mut i);
    assert!(i.take_notices().is_empty(), "re-announcement is silent");
    assert!(i.take_unrecognized().is_empty());
    assert_eq!(pressed(&f, 1), vec![J::A]);
    assert_eq!(i.pads()[1].status, PadStatus::Learned);
}

#[test]
fn replugged_learned_pad_is_attached_again() {
    let mut i = input(vec![
        vec![snes_raw()],
        vec![disconnect(SNES_ID)],
        vec![
            connect(PadId(7), "USB Gamepad", SNES, true, false),
            raw(PadId(7), RawInput::Axis(0), 1.0),
        ],
    ]);
    i.set_learned_maps(maps());
    poll(&mut i);
    poll(&mut i);
    assert_eq!(
        i.take_notices(),
        vec![
            GamepadNotice::Connected {
                name: "USB Gamepad".into(),
                port: 0
            },
            GamepadNotice::Disconnected { port: 0 },
        ]
    );
    let f = poll(&mut i);
    assert_eq!(pressed(&f, 0), vec![J::Right]);
    assert!(i.take_unrecognized().is_empty());
}

#[test]
fn raw_capture_feeds_take_raw_only() {
    let mut i = input(vec![
        vec![ds4(), snes_raw()],
        vec![
            raw(SNES_ID, RawInput::Button(9), 1.0),
            raw(SNES_ID, RawInput::Axis(0), -1.0),
            raw(SNES_ID, RawInput::Button(8), 1.0),
        ],
        vec![disconnect(SNES_ID)],
    ]);
    i.set_learned_maps(maps());
    poll(&mut i);
    i.set_ui_capture(true);
    i.set_raw_capture(Some(SNES_ID));
    assert_eq!(i.raw_capture(), Some(SNES_ID));
    poll(&mut i);
    assert_eq!(
        i.take_raw(),
        vec![
            (RawInput::Button(9), 1.0),
            (RawInput::Axis(0), -1.0),
            (RawInput::Button(8), 1.0),
        ]
    );
    assert!(i.take_raw().is_empty());
    assert!(i.take_hotkeys().is_empty());
    assert_eq!(i.take_nav(), Vec::<NavKey>::new());
    i.set_ui_capture(false);
    let f = poll(&mut i);
    assert_eq!(f.pads[1].bits(), 0);
    assert_eq!(i.raw_capture(), None, "unplugging ends the capture");
}

#[test]
fn capture_of_a_named_pad_ignores_its_named_events() {
    let mut i = input(vec![
        vec![ds4()],
        // A named event still in flight, then the source's re-announcement as raw.
        vec![
            btn(DS4_ID, P::South, true),
            disconnect(DS4_ID),
            connect(DS4_ID, "Wireless Controller", DS4, true, false),
            raw(DS4_ID, RawInput::Button(0x130), 1.0),
        ],
    ]);
    poll(&mut i);
    i.take_notices();
    i.set_raw_capture(Some(DS4_ID));
    let f = poll(&mut i);
    assert_eq!(f.pads[0].bits(), 0);
    assert_eq!(i.take_raw(), vec![(RawInput::Button(0x130), 1.0)]);
    assert!(i.take_notices().is_empty(), "silent re-announcement");
    let pads = i.pads();
    assert_eq!(pads[0].status, PadStatus::Mapped);
    assert_eq!(pads[0].port, None, "raw without a learned map: no port");
}

#[test]
fn device_hotkey_button_overrides_the_global_one() {
    let device = emu_ui::GamepadDeviceConfig {
        hotkey_button: Some(emu_ui::HotkeyButton::Select),
        ..Default::default()
    };
    let mut settings = GamepadSettings::default();
    settings.devices.insert(DS4.into(), device);
    let mut source = ScriptedPadSource::new();
    source.push_batch(vec![
        ds4(),
        btn(DS4_ID, P::Select, true),
        btn(DS4_ID, P::Start, true),
    ]);
    let mut i = GamepadInput::new(source, PadMap::default_for(PadStyle::Snes), settings);
    poll(&mut i);
    assert_eq!(i.take_hotkeys(), vec![HotkeyAction::Menu]);
}
