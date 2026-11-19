use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, vec2};
use emu_runtime::{CoreOptionValue, JoypadButton};

use super::*;

fn opt(
    key: &str,
    category: Option<&str>,
    values: &[&str],
    current: &str,
    visible: bool,
) -> CoreOption {
    CoreOption {
        key: key.into(),
        label: key.into(),
        info: Some(format!("info {key}")),
        category: category.map(Into::into),
        values: values
            .iter()
            .map(|v| CoreOptionValue {
                value: (*v).into(),
                label: v.to_uppercase(),
            })
            .collect(),
        default: values[0].into(),
        current: current.into(),
        visible,
    }
}

fn cat(key: &str) -> CoreOptionCategory {
    CoreOptionCategory {
        key: key.into(),
        label: format!("Cat {key}"),
        info: None,
    }
}

#[test]
fn capture_outcomes() {
    let hk = Hotkeys::default();
    let mut map = KeyMap::genesis_default();
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::Escape, 0, JoypadButton::Up),
        CaptureOutcome::Cancelled
    );
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::F2, 0, JoypadButton::Up),
        CaptureOutcome::Refused(HotkeyAction::SaveState)
    );
    assert_eq!(map, KeyMap::genesis_default());
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::KeyW, 0, JoypadButton::Up),
        CaptureOutcome::Bound { moved_from: None }
    );
    assert_eq!(map.keys_for(0, JoypadButton::Up), vec![KeyCode::KeyW]);
    // KeyZ is MD A (libretro Y): moving it to Start is a conflict, Y loses it.
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::KeyZ, 0, JoypadButton::Start),
        CaptureOutcome::Bound {
            moved_from: Some((0, JoypadButton::Y))
        }
    );
    assert!(map.keys_for(0, JoypadButton::Y).is_empty());
    assert_eq!(map.keys_for(0, JoypadButton::Start), vec![KeyCode::KeyZ]);
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::Backspace, 0, JoypadButton::Start),
        CaptureOutcome::Cleared
    );
    assert!(map.keys_for(0, JoypadButton::Start).is_empty());
    // Re-binding the same key to the same button is not a conflict.
    apply_capture(&mut map, &hk, KeyCode::KeyQ, 1, JoypadButton::A);
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::KeyQ, 1, JoypadButton::A),
        CaptureOutcome::Bound { moved_from: None }
    );
}

#[test]
fn option_cycling_wraps() {
    let o = opt("k", None, &["a", "b", "c"], "c", true);
    assert_eq!(cycle_value(&o, 1).as_deref(), Some("a"));
    assert_eq!(cycle_value(&o, -1).as_deref(), Some("b"));
    let unknown = opt("k", None, &["a", "b"], "zzz", true);
    assert_eq!(cycle_value(&unknown, 1).as_deref(), Some("b"));
    let mut empty = opt("k", None, &["a"], "a", true);
    empty.values.clear();
    assert_eq!(cycle_value(&empty, 1), None);
}

#[test]
fn options_grouping() {
    let options = vec![
        opt("a", Some("video"), &["x"], "x", true),
        opt("b", None, &["x"], "x", true),
        opt("c", Some("ghost"), &["x"], "x", true),
        opt("d", Some("video"), &["x"], "x", false),
        opt("e", Some("audio"), &["x"], "x", false),
    ];
    let cats = vec![cat("video"), cat("audio")];
    let groups = group_options(&options, &cats);
    let keys: Vec<(String, Vec<&str>)> = groups
        .iter()
        .map(|(t, o)| (t.clone(), o.iter().map(|o| o.key.as_str()).collect()))
        .collect();
    assert_eq!(
        keys,
        vec![
            (text::OPTIONS_GENERAL.to_owned(), vec!["b", "c"]),
            ("Cat video".to_owned(), vec!["a"]),
        ]
    );
}

#[test]
fn back_navigation() {
    let mut o = Overlay::default();
    o.open(Screen::Keys);
    o.capture = Some(JoypadButton::A);
    o.back();
    assert_eq!(o.screen(), Some(Screen::Keys));
    assert!(o.capture().is_none());
    o.back();
    assert_eq!(o.screen(), Some(Screen::Menu));
    o.back();
    assert!(!o.is_open());
}

#[test]
fn osd_expiry_and_cap() {
    let now = Instant::now();
    let mut osd = Osd::default();
    for i in 0..8 {
        osd.push(
            format!("m{i}"),
            OsdLevel::Info,
            Duration::from_millis(1),
            now,
        );
    }
    assert_eq!(osd.len(), 5);
    assert_eq!(osd.next_expiry(), Some(now + Duration::from_secs(1)));
    osd.prune(now + Duration::from_secs(2));
    assert!(osd.is_empty());
}

// Headless egui passes: keyboard navigation works without a window or a pointer.

struct Fixture {
    ctx: Context,
    config: Config,
    slots: Vec<Option<u64>>,
    options: Vec<CoreOption>,
    categories: Vec<CoreOptionCategory>,
}

