//! Library mode (ADR 0014 §8, design v1.0-bibliotheque §3): home grid, settings, cores screen, and the launch
//! flow towards a game played in the same window (`host`). Drawing code never blocks: scan and DAT import run
//! in `worker` threads; only quick SQLite reads/writes (game list, favorite, last played) happen here.

mod cd32x;
mod covers;
mod custom_launch;
mod dos;
mod firmware_banner;
pub(crate) mod firmware_gate;
mod grid;
mod home;
mod host;
mod pad_settings;
mod settings;
pub(crate) mod text;
pub(crate) mod tidy;
mod wizard;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{Color32, Key, RichText, Ui};
use emu_assets::cores::FirmwareVariant;
use emu_assets::cores::{CoreId, FirmwareStatus};
use emu_library::{
    DiscRegion, Game, GameFilter, GameId, Library, OpenOutcome, ScanProgress, SortOrder,
    SystemKind, filter_games, systems,
};
use emu_runtime::ContentSpec;

use self::covers::{CoverProvider, Covers, Placeholder};
pub(crate) use self::host::run;
use self::worker::{DatDownloadTask, DatProgress, DatTask, Notify, ScanTask};
use crate::GameContent;
use crate::config::{Config, DataDirs};
use crate::config_library::COVERS_MISSING_RESET;
use crate::cores_ui::{self, CoresEvent, CoresUi, LaunchCheck};
use crate::launch_firmware::{
    FirmwareDecision, GameFacts, LaunchFirmware, check_launch_firmware, decide_launch,
    fallback_option,
};
use crate::platform::{FolderOutcome, FolderPlan, plan_open_folder, spawn_open_folder};

const WARNING: Color32 = crate::theme::WARN;
const ERROR: Color32 = crate::theme::ERROR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Home,
    Settings,
    Cores,
    Help,
    /// « Ranger les ROMs… » (ADR 0023 §4): analysis, preview, run and report.
    Tidy,
    /// « Journal »: opened by the footer button (every page) or « Ouvrir le journal » in the settings.
    Journal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum View {
    #[default]
    All,
    Favorites,
    /// Played games only, most recent first.
    Recent,
}

/// A game ready to run: verified core (marked in use), ROM path.
#[derive(Debug, Clone)]
pub(crate) struct LaunchRequest {
    /// `None` for the computer desktop (no game in the library).
    pub game: Option<GameId>,
    pub title: String,
    pub core: CoreId,
    pub core_path: PathBuf,
    pub content: GameContent,
    /// System of kind `computer` (ADR 0015).
    pub computer: bool,
    /// Shown in the game's OSD (optional BIOS missing…).
    pub warnings: Vec<String>,
    /// libretro database name of the game's system: firmware table, « Réglages » section and « CD » menu of
    /// `App::build` (a Mega Drive game under genesis_plus_gx gets no Mega CD table). `None`: from the content.
    pub system: Option<String>,
    /// `Game::region` (Mega CD BIOS of the region).
    pub region: Option<DiscRegion>,
    /// Content planned by the library (`plan_disc_launch` for a `disc = true` system); `None`: from `content`.
    pub spec: Option<ContentSpec>,
}

#[derive(Debug)]
pub(crate) enum LibraryEvent {
    Launch(Box<LaunchRequest>),
    Quit,
}

// One instance at a time: the size difference between variants costs nothing.
#[allow(clippy::large_enum_variant)]
enum Dialog {
    Install {
        core: CoreId,
        game: String,
        game_id: GameId,
        /// Mega CD + 32X disc: the message says why this core and no other.
        needs_32x: bool,
    },
    Firmware {
        request: LaunchRequest,
        missing: Vec<String>,
    },
    /// The core needs a TOS: offer the free one (EmuTOS), then launch.
    FreeBios {
        request: LaunchRequest,
        bios: usize,
        /// Download running for this dialog.
        installing: bool,
    },
    Reinstall {
        core: CoreId,
        game_id: GameId,
    },
    Message(String),
    /// Information after a user action (« CD 32X » mark): title `CD32X_TITLE`, « OK ».
    Notice(String),
    /// First scan without any DAT: offer the download (ADR 0014 design §1).
    OfferDat,
    /// 3.0 firmware check (design v3.0 §2.1): fallback warned once per game, wrong or missing file.
    Firmware3 {
        request: LaunchRequest,
        /// Boxed: keeps `Dialog` small (a `GameContent` per field).
        args: Box<LaunchArgs>,
        decision: FirmwareDecision,
        /// This game's options file (`aros_acknowledged`, « Utiliser AROS »).
        options_path: PathBuf,
        /// `(option, value)` that selects the core's built-in ROM, when declared.
        fallback: Option<(String, String)>,
        /// System, region and other files of the table (generic « BIOS manquant » dialog, design v6.0 §2.1).
        info: Box<FirmwareInfo>,
        /// « Réessayer » found nothing new.
        retried: bool,
        /// « Copier le chemin » was used.
        copied: bool,
    },
    /// Several ROM folders: which one to open.
    ChooseFolder(Vec<PathBuf>),
    /// The folder cannot be opened here (container, no file manager): its path, and the host path if known.
    FolderPath {
        path: PathBuf,
        host_hint: Option<String>,
        container: bool,
        copied: bool,
    },
    /// Files of a shared extension that the scan could not attribute (design v3.0 §3.5.2).
    Unclassified(Vec<PathBuf>),
    /// Launch of a custom system (« non testé », dry run) or the notice of an interrupted launch.
    Custom(custom_launch::CustomDialog),
}

/// What a launch was asked with: enough to start it again after a BIOS import.
#[derive(Debug, Clone)]
pub(crate) struct LaunchArgs {
    game_id: Option<GameId>,
    title: String,
    system_id: String,
    ext: Option<String>,
    content: GameContent,
    computer: bool,
    /// `Game::region`.
    region: Option<DiscRegion>,
    /// The library entry of a `disc = true` system: its content is planned by `plan_disc_launch` once the core
    /// is known (several discs, zip refused).
    disc: Option<Box<Game>>,
}

impl LaunchArgs {
    fn new(
        game_id: Option<GameId>,
        title: String,
        system_id: &str,
        ext: Option<&str>,
        content: GameContent,
        computer: bool,
    ) -> Self {
        Self {
            game_id,
            title,
            system_id: system_id.to_owned(),
            ext: ext.map(str::to_owned),
            content,
            computer,
            region: None,
            disc: None,
        }
    }
}

/// What the generic firmware dialog says besides the decision (design v6.0 §2.1): « BIOS manquant :
/// bios_CD_U.bin (Mega CD / Sega CD, USA) », « bios_CD_E.bin est présent, mais ce jeu est USA ».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FirmwareInfo {
    /// Name of the system of the table (« Mega CD / Sega CD »).
    system: Option<String>,
    /// Region of the game (« Europe »); `None` for a table that is not keyed by region, or a region unknown.
    region: Option<&'static str>,
    /// Region table and unknown region: every file of the table with its region, any of them will do.
    alternatives: Vec<String>,
    /// Other files of the table present in `system/`.
    others_present: Vec<String>,
}

/// Result of the firmware check of one launch (library dialog, « Réessayer »).
struct GatedFirmware {
    system: String,
    decision: FirmwareDecision,
    options_path: PathBuf,
    info: FirmwareInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Grid,
    Search,
}

