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
        media: Vec::new(),
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

#[test]
fn finished_install_relaunches_the_waiting_game() {
    let mut h = harness(true);
    h.frame(Vec::new());
    h.frame(Vec::new());
    let game = h.ui.selected_game().unwrap().clone();
    // Another core finishing does not launch anything.
    h.ui.awaiting_core = Some((CoreId::new("other").unwrap(), game.id));
    h.ui.cores
        .push_completed(CoreId::new("mgba").unwrap(), true);
    h.ui.relaunch_after_install();
    assert!(h.ui.awaiting_core.is_some());
    // A failed install forgets the game.
    h.ui.cores
        .push_completed(CoreId::new("other").unwrap(), false);
    h.ui.relaunch_after_install();
    assert!(h.ui.awaiting_core.is_none());
    // Success: the launch flow runs again (here no core is installed: the install dialog comes back).
    h.ui.awaiting_core = Some((CoreId::new("other").unwrap(), game.id));
    h.ui.cores
        .push_completed(CoreId::new("other").unwrap(), true);
    h.ui.relaunch_after_install();
    assert!(h.ui.awaiting_core.is_none());
    assert_eq!(h.ui.page, Page::Home);
    assert!(h.ui.dialog.is_some());
}

#[test]
fn dat_download_imports_then_uses_etag_only_when_the_file_exists() {
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    let dat = std::fs::read(repo_root().join("tests/data/dat/sample.dat")).unwrap();
    let gb = "/Nintendo%20-%20Game%20Boy.dat";
    rt.block_on(
        Mock::given(method("GET"))
            .and(path(gb))
            .and(header("If-None-Match", "\"v1\""))
            .respond_with(ResponseTemplate::new(304))
            .with_priority(1)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("GET"))
            .and(path(gb))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "\"v1\"")
                    .set_body_bytes(dat),
            )
            .with_priority(2)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .with_priority(3)
            .mount(&server),
    );
    let client = emu_assets::http::HttpClient::new(emu_assets::http::HttpConfig {
        allow_loopback_http: true,
        timeout: Duration::from_secs(10),
        ..emu_assets::http::HttpConfig::default()
    })
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Library::open_in_memory().unwrap();
    let catalog: Vec<_> = systems()
        .iter()
        .filter(|s| s.id == "gb" || s.id == "nes")
        .cloned()
        .collect();
    let base = format!("{}/", server.uri());
    let run = |db: &mut Library| {
        let mut seen = Vec::new();
        let summary = worker::download_dats(
            db,
            &client,
            &base,
            tmp.path(),
            &catalog,
            &AtomicBool::new(false),
            &mut |p| seen.push(p),
        );
        assert_eq!(seen.len(), 2);
        summary
    };
    let first = run(&mut db);
    assert_eq!(first.updated.len(), 1, "{first:?}");
    assert_eq!(first.failed.len(), 1, "404 for the NES DAT: {first:?}");
    assert!(!first.offline);
    let info = db.dat_info("gb").unwrap().unwrap();
    assert_eq!(info.etag.as_deref(), Some("\"v1\""));
    let file = tmp.path().join("Nintendo - Game Boy.dat");
    assert!(file.is_file());
    // Same ETag, file present: 304, nothing re-imported.
    let second = run(&mut db);
    assert_eq!(second.unchanged, 1, "{second:?}");
    // File deleted: the ETag is not sent, the DAT is downloaded again.
    std::fs::remove_file(&file).unwrap();
    let third = run(&mut db);
    assert_eq!(third.updated.len(), 1, "{third:?}");
    assert!(file.is_file());
}

#[test]
fn first_scan_without_dat_offers_the_download_once() {
    let mut h = harness(false);
    h.ui.config.library.roots = vec![repo_root().join("tests/data/roms")];
    h.frame(Vec::new());
    assert!(!h.ui.has_dat);
    h.ui.start_scan(&h.ctx.clone());
    for _ in 0..500 {
        h.frame(Vec::new());
        if h.ui.scan.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(matches!(h.ui.dialog, Some(Dialog::OfferDat)));
    // « Plus tard » (Échap): not offered again this session.
    h.key(Key::Escape);
    assert!(h.ui.dialog.is_none());
    h.ui.start_scan(&h.ctx.clone());
    for _ in 0..500 {
        h.frame(Vec::new());
        if h.ui.scan.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.ui.dialog.is_none());
}

#[test]
fn covers_are_off_by_default_and_never_on_in_captures() {
    let mut h = harness(true);
    assert!(!h.ui.config.covers.download);
    h.ui.enable_cover_downloads();
    assert!(!h.ui.config.covers.download, "capture: nothing changed");
    let mut h = harness(false);
    h.frame(Vec::new());
    h.ui.enable_cover_downloads();
    assert!(h.ui.config.covers.download);
}

#[test]
fn untrusted_titles_are_sanitized_in_messages() {
    let evil = format!("Jeu\u{1b}[2J\u{7}{}", "x".repeat(4000));
    for msg in [
        text::game_failed(&evil, &evil),
        text::install_question(&evil, &evil),
        text::firmware_question(&evil, std::slice::from_ref(&evil)),
        text::dat_imported(&evil, 1),
        text::dat_unknown_system(&evil),
        text::tampered(&evil),
        text::game_title(&evil),
        text::dat_download_done(0, 0, &[(evil.clone(), evil.clone())], false),
    ] {
        assert!(!msg.contains('\u{1b}') && !msg.contains('\u{7}'), "{msg}");
        assert!(msg.len() < 2 * 512 + 400, "{}", msg.len());
    }
}

#[test]
fn captures_never_record_history() {
    let mut h = harness(true);
    h.frame(Vec::new());
    let g = h.ui.selected_game().unwrap().clone();
    let request = LaunchRequest {
        game: g.id,
        title: g.title.clone(),
        core: CoreId::new("gambatte").unwrap(),
        core_path: "/x".into(),
        rom: g.path.clone(),
        warnings: Vec::new(),
    };
    h.ui.game_started(&request);
    let played = h.ui.db.as_ref().unwrap().game(g.id).unwrap().unwrap();
    assert_eq!(played.last_played_unix, None);
    assert_eq!(played.play_count, 0);
}

#[test]
fn secret_input_state_is_wiped_after_validation() {
    let mut h = harness(true);
    h.ui.set_page(Page::Settings);
    h.frame(Vec::new());
    let id = settings::secret_edit_id(SecretField::IgdbSecret);
    h.ctx.data_mut(|d| {
        d.insert_persisted(id, egui::text_edit::TextEditState::default());
    });
    h.ui.settings.editing = Some(SecretField::IgdbSecret);
    h.ui.settings.secret_buf = "hunter2".into();
    h.key(Key::Escape);
    assert!(h.ui.settings.editing.is_none());
    assert!(h.ui.settings.secret_buf.is_empty());
    assert!(egui::text_edit::TextEditState::load(&h.ctx, id).is_none());
}

#[test]
fn starting_a_game_forgets_the_waiting_install() {
    let mut h = harness(true);
    h.frame(Vec::new());
    let game = h.ui.selected_game().unwrap().clone();
    h.ui.awaiting_core = Some((CoreId::new("other").unwrap(), game.id));
    let request = LaunchRequest {
        game: game.id,
        title: game.title.clone(),
        core: CoreId::new("mgba").unwrap(),
        core_path: PathBuf::new(),
        rom: PathBuf::new(),
        warnings: Vec::new(),
    };
    h.ui.game_started(&request);
    assert!(h.ui.awaiting_core.is_none());
}
