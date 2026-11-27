//! Cover art of the home grid (ADR 0014 §5, §7). A worker thread owns its own `Library` connection, the
//! `CoverCache` and the `CoverChain`: it reads cached covers and downloads missing ones, cards on screen first, then
//! the rest by title. Decoding (`decode_cover`) only happens there; the UI thread uploads textures for visible
//! cards only and keeps at most [`MAX_TEXTURES`] of them (least recently drawn evicted).
//!
//! Network: only when `covers.download` is on (off by default, an explicit user setting) and never in `--screen`
//! captures. `CoverChain::fetch` → `Ok(None)` is conclusive (recorded `Missing`); `Err` records nothing.
//! The worker is never joined: a request in flight is bounded by [`COVER_TIMEOUT`] and the thread dies with the
//! process when the library closes. A shared `CancelToken` is set on close or when downloads are switched off.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use egui::{ColorImage, TextureHandle, TextureOptions};
use emu_assets::http::{CancelToken, HttpClient, HttpConfig};
use emu_assets::{
    AssetError, CoverCache, CoverChain, CoverQuery, CoverSource, CoverSourceKind, IgdbCredentials,
    IgdbSource, LibretroThumbnails, MAX_COVER_BYTES, ScreenScraperCredentials, ScreenScraperSource,
    decode_cover,
};
use emu_library::{
    CoverKind, CoverRecord, CoverState, Game, GameId, Identification, Library, system,
};

use super::text;
use super::worker::Notify;
use crate::config::Config;
use crate::config_library::{CoversConfig, CredentialsConfig};

/// Longest side of a decoded cover (cards are 168 × 210 logical points; room for HiDPI).
pub(crate) const COVER_SIDE: u32 = 320;
/// Textures kept on the GPU (~400 KiB each at most).
pub(crate) const MAX_TEXTURES: usize = 128;
/// Total budget for one cover (whole chain, `CoverChain::fetch_with_deadline`).
pub(crate) const COVER_BUDGET: Duration = Duration::from_secs(30);
/// Per HTTP request: a request in flight is not interrupted, so the worst case for one cover is
/// [`COVER_BUDGET`] plus one request (~40 s).
pub(crate) const COVER_TIMEOUT: Duration = Duration::from_secs(10);
const COVER_CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// Token shared with the UI side; replaced by a fresh one when downloads are switched back on.
pub(crate) type SharedCancel = Arc<Mutex<CancelToken>>;
const DAY: i64 = 86_400;

pub(crate) trait CoverProvider {
    /// Drains worker replies; called once at the start of each frame.
    fn poll(&mut self, ctx: &egui::Context);
    /// Texture for `game` if it is decoded and uploaded; `None` draws the placeholder card.
    fn texture(&mut self, ctx: &egui::Context, game: &Game) -> Option<TextureHandle>;
    /// Games on screen this frame, first to load (called once per frame, cheap).
    fn visible(&mut self, games: &[GameId]);
    /// One-line status for the bottom bar (offline, queue length…), if any.
    fn status(&self) -> Option<String>;
    /// Settings may have changed (sources, credentials, download switch).
    fn configure(&mut self, _config: &Config) {}
    /// The game list changed (scan, DAT import): queue the new games.
    fn refresh(&mut self) {}
    /// « Rechercher à nouveau » : forget `Missing` records and suspended sources.
    fn retry_missing(&mut self) {}
    /// Forget the cover of one game (`Some`) or of all games (`None`) — record, cached file, texture — then
    /// search again with the configured sources. Only explicit user actions call this.
    fn reset(&mut self, _game: Option<GameId>) {}
    /// Requests still waiting for the worker (captures wait for them).
    fn busy(&self) -> bool {
        false
    }
}

/// No database: placeholder cards, nothing loaded.
pub(crate) struct Placeholder;

impl CoverProvider for Placeholder {
    fn poll(&mut self, _ctx: &egui::Context) {}

    fn texture(&mut self, _ctx: &egui::Context, _game: &Game) -> Option<TextureHandle> {
        None
    }

