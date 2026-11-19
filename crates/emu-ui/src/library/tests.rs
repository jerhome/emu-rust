//! Library screen tests: pure helpers, then `LibraryUi` driven through an egui `Context` on a temporary data
//! root holding the `tests/data/roms` fixture (no network, no window).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use egui::{Event, Key, Modifiers, RawInput};
use emu_library::{Game, GameId, Identification, Library, ScanOptions, systems};

use super::grid::{Nav, columns_for, detect_dat_system, launch_extension, step, system_counts};
use super::settings::{SecretField, secret_label, source_rows, sources_from_rows};
use super::*;
use emu_assets::Secret;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn game(id: i64, system: &str, path: &str, entry: Option<&str>) -> Game {
    Game {
        id: GameId(id),
        system: system.into(),
        path: path.into(),
        entry: entry.map(Into::into),
        title: path.into(),
        identification: Identification::Unknown,
        size: 1,
        crc32: None,
        sha1: None,
        favorite: false,
        last_played_unix: None,
        play_count: 0,
    }
}

#[test]
fn grid_navigation() {
    // 10 cards, 4 columns: rows 0-3, 4-7, 8-9.
    assert_eq!(step(0, 10, 4, 1, Nav::Left), 0);
    assert_eq!(step(3, 10, 4, 1, Nav::Right), 4);
    assert_eq!(step(9, 10, 4, 1, Nav::Right), 9);
    assert_eq!(step(1, 10, 4, 1, Nav::Up), 1);
    assert_eq!(step(5, 10, 4, 1, Nav::Up), 1);
    assert_eq!(step(1, 10, 4, 1, Nav::Down), 5);
    assert_eq!(step(7, 10, 4, 1, Nav::Down), 9, "partial last row");
    assert_eq!(step(9, 10, 4, 1, Nav::Down), 9);
    assert_eq!(step(9, 10, 4, 2, Nav::PageUp), 1);
    assert_eq!(step(0, 10, 4, 2, Nav::PageDown), 8);
    assert_eq!(step(5, 10, 4, 1, Nav::Home), 0);
    assert_eq!(step(5, 10, 4, 1, Nav::End), 9);
    assert_eq!(step(0, 0, 4, 1, Nav::Down), 0);
    assert_eq!(step(50, 3, 0, 0, Nav::Right), 2, "out of range clamped");
}

#[test]
fn columns_fit_width() {
    assert_eq!(columns_for(100.0, 168.0, 12.0), 1);
    assert_eq!(columns_for(168.0 * 2.0 + 12.0, 168.0, 12.0), 2);
    assert_eq!(columns_for(1264.0, 168.0, 12.0), 7);
}

#[test]
fn counts_and_extension() {
    let games = [
        game(1, "gb", "/r/a.gb", None),
        game(2, "gb", "/r/b.zip", Some("B.GBC")),
        game(3, "nes", "/r/c.nes", None),
    ];
    let counts = system_counts(&games, systems());
    assert!(counts.contains(&("gb".into(), 2)));
    assert!(counts.contains(&("nes".into(), 1)));
    assert!(counts.iter().all(|(_, n)| *n > 0));
    assert_eq!(launch_extension(&games[1]).as_deref(), Some("gbc"));
    assert_eq!(launch_extension(&games[0]).as_deref(), Some("gb"));
    assert_eq!(launch_extension(&game(4, "gb", "/r/noext", None)), None);
}

#[test]
fn dat_system_detection() {
    let cat = systems();
    let gb = detect_dat_system("Nintendo - Game Boy", "", cat).unwrap();
    assert_eq!(gb.id, "gb");
    let gbc = detect_dat_system("Nintendo - Game Boy Color (Parent-Clone)", "", cat).unwrap();
    assert_eq!(gbc.id, "gbc", "longest database name wins");
    let by_file = detect_dat_system("Whatever", "Sega - Game Gear", cat).unwrap();
    assert_eq!(by_file.id, "gamegear");
    assert!(detect_dat_system("Fixture - Mixed Systems", "sample", cat).is_none());
    assert!(detect_dat_system("Nintendo - Game Boyish", "", cat).is_none());
}

