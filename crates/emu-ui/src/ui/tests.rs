use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, vec2};
use emu_runtime::{CoreOptionValue, JoypadButton};

use std::collections::BTreeMap;
use std::path::PathBuf;

use emu_runtime::DiskSlot;

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
        apply_capture(&mut map, &hk, KeyCode::KeyQ, 0, JoypadButton::Up),
        CaptureOutcome::Bound { moved_from: None }
    );
    assert_eq!(map.keys_for(0, JoypadButton::Up), vec![KeyCode::KeyQ]);
    // KeyJ is MD A (libretro Y): moving it to Start is a conflict, Y loses it.
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::KeyJ, 0, JoypadButton::Start),
        CaptureOutcome::Bound {
            moved_from: Some((0, JoypadButton::Y))
        }
    );
    assert!(map.keys_for(0, JoypadButton::Y).is_empty());
    assert_eq!(map.keys_for(0, JoypadButton::Start), vec![KeyCode::KeyJ]);
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::Backspace, 0, JoypadButton::Start),
        CaptureOutcome::Cleared
    );
    assert!(map.keys_for(0, JoypadButton::Start).is_empty());
    // Re-binding the same key to the same button is not a conflict.
    apply_capture(&mut map, &hk, KeyCode::KeyT, 1, JoypadButton::A);
    assert_eq!(
        apply_capture(&mut map, &hk, KeyCode::KeyT, 1, JoypadButton::A),
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
fn amiga_main_options_come_first() {
    let options = vec![
        opt("puae_floppy_multidrive", Some("media"), &["x"], "x", true),
        opt("puae_video_standard", Some("video"), &["x"], "x", true),
        opt("puae_other", Some("video"), &["x"], "x", true),
        opt("puae_kickstart", Some("system"), &["x"], "x", true),
        opt("puae_model", Some("system"), &["x"], "x", true),
    ];
    let cats = vec![cat("system"), cat("video"), cat("media")];
    let groups = group_options(&options, &cats);
    assert_eq!(groups[0].0, text::OPTIONS_MAIN);
    let main: Vec<&str> = groups[0].1.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(
        main,
        [
            "puae_model",
            "puae_kickstart",
            "puae_video_standard",
            "puae_floppy_multidrive"
        ]
    );
    assert_eq!(
        groups.len(),
        2,
        "pinned options are not repeated: {groups:?}"
    );
    assert_eq!(groups[1].1[0].key, "puae_other");
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
    computer: Option<ComputerView>,
    disks: Option<DisksView>,
    overrides: BTreeMap<String, String>,
    dos: bool,
    hosted: bool,
    eightbit: EightBitView,
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
            computer: None,
            disks: None,
            overrides: BTreeMap::new(),
            dos: false,
            hosted: false,
            eightbit: EightBitView::default(),
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
        self.pass_events(overlay, slot, events)
    }

    fn pass_events(&self, overlay: &mut Overlay, slot: u8, events: Vec<Event>) -> Vec<UiAction> {
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..RawInput::default()
        };
        let data = UiData {
            game: "jeu",
            core_id: "mock",
            style: PadStyle::Generic,
            config: &self.config,
            slot,
            slots: &self.slots,
            options: &self.options,
            categories: &self.categories,
            hosted: self.hosted,
            stats: None,
            now: Instant::now(),
            computer: self.computer.as_ref(),
            disks: self.disks.as_ref(),
            game_scope: false,
            game_overrides: &self.overrides,
            dos: self.dos,
            featured: None,
            soundfont_hint: false,
            eightbit: &self.eightbit,
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

/// DOS (design v4.0 §2.2-§2.3): boot program and reset of the game's changes, keyboard only; the reset asks
/// first and « Non » has the focus.
#[test]
fn dos_menu_entries_by_keyboard() {
    let mut f = Fixture::new();
    f.dos = true;
    f.hosted = true;
    let mut o = opened(Screen::Menu, &f);
    let tabs = [Key::Tab; 5];
    let mut keys = tabs.to_vec();
    keys.push(Key::Enter);
    assert_eq!(f.keys(&mut o, 0, &keys), vec![UiAction::ChangeStartProgram]);
    let mut o = opened(Screen::Menu, &f);
    let mut keys = [Key::Tab; 6].to_vec();
    keys.push(Key::Enter);
    assert!(f.keys(&mut o, 0, &keys).is_empty());
    assert!(o.confirm_dos_reset);
    f.pass(&mut o, 0, &[]);
    // « Non » first.
    assert!(f.keys(&mut o, 0, &[Key::Enter]).is_empty());
    assert!(!o.confirm_dos_reset);
    let mut o = opened(Screen::Menu, &f);
    let mut keys = [Key::Tab; 6].to_vec();
    keys.extend([Key::Enter, Key::Tab, Key::Enter]);
    assert_eq!(f.keys(&mut o, 0, &keys), vec![UiAction::ResetDisks]);
    // Not a DOS game: no such entries.
    let f = Fixture::new();
    let mut o = opened(Screen::Menu, &f);
    let mut keys = tabs.to_vec();
    keys.push(Key::Enter);
    assert!(
        !f.keys(&mut o, 0, &keys)
            .contains(&UiAction::ChangeStartProgram)
    );
}

#[test]
fn featured_options_come_first_under_their_title() {
    let options = vec![
        opt(
            "dosbox_pure_cycles",
            Some("performance"),
            &["auto", "max"],
            "auto",
            true,
        ),
        opt(
            "dosbox_pure_midi",
            Some("audio"),
            &["disabled"],
            "disabled",
            true,
        ),
        opt("dosbox_pure_other", None, &["a"], "a", true),
    ];
    let cats = vec![cat("performance"), cat("audio")];
    let featured = Featured {
        title: "Réglages DOS".into(),
        keys: vec![
            "dosbox_pure_midi".into(),
            "dosbox_pure_cycles".into(),
            "absent".into(),
        ],
    };
    let groups = group_options_with(&options, &cats, Some(&featured));
    assert_eq!(groups[0].0, "Réglages DOS");
    let keys: Vec<&str> = groups[0].1.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys, ["dosbox_pure_midi", "dosbox_pure_cycles"]);
    // Featured options are not repeated below.
    assert!(
        groups[1..]
            .iter()
            .all(|(_, o)| o.iter().all(|o| !featured.keys.contains(&o.key)))
    );
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
    // Focus starts on the option row; the scope row then the unlock button are above it.
    let actions = f.keys(&mut o, 0, &[Key::ArrowUp, Key::ArrowUp, Key::Enter]);
    assert!(actions.is_empty());
    assert!(o.confirm_unlock);
    let actions = f.keys(&mut o, 0, &[Key::Tab, Key::Enter]);
    assert!(actions.contains(&UiAction::UnlockConfig), "{actions:?}");
}

#[test]
fn capture_refuses_modifiers() {
    let hk = Hotkeys::default();
    let mut map = KeyMap::genesis_default();
    for key in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ] {
        assert_eq!(
            apply_capture(&mut map, &hk, key, 0, JoypadButton::A),
            CaptureOutcome::RefusedModifier
        );
    }
    assert!(!map.binds_modifier());
    map.bind(KeyCode::AltLeft, 0, JoypadButton::A);
    assert!(map.binds_modifier());
}