    fn visible(&mut self, _games: &[GameId]) {}

    fn status(&self) -> Option<String> {
        None
    }
}

// ---- settings ---------------------------------------------------------------------------------------------------

/// What the worker needs from `config.toml`. `Debug` is safe: credentials are `Secret`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverSettings {
    pub download: bool,
    pub sources: Vec<CoverSourceKind>,
    pub retry_days: u32,
    pub screenscraper: Option<ScreenScraperCredentials>,
    pub igdb: Option<IgdbCredentials>,
}

impl CoverSettings {
    /// `capture`: `--screen` (cache only, ADR 0014 design §4). Incomplete credentials drop the source.
    pub(crate) fn from_config(
        covers: &CoversConfig,
        creds: &CredentialsConfig,
        capture: bool,
    ) -> Self {
        let mut sources = Vec::new();
        for name in &covers.sources {
            match CoverSourceKind::from_name(name) {
                Some(k) if !sources.contains(&k) => sources.push(k),
                Some(_) => {}
                None => tracing::warn!("[covers] source inconnue ignorée : {name}"),
            }
        }
        let screenscraper = creds
            .screenscraper
            .as_ref()
            .map(|c| ScreenScraperCredentials {
                dev_id: c.dev_id.clone(),
                dev_password: c.dev_password.clone(),
                user: c.user.clone(),
                password: c.password.clone(),
            })
            .filter(ScreenScraperCredentials::is_complete);
        let igdb = creds
            .igdb
            .as_ref()
            .map(|c| IgdbCredentials {
                client_id: c.client_id.clone(),
                client_secret: c.client_secret.clone(),
            })
            .filter(IgdbCredentials::is_complete);
        Self {
            download: covers.download && !capture,
            sources,
            retry_days: covers.retry_days,
            screenscraper,
            igdb,
        }
    }

    /// Sources that can actually be asked (libretro needs no account).
    pub(crate) fn usable_sources(&self) -> Vec<CoverSourceKind> {
        self.sources
            .iter()
            .copied()
            .filter(|k| match k {
                CoverSourceKind::LibretroThumbnails => true,
                CoverSourceKind::ScreenScraper => self.screenscraper.is_some(),
                CoverSourceKind::Igdb => self.igdb.is_some(),
            })
            .collect()
    }
}

pub(crate) fn cover_http_config() -> HttpConfig {
    HttpConfig {
        connect_timeout: COVER_CONNECT_TIMEOUT,
        timeout: COVER_TIMEOUT,
        ..HttpConfig::default()
    }
}

/// Builds the chain of the network sources. `None` when downloads are off or the HTTP client cannot be built.
pub(crate) type ChainFactory = Box<dyn Fn(&CoverSettings) -> Option<CoverChain> + Send>;

pub(crate) fn network_chain(settings: &CoverSettings) -> Option<CoverChain> {
    if !settings.download {
        return None;
    }
    let client = match HttpClient::new(cover_http_config()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("jaquettes : client HTTP indisponible : {e}");
            return None;
        }
    };
    let mut sources: Vec<Box<dyn CoverSource>> = Vec::new();
    for kind in settings.usable_sources() {
        match kind {
            CoverSourceKind::LibretroThumbnails => sources.push(Box::new(LibretroThumbnails::new(
                client.clone(),
                LibretroThumbnails::BASE_URL,
            ))),
            CoverSourceKind::ScreenScraper => {
                if let Some(c) = &settings.screenscraper {
                    sources.push(Box::new(ScreenScraperSource::new(
                        client.clone(),
                        c.clone(),
                        ScreenScraperSource::BASE_URL,
                    )));
                }
            }
            CoverSourceKind::Igdb => {
                if let Some(c) = &settings.igdb {
                    sources.push(Box::new(IgdbSource::new(
                        client.clone(),
                        c.clone(),
                        IgdbSource::TOKEN_URL,
                        IgdbSource::API_URL,
                        IgdbSource::IMAGES_URL,
                    )));
                }
            }
        }
    }
    Some(CoverChain::new(sources))
}

// ---- worker -----------------------------------------------------------------------------------------------------