#[test]
fn cover_sources_rows() {
    let rows = source_rows(&["igdb".into(), "bogus".into(), "libretro".into()]);
    assert_eq!(
        rows,
        vec![
            ("igdb".into(), true),
            ("libretro".into(), true),
            ("screenscraper".into(), false)
        ]
    );
    assert_eq!(sources_from_rows(&rows), vec!["igdb", "libretro"]);
}

#[test]
fn secret_state_label() {
    assert_eq!(secret_label(&Secret::default()), text::SECRET_UNSET);
    assert_eq!(secret_label(&Secret::new("x")), text::SECRET_SET);
}

// ---- LibraryUi through egui ---------------------------------------------------------------------------------

struct Harness {
    _tmp: tempfile::TempDir,
    ctx: egui::Context,
    ui: LibraryUi,
}

/// Data root with the ROM fixture scanned; `capture` as in `--screen`.
fn harness(capture: bool) -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::under(tmp.path());
    dirs.create_all().unwrap();
    {
        let (mut db, _) = Library::open(&dirs.library_db).unwrap();
        let options = ScanOptions {
            roots: vec![repo_root().join("tests/data/roms")],
            ..ScanOptions::default()
        };
        db.scan(&options, &AtomicBool::new(false), &mut |_| {})
            .unwrap();
    }
    let mut config = Config::default();
    config.library.scan_on_start = false;
    let ui = LibraryUi::open(dirs, config, capture);
    let ctx = egui::Context::default();
    crate::ui::apply_style(&ctx);
    Harness { _tmp: tmp, ctx, ui }
}

impl Harness {
    fn frame(&mut self, events: Vec<Event>) -> Vec<LibraryEvent> {
        let raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            focused: true,
            events,
            ..RawInput::default()
        };
        let mut out = Vec::new();
        let ui = &mut self.ui;
        let mut o = self.ctx.run_ui(raw, |root| out = ui.show(root));
        o.textures_delta.clear();
        out
    }

    fn key(&mut self, key: Key) -> Vec<LibraryEvent> {
        let ev = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let mut out = self.frame(vec![ev(true)]);
        out.extend(self.frame(vec![ev(false)]));
        out
    }

    fn selected_title(&self) -> String {
        self.ui.selected_game().unwrap().title.clone()
    }
}

#[test]
fn home_lists_fixture_and_navigates_with_keys() {
    let mut h = harness(true);
    assert!(h.ui.games.len() >= 10, "{}", h.ui.games.len());
    for _ in 0..3 {
        h.frame(Vec::new());
    }
    assert!(h.ui.columns > 1);
    let grid = egui::Id::unique("library-grid-focus");
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(grid),
        "grid focused at start"
    );
    let first = h.selected_title();
    h.key(Key::ArrowRight);
    assert_eq!(h.ui.selected, 1);
    assert_ne!(h.selected_title(), first);
    h.key(Key::ArrowDown);
    assert_eq!(h.ui.selected, 1 + h.ui.columns);
    h.key(Key::Home);
    assert_eq!(h.ui.selected, 0);
    h.key(Key::End);
    assert_eq!(h.ui.selected, h.ui.visible.len() - 1);
}

#[test]
fn search_filters_and_escape_clears() {
    let mut h = harness(true);
    h.frame(Vec::new());
    h.key(Key::Slash);
    h.frame(vec![Event::Text("racer".into())]);
    h.frame(Vec::new());
    assert_eq!(h.ui.visible.len(), 1, "{:?}", h.ui.filter.search);
    assert!(h.selected_title().contains("Racer"));
    h.key(Key::Escape);
    assert!(h.ui.filter.search.is_empty());
    assert_eq!(h.ui.visible.len(), h.ui.games.len());
}

