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
        is_dir: false,
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
        game: Some(g.id),
        title: g.title.clone(),
        core: emu_assets::cores::CoreId::new("gambatte").unwrap(),
        core_path: "/x".into(),
        content: crate::GameContent::Files(vec![g.path.clone()]),
        computer: false,
        warnings: Vec::new(),
        failure_hint: None,
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
            // Tall enough for the whole settings page: rows scrolled out of view are not drawn, and the page
            // grows with the catalog (10 systems since Mega CD and the Amiga).
            egui::vec2(1280.0, 2400.0),
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
    let gb = "/metadat/no-intro/Nintendo%20-%20Game%20Boy.dat";
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
    let file = tmp.path().join("no-intro").join("Nintendo - Game Boy.dat");
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
        game: Some(g.id),
        title: g.title.clone(),
        core: CoreId::new("gambatte").unwrap(),
        core_path: "/x".into(),
        content: crate::GameContent::Files(vec![g.path.clone()]),
        computer: false,
        warnings: Vec::new(),
        failure_hint: None,
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
        game: Some(game.id),
        title: game.title.clone(),
        core: CoreId::new("mgba").unwrap(),
        core_path: PathBuf::new(),
        content: crate::GameContent::NoGame,
        computer: false,
        warnings: Vec::new(),
        failure_hint: None,
    };
    h.ui.game_started(&request);
    assert!(h.ui.awaiting_core.is_none());
}

// ---- Mega CD (design v2.1-megacd-express §5) ------------------------------------------------------------

fn megacd_request(title: &str, path: &Path) -> LaunchRequest {
    LaunchRequest {
        game: None,
        title: title.to_owned(),
        core: emu_assets::cores::CoreId::new("genesis_plus_gx").unwrap(),
        core_path: "/x".into(),
        content: crate::GameContent::Files(vec![path.to_path_buf()]),
        computer: false,
        warnings: Vec::new(),
        failure_hint: Some("indice".into()),
    }
}

fn open_megacd_dialog(h: &mut Harness, region: Option<emu_library::DiscRegion>) {
    let system = h.ui.dirs.system.clone();
    let statuses = Vec::new();
    let verdict = megacd::bios_verdict(region, &statuses, &system);
    let path = h._tmp.path().join("Dune (Europe).cue");
    h.ui.open_dialog(Dialog::MegaCdBios {
        request: megacd_request("Dune", &path),
        verdict,
        region,
        retried: false,
        copied: false,
    });
}

#[test]
fn megacd_texts_name_the_file_the_region_and_the_folder() {
    let dir = Path::new("/Users/moi/Library/Application Support/emu-rust/system");
    let title = text::megacd_missing_title(&["bios_CD_E.bin"], Some("Mega CD, Europe"));
    assert_eq!(
        title,
        "BIOS Mega CD manquant : bios_CD_E.bin (Mega CD, Europe)"
    );
    let body = text::megacd_missing_body("Dune", Some("Europe"), &["bios_CD_E.bin"], &[]);
    assert!(body.contains("« Dune » est un disque Europe"), "{body}");
    assert!(body.contains("nom exact bios_CD_E.bin"), "{body}");
    assert!(body.contains("Réessayer"), "{body}");
    assert!(body.contains(text::MEGACD_NO_DOWNLOAD));
    assert!(!body.contains("présent"), "no other BIOS: {body}");
    let body = text::megacd_missing_body(
        "Dune",
        Some("Europe"),
        &["bios_CD_E.bin"],
        &["bios_CD_U.bin"],
    );
    assert!(
        body.contains("bios_CD_U.bin est présent, mais ce disque est Europe."),
        "{body}"
    );
    let unknown = text::megacd_missing_body("X", None, &megacd::BIOS_NAMES, &[]);
    assert!(
        unknown.contains("bios_CD_E.bin (Europe), bios_CD_U.bin (USA) ou bios_CD_J.bin (Japon)"),
        "{unknown}"
    );
    let failed = text::megacd_load_failed("Dune", &["bios_CD_E.bin"], dir);
    assert!(failed.contains("bios_CD_E.bin (Europe)"), "{failed}");
    assert!(failed.contains(&dir.display().to_string()), "{failed}");
    // Never an offer to download a BIOS.
    for t in [&body, &unknown, &failed] {
        assert!(!t.contains("Télécharg"), "{t}");
    }
    // Untrusted titles are cleaned.
    let body = text::megacd_missing_body("A\u{1b}[2J", Some("USA"), &["bios_CD_U.bin"], &[]);
    assert!(!body.contains('\u{1b}'));
}