pub(crate) enum Command {
    /// Replaces the priority queue: games on screen still waiting for an answer.
    Visible(Vec<GameId>),
    Configure(CoverSettings),
    Refresh,
    RetryMissing,
    /// Explicit user reset of one game or of every game.
    Reset(Option<GameId>),
    /// The UI dropped what it knew (reset, rescan, retry, settings): replies stamped with an older generation
    /// are stale. Always sent just before the command that caused it.
    Generation(u64),
}

pub(crate) enum Reply {
    /// Reply stamped with the worker's generation (see [`Command::Generation`]).
    Loaded(GameId, ColorImage, u64),
    /// No cover to show (not found, not cached and downloads off, or error): placeholder.
    Absent(GameId, u64),
    Status(WorkerStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct WorkerStatus {
    /// Every source is suspended for network reasons.
    pub offline: bool,
    /// Downloads on, but no source could be asked (none enabled, credentials refused).
    pub no_source: bool,
    /// Covers left to download.
    pub queued: usize,
    /// Session counters of the chain (found / conclusively missing).
    pub found: u32,
    pub missing: u32,
    /// A source was judged broken: its answers are not trusted.
    pub source_broken: bool,
}

pub(crate) struct Worker {
    db: Library,
    cache: CoverCache,
    settings: CoverSettings,
    factory: ChainFactory,
    chain: Option<CoverChain>,
    front: VecDeque<GameId>,
    back: VecDeque<GameId>,
    /// Network suspended until the settings change or « Rechercher à nouveau ».
    paused: bool,
    no_source: bool,
    /// Inconclusive this session (error): not asked again until a reset.
    tried: HashSet<GameId>,
    /// Games recorded `Missing` since the last reset: undone if a source turns out to be broken
    /// (its first 404s looked like genuine absences).
    session_missing: Vec<GameId>,
    /// Last chain answer per game (`true` = found): the status counters, so a game asked again replaces
    /// its earlier result instead of adding to it.
    outcomes: HashMap<GameId, bool>,
    tx: Sender<Reply>,
    notify: Notify,
    cancel: SharedCancel,
    sent: Option<WorkerStatus>,
    generation: u64,
}

impl Worker {
    pub(crate) fn new(
        db: Library,
        covers_root: &Path,
        settings: CoverSettings,
        factory: ChainFactory,
        tx: Sender<Reply>,
        notify: Notify,
        cancel: SharedCancel,
    ) -> Self {
        let chain = factory(&settings);
        let mut w = Self {
            db,
            cache: CoverCache::new(covers_root),
            settings,
            factory,
            chain,
            front: VecDeque::new(),
            back: VecDeque::new(),
            paused: false,
            no_source: false,
            tried: HashSet::new(),
            session_missing: Vec::new(),
            outcomes: HashMap::new(),
            tx,
            notify,
            cancel,
            sent: None,
            generation: 0,
        };
        w.refresh_back();
        w
    }

    fn downloading(&self) -> bool {
        self.settings.download && self.chain.is_some() && !self.paused
    }

    fn has_work(&self) -> bool {
        !self.front.is_empty() || (self.downloading() && !self.back.is_empty())
    }

    /// Until the UI side is dropped.
    pub(crate) fn run(mut self, rx: &Receiver<Command>) {
        loop {
            self.send_status();
            let command = if self.has_work() {
                match rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            };
            match command {
                Some(c) => self.handle(c),
                None => self.step(),
            }
        }
    }

    pub(crate) fn handle(&mut self, command: Command) {
        match command {
            Command::Visible(ids) => self.front = ids.into(),
            Command::Generation(g) => self.generation = g,
            Command::Configure(settings) => {
                self.chain = (self.factory)(&settings);
                self.settings = settings;
                self.outcomes.clear();
                self.reset_session();
            }
            Command::Refresh => self.refresh_back(),
            Command::Reset(target) => self.reset_covers(target),
            Command::RetryMissing => {
                if let Err(e) = self.db.clear_missing_covers() {
                    tracing::warn!("jaquettes : {e}");
                }
                if let Some(chain) = &mut self.chain {
                    chain.reset();
                    chain.reset_stats();
                }
                self.outcomes.clear();
                self.reset_session();
            }
        }
    }

    /// Explicit reset: removes the record (so the game is "never checked") and the cached file, safest order
    /// first (read the record, delete the file, then forget the record). Blocking file system calls: this runs
    /// in the worker, never on the UI thread.
    fn reset_covers(&mut self, target: Option<GameId>) {
        match target {
            Some(id) => {
                self.outcomes.remove(&id);
                let file = self.cached_path(id);
                self.remove_cached(file);
                if let Err(e) = self.db.clear_cover(id, CoverKind::Boxart) {
                    tracing::warn!("jaquettes : {e}");
                }
            }
            None => {
                self.outcomes.clear();
                let ids: Vec<GameId> = self
                    .db
                    .games()
                    .map(|g| g.into_iter().map(|g| g.id).collect())
                    .unwrap_or_default();
                for id in ids {
                    let file = self.cached_path(id);
                    self.remove_cached(file);
                }
                if let Err(e) = self.db.clear_all_covers() {
                    tracing::warn!("jaquettes : {e}");
                }
            }
        }
        if let Some(chain) = &mut self.chain {
            chain.reset();
        }
        self.reset_session();
    }

    /// Relative path of the cached boxart file of `id`, if the record says one is cached.
    fn cached_path(&self, id: GameId) -> Option<PathBuf> {
        match self.db.cover(id, CoverKind::Boxart) {
            Ok(Some(CoverRecord {
                state: CoverState::Cached { path, .. },
                ..
            })) => Some(path),
            _ => None,
        }
    }

    fn remove_cached(&self, relative: Option<PathBuf>) {
        let Some(relative) = relative else {
            return;
        };
        if let Err(e) = self.cache.remove(&relative) {
            tracing::warn!("jaquettes : suppression de {} : {e}", relative.display());
        }
    }

    fn reset_session(&mut self) {
        self.paused = false;
        self.no_source = false;
        self.tried.clear();
        self.session_missing.clear();
        self.refresh_back();
    }

    /// A broken source invalidates the "absent" answers given earlier this session: forget them so the
    /// games are asked again. `emu-library` only offers a global purge; it is used only when this session
    /// recorded some `Missing`.
    fn undo_session_missing(&mut self) {
        if self.session_missing.is_empty() {
            return;
        }
        tracing::warn!(
            "jaquettes : source injoignable, {} absence(s) de cette session annulée(s)",
            self.session_missing.len()
        );
        for id in self.session_missing.drain(..) {
            self.outcomes.remove(&id);
        }
        if let Err(e) = self.db.clear_missing_covers() {
            tracing::warn!("jaquettes : {e}");
        }
        self.refresh_back();
    }

    /// One unit of work: a visible card first, else one background download.
    pub(crate) fn step(&mut self) {
        if let Some(id) = self.front.pop_front() {
            let reply = match self.load(id) {
                Some(image) => Reply::Loaded(id, image, self.generation),
                None => Reply::Absent(id, self.generation),
            };
            self.send(reply);
        } else if self.downloading()
            && let Some(id) = self.back.pop_front()
            && let Ok(Some(game)) = self.db.game(id)
            && self.needs_download(id)
        {
            let _ = self.fetch(&game);
        }
    }

    fn retry_before(&self) -> i64 {
        super::now_unix() - i64::from(self.settings.retry_days) * DAY
    }

    fn needs_download(&self, id: GameId) -> bool {
        match self.db.cover(id, CoverKind::Boxart) {
            Ok(Some(CoverRecord {
                state: CoverState::Cached { path, .. },
                ..
            })) => self.cache.resolve(&path).is_none_or(|p| !p.is_file()),
            Ok(Some(CoverRecord {
                state: CoverState::Missing { checked_unix },
                ..
            })) => checked_unix < self.retry_before(),
            Ok(None) => true,
            Err(_) => false,
        }
    }

    /// Cached file first; a missing or unreadable one counts as "not cached" (download again).
    fn load(&mut self, id: GameId) -> Option<ColorImage> {
        let game = self.db.game(id).ok().flatten()?;
        match self.db.cover(id, CoverKind::Boxart) {
            Ok(Some(CoverRecord {
                state: CoverState::Cached { path, .. },
                ..
            })) => {
                if let Some(image) = self.read_cached(&path) {
                    return Some(image);
                }
            }
            Ok(Some(CoverRecord {
                state: CoverState::Missing { checked_unix },
                ..
            })) if checked_unix >= self.retry_before() => return None,
            _ => {}
        }
        let bytes = self.fetch(&game)?;
        to_color_image(&bytes)
    }

    fn read_cached(&self, relative: &Path) -> Option<ColorImage> {
        let path = self.cache.resolve(relative)?;
        let file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_COVER_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > MAX_COVER_BYTES {
            return None;
        }
        to_color_image(&bytes)
    }

    /// Downloads, stores and records a cover; returns its bytes. Nothing recorded on an inconclusive error.
    fn fetch(&mut self, game: &Game) -> Option<Vec<u8>> {
        if !self.downloading() || self.tried.contains(&game.id) {
            return None;
        }
        let query = cover_query(game)?;
        let token = self.cancel.lock().map(|t| t.clone()).unwrap_or_default();
        let chain = self.chain.as_mut()?;
        let result = chain.fetch_with_deadline(&query, COVER_BUDGET, Some(&token));
        let broken = chain.stats().source_broken;
        if broken {
            self.undo_session_missing();
        }
        match result {
            Ok(Some(found)) => {
                self.outcomes.insert(game.id, true);
                let key = cover_key(game);
                match self.cache.store(&game.system, &key, &found) {
                    Ok(path) => {
                        let state = CoverState::Cached {
                            path,
                            source: found.source.name().to_owned(),
                        };
                        if let Err(e) = self.db.set_cover(game.id, CoverKind::Boxart, &state) {
                            tracing::warn!("jaquette non enregistrée : {e}");
                        }
                    }
                    Err(e) => tracing::warn!("jaquette non mise en cache : {e}"),
                }
                Some(found.bytes)
            }
            // With a broken source "not found" proves nothing: record nothing.
            Ok(None) if broken => {
                self.tried.insert(game.id);
                None
            }
            Ok(None) => {
                let state = CoverState::Missing {
                    checked_unix: super::now_unix(),
                };
                match self.db.set_cover(game.id, CoverKind::Boxart, &state) {
                    Ok(()) => {
                        self.session_missing.push(game.id);
                        self.outcomes.insert(game.id, false);
                    }
                    Err(e) => tracing::warn!("jaquette : {e}"),
                }
                None
            }
            // Closing or downloads switched off: nothing to record, nothing to suspend.
            Err(AssetError::Cancelled) => None,
            Err(e) => {
                // `AssetError` never carries a URL nor a credential. Never a `Missing` here
                // (`SourceBroken`, 3xx, quota, network: inconclusive).
                tracing::warn!(
                    "jaquette « {} » ({}) non obtenue : {e}",
                    game.title,
                    game.system
                );
                self.tried.insert(game.id);
                if matches!(e, AssetError::NoSource) {
                    self.paused = true;
                    self.no_source = true;
                } else if self.chain.as_ref().is_some_and(CoverChain::offline) {
                    self.paused = true;
                }
                None
            }
        }
    }

    /// Games needing a cover, by title (ADR 0014: the rest after the visible ones).
    fn refresh_back(&mut self) {
        self.back.clear();
        if !self.settings.download || self.chain.is_none() {
            return;
        }
        let Ok(ids) = self
            .db
            .games_needing_cover(CoverKind::Boxart, self.retry_before())
        else {
            return;
        };
        let wanted: HashSet<GameId> = ids.into_iter().collect();
        let mut games: Vec<(String, GameId)> = self
            .db
            .games()
            .unwrap_or_default()
            .into_iter()
            .filter(|g| wanted.contains(&g.id))
            .map(|g| (g.title.to_lowercase(), g.id))
            .collect();
        games.sort();
        self.back = games.into_iter().map(|(_, id)| id).collect();
    }

    pub(crate) fn status(&self) -> WorkerStatus {
        let offline =
            self.paused && !self.no_source && self.chain.as_ref().is_some_and(CoverChain::offline);
        let stats = self
            .chain
            .as_ref()
            .map(CoverChain::stats)
            .unwrap_or_default();
        let count = |v: bool| self.outcomes.values().filter(|f| **f == v).count() as u32;
        WorkerStatus {
            found: count(true),
            missing: count(false),
            source_broken: stats.source_broken,
            offline,
            no_source: self.settings.download
                && (self.no_source || self.settings.usable_sources().is_empty()),
            queued: if self.downloading() {
                self.back.len()
            } else {
                0
            },
        }
    }

    fn send_status(&mut self) {
        let status = self.status();
        if self.sent != Some(status) {
            self.sent = Some(status);
            self.send(Reply::Status(status));
        }
    }

    fn send(&self, reply: Reply) {
        if self.tx.send(reply).is_ok() {
            (self.notify)();
        }
    }
}

fn to_color_image(bytes: &[u8]) -> Option<ColorImage> {
    let image = decode_cover(bytes, COVER_SIDE).ok()?;
    let size = [
        usize::try_from(image.width).ok()?,
        usize::try_from(image.height).ok()?,
    ];
    (image.rgba.len() == size[0] * size[1] * 4)
        .then(|| ColorImage::from_rgba_unmultiplied(size, &image.rgba))
}

pub(crate) fn cover_query(game: &Game) -> Option<CoverQuery> {
    let info = system(&game.system)?;
    let file_name = game
        .entry
        .as_deref()
        .map(Path::new)
        .unwrap_or(game.path.as_path())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(CoverQuery {
        title: game.title.clone(),
        identified: game.identification != Identification::Unknown,
        file_name,
        size: game.size,
        crc32: game.crc32,
        sha1: game.sha1,
        database: info.libretro_name().unwrap_or_default().to_owned(),
        screenscraper_system: info.screenscraper,
        igdb_platform: info.igdb,
    })
}

/// `[A-Za-z0-9_-]` only (`CoverCache::store`): SHA1 in hex, else an FNV-1a hash of the path.
pub(crate) fn cover_key(game: &Game) -> String {
    match game.sha1 {
        Some(sha1) => sha1.iter().map(|b| format!("{b:02x}")).collect(),
        None => {
            let h = game
                .path
                .to_string_lossy()
                .bytes()
                .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
                });
            format!("path-{h:016x}")
        }
    }
}

