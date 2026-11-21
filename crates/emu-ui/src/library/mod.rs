//! Library mode (ADR 0014 §8, design v1.0-bibliotheque §3): home grid, settings, cores screen, and the launch
//! flow towards a game played in the same window (`host`). Drawing code never blocks: scan and DAT import run
//! in `worker` threads; only quick SQLite reads/writes (game list, favorite, last played) happen here.

mod covers;
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
    Launch(LaunchRequest),
    Quit,
}

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
    /// libretro-database `metadat/no-intro/` (tests: a local server).
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
            dat_base_url: emu_assets::DAT_BASE_URL.to_owned(),
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

    fn launch_game(&mut self, game: Game) {
        let system = emu_library::system(&game.system);
        let computer = system.is_some_and(|s| s.kind == SystemKind::Computer);
        let files = if computer && !game.media.is_empty() {
            game.media.clone()
        } else {
            vec![game.path.clone()]
        };
        let ext = grid::launch_extension(&game);
        self.launch(
            Some(game.id),
            game.title.clone(),
            &game.system,
            ext.as_deref(),
            GameContent::Files(files),
            computer,
        );
    }

    /// « Bureau Atari ST »: the machine without a disk (`ContentSpec::NoGame`), keyboard mode.
    fn launch_desktop(&mut self, system_id: &str) {
        let title = emu_library::system(system_id)
            .map_or_else(|| system_id.to_owned(), |s| text::desktop_title(&s.name));
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
            GameContent::NoGame => None,
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
        match check {
            LaunchCheck::Ready { id, path, warnings } => {
                let names = firmware_names(&warnings);
                let warnings = if names.is_empty() {
                    Vec::new()
                } else {
                    vec![text::firmware_warning(&names)]
                };
                self.events
                    .push(LibraryEvent::Launch(request(id, path, warnings)));
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
                self.events.push(LibraryEvent::Launch(request));
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
                self.events.push(LibraryEvent::Launch(request));
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
        match self.page {
            Page::Home => self.home(ui),
            Page::Settings => self.settings_page(ui),
            Page::Cores => {
                self.cores.set_modal_open(self.dialog.is_some());
                egui::CentralPanel::default().show(ui, |ui| {
                    if self.cores.show(ui) == Some(CoresEvent::Back) {
                        self.set_page(Page::Home);
                    }
                });
            }
        }
        self.dialog_ui(&ctx);
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
                    self.events.push(LibraryEvent::Launch(request));
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
                self.events.push(LibraryEvent::Launch(request));
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
                Dialog::FreeBios { .. } | Dialog::MegaCdBios { .. } => {
                    (text::LAUNCH_FAILED_TITLE, String::new())
                }
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
                    | Dialog::MegaCdBios { .. } => None,
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
                self.events.push(LibraryEvent::Launch(request));
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