impl Fixture {
    fn new() -> Self {
        let mut slots = vec![None; 10];
        slots[0] = Some(1_795_100_400);
        Self {
            ctx: {
                let ctx = Context::default();
                apply_style(&ctx);
                ctx
            },
            config: Config::default(),
            slots,
            options: vec![opt(
                "mock_color",
                Some("video"),
                &["red", "green", "blue"],
                "red",
                true,
            )],
            categories: vec![cat("video")],
        }
    }

    fn pass(&self, overlay: &mut Overlay, slot: u8, keys: &[Key]) -> Vec<UiAction> {
        let events = keys
            .iter()
            .flat_map(|k| {
                [true, false].map(|pressed| Event::Key {
                    key: *k,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                })
            })
            .collect();
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..RawInput::default()
        };
        let data = UiData {
            game: "jeu",
            core_id: "mock",
            config: &self.config,
            slot,
            slots: &self.slots,
            options: &self.options,
            categories: &self.categories,
            stats: None,
            now: Instant::now(),
        };
        let mut actions = Vec::new();
        self.ctx
            .run_pass(raw, |ctx| actions = show(ctx, overlay, &data))
            .drop_without_applying_deltas();
        actions
    }

    /// Sends keys one per pass (focus moves at the end of a pass), returns every action.
    fn keys(&self, overlay: &mut Overlay, slot: u8, keys: &[Key]) -> Vec<UiAction> {
        let mut all = Vec::new();
        for k in keys {
            all.extend(self.pass(overlay, slot, &[*k]));
            all.extend(self.pass(overlay, slot, &[]));
        }
        all
    }
}

fn opened(screen: Screen, f: &Fixture) -> Overlay {
    let mut o = Overlay::default();
    o.open(screen);
    // Two empty passes: layout, then focus on the first row.
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    o
}

#[test]
fn menu_enter_resumes() {
    let f = Fixture::new();
    let mut o = opened(Screen::Menu, &f);
    assert_eq!(f.keys(&mut o, 0, &[Key::Enter]), vec![UiAction::Resume]);
}

#[test]
fn menu_arrow_down_then_enter_resets() {
    let f = Fixture::new();
    let mut o = opened(Screen::Menu, &f);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::ArrowDown, Key::Enter]),
        vec![UiAction::Reset]
    );
}

#[test]
fn menu_slot_row_steps_with_left_right() {
    let f = Fixture::new();
    let mut o = opened(Screen::Menu, &f);
    let actions = f.keys(
        &mut o,
        0,
        &[Key::ArrowDown, Key::ArrowDown, Key::ArrowRight],
    );
    assert_eq!(actions, vec![UiAction::SelectSlot(1)]);
    let actions = f.keys(&mut o, 0, &[Key::ArrowLeft]);
    assert_eq!(actions, vec![UiAction::SelectSlot(9)]);
}

#[test]
fn menu_tab_reaches_save() {
    let f = Fixture::new();
    let mut o = opened(Screen::Menu, &f);
    let actions = f.keys(&mut o, 0, &[Key::Tab, Key::Tab, Key::Tab, Key::Enter]);
    assert_eq!(actions, vec![UiAction::SaveState]);
}

#[test]
fn keys_screen_enter_starts_capture() {
    let f = Fixture::new();
    let mut o = opened(Screen::Keys, &f);
    // Profile, player, then the first button row (Up).
    f.keys(&mut o, 0, &[Key::ArrowDown, Key::ArrowDown, Key::Enter]);
    assert_eq!(o.capture(), Some((Profile::Core, 0, JoypadButton::Up)));
}

#[test]
fn keys_screen_switches_player() {
    let f = Fixture::new();
    let mut o = opened(Screen::Keys, &f);
    f.keys(&mut o, 0, &[Key::ArrowDown, Key::ArrowRight]);
    assert_eq!(o.port, 1);
    f.keys(&mut o, 0, &[Key::ArrowUp, Key::ArrowRight]);
    assert_eq!(o.profile, Profile::Default);
}

#[test]
fn options_screen_right_sets_next_value() {
    let f = Fixture::new();
    let mut o = opened(Screen::CoreOptions, &f);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::ArrowRight]),
        vec![UiAction::SetOption {
            key: "mock_color".into(),
            value: "green".into()
        }]
    );
}

#[test]
fn locked_config_needs_confirmation_to_unlock() {
    let mut f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "video = [[[").unwrap();
    f.config = Config::load_or_default(&path);
    let mut o = opened(Screen::CoreOptions, &f);
    // Focus starts on the option row; the unlock button is above it.
    let actions = f.keys(&mut o, 0, &[Key::ArrowUp, Key::Enter]);
    assert!(actions.is_empty());
    assert!(o.confirm_unlock);
    let actions = f.keys(&mut o, 0, &[Key::Tab, Key::Enter]);
    assert!(actions.contains(&UiAction::UnlockConfig), "{actions:?}");
}