#[test]
fn system_filter_and_views() {
    let mut h = harness(true);
    h.ui.filter.system = Some("snes".into());
    h.ui.refilter(None);
    assert_eq!(h.ui.visible.len(), 2);
    h.ui.filter.system = None;
    h.ui.view = View::Recent;
    h.ui.refilter(None);
    assert!(h.ui.visible.is_empty(), "nothing played yet");
}

#[test]
fn favorite_key_persists() {
    let mut h = harness(false);
    h.frame(Vec::new());
    h.frame(Vec::new());
    let id = h.ui.selected_game().unwrap().id;
    h.key(Key::F);
    let db = h.ui.db.as_ref().unwrap();
    assert!(db.game(id).unwrap().unwrap().favorite);
}

#[test]
fn enter_without_core_asks_to_install() {
    let mut h = harness(true);
    h.frame(Vec::new());
    h.frame(Vec::new());
    let events = h.key(Key::Enter);
    assert!(events.is_empty(), "no core installed: nothing to launch");
    if h.ui.cores.manager().is_some() {
        assert!(
            matches!(h.ui.dialog, Some(Dialog::Install { .. })),
            "install proposed"
        );
        h.key(Key::Escape);
        assert!(h.ui.dialog.is_none());
    } else {
        assert!(matches!(h.ui.dialog, Some(Dialog::Message(_))));
    }
}

#[test]
fn game_over_returns_home_and_records_history() {
    let mut h = harness(false);
    h.frame(Vec::new());
    let g = h.ui.selected_game().unwrap().clone();
    h.ui.set_page(Page::Settings);
    let request = LaunchRequest {
        game: g.id,
        title: g.title.clone(),
        core: emu_assets::cores::CoreId::new("gambatte").unwrap(),
        core_path: "/x".into(),
        rom: g.path.clone(),
        warnings: Vec::new(),
    };
    h.ui.game_started(&request);
    let mut config = h.ui.config.clone();
    config.video.scale = 5;
    h.ui.game_over(Some(config), Some("boom".into()));
    assert_eq!(h.ui.page, Page::Home);
    assert_eq!(h.ui.config.video.scale, 5);
    assert!(matches!(&h.ui.dialog, Some(Dialog::Message(m)) if m == "boom"));
    h.ui.view = View::Recent;
    h.ui.refilter(None);
    assert_eq!(h.ui.visible.len(), 1);
}

#[test]
fn secrets_are_never_prefilled_nor_drawn() {
    let mut h = harness(true);
    let ss =
        h.ui.config
            .credentials
            .screenscraper
            .get_or_insert_with(Default::default);
    ss.password = Secret::new("hunter2-ui");
    h.ui.set_page(Page::Settings);
    h.ui.settings.editing = None;
    let raw = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 720.0),
        )),
        ..RawInput::default()
    };
    let ui = &mut h.ui;
    let mut out = h.ctx.run_ui(raw, |root| {
        ui.show(root);
    });
    out.textures_delta.clear();
    let drawn = format!("{:?}", out.shapes);
    assert!(!drawn.contains("hunter2"));
    assert!(drawn.contains(text::SECRET_SET) || drawn.contains("défini"));
    // « Modifier » opens an empty masked field.
    *h.ui.secret_mut(SecretField::SsPassword) = Secret::new("hunter2-ui");
    h.ui.settings.editing = Some(SecretField::SsPassword);
    assert!(h.ui.settings.secret_buf.is_empty());
}

#[test]
fn unreadable_database_does_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::under(tmp.path());
    dirs.create_all().unwrap();
    // A directory where the database file should be: cannot be opened nor quarantined as a file.
    std::fs::create_dir_all(&dirs.library_db).unwrap();
    let mut ui = LibraryUi::open(dirs, Config::default(), false);
    assert!(ui.games.is_empty());
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(RawInput::default(), |root| {
        ui.show(root);
    });
    out.textures_delta.clear();
}