fn computer_fixture() -> Fixture {
    let mut f = Fixture::new();
    f.computer = Some(ComputerView {
        mode: KeyboardMode::Keyboard,
        auto_capture: true,
        sensitivity: 1.0,
        host_key: KeyCode::ControlRight,
    });
    f.disks = Some(DisksView {
        slots: vec![
            DiskSlot {
                index: 0,
                label: "Disque A".into(),
                path: None,
            },
            DiskSlot {
                index: 1,
                label: "Disque B".into(),
                path: None,
            },
        ],
        current: Some(0),
        addable: vec![PathBuf::from("/jeux/extra.st")],
        can_reset: true,
        added: Vec::new(),
        orphans: None,
    });
    f
}

#[test]
fn computer_menu_lists_disks_and_virtual_keyboard() {
    let f = computer_fixture();
    let mut o = opened(Screen::Menu, &f);
    // Resume, reset, slot, save, load (disabled), then Disquettes.
    let actions = f.keys(
        &mut o,
        0,
        &[Key::Tab, Key::Tab, Key::Tab, Key::Tab, Key::Tab, Key::Enter],
    );
    assert_eq!(actions, vec![UiAction::Open(Screen::Disks)]);
    let actions = f.keys(&mut o, 0, &[Key::Tab, Key::Enter]);
    assert_eq!(actions, vec![UiAction::ToggleVirtualKeyboard]);
    // Consoles: no such entries.
    let plain = Fixture::new();
    let mut o = opened(Screen::Menu, &plain);
    let actions = plain.keys(
        &mut o,
        0,
        &[Key::Tab, Key::Tab, Key::Tab, Key::Tab, Key::Tab, Key::Enter],
    );
    assert_eq!(actions, vec![UiAction::Open(Screen::Keys)]);
}