// ---- UI side ----------------------------------------------------------------------------------------------------

enum Slot {
    Requested,
    Decoded(ColorImage),
    Texture { handle: TextureHandle, used: u64 },
    Absent,
}

pub(crate) struct Covers {
    tx: Sender<Command>,
    rx: Receiver<Reply>,
    slots: HashMap<GameId, Slot>,
    /// Game file each slot was filled for: SQLite reuses the ids of removed games after a rescan, so a slot whose
    /// owner changed must be dropped (user test 2026-11-24: Fahrenheit's cover shown on Night Trap).
    owners: HashMap<GameId, std::path::PathBuf>,
    frame: u64,
    textures: usize,
    sent_visible: Vec<GameId>,
    status: WorkerStatus,
    /// Bumped whenever slots are dropped on purpose; replies of an older generation are ignored.
    generation: u64,
    settings: (CoversConfig, CredentialsConfig),
    capture: bool,
    alive: bool,
    cancel: SharedCancel,
}

impl Covers {
    /// Spawns the worker; `None` if the thread cannot start.
    pub(crate) fn spawn(
        db_path: PathBuf,
        covers_root: PathBuf,
        config: &Config,
        capture: bool,
        notify: Notify,
    ) -> Option<Self> {
        let settings = CoverSettings::from_config(&config.covers, &config.credentials, capture);
        Self::spawn_with(
            db_path,
            covers_root,
            settings,
            Box::new(network_chain),
            notify,
        )
        .map(|mut c| {
            c.settings = (config.covers.clone(), config.credentials.clone());
            c.capture = capture;
            c
        })
    }

