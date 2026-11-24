//! Gamepad logic of the UI crate through its public API only (ADR 0016, task T6): scripted events in, `InputFrame`,
//! hotkeys and notices out. Headless: gilrs is never initialised here.

use emu_runtime::{InputBackend, InputFrame, JoypadButton as J};
use emu_ui::{
    GamepadConfig, GamepadInput, GamepadNotice, GamepadSettings, GamepadSource, HotkeyAction,
    KeyMap, KeyboardInput, NavKey, PadAxis, PadButton as P, PadEvent, PadEventKind, PadId, PadMap,
    PadStyle, ScriptedPadSource,
};
use winit::keyboard::KeyCode;

const A: PadId = PadId(1);
const B: PadId = PadId(2);
const C: PadId = PadId(3);
const DS4: &str = "030000004c050000c405000000000000";

fn ev(id: PadId, kind: PadEventKind) -> PadEvent {
    PadEvent { id, kind }
}
fn connect(id: PadId) -> PadEvent {
    connect_guid(id, DS4)
}
fn connect_guid(id: PadId, guid: &str) -> PadEvent {
    ev(
        id,
        PadEventKind::Connected {
            name: "Wireless Controller".into(),
            guid: guid.into(),
            raw: false,
        },
    )
}
fn disconnect(id: PadId) -> PadEvent {
    ev(id, PadEventKind::Disconnected)
}
fn down(id: PadId, button: P) -> PadEvent {
    ev(
        id,
        PadEventKind::Button {
            button,
            pressed: true,
        },
    )
}
fn up(id: PadId, button: P) -> PadEvent {
    ev(
        id,
        PadEventKind::Button {
            button,
            pressed: false,
        },
    )
}
fn axis(id: PadId, axis: PadAxis, value: f32) -> PadEvent {
    ev(id, PadEventKind::Axis { axis, value })
}

