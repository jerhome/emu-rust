//! Library mode (ADR 0014 §8, design v1.0-bibliotheque §3): home grid, settings, cores screen, and the launch
//! flow towards a game played in the same window (`host`). Drawing code never blocks: scan and DAT import run
//! in `worker` threads; only quick SQLite reads/writes (game list, favorite, last played) happen here.

mod covers;
mod dos;
mod firmware_gate;
mod grid;
mod home;
mod host;
mod megacd;
pub use megacd::cli_failure_hint as megacd_cli_hint;
mod pad_settings;
mod settings;
pub(crate) mod text;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{Color32, Key, RichText, Ui};
use emu_assets::cores::{CoreId, FirmwareStatus};
use emu_library::{
    Game, GameFilter, GameId, Library, OpenOutcome, ScanProgress, SortOrder, SystemKind,
    filter_games, systems,
};

use self::covers::{CoverProvider, Covers, Placeholder};
pub(crate) use self::host::run;
use self::worker::{DatDownloadTask, DatProgress, DatTask, Notify, ScanTask};
use crate::GameContent;
use crate::config::{Config, DataDirs};
use crate::cores_ui::{self, CoresEvent, CoresUi, LaunchCheck};
use crate::launch_firmware::{FirmwareDecision, check_launch_firmware, decide, fallback_option};
use crate::platform::{FolderOutcome, FolderPlan, plan_open_folder, spawn_open_folder};

