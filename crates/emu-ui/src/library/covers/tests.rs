//! Cover worker and texture cache, without network: fake `CoverSource`s and the `tests/data` fixtures.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use emu_assets::{CoverFound, ImageFormat};
use emu_library::ScanOptions;

use super::*;
use crate::config_library::{IgdbConfig, ScreenScraperConfig};

type Answers = Arc<Mutex<VecDeque<Result<Option<CoverFound>, AssetError>>>>;

struct Fake {
    answers: Answers,
    calls: Arc<AtomicUsize>,
}

impl CoverSource for Fake {
    fn kind(&self) -> CoverSourceKind {
        CoverSourceKind::LibretroThumbnails
    }

    fn fetch(&mut self, _query: &CoverQuery) -> Result<Option<CoverFound>, AssetError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answers.lock().unwrap().pop_front().unwrap_or(Ok(None))
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn png() -> Vec<u8> {
    std::fs::read(repo().join("tests/data/covers/images/valid.png")).unwrap()
}

fn found() -> CoverFound {
    CoverFound {
        source: CoverSourceKind::LibretroThumbnails,
        format: ImageFormat::Png,
        bytes: png(),
    }
}

fn library() -> Library {
    let mut db = Library::open_in_memory().unwrap();
    let options = ScanOptions {
        roots: vec![repo().join("tests/data/roms")],
        ..ScanOptions::default()
    };
    db.scan(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    db
}

fn settings(download: bool) -> CoverSettings {
    CoverSettings {
        download,
        sources: vec![CoverSourceKind::LibretroThumbnails],
        retry_days: 30,
        screenscraper: None,
        igdb: None,
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    worker: Worker,
    rx: Receiver<Reply>,
    answers: Answers,
    calls: Arc<AtomicUsize>,
}

fn rig(download: bool) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("covers");
    let answers: Answers = Arc::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let (a, c) = (Arc::clone(&answers), Arc::clone(&calls));
    let factory: ChainFactory = Box::new(move |s: &CoverSettings| {
        s.download.then(|| {
            CoverChain::new(vec![Box::new(Fake {
                answers: Arc::clone(&a),
                calls: Arc::clone(&c),
            }) as Box<dyn CoverSource>])
        })
    });
    let (tx, rx) = mpsc::channel();
    let worker = Worker::new(
        library(),
        &root,
        settings(download),
        factory,
        tx,
        Arc::new(|| {}),
        Arc::default(),
    );
    Rig {
        _tmp: tmp,
        root,
        worker,
        rx,
        answers,
        calls,
    }
}

impl Rig {
    fn first_game(&self) -> Game {
        let mut games = self.worker.db.games().unwrap();
        games.sort_by_key(|g| (g.title.to_lowercase(), g.id));
        games.into_iter().next().unwrap()
    }

    fn answer(&self, a: Result<Option<CoverFound>, AssetError>) {
        self.answers.lock().unwrap().push_back(a);
    }

    fn show(&mut self, id: GameId) -> Vec<Reply> {
        self.worker.handle(Command::Visible(vec![id]));
        self.worker.step();
        self.rx.try_iter().collect()
    }

    fn state(&self, id: GameId) -> Option<CoverState> {
        self.worker
            .db
            .cover(id, CoverKind::Boxart)
            .unwrap()
            .map(|r| r.state)
    }
}

fn loaded(replies: &[Reply], id: GameId) -> bool {
    replies
        .iter()
        .any(|r| matches!(r, Reply::Loaded(g, img) if *g == id && img.size == [4, 4]))
}

fn absent(replies: &[Reply], id: GameId) -> bool {
    replies
        .iter()
        .any(|r| matches!(r, Reply::Absent(g) if *g == id))
}

#[test]
fn found_cover_is_cached_recorded_then_read_back_without_network() {
    let mut r = rig(true);
    let game = r.first_game();
    r.answer(Ok(Some(found())));
    assert!(loaded(&r.show(game.id), game.id));
    let Some(CoverState::Cached { path, source }) = r.state(game.id) else {
        panic!("not recorded");
    };
    assert_eq!(source, "libretro");
    assert!(r.root.join(&path).is_file(), "{}", path.display());
    assert!(path.starts_with(&game.system));
    // Second time: read from the cache, the source is not asked.
    let calls = r.calls.load(Ordering::SeqCst);
    assert!(loaded(&r.show(game.id), game.id));
    assert_eq!(r.calls.load(Ordering::SeqCst), calls);
    // A deleted cache file counts as "not cached": downloaded again.
    std::fs::remove_file(r.root.join(&path)).unwrap();
    r.answer(Ok(Some(found())));
    assert!(loaded(&r.show(game.id), game.id));
    assert_eq!(r.calls.load(Ordering::SeqCst), calls + 1);
}

#[test]
fn not_found_is_recorded_missing_but_errors_record_nothing() {
    let mut r = rig(true);
    let games = r.worker.db.games().unwrap();
    let (a, b) = (games[0].id, games[1].id);
    r.answer(Ok(None));
    assert!(absent(&r.show(a), a));
    assert!(matches!(r.state(a), Some(CoverState::Missing { .. })));
    // Missing and recent: not asked again.
    let calls = r.calls.load(Ordering::SeqCst);
    assert!(absent(&r.show(a), a));
    assert_eq!(r.calls.load(Ordering::SeqCst), calls);

    r.answer(Err(AssetError::Network("délai".into())));
    assert!(absent(&r.show(b), b));
    assert_eq!(r.state(b), None);
    // « Rechercher à nouveau » forgets Missing records.
    r.worker.handle(Command::RetryMissing);
    assert_eq!(r.state(a), None);
}

#[test]
fn three_network_errors_mean_offline_and_no_more_requests() {
    let mut r = rig(true);
    let games = r.worker.db.games().unwrap();
    for g in games.iter().take(3) {
        r.answer(Err(AssetError::Network("hors ligne".into())));
        r.show(g.id);
    }
    let status = r.worker.status();
    assert!(status.offline, "{status:?}");
    assert_eq!(status.queued, 0);
    let calls = r.calls.load(Ordering::SeqCst);
    assert!(absent(&r.show(games[4].id), games[4].id));
    assert_eq!(r.calls.load(Ordering::SeqCst), calls);
    for g in &games {
        assert_eq!(r.state(g.id), None, "nothing recorded while offline");
    }
    r.worker.handle(Command::RetryMissing);
    assert!(!r.worker.status().offline);
}

#[test]
fn downloads_off_reads_the_cache_only() {
    let mut r = rig(false);
    let game = r.first_game();
    assert!(absent(&r.show(game.id), game.id));
    assert_eq!(r.calls.load(Ordering::SeqCst), 0);
    assert_eq!(r.worker.status(), WorkerStatus::default());
    assert!(!r.worker.has_work());
}

#[test]
fn background_queue_by_title_after_visible_cards() {
    let mut r = rig(true);
    let total = r.worker.db.games().unwrap().len();
    assert_eq!(r.worker.status().queued, total);
    let first = r.first_game();
    assert_eq!(r.worker.back.front(), Some(&first.id));
    r.answer(Ok(Some(found())));
    r.worker.step();
    assert!(matches!(r.state(first.id), Some(CoverState::Cached { .. })));
    // Background downloads do not decode nor send images.
    assert!(!r.rx.try_iter().any(|m| matches!(m, Reply::Loaded(..))));
}

#[test]
fn keys_and_queries() {
    let mut g = library().games().unwrap().remove(0);
    g.sha1 = Some([0xab; 20]);
    assert_eq!(cover_key(&g), "ab".repeat(20));
    g.sha1 = None;
    let k = cover_key(&g);
    assert!(k.starts_with("path-") && k.len() == 21, "{k}");
    assert!(
        k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    let q = cover_query(&g).unwrap();
    assert_eq!(q.database, system(&g.system).unwrap().database);
    assert!(!q.file_name.is_empty());
}

#[test]
fn settings_drop_incomplete_credentials_and_captures_never_download() {
    let mut config = Config::default();
    config.covers.download = true;
    config.credentials.screenscraper = Some(ScreenScraperConfig {
        dev_id: "d".into(),
        dev_password: emu_assets::Secret::new("hunter2"),
        user: String::new(),
        password: emu_assets::Secret::default(),
    });
    config.credentials.igdb = Some(IgdbConfig {
        client_id: "id".into(),
        client_secret: emu_assets::Secret::new("hunter2-igdb"),
    });
    config.covers.sources = vec![
        "igdb".into(),
        "bogus".into(),
        "libretro".into(),
        "igdb".into(),
    ];
    let s = CoverSettings::from_config(&config.covers, &config.credentials, false);
    assert!(s.download);
    assert!(s.screenscraper.is_none());
    assert_eq!(
        s.sources,
        vec![CoverSourceKind::Igdb, CoverSourceKind::LibretroThumbnails]
    );
    assert!(!format!("{s:?}").contains("hunter2"));
    let capture = CoverSettings::from_config(&config.covers, &config.credentials, true);
    assert!(!capture.download);
    assert!(network_chain(&capture).is_none());
    assert_eq!(cover_http_config().timeout, COVER_TIMEOUT);
}

fn ui_side() -> (Covers, Sender<Reply>, Receiver<Command>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (reply_tx, reply_rx) = mpsc::channel();
    let covers = Covers {
        tx: cmd_tx,
        rx: reply_rx,
        slots: HashMap::new(),
        frame: 0,
        textures: 0,
        sent_visible: Vec::new(),
        status: WorkerStatus::default(),
        settings: Default::default(),
        capture: false,
        alive: true,
        cancel: Arc::default(),
    };
    (covers, reply_tx, cmd_rx)
}

#[test]
fn textures_only_for_visible_cards_and_bounded() {
    let ctx = egui::Context::default();
    let (mut covers, tx, cmds) = ui_side();
    let games: Vec<Game> = library().games().unwrap();
    let mut game = games[0].clone();
    let ids: Vec<GameId> = (0..(MAX_TEXTURES as i64 + 20)).map(GameId).collect();
    covers.poll(&ctx);
    covers.visible(&ids);
    let Ok(Command::Visible(asked)) = cmds.try_recv() else {
        panic!("no request");
    };
    assert_eq!(asked.len(), ids.len());
    assert!(covers.busy());
    // Same list next frame: nothing sent again.
    covers.visible(&ids);
    assert!(cmds.try_recv().is_err());
    for &id in &ids {
        tx.send(Reply::Loaded(
            id,
            ColorImage::filled([2, 2], egui::Color32::RED),
        ))
        .unwrap();
    }
    covers.poll(&ctx);
    for &id in &ids {
        game.id = id;
        // Every card of this frame is drawn even past the bound...
        assert!(covers.texture(&ctx, &game).is_some());
    }
    // ...and the oldest ones are evicted on the next frames.
    covers.poll(&ctx);
    game.id = GameId(0);
    let _ = covers.texture(&ctx, &game);
    assert!(
        covers.texture_count() <= MAX_TEXTURES + 1,
        "{}",
        covers.texture_count()
    );
    assert!(!covers.busy());
    // A decoded image whose card scrolled away is freed and asked again later.
    covers.visible(&[GameId(9999)]);
    tx.send(Reply::Loaded(
        GameId(9999),
        ColorImage::filled([2, 2], egui::Color32::RED),
    ))
    .unwrap();
    covers.poll(&ctx);
    covers.visible(&[]);
    assert!(!covers.slots.contains_key(&GameId(9999)));
    // Absent cards stay placeholders and are not asked again until a retry.
    let _ = cmds.try_iter().count();
    covers.visible(&[GameId(5000)]);
    tx.send(Reply::Absent(GameId(5000))).unwrap();
    covers.poll(&ctx);
    let _ = cmds.try_iter().count();
    covers.visible(&[GameId(5000)]);
    assert!(
        cmds.try_iter()
            .all(|c| !matches!(c, Command::Visible(v) if v.contains(&GameId(5000))))
    );
    covers.retry_missing();
    assert!(cmds.try_iter().any(|c| matches!(c, Command::RetryMissing)));
    game.id = GameId(5000);
    assert!(covers.texture(&ctx, &game).is_none());
}

#[test]
fn status_lines() {
    let ctx = egui::Context::default();
    let (mut covers, tx, _cmds) = ui_side();
    assert_eq!(covers.status(), None);
    tx.send(Reply::Status(WorkerStatus {
        offline: true,
        ..WorkerStatus::default()
    }))
    .unwrap();
    covers.poll(&ctx);
    assert_eq!(covers.status().as_deref(), Some(text::COVERS_OFFLINE));
    tx.send(Reply::Status(WorkerStatus {
        queued: 3,
        ..WorkerStatus::default()
    }))
    .unwrap();
    covers.poll(&ctx);
    assert_eq!(covers.status(), Some(text::covers_queued(3)));
    drop(tx);
    covers.poll(&ctx);
    assert_eq!(covers.status().as_deref(), Some(text::COVERS_UNAVAILABLE));
}

#[test]
fn cancelled_token_stops_fetching_and_records_nothing() {
    let mut r = rig(true);
    let id = r.worker.db.games().unwrap()[0].id;
    r.answer(Ok(Some(found())));
    r.worker.cancel.lock().unwrap().cancel();
    assert!(absent(&r.show(id), id));
    assert_eq!(r.state(id), None);
    assert_eq!(r.calls.load(Ordering::SeqCst), 0);
    assert!(!r.worker.status().offline);
}

#[test]
fn worker_thread_stops_cleanly_without_database() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("absent.db");
    let mut covers = Covers::spawn_with(
        missing.clone(),
        tmp.path().join("covers"),
        settings(false),
        Box::new(|_| None),
        Arc::new(|| {}),
    )
    .unwrap();
    for _ in 0..100 {
        covers.send(Command::Refresh);
        if !covers.alive {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!covers.alive);
    assert!(!missing.exists());
    assert!(covers.status().is_some());
}