#[test]
fn megacd_dialog_retry_with_the_keyboard_launches_once_the_bios_is_there() {
    let mut h = harness(true);
    h.frame(Vec::new());
    open_megacd_dialog(&mut h, Some(emu_library::DiscRegion::Europe));
    h.frame(Vec::new());
    // Focus on « Réessayer »: Enter retries; still missing.
    assert!(h.key(Key::Enter).is_empty());
    assert!(
        matches!(
            &h.ui.dialog,
            Some(Dialog::MegaCdBios { retried: true, verdict: megacd::BiosVerdict::Missing { wanted, .. }, .. })
                if wanted == &vec!["bios_CD_E.bin"]
        ),
        "dialog kept, marked as retried"
    );
    std::fs::write(h.ui.dirs.system.join("bios_CD_E.bin"), b"bios").unwrap();
    h.frame(Vec::new());
    let events = h.key(Key::Enter);
    assert!(h.ui.dialog.is_none());
    let [LibraryEvent::Launch(request)] = events.as_slice() else {
        panic!("one launch expected: {events:?}");
    };
    assert_eq!(request.title, "Dune");
    assert_eq!(request.failure_hint.as_deref(), Some("indice"));
}

#[test]
fn megacd_dialog_escape_cancels_without_launch() {
    let mut h = harness(true);
    h.frame(Vec::new());
    open_megacd_dialog(&mut h, None);
    h.frame(Vec::new());
    assert!(h.key(Key::Escape).is_empty());
    assert!(h.ui.dialog.is_none());
}

#[test]
fn megacd_dialog_launch_anyway_is_reachable_with_tab() {
    let mut h = harness(true);
    h.frame(Vec::new());
    open_megacd_dialog(&mut h, Some(emu_library::DiscRegion::Japan));
    h.frame(Vec::new());
    // Réessayer → Copier le chemin → Annuler → Lancer quand même.
    for _ in 0..3 {
        h.key(Key::Tab);
    }
    let events = h.key(Key::Enter);
    assert!(
        matches!(events.as_slice(), [LibraryEvent::Launch(_)]),
        "{events:?}"
    );
}

#[test]
fn dat_download_skips_megacd() {
    use std::time::Duration;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
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
        .filter(|s| s.id == "megacd" || s.id == "gb")
        .cloned()
        .collect();
    assert_eq!(catalog.len(), 2, "megacd is in the catalogue");
    let mut seen = Vec::new();
    let summary = worker::download_dats(
        &mut db,
        &client,
        &format!("{}/", server.uri()),
        tmp.path(),
        &catalog,
        &AtomicBool::new(false),
        &mut |p| seen.push(p),
    );
    assert_eq!(seen.len(), 1, "megacd not even announced");
    assert_eq!(
        summary.failed.len(),
        1,
        "only the Game Boy 404: {summary:?}"
    );
    let requests = rt.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests.iter().all(|r| !r.url.path().contains("Mega-CD")),
        "no request for the Mega CD DAT"
    );
}

#[test]
fn dat_repo_root_matches_the_asset_constants() {
    assert_eq!(
        format!("{}metadat/no-intro/", worker::DAT_REPO_ROOT),
        emu_assets::DAT_BASE_URL
    );
    assert_eq!(
        format!("{}metadat/tosec/", worker::DAT_REPO_ROOT),
        emu_assets::DAT_TOSEC_BASE_URL
    );
    assert_eq!(
        format!("{}dat/", worker::DAT_REPO_ROOT),
        emu_assets::DAT_LIBRETRO_BASE_URL
    );
}