fn pad_input(
    style: PadStyle,
    settings: GamepadSettings,
    batches: Vec<Vec<PadEvent>>,
) -> GamepadInput<ScriptedPadSource> {
    let mut source = ScriptedPadSource::new();
    for batch in batches {
        source.push_batch(batch);
    }
    GamepadInput::new(source, PadMap::default_for(style), settings)
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
fn mapping_by_system_matches_the_adr_columns() {
    // One assertion set per column of the ADR 0016 table: (physical button, RetroPad button).
    let dpad = [
        (P::DPadUp, J::Up),
        (P::DPadDown, J::Down),
        (P::DPadLeft, J::Left),
        (P::DPadRight, J::Right),
    ];
    let columns: Vec<(PadStyle, Vec<(P, J)>)> = vec![
        (
            PadStyle::MegaDrive,
            vec![
                (P::South, J::B),
                (P::East, J::A),
                (P::West, J::Y),
                (P::North, J::X),
                (P::L1, J::L),
                (P::R1, J::R),
                (P::Select, J::Select),
                (P::Start, J::Start),
            ],
        ),
        (
            PadStyle::MasterSystem,
            vec![(P::South, J::B), (P::East, J::A), (P::Start, J::Start)],
        ),
        (
            PadStyle::GameGear,
            vec![(P::South, J::B), (P::East, J::A), (P::Start, J::Start)],
        ),
        (
            PadStyle::Nes,
            vec![
                (P::South, J::B),
                (P::East, J::A),
                (P::Select, J::Select),
                (P::Start, J::Start),
            ],
        ),
        (
            PadStyle::GameBoy,
            vec![
                (P::South, J::B),
                (P::East, J::A),
                (P::Select, J::Select),
                (P::Start, J::Start),
            ],
        ),
        (
            PadStyle::Snes,
            vec![
                (P::South, J::B),
                (P::East, J::A),
                (P::West, J::Y),
                (P::North, J::X),
                (P::L1, J::L),
                (P::R1, J::R),
                (P::Select, J::Select),
                (P::Start, J::Start),
            ],
        ),
    ];
    for (style, face) in columns {
        let mut expected: Vec<(P, J)> = dpad.to_vec();
        expected.extend(face);
        let map = PadMap::default_for(style);
        // Exactly the listed buttons are bound: the table says "(rien)" for the others, and L2/R2/L3/R3/PS too.
        assert_eq!(map.bindings().len(), expected.len(), "{style:?}");
        for (physical, retro) in &expected {
            assert_eq!(
                map.target(*physical),
                Some(*retro),
                "{style:?} {physical:?}"
            );
        }
        for physical in [P::L2, P::R2, P::L3, P::R3, P::Mode] {
            assert_eq!(map.target(physical), None, "{style:?} {physical:?}");
        }
    }
}

#[test]
fn mapping_reaches_the_frame_per_system() {
    // End to end for the Mega Drive and the Master System: Cross + Circle + Square + Options + Share.
    let events = vec![
        connect(A),
        down(A, P::South),
        down(A, P::East),
        down(A, P::West),
        down(A, P::Start),
        down(A, P::Select),
    ];
    let mut md = pad_input(
        PadStyle::MegaDrive,
        GamepadSettings::default(),
        vec![events.clone()],
    );
    assert_eq!(
        pressed(&poll(&mut md), 0),
        vec![J::B, J::Y, J::Select, J::Start, J::A]
    );
    let mut sms = pad_input(
        PadStyle::MasterSystem,
        GamepadSettings::default(),
        vec![events],
    );
    // Square and Share go nowhere on the Master System.
    assert_eq!(pressed(&poll(&mut sms), 0), vec![J::B, J::Start, J::A]);
}

#[test]
fn generic_style_passes_triggers_and_sticks_clicks() {
    let mut input = pad_input(
        PadStyle::Generic,
        GamepadSettings::default(),
        vec![vec![
            connect(A),
            axis(A, PadAxis::L2, 0.9),
            axis(A, PadAxis::R2, 0.3),
            down(A, P::L3),
        ]],
    );
    // R2 at 0.3 is under the 0.5 threshold.
    assert_eq!(pressed(&poll(&mut input), 0), vec![J::L2, J::L3]);
}

#[test]
fn stick_acts_as_dpad_with_deadzone_and_hysteresis() {
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::default(),
        vec![
            vec![connect(A), axis(A, PadAxis::LeftX, -0.3)],
            vec![axis(A, PadAxis::LeftX, -0.8)],
            vec![axis(A, PadAxis::LeftX, -0.45)],
            vec![axis(A, PadAxis::LeftX, -0.2)],
        ],
    );
    assert!(
        pressed(&poll(&mut input), 0).is_empty(),
        "inside the deadzone"
    );
    assert_eq!(pressed(&poll(&mut input), 0), vec![J::Left]);
    assert_eq!(
        pressed(&poll(&mut input), 0),
        vec![J::Left],
        "hysteresis keeps it"
    );
    assert!(pressed(&poll(&mut input), 0).is_empty());
}

#[test]
fn keyboard_and_pad_combine_with_or() {
    let keymap = KeyMap::default_for(PadStyle::Snes);
    let key_b = keymap.keys_for(0, J::B)[0];
    let key_start = keymap.keys_for(0, J::Start)[0];
    let mut keyboard = KeyboardInput::new(keymap);
    keyboard.handle_key(key_b, true);
    keyboard.handle_key(key_start, true);
    let mut pad = pad_input(
        PadStyle::Snes,
        GamepadSettings::default(),
        vec![vec![connect(A), down(A, P::East), down(A, P::South)]],
    );
    // The same backends the app owns, polled into one frame.
    let mut frame = InputFrame::default();
    keyboard.poll(0, &mut frame);
    pad.poll(0, &mut frame);
    // Keyboard: B + Start; pad: A + B (B on both: still pressed once).
    assert_eq!(pressed(&frame, 0), vec![J::B, J::Start, J::A]);

    // Releasing the pad leaves the keyboard untouched, and vice versa.
    let mut pad_only = InputFrame::default();
    pad.poll(0, &mut pad_only);
    keyboard.release_all();
    let mut kb_only = InputFrame::default();
    keyboard.poll(0, &mut kb_only);
    assert!(pressed(&kb_only, 0).is_empty());
    assert_eq!(pressed(&pad_only, 0), vec![J::B, J::A]);
    // A key unrelated to the map is not consumed.
    assert!(!keyboard.handle_key(KeyCode::F12, true));
}