#[test]
fn disks_screen_inserts_adds_and_confirms_reset() {
    let f = computer_fixture();
    let mut o = opened(Screen::Disks, &f);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Enter]),
        vec![UiAction::InsertDisk(0)]
    );
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::InsertDisk(1)]
    );
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::AddDisk(PathBuf::from("/jeux/extra.st"))]
    );
    // Eject, then « Réinitialiser » asks first; the safe answer has the focus.
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::EjectDisk]
    );
    assert!(f.keys(&mut o, 0, &[Key::Tab, Key::Enter]).is_empty());
    assert!(o.confirm_reset);
    f.pass(&mut o, 0, &[]);
    assert!(f.keys(&mut o, 0, &[Key::Enter]).is_empty());
    assert!(!o.confirm_reset);
    o.confirm_reset = true;
    o.focus_first = true;
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::ResetDisks]
    );
}

#[test]
fn back_from_computer_screens_goes_to_menu() {
    let mut o = Overlay::default();
    for screen in [Screen::Disks, Screen::Computer] {
        o.open(screen);
        o.back();
        assert_eq!(o.screen(), Some(Screen::Menu));
    }
    o.open(Screen::Disks);
    o.confirm_reset = true;
    o.back();
    assert_eq!(o.screen(), Some(Screen::Disks));
    assert!(!o.confirm_reset);
}

#[test]
fn computer_screen_steps_values() {
    let f = computer_fixture();
    let mut o = opened(Screen::Computer, &f);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::ArrowRight]),
        vec![UiAction::ToggleKeyboardMode]
    );
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::ArrowRight]),
        vec![UiAction::SetAutoCapture(false)]
    );
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::ArrowLeft]),
        vec![UiAction::StepSensitivity(-1)]
    );
}

#[test]
fn options_scope_row_switches_to_this_game() {
    let f = Fixture::new();
    let mut o = opened(Screen::CoreOptions, &f);
    let actions = f.keys(&mut o, 0, &[Key::ArrowUp, Key::ArrowRight]);
    assert_eq!(actions, vec![UiAction::SetGameScope(true)]);
}

fn slot_fixture_menu(f: &Fixture, confirm: ConfirmLoad) -> Overlay {
    let mut o = Overlay::default();
    o.open(Screen::Menu);
    o.ask_load_confirmation(confirm);
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    o
}

#[test]
fn state_confirmation_defaults_to_no() {
    let f = Fixture::new();
    let confirm = ConfirmLoad {
        slot: 0,
        from_menu: true,
        has_disks: true,
    };
    // From the menu: Enter on the focused « Non » goes back to the menu, nothing loaded.
    let mut o = slot_fixture_menu(&f, confirm);
    assert!(f.keys(&mut o, 0, &[Key::Enter]).is_empty());
    assert_eq!(o.confirm_load, None);
    assert_eq!(o.screen(), Some(Screen::Menu));
    // Then the menu is back, its first row focused.
    assert_eq!(f.keys(&mut o, 0, &[Key::Enter]), vec![UiAction::Resume]);
    // « Oui, charger » is one row below.
    let mut o = slot_fixture_menu(&f, confirm);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::ConfirmLoadState(0)]
    );
    // From a shortcut: « Non » resumes the game.
    let mut o = slot_fixture_menu(
        &f,
        ConfirmLoad {
            from_menu: false,
            ..confirm
        },
    );
    assert_eq!(f.keys(&mut o, 0, &[Key::Enter]), vec![UiAction::Resume]);
}