/// Écart 2.0 corrected (design v3.0 §10 #4): each `(system, source)` is fetched from its own folder and kept in
/// its own local folder; the Atari ST gets the TOSEC DAT, the Amiga both TOSEC and WHDLoad (same file name).
#[test]
fn dat_download_fetches_each_source_of_each_system() {
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    let dat = std::fs::read(repo_root().join("tests/data/dat/sample.dat")).unwrap();
    for p in [
        "/metadat/tosec/Atari%20-%20ST.dat",
        "/metadat/tosec/Commodore%20-%20Amiga.dat",
        "/dat/Commodore%20-%20Amiga.dat",
    ] {
        rt.block_on(
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(dat.clone()))
                .mount(&server),
        );
    }
    rt.block_on(
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .with_priority(5)
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
        .filter(|s| s.id == "atarist" || s.id == "amiga")
        .cloned()
        .collect();
    let summary = worker::download_dats(
        &mut db,
        &client,
        &format!("{}/", server.uri()),
        tmp.path(),
        &catalog,
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert!(summary.failed.is_empty(), "{summary:?}");
    assert_eq!(summary.updated.len(), 3, "{summary:?}");
    let requests = rt.block_on(server.received_requests()).unwrap();
    assert!(
        requests.iter().all(|r| !r.url.path().contains("no-intro")),
        "no No-Intro request for computers"
    );
    assert!(tmp.path().join("tosec/Atari - ST.dat").is_file());
    assert!(tmp.path().join("tosec/Commodore - Amiga.dat").is_file());
    assert!(tmp.path().join("libretro/Commodore - Amiga.dat").is_file());
    use emu_library::DatSource;
    assert!(
        db.dat_info_for("amiga", DatSource::Tosec)
            .unwrap()
            .is_some()
    );
    assert!(
        db.dat_info_for("amiga", DatSource::Libretro)
            .unwrap()
            .is_some()
    );
    assert!(
        db.dat_info_for("atarist", DatSource::Tosec)
            .unwrap()
            .is_some()
    );
}

#[test]
fn rom_folder_in_a_container_shows_the_path_without_opener() {
    let mut h = harness(false);
    h.ui.container = true;
    h.ui.host_roms_hint = Some("D:\\Jeux\\roms".into());
    h.ui.config.library.roots.clear();
    let roms = h.ui.dirs.root.join("roms");
    let _ = std::fs::remove_dir_all(&roms);
    h.ui.open_rom_folder();
    assert!(h.ui.folder_task.is_none(), "the opener is never called");
    assert!(roms.is_dir(), "<data>/roms created");
    match &h.ui.dialog {
        Some(Dialog::FolderPath {
            path,
            host_hint,
            container,
            ..
        }) => {
            assert_eq!(path, &roms);
            assert_eq!(host_hint.as_deref(), Some("D:\\Jeux\\roms"));
            assert!(*container);
        }
        _ => panic!("FolderPath dialog expected"),
    }
    h.frame(Vec::new());
    h.key(Key::Enter);
    assert!(h.ui.dialog.is_none(), "OK has the focus");
}

#[test]
fn several_rom_folders_ask_which_one() {
    let mut h = harness(false);
    h.ui.container = true;
    let extra = h.ui.dirs.root.join("more-roms");
    std::fs::create_dir_all(&extra).unwrap();
    h.ui.config.library.roots = vec![extra];
    h.ui.open_rom_folder();
    assert!(matches!(&h.ui.dialog, Some(Dialog::ChooseFolder(r)) if r.len() == 2));
    h.frame(Vec::new());
    h.key(Key::Enter);
    assert!(
        matches!(&h.ui.dialog, Some(Dialog::FolderPath { path, .. }) if path.ends_with("roms")),
        "first folder chosen with Entrée"
    );
}

/// Design v3.0 §2.1 / §9 q.3: no Kickstart ⇒ AROS after a warning given once per game; the game's options keep
/// `aros_acknowledged`, the second launch goes straight on.
#[test]
fn missing_kickstart_warns_once_then_launches_with_aros() {
    let mut h = harness(false);
    let game = h.ui.dirs.root.join("roms/amiga/Game.adf");
    std::fs::create_dir_all(game.parent().unwrap()).unwrap();
    std::fs::write(&game, vec![0u8; 1024]).unwrap();
    let prefs = cores_ui::preferences(&h.ui.config.cores);
    let puae = CoreId::new("puae").unwrap();
    let args = LaunchArgs {
        game_id: None,
        title: "Game".into(),
        system_id: "amiga".into(),
        ext: Some("adf".into()),
        content: GameContent::Files(vec![game.clone()]),
        computer: true,
    };
    let request = |core: CoreId, core_path: PathBuf, warnings: Vec<String>| LaunchRequest {
        game: None,
        title: "Game".into(),
        core,
        core_path,
        content: GameContent::Files(vec![game.clone()]),
        computer: true,
        warnings,
        failure_hint: None,
    };
    let ready = || LaunchCheck::Ready {
        id: puae.clone(),
        path: PathBuf::from("/cores/puae_libretro.so"),
        warnings: Vec::new(),
    };
    assert!(
        h.ui.firmware_gate(&prefs, ready(), args.clone(), &request)
            .is_none()
    );
    let Some(Dialog::Firmware3 {
        decision,
        options_path,
        ..
    }) = &h.ui.dialog
    else {
        panic!("Kickstart dialog expected");
    };
    assert!(matches!(
        decision,
        FirmwareDecision::FallbackFirstTime { wanted, .. } if wanted == "kick34005.A500"
    ));
    let options_path = options_path.clone();
    h.frame(Vec::new());
    // Focus on « Importer un Kickstart… »; Tab reaches « Continuer avec la ROM intégrée ».
    h.key(Key::Tab);
    let events = h.key(Key::Enter);
    assert!(
        events.iter().any(|e| matches!(e, LibraryEvent::Launch(_))),
        "{events:?}"
    );
    let saved = crate::game_options::GameOptions::load(&options_path);
    assert!(saved.options.input.aros_acknowledged);
    // Second launch: no dialog.
    let again = h.ui.firmware_gate(&prefs, ready(), args, &request);
    assert!(matches!(again, Some(LaunchCheck::Ready { .. })));
    assert!(h.ui.dialog.is_none());
}

#[test]
fn consoles_keep_the_info_check() {
    let mut h = harness(false);
    let prefs = cores_ui::preferences(&h.ui.config.cores);
    let args = LaunchArgs {
        game_id: None,
        title: "G".into(),
        system_id: "gb".into(),
        ext: Some("gb".into()),
        content: GameContent::Files(vec![PathBuf::from("/r/g.gb")]),
        computer: false,
    };
    let request = |core: CoreId, core_path: PathBuf, warnings: Vec<String>| LaunchRequest {
        game: None,
        title: "G".into(),
        core,
        core_path,
        content: GameContent::NoGame,
        computer: false,
        warnings,
        failure_hint: None,
    };
    let check = LaunchCheck::FirmwareMissing {
        id: CoreId::new("gambatte").unwrap(),
        path: PathBuf::from("/c.so"),
        missing: Vec::new(),
    };
    assert!(matches!(
        h.ui.firmware_gate(&prefs, check, args, &request),
        Some(LaunchCheck::FirmwareMissing { .. })
    ));
}

#[test]
fn unclassified_files_are_listed_with_their_folders() {
    let line = text::unclassified_line(Path::new("/jeux/Game.ipf"));
    assert!(line.contains("roms/atarist/"), "{line}");
    assert!(line.contains("roms/amiga/"), "{line}");
    let mut h = harness(false);
    h.ui.unclassified = vec![PathBuf::from("/jeux/Game.ipf")];
    h.ui.open_dialog(Dialog::Unclassified(h.ui.unclassified.clone()));
    h.frame(Vec::new());
    h.key(Key::Enter);
    assert!(h.ui.dialog.is_none());
}

#[test]
fn preferred_core_is_written_and_cleared() {
    let mut h = harness(true);
    let p2021 = CoreId::new("puae2021").unwrap();
    h.ui.prefer_core("Commodore - Amiga", p2021.clone());
    assert_eq!(
        h.ui.config.cores.preferred.get("amiga").map(String::as_str),
        Some("puae2021")
    );
    assert_eq!(
        cores_ui::preferences(&h.ui.config.cores)
            .for_system("Commodore - Amiga")
            .first(),
        Some(&p2021)
    );
    h.ui.prefer_core("Commodore - Amiga", CoreId::new("puae").unwrap());
    assert!(!h.ui.config.cores.preferred.contains_key("amiga"));
}

fn dos_request(dir: &Path, start: Option<&str>) -> LaunchRequest {
    LaunchRequest {
        game: None,
        title: "Jeu".to_owned(),
        core: emu_assets::cores::CoreId::new("dosbox_pure").unwrap(),
        core_path: "/x".into(),
        content: crate::GameContent::Folder {
            dir: dir.to_path_buf(),
            start: start.map(str::to_owned),
        },
        computer: true,
        warnings: Vec::new(),
        failure_hint: None,
    }
}

/// Design v4.0 §2.1: a folder game is held back while its snapshot is built (job, « Préparation du jeu… »), then
/// launched; the next launch finds the snapshot current and goes straight through.
#[test]
fn dos_folder_launch_waits_for_its_snapshot() {
    let mut h = harness(true);
    h.frame(Vec::new());
    let dir = h._tmp.path().join("roms/dos/Jeu");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("GAME.EXE"), vec![1u8; 300_000]).unwrap();
    h.ui.events
        .push(LibraryEvent::Launch(Box::new(dos_request(&dir, None))));
    assert!(
        h.frame(Vec::new()).is_empty(),
        "held back during the snapshot"
    );
    assert!(h.ui.snapshot_busy());
    let mut launched = Vec::new();
    for _ in 0..500 {
        launched = h.frame(Vec::new());
        if !launched.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        matches!(launched.as_slice(), [LibraryEvent::Launch(r)] if r.title == "Jeu"),
        "{launched:?}"
    );
    assert!(!h.ui.snapshot_busy());
    assert!(h.ui.dirs.cache_dir().join("dosbox_pure/Jeu.zip").is_file());
    h.ui.events
        .push(LibraryEvent::Launch(Box::new(dos_request(&dir, None))));
    assert_eq!(
        h.frame(Vec::new()).len(),
        1,
        "current snapshot: immediate launch"
    );
    // A program that is not at the root: refused with a message, no launch.
    h.ui.events.push(LibraryEvent::Launch(Box::new(dos_request(
        &dir,
        Some("NOPE.EXE"),
    ))));
    assert!(h.frame(Vec::new()).is_empty());
    assert!(matches!(h.ui.dialog, Some(Dialog::Message(_))));
}