const WARNING: Color32 = Color32::from_rgb(255, 190, 80);
const ERROR: Color32 = Color32::from_rgb(255, 110, 110);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Home,
    Settings,
    Cores,
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
    /// Replaces the generic message when the core refuses the game (Mega CD: BIOS names and folder).
    pub failure_hint: Option<String>,
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
    /// Mega CD disc without its BIOS (or with another revision): exact name, region, `system/` folder.
    MegaCdBios {
        request: LaunchRequest,
        verdict: megacd::BiosVerdict,
        region: Option<emu_library::DiscRegion>,
        /// « Réessayer » found nothing new.
        retried: bool,
        /// « Copier le chemin » was used.
        copied: bool,
    },
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
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
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
        let cores = CoresUi::open(&dirs, &config);
        let scan_on_open = !capture && config.library.scan_on_start && db.is_some();
        let mut ui = Self {
            dirs,
            config,
            capture,
            page: Page::Home,
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
        };
        ui.reload_games();
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
        self.settings.focus_first = page == Page::Settings;
        if page == Page::Settings {
            self.refresh_settings_cache();
        }
        if page == Page::Home {
            self.focus = Some(Focus::Grid);
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

    /// Writes `config.toml` (0600, atomic). Locked file or capture: nothing written.
    fn save_config(&mut self) {
        if self.capture {
            return;
        }
        match self.config.save(&self.dirs.config_file) {
            Ok(()) => self.set_status(text::SAVED.to_owned(), Level::Info),
            Err(e) => {
                let msg = if self.config.is_locked() {
                    text::LOCKED.to_owned()
                } else {
                    text::save_failed(&e.to_string())
                };
                self.set_status(msg, Level::Warn);
            }
        }
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
        self.launch(
            args.game_id,
            args.title,
            &args.system_id,
            args.ext.as_deref(),
            args.content,
            args.computer,
        );
    }

    fn launch_game(&mut self, game: Game) {
        let system = emu_library::system(&game.system);
        let computer = system.is_some_and(|s| s.kind == SystemKind::Computer);
        // A DOS zip is `path` (never `entry`); a folder game is `Folder` (`[dos] start` applied by `App::build`).
        let content = crate::computer::dos::game_content(&game, computer, None);
        let ext = grid::launch_extension(&game);
        self.launch(
            Some(game.id),
            game.title.clone(),
            &game.system,
            ext.as_deref(),
            content,
            computer,
        );
    }

    /// « Bureau Atari ST »: the machine without a disk (`ContentSpec::NoGame`), keyboard mode.
    fn launch_desktop(&mut self, system_id: &str) {
        let title = emu_library::system(system_id).map_or_else(
            || system_id.to_owned(),
            |s| text::desktop_title(system_id, &s.name),
        );
        self.launch(None, title, system_id, None, GameContent::NoGame, true);
    }

    fn launch(
        &mut self,
        game_id: Option<GameId>,
        title: String,
        system_id: &str,
        ext: Option<&str>,
        content: GameContent,
        computer: bool,
    ) {
        let Some(manager) = self.cores.manager().cloned() else {
            self.open_dialog(Dialog::Message(text::NO_MANAGER.to_owned()));
            return;
        };
        let prefs = cores_ui::preferences(&self.config.cores);
        let database = emu_library::system(system_id).map(|s| s.database.as_str());
        let check = cores_ui::plan_launch(&manager, &prefs, database, ext);
        let megacd_region = (system_id == megacd::SYSTEM_ID).then(|| match &content {
            GameContent::Files(files) => files.first().and_then(|p| megacd::region_of(p)),
            GameContent::NoGame | GameContent::Folder { .. } => None,
        });
        let failure_hint = megacd_region.map(|region| {
            let names = region.map_or(megacd::BIOS_NAMES.to_vec(), |r| vec![megacd::bios_name(r)]);
            text::megacd_load_failed(&title, &names, &self.dirs.system)
        });
        let request = |core: CoreId, core_path: PathBuf, warnings: Vec<String>| LaunchRequest {
            game: game_id,
            title: title.clone(),
            core,
            core_path,
            content: content.clone(),
            computer,
            warnings,
            failure_hint: failure_hint.clone(),
        };
        let check = match megacd_region {
            Some(region) => match self.megacd_gate(&manager, check, region, &request) {
                Some(check) => check,
                None => return,
            },
            None => check,
        };
        let args = LaunchArgs {
            game_id,
            title: title.clone(),
            system_id: system_id.to_owned(),
            ext: ext.map(str::to_owned),
            content: content.clone(),
            computer,
        };
        let Some(check) = self.firmware_gate(&prefs, check, args, &request) else {
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
                }),
                // The desktop is not a library game: the core is installed from the cores screen.
                None => self.open_dialog(Dialog::Message(text::desktop_needs_core(core.as_str()))),
            },
            LaunchCheck::FirmwareMissing { id, path, missing } => {
                let free = database.and_then(|db| self.cores.bios.for_missing(db, &missing));
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

    /// Cores with a `required_firmware` table or a built-in ROM (design v3.0 §2.1, §3.5.5): the single 3.0 check
    /// replaces the `.info` one. `None`: a dialog took over (the core stays marked in use until it is closed).
    fn firmware_gate(
        &mut self,
        prefs: &emu_assets::cores::CorePreferences,
        check: LaunchCheck,
        args: LaunchArgs,
        request: &dyn Fn(CoreId, PathBuf, Vec<String>) -> LaunchRequest,
    ) -> Option<LaunchCheck> {
        let (id, path) = match &check {
            LaunchCheck::Ready { id, path, .. } | LaunchCheck::FirmwareMissing { id, path, .. } => {
                (id.clone(), path.clone())
            }
            _ => return Some(check),
        };
        // Same options path and layers as `App::build`: `resolve_game` gives the per-game options file.
        let spec = crate::content::content_spec(id.as_str(), &args.content, prefs);
        let resolved = crate::game_identity::resolve_game(id.as_str(), spec, &self.dirs);
        let loaded = crate::game_options::GameOptions::load(&resolved.options_path);
        let global = self
            .config
            .core_options
            .get(id.as_str())
            .cloned()
            .unwrap_or_default();
        let Some(lf) = check_launch_firmware(
            &self.dirs.system,
            prefs,
            &id,
            args.content.first(),
            &global,
            &loaded.options.core_options,
        ) else {
            // Neither a table nor a built-in ROM: the 1.0 `.info` check stands.
            return Some(check);
        };
        let decision = decide(&lf.check, loaded.options.input.aros_acknowledged);
        tracing::info!("BIOS de {id} avant lancement : {decision:?}");
        if decision == FirmwareDecision::Launch {
            return Some(LaunchCheck::Ready {
                id,
                path,
                warnings: Vec::new(),
            });
        }
        let fallback = fallback_option(prefs, &id, &lf.system);
        self.open_dialog(Dialog::Firmware3 {
            request: request(id, path, Vec::new()),
            args: Box::new(args),
            decision,
            options_path: resolved.options_path,
            fallback,
        });
        None
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
            .find(|s| s.database == system_db)
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
        self.cores
            .set_preferences(cores_ui::preferences(&self.config.cores));
    }

    /// Mega CD (design v2.1-megacd-express §2.3): checks the BIOS of the disc's region. `None`: a dialog took
    /// over (the core stays marked in use until it is closed). Otherwise the check to carry on with, without the
    /// `bios_CD_*` names (handled here, never in the generic BIOS warning).
    fn megacd_gate(
        &mut self,
        manager: &emu_assets::cores::CoreManager,
        check: LaunchCheck,
        region: Option<emu_library::DiscRegion>,
        request: &dyn Fn(CoreId, PathBuf, Vec<String>) -> LaunchRequest,
    ) -> Option<LaunchCheck> {
        let (id, path, others, required) = match check {
            LaunchCheck::Ready { id, path, warnings } => (id, path, warnings, false),
            LaunchCheck::FirmwareMissing { id, path, missing } => (id, path, missing, true),
            other => return Some(other),
        };
        let others: Vec<FirmwareStatus> = others
            .into_iter()
            .filter(|f| !megacd::is_bios_name(&f.spec.path))
            .collect();
        let verdict =
            megacd::bios_verdict(region, &manager.firmware_status(&id), &self.dirs.system);
        if verdict == megacd::BiosVerdict::Ready {
            return Some(if required && !others.is_empty() {
                LaunchCheck::FirmwareMissing {
                    id,
                    path,
                    missing: others,
                }
            } else {
                LaunchCheck::Ready {
                    id,
                    path,
                    warnings: if required { Vec::new() } else { others },
                }
            });
        }
        let names = firmware_names(&others);
        let warnings = if names.is_empty() {
            Vec::new()
        } else {
            vec![text::firmware_warning(&names)]
        };
        tracing::info!("BIOS Mega CD : {verdict:?} (région {region:?})");
        self.open_dialog(Dialog::MegaCdBios {
            request: request(id, path, warnings),
            verdict,
            region,
            retried: false,
            copied: false,
        });
        None
    }

    /// « Réessayer »: the same check again; the game starts as soon as the BIOS is there.
    fn megacd_retry(&mut self) {
        let manager = self.cores.manager().cloned();
        let Some(Dialog::MegaCdBios {
            request,
            verdict,
            region,
            retried,
            ..
        }) = &mut self.dialog
        else {
            return;
        };
        let statuses = manager
            .map(|m| m.firmware_status(&request.core))
            .unwrap_or_default();
        let fresh = megacd::bios_verdict(*region, &statuses, &self.dirs.system);
        if fresh == megacd::BiosVerdict::Ready {
            if let Some(Dialog::MegaCdBios { request, .. }) = self.dialog.take() {
                self.focus = Some(Focus::Grid);
                self.events.push(LibraryEvent::Launch(Box::new(request)));
            }
            return;
        }
        *retried = fresh == *verdict;
        *verdict = fresh;
        self.focus_dialog = true;
    }

    fn megacd_dialog(&mut self, ctx: &egui::Context) {
        let Some(Dialog::MegaCdBios {
            request,
            verdict,
            region,
            retried,
            copied,
        }) = &self.dialog
        else {
            return;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let dir = text::megacd_system_dir(&self.dirs.system);
        let region_name = region.map(megacd::region_name);
        let (title, body, missing) = match verdict {
            megacd::BiosVerdict::Missing {
                wanted,
                others_present,
            } => (
                text::megacd_missing_title(wanted, region.map(megacd::region_label)),
                text::megacd_missing_body(&request.title, region_name, wanted, others_present),
                true,
            ),
            megacd::BiosVerdict::Different { name } => (
                text::MEGACD_DIFFERENT_TITLE.to_owned(),
                text::megacd_different_body(&request.title, name),
                false,
            ),
            megacd::BiosVerdict::Ready => (String::new(), String::new(), false),
        };
        let (retried, copied) = (*retried, *copied);
        let (mut retry, mut copy, mut cancel, mut anyway) = (false, false, escape, false);
        egui::Modal::new(egui::Id::unique("library-megacd-dialog")).show(ctx, |ui| {
            ui.set_max_width(600.0);
            ui.heading(&title);
            ui.label(&body);
            if missing {
                ui.add_space(6.0);
                // Selectable: the path can also be copied with the mouse or Ctrl+C.
                ui.add(egui::Label::new(RichText::new(&dir).monospace()).selectable(true));
                if retried {
                    ui.colored_label(WARNING, text::MEGACD_STILL_MISSING);
                }
                if copied {
                    weak(ui, text::MEGACD_PATH_COPIED);
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // Safe default first: « Réessayer » (or « Annuler »), never the launch.
                let first = missing.then(|| {
                    let r = ui.button(text::MEGACD_RETRY);
                    retry = r.clicked();
                    copy = ui.button(text::MEGACD_COPY_PATH).clicked();
                    r
                });
                let c = ui.button(text::LAUNCH_CANCEL);
                cancel |= c.clicked();
                anyway = ui.button(text::LAUNCH_ANYWAY).clicked();
                if focus {
                    first.unwrap_or(c).request_focus();
                }
            });
        });
        if copy {
            ctx.copy_text(self.dirs.system.display().to_string());
            if let Some(Dialog::MegaCdBios { copied, .. }) = &mut self.dialog {
                *copied = true;
            }
        } else if retry {
            self.megacd_retry();
        } else if anyway {
            if let Some(Dialog::MegaCdBios { request, .. }) = self.dialog.take() {
                self.focus = Some(Focus::Grid);
                self.events.push(LibraryEvent::Launch(Box::new(request)));
            }
        } else if cancel {
            self.dialog = None;
            self.focus = Some(Focus::Grid);
            // The core was marked in use by `prepare_launch`.
            if let Some(m) = self.cores.manager() {
                cores_ui::release(m);
            }
        }
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
        self.relaunch_after_install();
        self.bios_completed();
        self.poll_folder();
        match self.page {
            Page::Home => self.home(ui),
            Page::Settings => self.settings_page(ui),
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
        } else if self.programs_open() {
            self.programs_dialog(&ctx);
        } else {
            self.dialog_ui(&ctx);
        }
        self.gate_dos_launches(&ctx);
        std::mem::take(&mut self.events)
    }

    fn status_line(&self, ui: &mut Ui) {
        if let Some(b) = &self.db_banner {
            ui.colored_label(WARNING, b);
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
            ui.colored_label(color, msg);
        }
    }

    /// Free BIOS installs that finished (cores screen or launch dialog): variant remembered, game launched.
    fn bios_completed(&mut self) {
        for done in self.cores.bios.take_done() {
            self.cores.mark_dirty();
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
                    self.open_dialog(Dialog::Message(text::game_failed(&request.title, &msg)));
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
        if matches!(self.dialog, Some(Dialog::MegaCdBios { .. })) {
            return self.megacd_dialog(ctx);
        }
        if matches!(self.dialog, Some(Dialog::Firmware3 { .. })) {
            return self.firmware3_dialog(ctx);
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
                Dialog::Install { core, game, .. } => {
                    let name = manager
                        .as_ref()
                        .and_then(|m| m.info(core))
                        .map_or_else(|| core.to_string(), |i| i.display_name);
                    (
                        text::LAUNCH_INSTALL_TITLE,
                        text::install_question(&name, game),
                    )
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
                Dialog::FreeBios { .. }
                | Dialog::MegaCdBios { .. }
                | Dialog::Firmware3 { .. }
                | Dialog::ChooseFolder(_)
                | Dialog::FolderPath { .. }
                | Dialog::Unclassified(_) => (text::LAUNCH_FAILED_TITLE, String::new()),
                Dialog::OfferDat => (text::DAT_OFFER_TITLE, text::DAT_OFFER.to_owned()),
            };
            ui.heading(title);
            ui.label(body);
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
                    | Dialog::FreeBios { .. }
                    | Dialog::MegaCdBios { .. }
                    | Dialog::Firmware3 { .. }
                    | Dialog::ChooseFolder(_)
                    | Dialog::FolderPath { .. }
                    | Dialog::Unclassified(_) => None,
                };
                // Safe default first: « Annuler » / « OK » gets the focus, never the launch.
                let cancel_label = match dialog {
                    Dialog::Message(_) => text::OK,
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

    /// 3.0 firmware dialog (design v3.0 §2.1). Safe default first: « Importer… », never the launch.
    fn firmware3_dialog(&mut self, ctx: &egui::Context) {
        let Some(Dialog::Firmware3 {
            request,
            decision,
            fallback,
            ..
        }) = &self.dialog
        else {
            return;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let (title, body, wanted) = match decision {
            FirmwareDecision::FallbackFirstTime { label, wanted } => (
                text::fallback_title(wanted),
                text::fallback_body(&request.title, label, wanted),
                wanted.clone(),
            ),
            FirmwareDecision::WrongHash { wanted, .. } => (
                text::LAUNCH_FIRMWARE_TITLE.to_owned(),
                text::wrong_firmware_body(&request.title, wanted),
                wanted.clone(),
            ),
            FirmwareDecision::Missing { wanted } => (
                text::LAUNCH_FIRMWARE_TITLE.to_owned(),
                text::firmware_question(&request.title, std::slice::from_ref(wanted)),
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
        let wrong = matches!(decision, FirmwareDecision::WrongHash { .. });
        let can_fallback = wrong && fallback.is_some();
        let kickstart = wanted.to_ascii_lowercase().starts_with("kick");
        let dir = text::megacd_system_dir(&self.dirs.system);
        let (mut import, mut cont, mut cancel, mut anyway, mut use_fallback) =
            (false, false, escape, false, false);
        egui::Modal::new(egui::Id::unique("library-firmware3-dialog")).show(ctx, |ui| {
            ui.set_max_width(600.0);
            ui.heading(&title);
            ui.label(&body);
            ui.add(egui::Label::new(RichText::new(&dir).monospace()).selectable(true));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let label = if kickstart {
                    cores_ui::text::IMPORT_KICKSTART
                } else {
                    cores_ui::text::IMPORT_BUTTON
                };
                let first = ui.button(label);
                import = first.clicked();
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
                    first.request_focus();
                }
            });
            if first_time {
                weak(ui, text::FALLBACK_NOTE);
            }
        });
        if import {
            self.cores.import.start(Some(wanted));
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
                        weak(ui, text::MEGACD_PATH_COPIED);
                    }
                    ui.horizontal(|ui| {
                        let ok = ui.button(text::OK);
                        if focus {
                            ok.request_focus();
                        }
                        close |= ok.clicked();
                        if ui.button(text::MEGACD_COPY_PATH).clicked() {
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

    pub(crate) fn wants_launch_capture(&mut self) {
        self.launch_selected();
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