pub(crate) struct LibraryUi {
    dirs: DataDirs,
    config: Config,
    /// `--screen` capture: no scan, no network, nothing written.
    capture: bool,
    page: Page,
    /// Page to return to when the manual closes.
    help_back: Page,
    help: crate::help::HelpState,
    /// Page to return to when the « Journal » closes.
    journal_back: Page,
    journal: crate::ui::log_panel::LogPanel,
    db: Option<Library>,
    /// Persistent banner: base recovered or unavailable.
    db_banner: Option<String>,
    games: Vec<Game>,
    has_dat: bool,
    filter: GameFilter,
    view: View,
    /// Indices into `games`, display order.
    visible: Vec<usize>,
    /// Index into `visible`.
    selected: usize,
    columns: usize,
    page_rows: usize,
    grid_offset: f32,
    grid_height: f32,
    focus: Option<Focus>,
    scroll_to_selected: bool,
    scan: Option<ScanTask>,
    scan_progress: Option<ScanProgress>,
    scan_on_open: bool,
    dat: Option<DatTask>,
    dat_download: Option<DatDownloadTask>,
    dat_progress: Option<DatProgress>,
    /// DAT download offered once per session.
    dat_offered: bool,
    /// libretro-database root, each DAT source below it (tests: a local server).
    dat_base_url: String,
    status: Option<(String, Level)>,
    dialog: Option<Dialog>,
    focus_dialog: bool,
    cores: CoresUi,
    /// Game whose launch asked for this core install: relaunched when the install succeeds.
    awaiting_core: Option<(CoreId, GameId)>,
    covers: Box<dyn CoverProvider>,
    /// The cover worker is spawned on the first frame (it needs the egui context to wake the loop).
    covers_started: bool,
    settings: settings::State,
    pad: pad_settings::PadUi,
    events: Vec<LibraryEvent>,
    /// `EMU_RUST_CONTAINER=1` (read once): the ROM folder is shown, never opened.
    container: bool,
    /// `EMU_HOST_ROMS_DIR` (read once), shown with the path in a container.
    host_roms_hint: Option<String>,
    /// « Ouvrir le dossier des ROMs » waiting for the opener (worker thread).
    folder_task: Option<std::sync::mpsc::Receiver<FolderOutcome>>,
    /// Unclassified files of the last scan (design v3.0 §3.5.2).
    unclassified: Vec<PathBuf>,
    /// DOS folder snapshot being built before a launch (design v4.0 §2.1).
    snapshot: Option<dos::SnapshotTask>,
    /// « Lancer un autre programme… » of a DOS folder game.
    programs: Option<dos::ProgramsDialog>,
    dos_cache: dos::DosCache,
    /// « Fichier d'origine » banner of the selected game (`firmware_banner`).
    advice: Option<(GameId, Option<firmware_banner::Banner>)>,
    /// First-run wizard, drawn instead of the home grid while `Some`.
    wizard: Option<wizard::Wizard>,
    tidy: tidy::TidyUi,
    /// Dry run and steps of the launches of custom systems.
    custom: custom_launch::State,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Forgets the `Missing` cover records once per purge generation (records written while a source was
/// answering 404 for everything). The new generation is saved only after the purge succeeded.
pub(crate) fn purge_missing_covers_once(db: &mut Library, config: &mut Config, path: &Path) {
    if config.library.covers_missing_reset >= COVERS_MISSING_RESET {
        return;
    }
    if let Err(e) = db.clear_missing_covers() {
        tracing::warn!("jaquettes : purge des absences impossible : {e}");
        return;
    }
    config.library.covers_missing_reset = COVERS_MISSING_RESET;
    if let Err(e) = config.save(path) {
        tracing::warn!("réglages non enregistrés après la purge des jaquettes : {e}");
    }
}

fn notify(ctx: &egui::Context) -> Notify {
    let ctx = ctx.clone();
    Arc::new(move || ctx.request_repaint())
}

impl LibraryUi {
    pub(crate) fn open(dirs: DataDirs, config: Config, capture: bool) -> Self {
        let (db, db_banner) = match Library::open(&dirs.library_db) {
            Ok((db, OpenOutcome::Recovered { quarantined })) => {
                (Some(db), Some(text::recovered(&quarantined)))
            }
            Ok((db, _)) => (Some(db), None),
            Err(e) => {
                tracing::warn!("bibliothèque indisponible : {e}");
                (None, Some(text::db_error(&e.to_string())))
            }
        };
        let mut config = config;
        let mut db = db;
        if let Some(db) = db.as_mut()
            && !capture
        {
            purge_missing_covers_once(db, &mut config, &dirs.config_file);
        }
        let cores = CoresUi::open(&dirs, &config);
        let scan_on_open = !capture && config.library.scan_on_start && db.is_some();
        let mut ui = Self {
            dirs,
            config,
            capture,
            page: Page::Home,
            help_back: Page::Home,
            help: crate::help::HelpState::default(),
            journal_back: Page::Home,
            journal: crate::ui::log_panel::LogPanel::default(),
            db,
            db_banner,
            games: Vec::new(),
            has_dat: false,
            filter: GameFilter::default(),
            view: View::All,
            visible: Vec::new(),
            selected: 0,
            columns: 1,
            page_rows: 1,
            grid_offset: 0.0,
            grid_height: 0.0,
            focus: Some(Focus::Grid),
            scroll_to_selected: false,
            scan: None,
            scan_progress: None,
            scan_on_open,
            dat: None,
            dat_download: None,
            dat_progress: None,
            dat_offered: false,
            dat_base_url: worker::DAT_REPO_ROOT.to_owned(),
            status: None,
            dialog: None,
            focus_dialog: false,
            cores,
            awaiting_core: None,
            covers: Box::new(Placeholder),
            covers_started: false,
            settings: settings::State::default(),
            pad: pad_settings::PadUi::default(),
            events: Vec::new(),
            container: crate::platform::running_in_container(),
            host_roms_hint: crate::platform::host_roms_hint(),
            folder_task: None,
            unclassified: Vec::new(),
            snapshot: None,
            programs: None,
            dos_cache: dos::DosCache::default(),
            advice: None,
            wizard: None,
            tidy: tidy::TidyUi::default(),
            custom: custom_launch::State::default(),
        };
        ui.reload_games();
        ui.show_crash_notices();
        ui
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn dirs(&self) -> &DataDirs {
        &self.dirs
    }

    pub(crate) fn set_page(&mut self, page: Page) {
        self.page = page;
        self.forget_firmware_banner();
        self.settings.focus_first = page == Page::Settings;
        if page == Page::Settings {
            self.refresh_settings_cache();
        }
        if page == Page::Home {
            self.focus = Some(Focus::Grid);
        }
    }

    /// Opens the manual at the section named by `section` (anchor or number; `None`: start).
    pub(crate) fn open_help(&mut self, section: Option<&str>) {
        if self.page != Page::Help {
            self.help_back = self.page;
        }
        self.help.open(section);
        self.set_page(Page::Help);
    }

    fn close_help(&mut self) {
        let back = if self.help_back == Page::Help {
            Page::Home
        } else {
            self.help_back
        };
        self.set_page(back);
    }

    /// Opens the « Journal » page (unread counters reset); Escape or « Retour » comes back here.
    pub(crate) fn open_journal(&mut self) {
        if self.page != Page::Journal {
            self.journal_back = self.page;
        }
        self.journal.open();
        self.set_page(Page::Journal);
    }

    fn close_journal(&mut self) {
        let back = if self.journal_back == Page::Journal {
            Page::Home
        } else {
            self.journal_back
        };
        self.set_page(back);
    }

    /// F1, `?` and Ctrl+H, unless a field or a dialog has the keyboard.
    fn help_shortcuts(&mut self, ui: &Ui) {
        let busy = self.dialog.is_some()
            || self.cores.import.is_active()
            || self.snapshot_busy()
            || self.programs_open()
            || ui.ctx().text_edit_focused();
        if self.page != Page::Help && !busy && crate::help::open_requested(ui) {
            self.open_help(None);
        }
    }

    /// Re-reads the game list (after a scan, an import, a game) and keeps the selection on the same game.
    fn reload_games(&mut self) {
        let keep = self.selected_game().map(|g| g.id);
        let Some(db) = &self.db else {
            self.games.clear();
            self.refilter(keep);
            return;
        };
        let games = db.games();
        self.has_dat = systems()
            .iter()
            .any(|s| db.dat_info(&s.id).ok().flatten().is_some());
        match games {
            Ok(games) => self.games = games,
            Err(e) => self.set_status(text::db_error(&e.to_string()), Level::Error),
        }
        self.refilter(keep);
    }

    /// Recomputes `visible` from the filter and view; the selection follows `keep` when still visible.
    fn refilter(&mut self, keep: Option<GameId>) {
        let mut filter = self.filter.clone();
        filter.favorites_only = self.view == View::Favorites;
        if self.view == View::Recent {
            filter.sort = SortOrder::LastPlayed;
        }
        let mut visible = filter_games(&self.games, &filter);
        if self.view == View::Recent {
            visible.retain(|&i| self.games[i].last_played_unix.is_some());
        }
        self.visible = visible;
        self.selected = keep
            .and_then(|id| self.visible.iter().position(|&i| self.games[i].id == id))
            .unwrap_or(0);
        self.scroll_to_selected = true;
    }

    fn selected_game(&self) -> Option<&Game> {
        self.visible
            .get(self.selected)
            .and_then(|&i| self.games.get(i))
    }

    fn set_status(&mut self, message: String, level: Level) {
        if level == Level::Error {
            tracing::warn!("{message}");
        }
        self.status = Some((message, level));
    }

    fn scan_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.dirs.root.join("roms")];
        roots.extend(self.config.library.roots.iter().cloned());
        roots
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.capture || self.scan.is_some() || self.db.is_none() {
            return;
        }
        self.scan_progress = None;
        self.scan = Some(worker::spawn_scan(
            self.dirs.library_db.clone(),
            self.scan_roots(),
            notify(ctx),
        ));
    }

    fn start_dat_import(&mut self, ctx: &egui::Context, path: PathBuf, system: Option<String>) {
        if self.capture || self.dat.is_some() || self.db.is_none() {
            return;
        }
        self.dat = Some(worker::spawn_dat_import(
            self.dirs.library_db.clone(),
            path,
            system,
            notify(ctx),
        ));
    }

    fn start_covers(&mut self, ctx: &egui::Context) {
        if std::mem::replace(&mut self.covers_started, true) || self.db.is_none() {
            return;
        }
        if let Some(c) = Covers::spawn(
            self.dirs.library_db.clone(),
            self.dirs.covers.clone(),
            &self.config,
            self.capture,
            notify(ctx),
        ) {
            self.covers = Box::new(c);
        }
    }

    /// « Mettre à jour la base (DAT) » : every catalog system, conditional GET, then import.
    fn start_dat_download(&mut self, ctx: &egui::Context) {
        if self.capture || self.dat_download.is_some() || self.db.is_none() {
            return;
        }
        self.dat_progress = None;
        self.dat_download = Some(worker::spawn_dat_download(
            self.dirs.library_db.clone(),
            self.dirs.database.clone(),
            self.dat_base_url.clone(),
            emu_assets::http::HttpConfig::default(),
            notify(ctx),
        ));
    }

    /// Drains worker events once per frame.
    fn poll_workers(&mut self) {
        let mut offer_dat = false;
        if let Some(task) = &mut self.scan {
            let (progress, done) = task.poll();
            if let Some(p) = progress.into_iter().last() {
                self.scan_progress = Some(p);
            }
            if let Some(result) = done {
                let cancelled = task.is_cancelled();
                self.scan = None;
                self.scan_progress = None;
                match result {
                    Some(Ok(report)) => {
                        for (path, e) in &report.errors {
                            tracing::info!("scan : {} : {e}", path.display());
                        }
                        if report.cautious {
                            tracing::info!("{}", text::SCAN_CAUTIOUS);
                        }
                        if report.marker_failed {
                            tracing::info!("{}", text::SCAN_MARKER_FAILED);
                        }
                        self.set_status(text::scan_done(&report), Level::Info);
                        self.unclassified = report.unclassified_paths.clone();
                        if report.unclassified > 0 {
                            tracing::info!(
                                "scan : {} fichier(s) non classé(s)",
                                report.unclassified
                            );
                        }
                        offer_dat = true;
                    }
                    Some(Err(e)) if cancelled => self.set_status(e, Level::Info),
                    Some(Err(e)) => self.set_status(text::scan_failed(&e), Level::Error),
                    None => self.set_status(text::scan_failed("tâche interrompue"), Level::Error),
                }
                self.reload_games();
                self.covers.refresh();
                if offer_dat
                    && !self.has_dat
                    && !self.games.is_empty()
                    && !self.capture
                    && self.dialog.is_none()
                    && !std::mem::replace(&mut self.dat_offered, true)
                {
                    self.open_dialog(Dialog::OfferDat);
                }
            }
        }
        if let Some(task) = &mut self.dat {
            let (_, done) = task.poll();
            if let Some(result) = done {
                self.dat = None;
                match result {
                    Some(Ok((system, n))) => {
                        let name =
                            emu_library::system(&system).map_or(system.as_str(), |s| &s.name);
                        let msg = text::dat_imported(name, n);
                        self.set_status(msg, Level::Info);
                    }
                    Some(Err(e)) => self.set_status(text::dat_failed(&e), Level::Error),
                    None => self.set_status(text::dat_failed("tâche interrompue"), Level::Error),
                }
                self.reload_games();
                self.covers.retry_missing();
                self.refresh_settings_cache();
            }
        }
        if let Some(task) = &mut self.dat_download {
            let (progress, done) = task.poll();
            if let Some(p) = progress.into_iter().last() {
                self.dat_progress = Some(p);
            }
            if let Some(result) = done {
                self.dat_download = None;
                self.dat_progress = None;
                match result {
                    Some(Ok(summary)) => {
                        let level = if summary.failed.is_empty() {
                            Level::Info
                        } else {
                            Level::Warn
                        };
                        let msg = text::dat_download_done(
                            summary.updated.len(),
                            summary.unchanged,
                            &summary.failed,
                            summary.offline,
                        );
                        self.set_status(msg, level);
                        if !summary.updated.is_empty() {
                            // Titles changed: covers recorded missing under the old names may now be found.
                            self.covers.retry_missing();
                        }
                    }
                    Some(Err(e)) => self.set_status(text::dat_failed(&e), Level::Error),
                    None => self.set_status(text::dat_failed("tâche interrompue"), Level::Error),
                }
                self.reload_games();
                self.refresh_settings_cache();
            }
        }
    }

    /// Writes `config.toml` (0600, atomic); `false` when the write failed (status line set). Locked file or
    /// capture: nothing written (a capture counts as written).
    fn save_config(&mut self) -> bool {
        if self.capture {
            return true;
        }
        match self.config.save(&self.dirs.config_file) {
            Ok(()) => {
                self.set_status(text::SAVED.to_owned(), Level::Info);
                true
            }
            Err(e) => {
                let msg = if self.config.is_locked() {
                    text::LOCKED.to_owned()
                } else {
                    text::save_failed(&e.to_string())
                };
                self.set_status(msg, Level::Warn);
                false
            }
        }
    }

    /// « Ce jeu est un CD 32X » / « Ce n'est pas un CD 32X » for the selected game (ADR 0024 §2, §4): toggles the
    /// manual mark and saves `config.toml`; a failed write undoes the mark in memory and says so.
    fn toggle_cd32x(&mut self) {
        let Some(game) = self.selected_game().cloned() else {
            return;
        };
        let manual = &self.config.library.cd32x;
        if !cd32x::can_toggle(&game, manual) {
            return;
        }
        let on = cd32x::cd32x_mark(&game, manual) != cd32x::Cd32xMark::Manual;
        let before = self.config.library.cd32x.clone();
        if !cd32x::set_manual_mark(&mut self.config.library.cd32x, &game, on) {
            return;
        }
        if !self.save_config() {
            self.config.library.cd32x = before;
            let reason = self
                .status
                .as_ref()
                .map(|(m, _)| m.clone())
                .unwrap_or_default();
            self.set_status(text::cd32x_save_failed(&game.title, &reason), Level::Error);
            return;
        }
        self.forget_firmware_banner();
        let same = cd32x::manual_key(&game).map_or(1, |k| cd32x::same_key_count(&self.games, &k));
        let message = if on {
            text::cd32x_marked(&game.title, same)
        } else {
            text::cd32x_unmarked(&game.title, same)
        };
        self.open_dialog(Dialog::Notice(message));
    }

    // ---- launch flow -------------------------------------------------------------------------------------

    fn launch_selected(&mut self) {
        let Some(game) = self.selected_game().cloned() else {
            return;
        };
        self.launch_game(game);
    }

    /// Starts again a launch that stopped on a BIOS dialog (after an import).
    fn relaunch(&mut self, args: LaunchArgs) {
        self.launch(args);
    }

    fn launch_game(&mut self, game: Game) {
        let system = emu_library::system(&game.system);
        let computer = system.is_some_and(|s| s.kind == SystemKind::Computer);
        // A DOS zip is `path` (never `entry`); a folder game is `Folder` (`[dos] start` applied by `App::build`).
        let content = crate::computer::dos::game_content(&game, computer, None);
        let ext = grid::launch_extension(&game);
        let mut args = LaunchArgs::new(
            Some(game.id),
            game.title.clone(),
            &game.system,
            ext.as_deref(),
            content,
            computer,
        );
        args.region = game.region;
        if system.is_some_and(|s| s.disc) {
            args.disc = Some(Box::new(game));
        }
        self.launch(args);
    }

    /// « Bureau Atari ST »: the machine without a disk (`ContentSpec::NoGame`), keyboard mode.
    fn launch_desktop(&mut self, system_id: &str) {
        let title = emu_library::system(system_id).map_or_else(
            || system_id.to_owned(),
            |s| text::desktop_title(system_id, &s.name),
        );
        self.launch(LaunchArgs::new(
            None,
            title,
            system_id,
            None,
            GameContent::NoGame,
            true,
        ));
    }

    /// `[cores.preferred]` of a system as the user wrote it (`None`: built-in order).
    fn explicit_core(&self, system_id: &str) -> Option<CoreId> {
        self.config
            .cores
            .preferred
            .get(system_id)
            .and_then(|c| CoreId::new(c.trim()))
    }

    fn launch(&mut self, args: LaunchArgs) {
        let Some(manager) = self.cores.manager().cloned() else {
            self.open_dialog(Dialog::Message(text::NO_MANAGER.to_owned()));
            return;
        };
        let prefs = cores_ui::preferences(&self.config.cores);
        let database = emu_library::system(&args.system_id).map(|s| s.core_key().to_owned());
        let needs_32x = args
            .disc
            .as_ref()
            .is_some_and(|g| cd32x::effective_needs_32x(g, &self.config.library.cd32x));
        let explicit = self.explicit_core(&args.system_id);
        let needs = cores_ui::GameNeeds {
            needs_32x,
            explicit: explicit.as_ref(),
        };
        let check = cores_ui::plan_launch(
            &manager,
            &prefs,
            database.as_deref(),
            args.ext.as_deref(),
            needs,
        );
        // A custom system: the BIOS files the user ignored do not stop the launch.
        let check = match emu_library::system(&args.system_id)
            .filter(|s| s.is_custom())
            .and_then(|s| emu_library::catalog().custom(&s.id))
        {
            Some(custom) => cores_ui::without_ignored(check, &custom.ignored_firmware),
            None => check,
        };
        // Disc systems (design v6.0 §2.1): the discs of the game for this core, a zip refused before anything.
        let core = match &check {
            LaunchCheck::Ready { id, .. }
            | LaunchCheck::FirmwareMissing { id, .. }
            | LaunchCheck::NeedsInstall(id) => Some(id.clone()),
            _ => None,
        };
        let spec = match (&args.disc, core) {
            (Some(game), Some(core)) => match crate::content::plan_disc_launch(game, &core, &prefs)
            {
                Ok(spec) => Some(spec),
                Err(refusal) => {
                    if !matches!(check, LaunchCheck::NeedsInstall(_)) {
                        // `prepare_launch` marked the core in use.
                        cores_ui::release(&manager);
                    }
                    let msg = text::game_failed(&args.title, refusal.message());
                    self.open_dialog(Dialog::Message(msg));
                    return;
                }
            },
            _ => None,
        };
        let title = args.title.clone();
        let game_id = args.game_id;
        let request = |core: CoreId, core_path: PathBuf, warnings: Vec<String>| LaunchRequest {
            game: args.game_id,
            title: args.title.clone(),
            core,
            core_path,
            content: args.content.clone(),
            computer: args.computer,
            warnings,
            system: database.clone(),
            region: args.region,
            spec: spec.clone(),
        };
        let Some(check) = self.gate_firmware(&prefs, check, &args, spec.as_ref(), &request) else {
            return;
        };
        match check {
            LaunchCheck::Ready { id, path, warnings } => {
                let names = firmware_names(&warnings);
                let warnings = if names.is_empty() {
                    Vec::new()
                } else {
                    vec![text::firmware_warning(&names)]
                };
                self.events
                    .push(LibraryEvent::Launch(Box::new(request(id, path, warnings))));
            }
            LaunchCheck::NeedsInstall(core) => match game_id {
                Some(game_id) => self.open_dialog(Dialog::Install {
                    core,
                    game: title.clone(),
                    game_id,
                    needs_32x,
                }),
                // The desktop is not a library game: the core is installed from the cores screen.
                None => self.open_dialog(Dialog::Message(text::desktop_needs_core(core.as_str()))),
            },
            LaunchCheck::FirmwareMissing { id, path, missing } => {
                let free = database
                    .as_deref()
                    .and_then(|db| self.cores.bios.for_missing(db, &missing));
                let request = request(id, path, Vec::new());
                match free {
                    Some(bios) => self.open_dialog(Dialog::FreeBios {
                        request,
                        bios,
                        installing: false,
                    }),
                    None => self.open_dialog(Dialog::Firmware {
                        request,
                        missing: firmware_names(&missing),
                    }),
                }
            }
            LaunchCheck::Failed(emu_assets::cores::CoreError::Tampered { id, .. }) => match game_id
            {
                Some(game_id) => self.open_dialog(Dialog::Reinstall { core: id, game_id }),
                None => self.open_dialog(Dialog::Message(text::tampered(id.as_str()))),
            },
            other => {
                let msg = other
                    .blocking_message()
                    .unwrap_or_else(|| text::LAUNCH_FAILED_TITLE.to_owned());
                self.open_dialog(Dialog::Message(text::game_failed(&title, &msg)));
            }
        }
    }

    /// The single 3.0 check of `core` for this launch (design v3.0 §3.5.5, v6.0 §2.2), gated as in 2.1 (unknown
    /// region + another BIOS present ⇒ launch). `None`: the core has neither a table nor a built-in ROM.
    fn firmware_decision(
        &self,
        prefs: &emu_assets::cores::CorePreferences,
        core: &CoreId,
        args: &LaunchArgs,
        spec: Option<&ContentSpec>,
    ) -> Option<GatedFirmware> {
        // Same options path and layers as `App::build`: `resolve_game` gives the per-game options file.
        let spec = spec
            .cloned()
            .unwrap_or_else(|| crate::content::content_spec(core.as_str(), &args.content, prefs));
        let resolved = crate::game_identity::resolve_game(core.as_str(), spec, &self.dirs);
        let loaded = crate::game_options::GameOptions::load(&resolved.options_path);
        let global = self
            .config
            .core_options
            .get(core.as_str())
            .cloned()
            .unwrap_or_default();
        let database = emu_library::system(&args.system_id).map(|s| s.core_key());
        let lf = check_launch_firmware(
            &self.dirs.system,
            prefs,
            core,
            args.content.first(),
            GameFacts {
                database,
                region: args.region,
            },
            &global,
            &loaded.options.core_options,
        )?;
        let decision = decide_launch(&lf, core.as_str(), loaded.options.input.aros_acknowledged);
        let info = firmware_info(prefs, core, &lf);
        Some(GatedFirmware {
            system: lf.system,
            decision,
            options_path: resolved.options_path,
            info,
        })
    }

    /// Cores with a `required_firmware` table or a built-in ROM (design v3.0 §2.1, §3.5.5): the single 3.0 check
    /// replaces the `.info` one. `None`: a dialog took over (the core stays marked in use until it is closed).
    fn gate_firmware(
        &mut self,
        prefs: &emu_assets::cores::CorePreferences,
        check: LaunchCheck,
        args: &LaunchArgs,
        spec: Option<&ContentSpec>,
        request: &dyn Fn(CoreId, PathBuf, Vec<String>) -> LaunchRequest,
    ) -> Option<LaunchCheck> {
        let (id, path) = match &check {
            LaunchCheck::Ready { id, path, .. } | LaunchCheck::FirmwareMissing { id, path, .. } => {
                (id.clone(), path.clone())
            }
            _ => return Some(check),
        };
        let Some(gated) = self.firmware_decision(prefs, &id, args, spec) else {
            // Neither a table nor a built-in ROM: the 1.0 `.info` check stands.
            return Some(check);
        };
        tracing::info!("BIOS de {id} avant lancement : {:?}", gated.decision);
        if gated.decision == FirmwareDecision::Launch {
            return Some(LaunchCheck::Ready {
                id,
                path,
                warnings: Vec::new(),
            });
        }
        let fallback = fallback_option(prefs, &id, &gated.system);
        self.open_dialog(Dialog::Firmware3 {
            request: request(id, path, Vec::new()),
            args: Box::new(args.clone()),
            decision: gated.decision,
            options_path: gated.options_path,
            fallback,
            info: Box::new(gated.info),
            retried: false,
            copied: false,
        });
        None
    }

    /// « Réessayer » of the firmware dialog: the same check again; the game starts as soon as it passes.
    fn firmware_retry(&mut self) {
        let Some(Dialog::Firmware3 { request, args, .. }) = &self.dialog else {
            return;
        };
        let (core, args, spec) = (request.core.clone(), (**args).clone(), request.spec.clone());
        let prefs = cores_ui::preferences(&self.config.cores);
        let fresh = self.firmware_decision(&prefs, &core, &args, spec.as_ref());
        let launch = fresh
            .as_ref()
            .is_none_or(|g| g.decision == FirmwareDecision::Launch);
        if launch {
            if let Some(Dialog::Firmware3 { request, .. }) = self.dialog.take() {
                self.focus = Some(Focus::Grid);
                self.events.push(LibraryEvent::Launch(Box::new(request)));
            }
            return;
        }
        let Some(fresh) = fresh else {
            return;
        };
        if let Some(Dialog::Firmware3 {
            decision,
            info,
            retried,
            ..
        }) = &mut self.dialog
        {
            *retried = *decision == fresh.decision && **info == fresh.info;
            *decision = fresh.decision;
            **info = fresh.info;
        }
        self.focus_dialog = true;
    }

    /// Writes one change to a game's options file (« Continuer avec AROS », « Utiliser AROS »). Capture mode: no.
    fn update_game_options(
        &mut self,
        path: &std::path::Path,
        change: impl FnOnce(&mut crate::game_options::GameOptions),
    ) {
        if self.capture {
            return;
        }
        let mut loaded = crate::game_options::GameOptions::load(path);
        change(&mut loaded.options);
        if let Err(e) = loaded.options.save(path, loaded.locked) {
            self.set_status(text::game_options_failed(&e.to_string()), Level::Warn);
        }
    }

    /// « Ouvrir le dossier des ROMs » (design v3.0 §2.3): `<data>/roms` (created if missing) and the library roots.
    fn open_rom_folder(&mut self) {
        let roots = self.scan_roots();
        match roots.as_slice() {
            [only] => {
                let only = only.clone();
                self.open_folder(only);
            }
            _ => self.open_dialog(Dialog::ChooseFolder(roots)),
        }
    }

    fn open_folder(&mut self, path: PathBuf) {
        if path == self.dirs.root.join("roms")
            && !self.capture
            && let Err(e) = std::fs::create_dir_all(&path)
        {
            tracing::warn!("{} non créé : {e}", path.display());
        }
        // The container is known before any opener call (WSL2 would otherwise try explorer.exe).
        match plan_open_folder(
            &path,
            self.container || self.capture,
            self.host_roms_hint.clone(),
        ) {
            FolderPlan::ShowPath { path, host_hint } => self.show_folder_path(path, host_hint),
            FolderPlan::TryOpen => self.folder_task = Some(spawn_open_folder(path)),
        }
    }

    fn show_folder_path(&mut self, path: PathBuf, host_hint: Option<String>) {
        let container = self.container;
        self.open_dialog(Dialog::FolderPath {
            path,
            host_hint,
            container,
            copied: false,
        });
    }

    fn poll_folder(&mut self) {
        let Some(rx) = &self.folder_task else {
            return;
        };
        let outcome = match rx.try_recv() {
            Ok(o) => o,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => FolderOutcome::ShowPath {
                path: self.dirs.root.join("roms"),
                host_hint: self.host_roms_hint.clone(),
            },
        };
        self.folder_task = None;
        match outcome {
            FolderOutcome::Opened => self.set_status(text::FOLDER_OPENED.to_owned(), Level::Info),
            FolderOutcome::ShowPath { path, host_hint } => self.show_folder_path(path, host_hint),
        }
    }

    /// The import dialog closed: a launch waiting on a BIOS dialog starts again when something was installed.
    fn import_closed(&mut self, installed: bool) {
        if installed {
            self.cores.mark_dirty();
            self.forget_firmware_banner();
        }
        if !matches!(self.dialog, Some(Dialog::Firmware3 { .. })) {
            return;
        }
        if !installed {
            self.focus_dialog = true;
            return;
        }
        if let Some(Dialog::Firmware3 { args, .. }) = self.dialog.take() {
            if let Some(m) = self.cores.manager() {
                cores_ui::release(m);
            }
            self.relaunch(*args);
        }
    }

    /// « Core utilisé » changed on the cores screen: `[cores.preferred]` written (the built-in first core is the
    /// default and is not written).
    fn prefer_core(&mut self, system_db: &str, core: CoreId) {
        let Some(info) = emu_library::systems()
            .iter()
            .find(|s| s.core_key() == system_db)
        else {
            return;
        };
        let builtin_first = emu_assets::cores::CorePreferences::builtin()
            .for_system(system_db)
            .first()
            .cloned();
        if builtin_first.as_ref() == Some(&core) {
            self.config.cores.preferred.remove(&info.id);
        } else {
            self.config
                .cores
                .preferred
                .insert(info.id.clone(), core.to_string());
        }
        self.save_config();
        self.forget_firmware_banner();
        self.cores
            .set_preferences(cores_ui::preferences(&self.config.cores));
    }

    /// An install requested by a launch dialog finished: relaunch that game (success) or forget it.
    fn relaunch_after_install(&mut self) {
        for (core, ok) in self.cores.take_completed() {
            let Some((wanted, game_id)) = self.awaiting_core.clone() else {
                continue;
            };
            if wanted != core {
                continue;
            }
            self.awaiting_core = None;
            if !ok || self.dialog.is_some() {
                continue;
            }
            if let Some(game) = self.games.iter().find(|g| g.id == game_id).cloned() {
                if self.page == Page::Settings {
                    self.leave_settings();
                }
                self.set_page(Page::Home);
                self.launch_game(game);
            }
        }
    }

    fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.focus_dialog = true;
    }