#[test]
fn dos_snapshot_can_be_cancelled_with_the_keyboard() {
    let mut h = harness(true);
    h.frame(Vec::new());
    let dir = h._tmp.path().join("roms/dos/Gros");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..40 {
        std::fs::write(dir.join(format!("F{i}.DAT")), vec![0u8; 1 << 20]).unwrap();
    }
    std::fs::write(dir.join("GO.EXE"), b"MZ").unwrap();
    h.ui.events
        .push(LibraryEvent::Launch(Box::new(dos_request(&dir, None))));
    h.frame(Vec::new());
    assert!(h.ui.snapshot_busy());
    // « Annuler » has the focus: Escape or Enter cancels.
    h.key(Key::Escape);
    let mut events = Vec::new();
    for _ in 0..500 {
        events.extend(h.frame(Vec::new()));
        if !h.ui.snapshot_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!h.ui.snapshot_busy());
    // Either cancelled in time (no launch, no residue) or already finished (launch): never a half snapshot.
    let zip = h.ui.dirs.cache_dir().join("dosbox_pure/Gros.zip");
    assert_eq!(events.is_empty(), !zip.exists(), "{events:?}");
}

#[test]
fn dos_cache_size_and_clear() {
    let mut h = harness(false);
    let cache = h.ui.dirs.cache_dir().join("dosbox_pure");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("Jeu.zip"), vec![0u8; 4096]).unwrap();
    h.ui.refresh_dos_cache();
    for _ in 0..200 {
        h.ui.poll_dos_cache();
        if h.ui.dos_cache.size.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(h.ui.dos_cache.size, Some(4096));
    // Private copies of the cores (ADR 0022) live next to it and are never touched.
    let copies = h.ui.dirs.cache_dir().join("core-copies");
    std::fs::create_dir_all(&copies).unwrap();
    std::fs::write(copies.join("dosbox_pure.so"), b"core").unwrap();
    h.ui.clear_dos_cache();
    assert!(!cache.exists());
    assert!(copies.join("dosbox_pure.so").is_file());
    assert_eq!(
        h.ui.dos_cache.message.as_deref(),
        Some(text::DOS_CACHE_CLEARED)
    );
}

