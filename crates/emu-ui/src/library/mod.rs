//! Library mode (ADR 0014 §8, design v1.0-bibliotheque §3): home grid, settings, cores screen, and the launch
//! flow towards a game played in the same window (`host`). Drawing code never blocks: scan and DAT import run
//! in `worker` threads; only quick SQLite reads/writes (game list, favorite, last played) happen here.

mod covers;
mod grid;
mod home;
mod host;
mod settings;
pub(crate) mod text;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{Color32, Key, RichText, Ui};
use emu_assets::cores::{CoreId, FirmwareStatus};
use emu_library::{
    Game, GameFilter, GameId, Library, OpenOutcome, ScanProgress, SortOrder, filter_games, systems,
};

use self::covers::{CoverProvider, Placeholder};
pub(crate) use self::host::run;
use self::worker::{DatTask, Notify, ScanTask};
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
    pub game: GameId,
    pub title: String,
    pub core: CoreId,
    pub core_path: PathBuf,
    pub rom: PathBuf,
    /// Shown in the game's OSD (optional BIOS missing…).
    pub warnings: Vec<String>,
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
    },
    Firmware {
        request: LaunchRequest,
        missing: Vec<String>,
    },
    Reinstall {
        core: CoreId,
    },
    Message(String),
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
    status: Option<(String, Level)>,
    dialog: Option<Dialog>,
    focus_dialog: bool,
    cores: CoresUi,
    covers: Box<dyn CoverProvider>,
    settings: settings::State,
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
            status: None,
            dialog: None,
            focus_dialog: false,
            cores,
            covers: Box::new(Placeholder),
            settings: settings::State::default(),
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

    /// Drains worker events once per frame.
    fn poll_workers(&mut self) {
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
                    }
                    Some(Err(e)) if cancelled => self.set_status(e, Level::Info),
                    Some(Err(e)) => self.set_status(text::scan_failed(&e), Level::Error),
                    None => self.set_status(text::scan_failed("tâche interrompue"), Level::Error),
                }
                self.reload_games();
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
        let Some(manager) = self.cores.manager().cloned() else {
            self.open_dialog(Dialog::Message(text::NO_MANAGER.to_owned()));
            return;
        };
        let prefs = cores_ui::preferences(&self.config.cores);
        let database = emu_library::system(&game.system).map(|s| s.database.as_str());
        let ext = grid::launch_extension(&game);
        let check = cores_ui::plan_launch(&manager, &prefs, database, ext.as_deref());
        let request = |core: CoreId, core_path: PathBuf, warnings: Vec<String>| LaunchRequest {
            game: game.id,
            title: game.title.clone(),
            core,
            core_path,
            rom: game.path.clone(),
            warnings,
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
            LaunchCheck::NeedsInstall(core) => self.open_dialog(Dialog::Install {
                core,
                game: game.title.clone(),
            }),
            LaunchCheck::FirmwareMissing { id, path, missing } => {
                let missing = firmware_names(&missing);
                self.open_dialog(Dialog::Firmware {
                    request: request(id, path, Vec::new()),
                    missing,
                });
            }
            LaunchCheck::Failed(emu_assets::cores::CoreError::Tampered { id, .. }) => {
                self.open_dialog(Dialog::Reinstall { core: id });
            }
            other => {
                let msg = other
                    .blocking_message()
                    .unwrap_or_else(|| text::LAUNCH_FAILED_TITLE.to_owned());
                self.open_dialog(Dialog::Message(text::game_failed(&game.title, &msg)));
            }
        }
    }

    fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.focus_dialog = true;
    }

    /// The host started the game: history updated, the core screen shows it in use.
    pub(crate) fn game_started(&mut self, request: &LaunchRequest) {
        if let Some(db) = &mut self.db
            && let Err(e) = db.record_played(request.game, now_unix())
        {
            tracing::warn!("historique non enregistré : {e}");
        }
        self.cores.set_in_use(Some(request.core.clone()));
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
        self.poll_workers();
        match self.page {
            Page::Home => self.home(ui),
            Page::Settings => self.settings_page(ui),
            Page::Cores => {
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

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        enum Choice {
            Close,
            Install(CoreId),
            Launch,
        }
        let mut choice = None;
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let manager = self.cores.manager().cloned();
        egui::Modal::new(egui::Id::unique("library-dialog")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            let (title, body) = match dialog {
                Dialog::Install { core, game } => {
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
                Dialog::Reinstall { core } => {
                    (text::LAUNCH_FAILED_TITLE, text::tampered(core.as_str()))
                }
                Dialog::Message(m) => (text::LAUNCH_FAILED_TITLE, m.clone()),
            };
            ui.heading(title);
            ui.label(body);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let first = match dialog {
                    Dialog::Install { core, .. } => {
                        let r = ui.button(text::LAUNCH_INSTALL);
                        if r.clicked() {
                            choice = Some(Choice::Install(core.clone()));
                        }
                        Some(r)
                    }
                    Dialog::Reinstall { core } => {
                        let r = ui.button(text::LAUNCH_REINSTALL);
                        if r.clicked() {
                            choice = Some(Choice::Install(core.clone()));
                        }
                        Some(r)
                    }
                    Dialog::Firmware { .. } | Dialog::Message(_) => None,
                };
                // Safe default first: « Annuler » / « OK » gets the focus, never the launch.
                let cancel_label = if matches!(dialog, Dialog::Message(_)) {
                    text::OK
                } else {
                    text::LAUNCH_CANCEL
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
            (Choice::Install(core), _) => {
                self.cores.request_install(core, ctx);
                self.set_page(Page::Cores);
            }
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