#[test]
fn back_cancels_a_state_confirmation() {
    let confirm = ConfirmLoad {
        slot: 3,
        from_menu: true,
        has_disks: false,
    };
    let mut o = Overlay::default();
    o.open(Screen::Menu);
    o.ask_load_confirmation(confirm);
    o.back();
    assert_eq!(o.screen(), Some(Screen::Menu));
    assert_eq!(o.confirm_load, None);
    o.ask_load_confirmation(ConfirmLoad {
        from_menu: false,
        ..confirm
    });
    o.back();
    assert!(!o.is_open());
}

fn added_fixture() -> Fixture {
    let mut f = computer_fixture();
    let disks = f.disks.as_mut().unwrap();
    disks.addable.clear();
    disks.can_reset = false;
    disks.added = vec![AddedDiskView {
        copy: PathBuf::from("/s/hatari/Jeu.disks/Extra.st"),
        name: "Extra.st".into(),
        size: 737_280,
        modified: None,
        source_missing: true,
        inserted: false,
    }];
    disks.orphans = Some(vec![
        OrphanCopy {
            name: "02 Extra.st".into(),
            kind: emu_runtime::OrphanKind::Copy,
            size: 737_280,
            modified: None,
        },
        OrphanCopy {
            name: "Jeu.m3u".into(),
            kind: emu_runtime::OrphanKind::StalePlaylist,
            size: 20,
            modified: None,
        },
    ]);
    f
}

#[test]
fn added_disk_can_be_removed_with_or_without_its_copy() {
    let f = added_fixture();
    let copy = PathBuf::from("/s/hatari/Jeu.disks/Extra.st");
    let mut o = opened(Screen::Disks, &f);
    // Slots A, B, then the added disk opens its dialog.
    assert!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Tab, Key::Enter])
            .is_empty()
    );
    assert_eq!(o.disk_forget.as_ref(), Some(&copy));
    f.pass(&mut o, 0, &[]);
    // « Annuler » has the focus.
    assert!(f.keys(&mut o, 0, &[Key::Enter]).is_empty());
    assert_eq!(o.disk_forget, None);
    o.disk_forget = Some(copy.clone());
    o.focus_first = true;
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::ForgetDisk {
            copy: copy.clone(),
            delete: false
        }]
    );
    // Deleting the copy asks again, « Non » first.
    o.disk_forget = Some(copy.clone());
    o.focus_first = true;
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    assert!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Tab, Key::Enter])
            .is_empty()
    );
    assert!(o.disk_delete_confirm);
    f.pass(&mut o, 0, &[]);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::ForgetDisk { copy, delete: true }]
    );
    // Back closes the confirmation, then the dialog, then the screen.
    o.back();
    assert!(!o.disk_delete_confirm);
    o.back();
    assert_eq!(o.disk_forget, None);
    assert_eq!(o.screen(), Some(Screen::Disks));
}