    /// The host started the game: history updated, the core screen shows it in use.
    pub(crate) fn game_started(&mut self, request: &LaunchRequest) {
        // Another game is running: a pending install must not relaunch the previous one over it.
        self.awaiting_core = None;
        if !self.capture
            && let Some(db) = &mut self.db
            && let Some(game) = request.game
            && let Err(e) = db.record_played(game, now_unix())
        {
            tracing::warn!("historique non enregistré : {e}");
        }
        self.cores.set_in_use(Some(request.core.clone()));
    }

    /// A game restarting keeps its core in use; only the configuration it edited comes back.
    pub(crate) fn config_from_game(&mut self, config: Config) {
        self.config = config;
    }

    /// Back from a game (or a failed load): core released, config taken back, error shown.
    pub(crate) fn game_over(&mut self, config: Option<Config>, error: Option<String>) {
        self.forget_firmware_banner();
        if let Some(manager) = self.cores.manager() {
            cores_ui::release(manager);
        }
        self.cores.set_in_use(None);
        if let Some(config) = config {
            self.config = config;
        }
        if let Some(e) = error {
            self.open_dialog(Dialog::Message(e));
        }
        self.reload_games();
        self.set_page(Page::Home);
    }

    /// Explicit user choice from the home bar: saved, then the worker starts downloading.
    fn enable_cover_downloads(&mut self) {
        if self.capture {
            return;
        }
        self.config.covers.download = true;
        self.save_config();
        self.covers.configure(&self.config);
    }