    pub(crate) fn spawn_with(
        db_path: PathBuf,
        covers_root: PathBuf,
        settings: CoverSettings,
        factory: ChainFactory,
        notify: Notify,
    ) -> Option<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();
        let cancel: SharedCancel = Arc::default();
        let worker_cancel = Arc::clone(&cancel);
        let spawned = thread::Builder::new()
            .name("emu-covers".into())
            .spawn(move || {
                // Never creates nor quarantines the database: that is the UI thread's job.
                let db = match Library::open_existing(&db_path) {
                    Ok(db) => db,
                    Err(e) => {
                        tracing::warn!("jaquettes : bibliothèque indisponible : {e}");
                        return;
                    }
                };
                Worker::new(
                    db,
                    &covers_root,
                    settings,
                    factory,
                    reply_tx,
                    notify,
                    worker_cancel,
                )
                .run(&cmd_rx);
            });
        // The handle is dropped on purpose: never joined (a download may still be running at exit).
        match spawned {
            Ok(_) => Some(Self {
                tx: cmd_tx,
                rx: reply_rx,
                slots: HashMap::new(),
                owners: HashMap::new(),
                frame: 0,
                textures: 0,
                sent_visible: Vec::new(),
                status: WorkerStatus::default(),
                generation: 0,
                settings: Default::default(),
                capture: false,
                alive: true,
                cancel,
            }),
            Err(e) => {
                tracing::error!("impossible de lancer la file des jaquettes : {e}");
                None
            }
        }
    }

    fn send(&mut self, command: Command) {
        if self.tx.send(command).is_err() {
            self.worker_gone();
        }
    }

    /// Starts a new generation: every reply already in flight becomes stale.
    fn new_generation(&mut self) {
        self.generation += 1;
        self.send(Command::Generation(self.generation));
    }

    fn worker_gone(&mut self) {
        self.alive = false;
        for slot in self.slots.values_mut() {
            if matches!(slot, Slot::Requested) {
                *slot = Slot::Absent;
            }
        }
    }

    fn evict(&mut self) {
        while self.textures > MAX_TEXTURES {
            let oldest = self
                .slots
                .iter()
                .filter_map(|(id, s)| match s {
                    Slot::Texture { used, .. } if *used < self.frame => Some((*used, *id)),
                    _ => None,
                })
                .min();
            let Some((_, id)) = oldest else {
                break;
            };
            self.slots.remove(&id);
            self.textures -= 1;
        }
    }

    #[cfg(test)]
    pub(crate) fn texture_count(&self) -> usize {
        self.textures
    }
}