#[test]
fn clean_dialog_deletes_only_checked_rows() {
    let f = added_fixture();
    let mut o = opened(Screen::Disks, &f);
    // Slots A, B, added disk, eject, then « Nettoyer les copies… ».
    let actions = f.keys(
        &mut o,
        0,
        &[Key::Tab, Key::Tab, Key::Tab, Key::Tab, Key::Enter],
    );
    assert!(actions.is_empty(), "{actions:?}");
    assert_eq!(o.clean, Some(CleanDialog::default()));
    f.pass(&mut o, 0, &[]);
    // Nothing checked: the delete row is disabled, Tab goes from the first check box to « Reprendre ».
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::AdoptOrphan("02 Extra.st".into())]
    );
    // Check the playlist only (Space on the check box), then delete.
    let actions = f.keys(&mut o, 0, &[Key::Tab, Key::Space]);
    assert!(actions.is_empty());
    let clean = o.clean.clone().unwrap();
    assert_eq!(
        clean.selected.iter().cloned().collect::<Vec<_>>(),
        vec!["Jeu.m3u".to_owned()]
    );
    assert!(f.keys(&mut o, 0, &[Key::Tab, Key::Enter]).is_empty());
    assert!(o.clean.as_ref().unwrap().confirm);
    f.pass(&mut o, 0, &[]);
    // « Non » first.
    assert!(f.keys(&mut o, 0, &[Key::Enter]).is_empty());
    assert!(!o.clean.as_ref().unwrap().confirm);
    o.clean.as_mut().unwrap().confirm = true;
    o.focus_first = true;
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    assert_eq!(
        f.keys(&mut o, 0, &[Key::Tab, Key::Enter]),
        vec![UiAction::CleanCopies(vec!["Jeu.m3u".into()])]
    );
    o.back();
    assert_eq!(o.clean, None);
    assert_eq!(o.screen(), Some(Screen::Disks));
}

/// Design v5.0 §4: « Cassette » and « Port joystick » rows of the menu, by keyboard.
#[test]
fn eight_bit_menu_rows_by_keyboard() {
    let mut f = computer_fixture();
    f.eightbit = EightBitView {
        tape: true,
        joyport: Some(JoyportView {
            port: 2,
            forced: None,
        }),
        launch: None,
    };
    // Resume, reset, slot, save, (load disabled), Disquettes, then Lecture / Arrêt / Rembobiner, Port joystick.
    let mut o = opened(Screen::Menu, &f);
    let mut keys = [Key::Tab; 6].to_vec();
    keys.push(Key::Enter);
    assert_eq!(
        f.keys(&mut o, 0, &keys),
        vec![UiAction::Tape(TapeAction::Play)]
    );
    let mut o = opened(Screen::Menu, &f);
    let mut keys = [Key::Tab; 8].to_vec();
    keys.push(Key::Enter);
    assert_eq!(
        f.keys(&mut o, 0, &keys),
        vec![UiAction::Tape(TapeAction::Rewind)]
    );
    let mut o = opened(Screen::Menu, &f);
    let mut keys = [Key::Tab; 9].to_vec();
    keys.push(Key::Enter);
    assert_eq!(f.keys(&mut o, 0, &keys), vec![UiAction::ToggleJoyport]);
    // A port fixed by the file name cannot be switched: the row is skipped.
    f.eightbit.joyport = Some(JoyportView {
        port: 2,
        forced: Some(1),
    });
    let mut o = opened(Screen::Menu, &f);
    let mut keys = [Key::Tab; 9].to_vec();
    keys.push(Key::Enter);
    assert!(!f.keys(&mut o, 0, &keys).contains(&UiAction::ToggleJoyport));
}

/// « Commande de démarrage… »: typed text, Enter saves it; an invalid text cannot be saved; the menu key cancels.
#[test]
fn launch_text_is_edited_by_keyboard() {
    let mut f = Fixture::new();
    f.eightbit.launch = Some(LaunchView {
        kind: M3uLaunch::Command,
        current: None,
    });
    let mut o = opened(Screen::CoreOptions, &f);
    o.launch_edit = Some(String::new());
    o.focus_first = true;
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    f.pass_events(&mut o, 0, vec![Event::Text("RUN\"DISC".into())]);
    let enter = [true, false]
        .map(|pressed| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .to_vec();
    let actions = f.pass_events(&mut o, 0, enter.clone());
    assert_eq!(actions, vec![UiAction::SetLaunch("RUN\"DISC".into())]);
    assert!(o.launch_edit.is_none());

    f.eightbit.launch = Some(LaunchView {
        kind: M3uLaunch::Program,
        current: Some("intro".into()),
    });
    o.launch_edit = Some("A:B".into());
    o.focus_first = true;
    f.pass(&mut o, 0, &[]);
    f.pass(&mut o, 0, &[]);
    assert!(f.pass_events(&mut o, 0, enter).is_empty());
    o.back();
    assert!(o.launch_edit.is_none());
    assert_eq!(o.screen(), Some(Screen::CoreOptions));
}