    fn toggle_favorite(&mut self) {
        let Some(game) = self.selected_game() else {
            return;
        };
        let (id, favorite) = (game.id, !game.favorite);
        let Some(db) = &mut self.db else {
            return;
        };
        if self.capture {
            return;
        }
        match db.set_favorite(id, favorite) {
            Ok(()) => self.reload_games(),
            Err(e) => self.set_status(e.to_string(), Level::Error),
        }
    }

    // ---- drawing -------------------------------------------------------------------------------------------

    /// One frame of the library; returns what the host must do.
    pub(crate) fn show(&mut self, ui: &mut Ui) -> Vec<LibraryEvent> {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.scan_on_open) {
            self.start_scan(&ctx);
        }
        self.start_covers(&ctx);
        self.covers.poll(&ctx);
        self.poll_workers();
        if self.page != Page::Cores {
            // The install keeps running behind the other pages; its result is still needed.
            self.cores.poll(&ctx);
        }
        if std::mem::take(&mut self.tidy.pending) {
            self.start_tidy(&ctx);
        }
        self.relaunch_after_install();
        self.bios_completed();
        self.poll_folder();
        self.help_shortcuts(ui);
        // First bottom panel: the lowest one on every page (ADR 0023 §1).
        if crate::ui::footer::panel(ui) && self.dialog.is_none() {
            self.open_journal();
        }
        match self.page {
            Page::Help => {
                if crate::help::show(ui, crate::help::manual(), &mut self.help) {
                    self.close_help();
                }
            }
            Page::Journal => {
                let escape = self.dialog.is_none() && !self.cores.import.is_active();
                let mut closed = false;
                egui::CentralPanel::default().show(ui, |ui| {
                    closed = self.journal.show(ui, escape)
                        == Some(crate::ui::log_panel::LogEvent::Close);
                });
                if closed {
                    self.close_journal();
                }
            }
            Page::Home if self.wizard.is_some() => self.wizard_page(ui),
            Page::Home => self.home(ui),
            Page::Settings => self.settings_page(ui),
            Page::Tidy => self.tidy_page(ui),
            Page::Cores => {
                self.cores.set_modal_open(self.dialog.is_some());
                let mut event = None;
                egui::CentralPanel::default().show(ui, |ui| {
                    event = self.cores.show(ui);
                });
                match event {
                    Some(CoresEvent::Back) => self.set_page(Page::Home),
                    Some(CoresEvent::PreferCore { system, core }) => {
                        self.prefer_core(&system, core)
                    }
                    Some(CoresEvent::SetCoreOption { core, key, value }) => {
                        self.config
                            .core_options
                            .entry(core)
                            .or_default()
                            .insert(key, value);
                        self.save_config();
                    }
                    None => {}
                }
            }
        }
        if self.cores.import.is_active() {
            if let Some(closed) = self.cores.import.show(&ctx) {
                self.import_closed(closed.installed);
            }
        } else if self.snapshot_busy() {
            self.snapshot_dialog(&ctx);
        } else if self.custom_trying() {
            self.try_dialog(&ctx);
        } else if self.programs_open() {
            self.programs_dialog(&ctx);
        } else {
            self.dialog_ui(&ctx);
        }
        self.gate_dos_launches(&ctx);
        self.gate_custom_launches();
        std::mem::take(&mut self.events)
    }

    fn status_line(&self, ui: &mut Ui) {
        if let Some(b) = &self.db_banner {
            crate::copyable::message(ui, WARNING, b);
        }
        if let Some(p) = &self.dat_progress {
            ui.label(text::dat_downloading(&p.system, p.done, p.total));
        } else if self.dat_download.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text::DAT_DOWNLOAD);
            });
        }
        if let Some(p) = &self.scan_progress {
            ui.label(text::scan_running(p.files_seen, p.hashed));
        } else if self.scan.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text::scan_running(0, 0));
            });
        }
        if let Some((msg, level)) = &self.status {
            let color = match level {
                Level::Info => ui.visuals().text_color(),
                Level::Warn => WARNING,
                Level::Error => ERROR,
            };
            if matches!(level, Level::Info) {
                ui.add(
                    egui::Label::new(egui::RichText::new(msg.as_str()).color(color))
                        .selectable(true),
                );
            } else {
                crate::copyable::message(ui, color, msg);
            }
        }
    }

    /// Free BIOS installs that finished (cores screen or launch dialog): variant remembered, game launched.
    fn bios_completed(&mut self) {
        for done in self.cores.bios.take_done() {
            self.cores.mark_dirty();
            self.forget_firmware_banner();
            if done.ok && done.bios == "emutos" && !self.capture {
                self.config.computer.emutos_variant = Some(done.variant.clone());
                self.save_config();
            }
            if let Some(Dialog::FreeBios {
                installing: true, ..
            }) = &self.dialog
            {
                let Some(Dialog::FreeBios { request, .. }) = self.dialog.take() else {
                    continue;
                };
                if done.ok {
                    self.events.push(LibraryEvent::Launch(Box::new(request)));
                } else {
                    if let Some(m) = self.cores.manager() {
                        cores_ui::release(m);
                    }
                    let msg = done.error.unwrap_or_default();
                    self.open_dialog(Dialog::Message(text::launch_failed(&request.title, &msg)));
                }
            }
        }
    }

    fn free_bios_dialog(&mut self, ctx: &egui::Context) {
        let Some(Dialog::FreeBios {
            request,
            bios,
            installing,
        }) = &self.dialog
        else {
            return;
        };
        let (index, installing, title) = (*bios, *installing, request.title.clone());
        let Some(info) = self.cores.bios.bios(index).cloned() else {
            return;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let progress = self.cores.bios.progress();
        let (mut install, mut cancel, mut anyway) = (false, escape, false);
        let bios_ui = &mut self.cores.bios;
        egui::Modal::new(egui::Id::unique("library-bios-dialog")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.heading(text::LAUNCH_FIRMWARE_TITLE);
            ui.label(cores_ui::text::bios_offer(
                &info.name,
                &info.license,
                &title,
            ));
            ui.horizontal(|ui| {
                ui.label(cores_ui::text::BIOS_VARIANT);
                bios_ui.variant_picker(ui, index, !installing);
            });
            ui.add_space(8.0);
            if installing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(cores_ui::text::BIOS_INSTALLING);
                    if let Some(p) = progress {
                        ui.label(cores_ui::text::progress(p.done, p.total));
                    }
                    let c = ui.button(text::LAUNCH_CANCEL);
                    if focus {
                        c.request_focus();
                    }
                    cancel |= c.clicked();
                });
                return;
            }
            ui.horizontal(|ui| {
                install = ui.button(text::BIOS_INSTALL_AND_LAUNCH).clicked();
                let c = ui.button(text::LAUNCH_CANCEL);
                if focus {
                    c.request_focus();
                }
                cancel |= c.clicked();
                anyway = ui.button(text::LAUNCH_ANYWAY).clicked();
            });
            ui.label(RichText::new(cores_ui::text::BIOS_NOTE).small().weak());
        });
        if install && !self.cores.bios.busy() {
            self.cores
                .bios
                .start_install(index, emu_assets::bios::ReplacePolicy::Refuse, ctx);
            if let Some(Dialog::FreeBios { installing, .. }) = &mut self.dialog {
                *installing = true;
            }
            self.focus_dialog = true;
        } else if anyway {
            if let Some(Dialog::FreeBios { request, .. }) = self.dialog.take() {
                self.events.push(LibraryEvent::Launch(Box::new(request)));
            }
        } else if cancel {
            if installing {
                self.cores.bios.cancel();
            }
            self.dialog = None;
            self.focus = Some(Focus::Grid);
            if let Some(m) = self.cores.manager() {
                cores_ui::release(m);
            }
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        if matches!(self.dialog, Some(Dialog::FreeBios { .. })) {
            return self.free_bios_dialog(ctx);
        }
        if matches!(self.dialog, Some(Dialog::Firmware3 { .. })) {
            return self.firmware3_dialog(ctx);
        }
        if matches!(self.dialog, Some(Dialog::Custom(_))) {
            return self.custom_dialog(ctx);
        }
        if matches!(
            self.dialog,
            Some(Dialog::ChooseFolder(_) | Dialog::FolderPath { .. } | Dialog::Unclassified(_))
        ) {
            return self.folder_dialog(ctx);
        }
        let Some(dialog) = &self.dialog else {
            return;
        };
        enum Choice {
            Close,
            Install(CoreId, GameId),
            Launch,
            DownloadDat,
        }
        let mut choice = None;
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let manager = self.cores.manager().cloned();
        egui::Modal::new(egui::Id::unique("library-dialog")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            let (title, body) = match dialog {
                Dialog::Install {
                    core,
                    game,
                    needs_32x,
                    ..
                } => {
                    let name = manager
                        .as_ref()
                        .and_then(|m| m.info(core))
                        .map_or_else(|| core.to_string(), |i| i.display_name);
                    let body = if *needs_32x {
                        text::install_question_32x(&name, game)
                    } else {
                        text::install_question(&name, game)
                    };
                    (text::LAUNCH_INSTALL_TITLE, body)
                }
                Dialog::Firmware { request, missing } => (
                    text::LAUNCH_FIRMWARE_TITLE,
                    format!(
                        "{}\n{}",
                        text::firmware_question(&request.title, missing),
                        text::FIRMWARE_NOTE
                    ),
                ),
                Dialog::Reinstall { core, .. } => {
                    (text::LAUNCH_FAILED_TITLE, text::tampered(core.as_str()))
                }
                Dialog::Message(m) => (text::LAUNCH_FAILED_TITLE, m.clone()),
                Dialog::Notice(m) => (text::CD32X_TITLE, m.clone()),
                Dialog::FreeBios { .. }
                | Dialog::Firmware3 { .. }
                | Dialog::Custom(_)
                | Dialog::ChooseFolder(_)
                | Dialog::FolderPath { .. }
                | Dialog::Unclassified(_) => (text::LAUNCH_FAILED_TITLE, String::new()),
                Dialog::OfferDat => (text::DAT_OFFER_TITLE, text::DAT_OFFER.to_owned()),
            };
            ui.heading(title);
            // Selectable: an error can be copied into a report (ADR 0023 §1).
            ui.add(egui::Label::new(&body).selectable(true));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let first = match dialog {
                    Dialog::Install { core, game_id, .. } => {
                        let r = ui.button(text::LAUNCH_INSTALL);
                        if r.clicked() {
                            choice = Some(Choice::Install(core.clone(), *game_id));
                        }
                        Some(r)
                    }
                    Dialog::Reinstall { core, game_id } => {
                        let r = ui.button(text::LAUNCH_REINSTALL);
                        if r.clicked() {
                            choice = Some(Choice::Install(core.clone(), *game_id));
                        }
                        Some(r)
                    }
                    Dialog::OfferDat => {
                        let r = ui.button(text::DAT_OFFER_YES);
                        if r.clicked() {
                            choice = Some(Choice::DownloadDat);
                        }
                        Some(r)
                    }
                    Dialog::Firmware { .. }
                    | Dialog::Message(_)
                    | Dialog::Notice(_)
                    | Dialog::FreeBios { .. }
                    | Dialog::Firmware3 { .. }
                    | Dialog::Custom(_)
                    | Dialog::ChooseFolder(_)
                    | Dialog::FolderPath { .. }
                    | Dialog::Unclassified(_) => None,
                };
                // Safe default first: « Annuler » / « OK » gets the focus, never the launch.
                let cancel_label = match dialog {
                    Dialog::Message(_) | Dialog::Notice(_) => text::OK,
                    Dialog::OfferDat => text::DAT_OFFER_LATER,
                    _ => text::LAUNCH_CANCEL,
                };
                let cancel = ui.button(cancel_label);
                if cancel.clicked() {
                    choice = Some(Choice::Close);
                }
                if matches!(dialog, Dialog::Firmware { .. })
                    && ui.button(text::LAUNCH_ANYWAY).clicked()
                {
                    choice = Some(Choice::Launch);
                }
                if matches!(dialog, Dialog::Message(_)) {
                    crate::copyable::copy_button(ui, &body);
                }
                if focus {
                    first.unwrap_or(cancel).request_focus();
                }
            });
        });
        if escape && choice.is_none() {
            choice = Some(Choice::Close);
        }
        let Some(choice) = choice else {
            return;
        };
        let dialog = self.dialog.take();
        self.focus = Some(Focus::Grid);
        match (choice, dialog) {
            (Choice::Install(core, game), _) => {
                self.awaiting_core = Some((core.clone(), game));
                self.cores.request_install(core, ctx);
                self.set_page(Page::Cores);
            }
            (Choice::DownloadDat, _) => self.start_dat_download(ctx),
            (Choice::Launch, Some(Dialog::Firmware { request, .. })) => {
                self.events.push(LibraryEvent::Launch(Box::new(request)));
            }
            (_, Some(Dialog::Firmware { .. })) => {
                // Cancelled: the core was marked in use by `prepare_launch`.
                if let Some(m) = self.cores.manager() {
                    cores_ui::release(m);
                }
            }
            _ => {}
        }
    }

    /// Firmware dialog (design v3.0 §2.1, generic since v6.0 §2.1: exact name, region, `system/` folder). Safe
    /// default first: « Réessayer » for a missing file (2.1), « Importer… » otherwise; never the launch.
    fn firmware3_dialog(&mut self, ctx: &egui::Context) {
        let Some(Dialog::Firmware3 {
            request,
            decision,
            fallback,
            info,
            retried,
            copied,
            ..
        }) = &self.dialog
        else {
            return;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let (title, body, wanted) = match decision {
            FirmwareDecision::FallbackFirstTime {
                label,
                wanted,
                others,
            } => (
                text::fallback_title(wanted),
                text::fallback_body(&request.title, label, wanted, others),
                wanted.clone(),
            ),
            FirmwareDecision::WrongHash { wanted, .. } => (
                text::LAUNCH_FIRMWARE_TITLE.to_owned(),
                text::wrong_firmware_body(&request.title, wanted),
                wanted.clone(),
            ),
            FirmwareDecision::Missing { wanted } => (
                text::firmware_missing_title(
                    wanted,
                    info.system.as_deref(),
                    info.region,
                    !info.alternatives.is_empty(),
                ),
                text::firmware_missing_body(
                    &request.title,
                    info.region,
                    wanted,
                    &info.alternatives,
                    &info.others_present,
                ),
                wanted.clone(),
            ),
            FirmwareDecision::Unreadable { wanted, error } => (
                text::LAUNCH_FIRMWARE_TITLE.to_owned(),
                text::unreadable_firmware_body(wanted, error),
                wanted.clone(),
            ),
            FirmwareDecision::Launch => (String::new(), String::new(), String::new()),
        };
        let first_time = matches!(decision, FirmwareDecision::FallbackFirstTime { .. });
        let missing = matches!(decision, FirmwareDecision::Missing { .. });
        let wrong = matches!(decision, FirmwareDecision::WrongHash { .. });
        let can_fallback = wrong && fallback.is_some();
        let kickstart = wanted.to_ascii_lowercase().starts_with("kick");
        let dir = text::system_dir_text(&self.dirs.system);
        let (retried, copied) = (*retried, *copied);
        let (mut import, mut cont, mut cancel, mut anyway, mut use_fallback) =
            (false, false, escape, false, false);
        let (mut retry, mut copy) = (false, false);
        egui::Modal::new(egui::Id::unique("library-firmware3-dialog")).show(ctx, |ui| {
            ui.set_max_width(600.0);
            ui.heading(&title);
            ui.label(&body);
            ui.add_space(6.0);
            // Selectable: the path can also be copied with the mouse or Ctrl+C.
            ui.add(egui::Label::new(RichText::new(&dir).monospace()).selectable(true));
            if retried {
                ui.colored_label(WARNING, text::FIRMWARE_STILL_MISSING);
            }
            if copied {
                weak(ui, text::PATH_COPIED);
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let label = if kickstart {
                    cores_ui::text::IMPORT_KICKSTART
                } else {
                    cores_ui::text::IMPORT_BUTTON
                };
                let first = ui.button(label);
                import = first.clicked();
                let mut default = first;
                if !first_time {
                    let r = ui.button(text::FIRMWARE_RETRY);
                    retry = r.clicked();
                    copy = ui.button(text::COPY_PATH).clicked();
                    if missing {
                        default = r;
                    }
                }
                if first_time {
                    cont = ui.button(text::FALLBACK_CONTINUE).clicked();
                }
                cancel |= ui.button(text::LAUNCH_CANCEL).clicked();
                if !first_time {
                    anyway = ui.button(text::LAUNCH_ANYWAY).clicked();
                }
                if can_fallback {
                    use_fallback = ui.button(text::USE_FALLBACK).clicked();
                }
                if focus {
                    default.request_focus();
                }
            });
            if first_time {
                weak(ui, text::FALLBACK_NOTE);
            } else {
                weak(ui, text::FIRMWARE_NO_DOWNLOAD);
            }
        });
        if import {
            self.cores.import.start(Some(wanted));
        } else if copy {
            ctx.copy_text(self.dirs.system.display().to_string());
            if let Some(Dialog::Firmware3 { copied, .. }) = &mut self.dialog {
                *copied = true;
            }
        } else if retry {
            self.firmware_retry();
        } else if cont || anyway || use_fallback {
            let Some(Dialog::Firmware3 {
                request,
                options_path,
                fallback,
                ..
            }) = self.dialog.take()
            else {
                return;
            };
            if cont {
                self.update_game_options(&options_path, |o| o.input.aros_acknowledged = true);
            } else if use_fallback && let Some((option, value)) = fallback {
                self.update_game_options(&options_path, |o| {
                    o.core_options.insert(option, value);
                });
            }
            self.focus = Some(Focus::Grid);
            self.events.push(LibraryEvent::Launch(Box::new(request)));
        } else if cancel {
            self.dialog = None;
            self.focus = Some(Focus::Grid);
            if let Some(m) = self.cores.manager() {
                cores_ui::release(m);
            }
        }
    }

    /// ROM folder choice, folder path (container, no file manager) and unclassified files of the scan.
    fn folder_dialog(&mut self, ctx: &egui::Context) {
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let mut close = escape;
        let mut open = None;
        let mut copy = None;
        let Some(dialog) = &self.dialog else {
            return;
        };
        egui::Modal::new(egui::Id::unique("library-folder-dialog")).show(ctx, |ui| {
            ui.set_max_width(620.0);
            match dialog {
                Dialog::ChooseFolder(roots) => {
                    ui.heading(text::FOLDER_CHOOSE);
                    for (i, root) in roots.iter().enumerate() {
                        let r = ui.button(root.display().to_string());
                        if focus && i == 0 {
                            r.request_focus();
                        }
                        if r.clicked() {
                            open = Some(root.clone());
                        }
                    }
                    close |= ui.button(text::LAUNCH_CANCEL).clicked();
                }
                Dialog::FolderPath {
                    path,
                    host_hint,
                    container,
                    copied,
                } => {
                    ui.heading(text::FOLDER_TITLE);
                    ui.label(if *container {
                        text::FOLDER_CONTAINER
                    } else {
                        text::FOLDER_NO_MANAGER
                    });
                    ui.add(
                        egui::Label::new(RichText::new(path.display().to_string()).monospace())
                            .selectable(true),
                    );
                    if *container {
                        match host_hint {
                            Some(h) => ui.label(text::folder_host(h)),
                            None => ui.label(text::FOLDER_HOST_UNKNOWN),
                        };
                    }
                    if *copied {
                        weak(ui, text::PATH_COPIED);
                    }
                    ui.horizontal(|ui| {
                        let ok = ui.button(text::OK);
                        if focus {
                            ok.request_focus();
                        }
                        close |= ok.clicked();
                        if ui.button(text::COPY_PATH).clicked() {
                            copy = Some(path.display().to_string());
                        }
                    });
                }
                Dialog::Unclassified(paths) => {
                    ui.heading(text::unclassified_title(paths.len()));
                    ui.label(text::UNCLASSIFIED_HELP);
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for p in paths {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(text::unclassified_line(p)).small(),
                                    )
                                    .selectable(true),
                                );
                            }
                        });
                    let ok = ui.button(text::OK);
                    if focus {
                        ok.request_focus();
                    }
                    close |= ok.clicked();
                }
                _ => close = true,
            }
        });
        if let Some(text) = copy {
            ctx.copy_text(text);
            if let Some(Dialog::FolderPath { copied, .. }) = &mut self.dialog {
                *copied = true;
            }
        } else if let Some(path) = open {
            self.dialog = None;
            self.open_folder(path);
        } else if close {
            self.dialog = None;
            self.focus = Some(Focus::Grid);
        }
    }

    /// Covers on screen still being read (captures wait for them).
    pub(crate) fn covers_busy(&self) -> bool {
        self.covers.busy()
    }

    /// `--screen library-tidy`: the analysis starts on the first frame (it needs the egui context).
    pub(crate) fn open_tidy_on_first_frame(&mut self) {
        self.tidy.pending = true;
    }

    /// `--screen library-tidy-report` / `library-tidy-undo`: the report screen with sample content, with the
    /// undo confirmation open for `confirm_undo` (captures of their layout; nothing is read or moved).
    pub(crate) fn open_tidy_report_demo(&mut self, confirm_undo: bool) {
        let outcome = tidy::exec::demo_outcome(&self.dirs);
        if confirm_undo && let Some(info) = &outcome.last.info {
            self.tidy.confirm_undo = Some(tidy::UndoAsk {
                journal: info.path.clone(),
                moved: info.moved,
                unix: info.created_unix,
                focus: true,
            });
        }
        self.tidy.stage = tidy::Stage::Report(Box::new(outcome));
        self.tidy.focus_first = !confirm_undo;
        self.set_page(Page::Tidy);
    }

    pub(crate) fn wants_launch_capture(&mut self) {
        self.launch_selected();
    }
}