#[test]
fn two_pads_take_ports_one_and_two_and_a_third_is_ignored() {
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::default(),
        vec![
            vec![
                connect(A),
                connect(B),
                connect(C),
                down(A, P::South),
                down(B, P::East),
                down(C, P::Start),
            ],
            vec![disconnect(A)],
            vec![connect(C), down(C, P::North)],
        ],
    );
    let frame = poll(&mut input);
    assert_eq!(pressed(&frame, 0), vec![J::B]);
    assert_eq!(pressed(&frame, 1), vec![J::A]);
    assert_eq!(input.attached(), vec![(0, A), (1, B)]);
    let notices = input.take_notices();
    assert_eq!(notices.len(), 2, "the third pad is silently ignored");

    // Pad A leaves: its port is released, pad B keeps port 1.
    let frame = poll(&mut input);
    assert!(pressed(&frame, 0).is_empty());
    assert_eq!(input.attached(), vec![(1, B)]);

    // Pad C reconnects and reuses the freed port 0.
    let frame = poll(&mut input);
    assert_eq!(pressed(&frame, 0), vec![J::X]);
    assert_eq!(pressed(&frame, 1), vec![J::A]);
    assert_eq!(input.attached(), vec![(0, C), (1, B)]);
}

#[test]
fn device_override_forces_the_port() {
    let config: GamepadConfig =
        toml::from_str(&format!("[devices.\"{DS4}\"]\nport = 1\n")).expect("config");
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::from_config(&config),
        vec![vec![connect(A), down(A, P::South)]],
    );
    let frame = poll(&mut input);
    assert!(pressed(&frame, 0).is_empty());
    assert_eq!(pressed(&frame, 1), vec![J::B]);
}

#[test]
fn disconnect_releases_buttons_and_reports_it() {
    let config: GamepadConfig = toml::from_str("pause_on_disconnect = true").expect("config");
    let settings = GamepadSettings::from_config(&config);
    assert!(settings.pause_on_disconnect);
    let mut input = pad_input(
        PadStyle::Snes,
        settings,
        vec![
            vec![connect(A), down(A, P::South), down(A, P::DPadLeft)],
            vec![disconnect(A)],
            // Events of a pad that is gone must not panic nor resurrect it.
            vec![down(A, P::South), axis(A, PadAxis::LeftX, 1.0)],
        ],
    );
    assert_eq!(pressed(&poll(&mut input), 0), vec![J::B, J::Left]);
    assert_eq!(
        input.take_notices(),
        vec![GamepadNotice::Connected {
            name: "Wireless Controller".into(),
            port: 0
        }]
    );
    assert!(pressed(&poll(&mut input), 0).is_empty());
    assert_eq!(
        input.take_notices(),
        vec![GamepadNotice::Disconnected { port: 0 }]
    );
    assert!(pressed(&poll(&mut input), 0).is_empty());
    assert!(input.attached().is_empty());
    assert!(input.take_notices().is_empty());
}

#[test]
fn ps_button_shortcuts() {
    let cases = [
        (P::Start, HotkeyAction::Menu),
        (P::Select, HotkeyAction::TogglePause),
        (P::R1, HotkeyAction::SaveState),
        (P::L1, HotkeyAction::LoadState),
        (P::DPadRight, HotkeyAction::NextSlot),
        (P::DPadLeft, HotkeyAction::PrevSlot),
        (P::North, HotkeyAction::ToggleFullscreen),
        (P::West, HotkeyAction::Reset),
        (P::South, HotkeyAction::ToggleStats),
        // Computer systems: the app drops these two on consoles (emu-ui app.rs, `computer_only`).
        (P::R3, HotkeyAction::ToggleVirtualKeyboard),
        (P::L3, HotkeyAction::NextDisk),
    ];
    for (button, action) in cases {
        let mut input = pad_input(
            PadStyle::Snes,
            GamepadSettings::default(),
            vec![vec![connect(A), down(A, P::Mode), down(A, button)]],
        );
        let frame = poll(&mut input);
        assert_eq!(input.take_hotkeys(), vec![action], "PS + {button:?}");
        assert!(
            pressed(&frame, 0).is_empty(),
            "the core sees nothing while PS is held ({button:?})"
        );
    }
}