impl Drop for Covers {
    fn drop(&mut self) {
        if let Ok(token) = self.cancel.lock() {
            token.cancel();
        }
    }
}

impl CoverProvider for Covers {
    fn poll(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        self.evict();
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(Reply::Loaded(id, image, generation)) => {
                    if generation == self.generation
                        && matches!(self.slots.get(&id), Some(Slot::Requested))
                    {
                        self.slots.insert(id, Slot::Decoded(image));
                        changed = true;
                    }
                }
                Ok(Reply::Absent(id, generation)) => {
                    if generation == self.generation
                        && matches!(self.slots.get(&id), Some(Slot::Requested))
                    {
                        self.slots.insert(id, Slot::Absent);
                        changed = true;
                    }
                }
                Ok(Reply::Status(s)) => {
                    changed |= self.status != s;
                    self.status = s;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.alive {
                        self.worker_gone();
                        changed = true;
                    }
                    break;
                }
            }
        }
        if changed {
            ctx.request_repaint();
        }
    }

    fn texture(&mut self, ctx: &egui::Context, game: &Game) -> Option<TextureHandle> {
        let frame = self.frame;
        match self.owners.get(&game.id) {
            Some(owner) if *owner == game.path => {}
            Some(_) => {
                if let Some(Slot::Texture { .. }) = self.slots.remove(&game.id) {
                    self.textures -= 1;
                }
                self.owners.insert(game.id, game.path.clone());
                return None; // requested again by the next `visible`
            }
            None => {
                self.owners.insert(game.id, game.path.clone());
            }
        }
        let slot = self.slots.get_mut(&game.id)?;
        match slot {
            Slot::Texture { handle, used } => {
                *used = frame;
                Some(handle.clone())
            }
            Slot::Decoded(image) => {
                let image = std::mem::take(image);
                let handle = ctx.load_texture(
                    format!("cover-{}", game.id.0),
                    image,
                    TextureOptions::LINEAR,
                );
                *slot = Slot::Texture {
                    handle: handle.clone(),
                    used: frame,
                };
                self.textures += 1;
                self.evict();
                Some(handle)
            }
            Slot::Requested | Slot::Absent => None,
        }
    }

    fn visible(&mut self, games: &[GameId]) {
        if !self.alive {
            return;
        }
        let on_screen: HashSet<GameId> = games.iter().copied().collect();
        // Off-screen requests are dropped by the worker (queue replaced); decoded images not drawn are freed.
        self.slots.retain(|id, s| {
            on_screen.contains(id) || !matches!(s, Slot::Requested | Slot::Decoded(_))
        });
        let mut wanted = Vec::new();
        for &id in games {
            let slot = self.slots.entry(id).or_insert(Slot::Requested);
            if matches!(slot, Slot::Requested) {
                wanted.push(id);
            }
        }
        if wanted != self.sent_visible {
            self.sent_visible.clone_from(&wanted);
            self.send(Command::Visible(wanted));
        }
    }

    fn status(&self) -> Option<String> {
        if !self.alive {
            return Some(text::COVERS_UNAVAILABLE.to_owned());
        }
        if self.status.offline {
            Some(text::COVERS_OFFLINE.to_owned())
        } else if self.status.no_source {
            Some(text::COVERS_NO_SOURCE.to_owned())
        } else if self.status.queued > 0 {
            Some(text::covers_queued(self.status.queued))
        } else if self.status.found + self.status.missing > 0 || self.status.source_broken {
            Some(text::covers_summary(
                self.status.found,
                self.status.missing,
                self.status.source_broken,
            ))
        } else {
            None
        }
    }

    fn configure(&mut self, config: &Config) {
        let now = (config.covers.clone(), config.credentials.clone());
        if now == self.settings {
            return;
        }
        self.settings = now;
        let settings =
            CoverSettings::from_config(&config.covers, &config.credentials, self.capture);
        // Absent cards may now be found.
        self.slots.retain(|_, s| !matches!(s, Slot::Absent));
        self.sent_visible.clear();
        if let Ok(mut token) = self.cancel.lock() {
            if !settings.download {
                token.cancel();
            } else if token.is_cancelled() {
                *token = CancelToken::new();
            }
        }
        self.new_generation();
        self.send(Command::Configure(settings));
    }

    fn refresh(&mut self) {
        self.slots.retain(|_, s| !matches!(s, Slot::Absent));
        self.sent_visible.clear();
        self.new_generation();
        self.send(Command::Refresh);
    }

    fn retry_missing(&mut self) {
        self.slots.retain(|_, s| !matches!(s, Slot::Absent));
        self.sent_visible.clear();
        self.new_generation();
        self.send(Command::RetryMissing);
    }

    fn reset(&mut self, game: Option<GameId>) {
        match game {
            Some(id) => {
                self.slots.remove(&id);
                self.owners.remove(&id);
            }
            None => {
                self.slots.clear();
                self.owners.clear();
            }
        }
        self.textures = self
            .slots
            .values()
            .filter(|s| matches!(s, Slot::Texture { .. }))
            .count();
        self.sent_visible.clear();
        self.new_generation();
        self.send(Command::Reset(game));
    }

    fn busy(&self) -> bool {
        self.alive
            && self
                .slots
                .values()
                .any(|s| matches!(s, Slot::Requested | Slot::Decoded(_)))
    }
}

#[cfg(test)]
mod tests;