/// [`FirmwareInfo`] of a check: system name, region of the game (region tables), every file of the table when
/// the region is unknown, other files present.
fn firmware_info(
    prefs: &emu_assets::cores::CorePreferences,
    core: &CoreId,
    lf: &LaunchFirmware,
) -> FirmwareInfo {
    let table = prefs
        .required_firmware(core, &lf.system)
        .filter(|t| t.variant == FirmwareVariant::Region);
    let region = table
        .and(lf.variant.as_deref())
        .and_then(DiscRegion::from_key)
        .map(text::region_name);
    let mut alternatives: Vec<(&String, &String)> = match table {
        Some(t) if !lf.variant_known => t.files.iter().collect(),
        _ => Vec::new(),
    };
    alternatives.sort_by_key(|(key, _)| text::region_order(key));
    FirmwareInfo {
        system: emu_library::systems()
            .iter()
            .find(|s| s.core_key() == lf.system)
            .map(|s| s.name.clone()),
        region,
        alternatives: alternatives
            .into_iter()
            .map(|(key, file)| {
                text::file_with_region(file, DiscRegion::from_key(key).map(text::region_name))
            })
            .collect(),
        others_present: lf.check.others_present.clone(),
    }
}

fn firmware_names(list: &[FirmwareStatus]) -> Vec<String> {
    list.iter().map(|f| f.spec.path.clone()).collect()
}

/// Small coloured label used by both pages.
fn weak(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).small().weak());
}

#[cfg(test)]
mod tests;