#[test]
fn ps_alone_does_nothing_and_buttons_flow_again_after_release() {
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::default(),
        vec![
            vec![connect(A), down(A, P::Mode), up(A, P::Mode)],
            vec![down(A, P::South)],
        ],
    );
    assert!(poll(&mut input).pads[0].bits() == 0);
    assert!(
        input.take_hotkeys().is_empty(),
        "a short PS press is silent"
    );
    assert_eq!(pressed(&poll(&mut input), 0), vec![J::B]);
    assert!(input.take_hotkeys().is_empty());
}

#[test]
fn r3_and_l3_without_ps_are_plain_buttons() {
    let mut input = pad_input(
        PadStyle::Generic,
        GamepadSettings::default(),
        vec![vec![connect(A), down(A, P::R3), down(A, P::L3)]],
    );
    assert_eq!(pressed(&poll(&mut input), 0), vec![J::L3, J::R3]);
    assert!(input.take_hotkeys().is_empty());
}

#[test]
fn ps_plus_r2_is_a_held_fast_forward() {
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::default(),
        vec![
            vec![connect(A), down(A, P::Mode)],
            vec![axis(A, PadAxis::R2, 1.0)],
            vec![axis(A, PadAxis::R2, 0.0)],
            vec![axis(A, PadAxis::R2, 1.0), up(A, P::Mode)],
        ],
    );
    poll(&mut input);
    assert!(!input.fast_forward_held());
    poll(&mut input);
    assert!(input.fast_forward_held());
    assert!(input.take_hotkeys().is_empty(), "held action, not a press");
    poll(&mut input);
    assert!(!input.fast_forward_held());
    // R2 alone (PS released) is not fast-forward.
    poll(&mut input);
    assert!(!input.fast_forward_held());
}

#[test]
fn select_as_modifier_leaves_ps_for_pause() {
    let config: GamepadConfig = toml::from_str("hotkey_button = \"select\"").expect("config");
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::from_config(&config),
        vec![vec![
            connect(A),
            down(A, P::Select),
            down(A, P::R1),
            down(A, P::Mode),
        ]],
    );
    let frame = poll(&mut input);
    assert_eq!(
        input.take_hotkeys(),
        vec![HotkeyAction::SaveState, HotkeyAction::TogglePause]
    );
    assert!(pressed(&frame, 0).is_empty());
}

#[test]
fn ui_capture_turns_buttons_into_navigation() {
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::default(),
        vec![
            vec![connect(A)],
            vec![down(A, P::DPadDown), down(A, P::South), down(A, P::East)],
        ],
    );
    input.set_ui_capture(true);
    poll(&mut input);
    let frame = poll(&mut input);
    assert!(
        pressed(&frame, 0).is_empty(),
        "the core sees nothing in a menu"
    );
    assert_eq!(
        input.take_nav(),
        vec![NavKey::Down, NavKey::Activate, NavKey::Back]
    );
}

#[test]
fn disabled_pad_does_nothing() {
    let config: GamepadConfig = toml::from_str("enabled = false").expect("config");
    let mut input = pad_input(
        PadStyle::Snes,
        GamepadSettings::from_config(&config),
        vec![vec![connect(A), down(A, P::South)]],
    );
    assert!(poll(&mut input).pads[0].bits() == 0);
    assert!(input.attached().is_empty());
}

#[test]
fn config_overrides_remap_one_button_and_keep_the_rest() {
    let config: GamepadConfig = toml::from_str(
        "[bindings.snes9x]\nsouth = \"a\"\nwest = \"none\"\nbogus = \"b\"\nnorth = \"nope\"\n",
    )
    .expect("config");
    let map = PadMap::from_config(PadStyle::Snes, "snes9x", &config);
    assert_eq!(map.target(P::South), Some(J::A));
    assert_eq!(map.target(P::West), None);
    assert_eq!(map.target(P::North), Some(J::X), "unknown target ignored");
    assert_eq!(map.target(P::East), Some(J::A));
    // Another profile keeps the defaults.
    let other = PadMap::from_config(PadStyle::Snes, "genesis_plus_gx", &config);
    assert_eq!(other, PadMap::default_for(PadStyle::Snes));
}

/// Compile-time check that the source trait stays object safe (the app boxes it).
#[test]
fn source_trait_is_object_safe() {
    let mut boxed: Box<dyn GamepadSource> = Box::new(ScriptedPadSource::new());
    let mut out = Vec::new();
    boxed.poll_events(&mut out);
    assert!(out.is_empty());
}