/// « Lancer un autre programme… » (design v4.0 §2.2), keyboard only: P on a folder game, Enter on a program, then
/// « Par défaut » remembers it in the game's options (`[dos] start`).
#[test]
fn dos_programs_dialog_by_keyboard() {
    let mut h = harness(false);
    h.frame(Vec::new());
    let dir = h._tmp.path().join("roms/dos/Jeu");
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["SETUP.EXE", "go.exe", "README.TXT"] {
        std::fs::write(dir.join(f), b"MZ").unwrap();
    }
    let mut g = game(9999, "dos", dir.to_str().unwrap(), None);
    g.is_dir = true;
    g.title = "Jeu".into();
    h.ui.games.push(g);
    h.ui.visible = vec![h.ui.games.len() - 1];
    h.ui.selected = 0;
    h.frame(Vec::new());
    h.key(Key::P);
    assert!(h.ui.programs_open());
    let d = h.ui.programs.as_ref().unwrap();
    assert_eq!(d.programs, ["GO.EXE", "SETUP.EXE"]);
    let options_path = d.options_path.clone();
    h.frame(Vec::new());
    h.key(Key::Enter); // GO.EXE
    assert_eq!(
        h.ui.programs.as_ref().unwrap().chosen.as_deref(),
        Some("GO.EXE")
    );
    h.frame(Vec::new());
    h.key(Key::Tab); // « Par défaut »
    h.key(Key::Enter);
    assert!(!h.ui.programs_open());
    let saved = crate::game_options::GameOptions::load(&options_path);
    assert_eq!(saved.options.dos.start.as_deref(), Some("GO.EXE"));
    // Escape closes without anything.
    h.frame(Vec::new());
    h.ui.dialog = None;
    h.key(Key::P);
    assert!(h.ui.programs_open());
    h.frame(Vec::new());
    h.key(Key::Escape);
    assert!(!h.ui.programs_open());
}
