//! « Cores » screen: installed cores and catalog, install / update / remove / rollback / pin, BIOS state
//! (ADR 0013, design v1.0-cores §4). Network work runs in `Job`s polled once per frame; nothing blocks the UI
//! thread except `installed()`, `verified_path()` and `firmware_status()` (a few files, snapshot refreshed only
//! after an action). BIOS files are only checked, never downloaded.

mod bios;
mod capture;
mod import;
mod launch;
mod rows;
pub(crate) mod text;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use egui::{Button, Color32, RichText, ScrollArea, Ui};
use emu_assets::cores::{
    BuildbotSource, CoreDirs, CoreError, CoreId, CoreInfo, CoreManager, CorePreferences,
    FirmwareState, FirmwareStatus, InstalledCore, KnownFirmware, Origin, Platform, RemoteCatalog,
    check_core_firmware,
};
use emu_assets::http::{HttpClient, HttpConfig, Progress};
use emu_assets::jobs::{Job, JobEvent, Notify};

pub(crate) use self::bios::BiosUi;
pub use self::bios::open_free_bios_manager;
pub(crate) use self::capture::run_capture;
pub(crate) use self::import::ImportUi;
#[cfg(test)]
use self::launch::prepare_launch;
pub(crate) use self::launch::{LaunchCheck, plan_launch, release};
use self::rows::{Checks, CoreRow, CoreState, RowFilter, Scope, recommended_missing, rows};
use crate::Config;
use crate::config::DataDirs;
use crate::config_library::CoresConfig;
use crate::ui::text::sanitize_core_text;

/// Core zips are large: the default 60 s of cover downloads is too short.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const WARNING: Color32 = Color32::from_rgb(255, 190, 80);
const ERROR: Color32 = Color32::from_rgb(255, 110, 110);
const GOOD: Color32 = Color32::from_rgb(120, 220, 140);
const INFO: Color32 = Color32::from_rgb(130, 190, 255);

/// Opens the core manager for `dirs` / `config` (no network). Shared with the CLI.
pub fn open_core_manager(dirs: &DataDirs, config: &Config) -> Result<Arc<CoreManager>, String> {
    let platform = Platform::current().ok_or(
        "gestionnaire de cores indisponible sur cette plate-forme (cores manuels uniquement)",
    )?;
    let source = match &config.cores.buildbot_url {
        Some(url) => BuildbotSource::custom(url)
            .map_err(|e| format!("[cores] buildbot_url invalide : {e}"))?,
        None => BuildbotSource::official(),
    };
    let http = HttpClient::new(HttpConfig {
        timeout: DOWNLOAD_TIMEOUT,
        ..HttpConfig::default()
    })
    .map_err(|e| format!("client HTTP : {e}"))?;
    let core_dirs = CoreDirs {
        cores: dirs.cores.clone(),
        system: dirs.system.clone(),
    };
    CoreManager::open(core_dirs, platform, source, http)
        .map(Arc::new)
        .map_err(|e| format!("gestionnaire de cores : {e}"))
}

/// Built-in preferences plus `[cores.preferred]` (keys are library system ids, converted to database names).
pub(crate) fn preferences(config: &CoresConfig) -> CorePreferences {
    let mut overrides = BTreeMap::new();
    for (system, core) in &config.preferred {
        match emu_library::system(system) {
            Some(info) => {
                overrides.insert(info.database.clone(), core.clone());
            }
            None => tracing::warn!("[cores.preferred] {system} : système inconnu, ignoré"),
        }
    }
    CorePreferences::builtin().with_overrides(&overrides)
}

/// VICE option switching the JiffyDOS ROMs on.
const JIFFYDOS_OPTION: &str = "vice_jiffydos";

/// What the screen asks its host to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoresEvent {
    Back,
    /// « Core utilisé » of a system (libretro database name) changed: `[cores.preferred]` to write.
    PreferCore {
        system: String,
        core: CoreId,
    },
    /// « Utiliser pour tous les jeux DOS »: `[core_options.<core>] <key> = <value>` to write.
    SetCoreOption {
        core: String,
        key: String,
        value: String,
    },
}

struct Snapshot {
    installed: Vec<InstalledCore>,
    infos: BTreeMap<CoreId, CoreInfo>,
    catalog: Option<RemoteCatalog>,
    checks: Checks,
}

struct Running {
    job: Job<Result<InstalledCore, CoreError>>,
    progress: Option<Progress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpKind {
    Remove,
    Rollback,
    Pin(bool),
}

/// A manager call that may wait for the cross-process folder lock: always run in a job.
struct Op {
    kind: OpKind,
    job: Job<Result<(), CoreError>>,
}

enum RowAction {
    Install(CoreId),
    AskRemove(CoreId),
    Remove(CoreId),
    CancelRemove,
    Rollback(CoreId),
    Pin(CoreId, bool),
    Details(CoreId),
    CancelInstall(CoreId),
}

pub(crate) struct CoresUi {
    manager: Option<Arc<CoreManager>>,
    init_error: Option<String>,
    system_dir: std::path::PathBuf,
    prefs: CorePreferences,
    snapshot: Option<Snapshot>,
    dirty: bool,
    filter: RowFilter,
    refresh: Option<Job<Result<RemoteCatalog, CoreError>>>,
    refresh_progress: Option<Progress>,
    refresh_error: Option<String>,
    installs: BTreeMap<CoreId, Running>,
    ops: BTreeMap<CoreId, Op>,
    /// Cores whose rollback found an altered file: the row offers « Réinstaller ».
    offer_reinstall: BTreeSet<CoreId>,
    /// Installs finished since the last [`CoresUi::take_completed`] (`true` = success).
    completed: Vec<(CoreId, bool)>,
    /// Installs waiting for the first catalog.
    queued: Vec<CoreId>,
    errors: BTreeMap<CoreId, String>,
    confirm_remove: Option<CoreId>,
    detail: Option<CoreId>,
    /// Firmware of the core shown in `detail`, computed once when it opens (MD5 of BIOS files: not per frame).
    detail_firmware: Vec<FirmwareStatus>,
    /// « Activer JiffyDOS pour tous les jeux C64 » clicked: `(core, option, value)` for `[core_options]`.
    detail_option: Option<(String, String, String)>,
    /// Message shown after `detail_option`.
    detail_note: Option<String>,
    /// A dialog of the host is open over this screen: Échap belongs to it.
    modal_open: bool,
    in_use: Option<CoreId>,
    focus_first: bool,
    focus_confirm: bool,
    /// Free BIOS section (EmuTOS).
    pub(crate) bios: BiosUi,
    /// « Importer un BIOS… » dialog; drawn by the library over any page.
    pub(crate) import: ImportUi,
}

impl CoresUi {
    pub(crate) fn open(dirs: &DataDirs, config: &Config) -> Self {
        let (manager, init_error) = match open_core_manager(dirs, config) {
            Ok(m) => (Some(m), None),
            Err(e) => (None, Some(e)),
        };
        let mut ui = Self::new(
            manager,
            init_error,
            dirs.system.clone(),
            preferences(&config.cores),
        );
        let bios = open_free_bios_manager(dirs)
            .inspect_err(|e| tracing::warn!("{e}"))
            .ok();
        ui.bios = BiosUi::new(bios, config.computer.emutos_variant.as_deref());
        ui
    }

    pub(crate) fn new(
        manager: Option<Arc<CoreManager>>,
        init_error: Option<String>,
        system_dir: std::path::PathBuf,
        prefs: CorePreferences,
    ) -> Self {
        let system_dir_for_import = system_dir.clone();
        let manager_for_import = manager.clone();
        Self {
            manager,
            init_error,
            system_dir,
            prefs,
            snapshot: None,
            dirty: true,
            filter: RowFilter::default(),
            refresh: None,
            refresh_progress: None,
            refresh_error: None,
            installs: BTreeMap::new(),
            ops: BTreeMap::new(),
            offer_reinstall: BTreeSet::new(),
            completed: Vec::new(),
            queued: Vec::new(),
            errors: BTreeMap::new(),
            confirm_remove: None,
            detail: None,
            detail_firmware: Vec::new(),
            detail_option: None,
            detail_note: None,
            modal_open: false,
            in_use: None,
            focus_first: true,
            focus_confirm: false,
            bios: BiosUi::new(None, None),
            import: ImportUi::new(system_dir_for_import, manager_for_import),
        }
    }

    /// `[cores.preferred]` changed (« Core utilisé »).
    pub(crate) fn set_preferences(&mut self, prefs: CorePreferences) {
        self.prefs = prefs;
        self.dirty = true;
    }

    /// The core being played (its actions are disabled), `None` back in the library.
    pub(crate) fn set_in_use(&mut self, id: Option<CoreId>) {
        self.in_use = id;
    }

    /// Installs that finished (or failed) since the last call, for the library launch flow.
    pub(crate) fn take_completed(&mut self) -> Vec<(CoreId, bool)> {
        std::mem::take(&mut self.completed)
    }

    #[cfg(test)]
    pub(crate) fn push_completed(&mut self, id: CoreId, ok: bool) {
        self.completed.push((id, ok));
    }

    /// Firmware states changed (free BIOS installed or removed).
    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
        self.bios.mark_dirty();
    }

    pub(crate) fn manager(&self) -> Option<&Arc<CoreManager>> {
        self.manager.as_ref()
    }

    /// Installs `id` now, or as soon as the catalog is known (checked first when there is none).
    pub(crate) fn request_install(&mut self, id: CoreId, ctx: &egui::Context) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        if manager.catalog().is_some_and(|c| c.cores.contains_key(&id)) {
            self.start_install(id, ctx);
        } else {
            if !self.queued.contains(&id) {
                self.queued.push(id);
            }
            self.start_refresh(ctx);
        }
    }

    fn notify(ctx: &egui::Context) -> Notify {
        let ctx = ctx.clone();
        Arc::new(move || ctx.request_repaint())
    }

    fn start_refresh(&mut self, ctx: &egui::Context) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        if self.refresh.is_some() {
            return;
        }
        self.refresh_error = None;
        self.refresh_progress = None;
        self.refresh = Some(Job::spawn(
            "cores-refresh",
            Self::notify(ctx),
            move |progress, cancel| manager.refresh(progress, cancel),
        ));
    }

    fn start_install(&mut self, id: CoreId, ctx: &egui::Context) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        if self.installs.contains_key(&id) {
            return;
        }
        self.errors.remove(&id);
        self.offer_reinstall.remove(&id);
        let target = id.clone();
        let job = Job::spawn(
            &format!("core-{id}"),
            Self::notify(ctx),
            move |progress, cancel| manager.install(&target, progress, cancel),
        );
        self.installs.insert(
            id,
            Running {
                job,
                progress: None,
            },
        );
    }

    fn start_op(&mut self, id: CoreId, kind: OpKind, ctx: &egui::Context) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        if self.ops.contains_key(&id) || self.installs.contains_key(&id) {
            return;
        }
        self.errors.remove(&id);
        self.offer_reinstall.remove(&id);
        let target = id.clone();
        let job = Job::spawn(
            &format!("core-op-{id}"),
            Self::notify(ctx),
            move |_progress, _cancel| match kind {
                OpKind::Remove => manager.remove(&target),
                OpKind::Rollback => manager.rollback(&target).map(|_| ()),
                OpKind::Pin(p) => manager.set_pinned(&target, p),
            },
        );
        self.ops.insert(id, Op { kind, job });
    }

    /// Drains job events of the refresh, installs and manager operations (also called while another page is
    /// shown, so that an install started from the library finishes and is reported).
    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        self.bios.poll();
        let mut refreshed = None;
        if let Some(job) = &mut self.refresh {
            for ev in job.poll() {
                match ev {
                    JobEvent::Progress(p) => self.refresh_progress = Some(p),
                    JobEvent::Done(r) => refreshed = Some(r),
                }
            }
        }
        if let Some(result) = refreshed {
            self.refresh = None;
            self.refresh_progress = None;
            self.dirty = true;
            match result {
                Ok(_) => {
                    for id in std::mem::take(&mut self.queued) {
                        self.start_install(id, ctx);
                    }
                }
                Err(e) => {
                    self.queued.clear();
                    self.refresh_error = Some(text::error(&e));
                }
            }
        }
        let mut done: Vec<(CoreId, Result<InstalledCore, CoreError>)> = Vec::new();
        for (id, run) in &mut self.installs {
            for ev in run.job.poll() {
                match ev {
                    JobEvent::Progress(p) => run.progress = Some(p),
                    JobEvent::Done(r) => done.push((id.clone(), r)),
                }
            }
            if run.job.is_finished() && !done.iter().any(|(d, _)| d == id) {
                // Worker ended without a result (panic): do not wait forever.
                done.push((
                    id.clone(),
                    Err(CoreError::Manifest("tâche interrompue".to_owned())),
                ));
            }
        }
        for (id, result) in done {
            self.installs.remove(&id);
            self.dirty = true;
            self.completed.push((id.clone(), result.is_ok()));
            if let Err(e) = result {
                self.errors.insert(id, text::error(&e));
            }
        }
        let mut finished: Vec<(CoreId, OpKind, Result<(), CoreError>)> = Vec::new();
        for (id, op) in &mut self.ops {
            for ev in op.job.poll() {
                if let JobEvent::Done(r) = ev {
                    finished.push((id.clone(), op.kind, r));
                }
            }
            if op.job.is_finished() && !finished.iter().any(|(d, ..)| d == id) {
                finished.push((
                    id.clone(),
                    op.kind,
                    Err(CoreError::Manifest("tâche interrompue".to_owned())),
                ));
            }
        }
        for (id, kind, result) in finished {
            self.ops.remove(&id);
            self.dirty = true;
            match result {
                Ok(()) => {}
                Err(CoreError::Tampered { .. }) if kind == OpKind::Rollback => {
                    self.errors
                        .insert(id.clone(), text::ROLLBACK_TAMPERED.to_owned());
                    self.offer_reinstall.insert(id);
                }
                Err(e) => {
                    self.errors.insert(id, text::error(&e));
                }
            }
        }
    }

    /// From the snapshot (installed cores); otherwise computed once (cores not installed).
    fn firmware_for(&self, id: &CoreId) -> Vec<FirmwareStatus> {
        if let Some(f) = self
            .snapshot
            .as_ref()
            .and_then(|s| s.checks.firmware.get(id))
        {
            return f.clone();
        }
        self.manager
            .as_ref()
            .map(|m| m.firmware_status(id))
            .unwrap_or_default()
    }

    /// Set by the host each frame: while its dialog is open, Échap does not leave this screen.
    pub(crate) fn set_modal_open(&mut self, open: bool) {
        self.modal_open = open;
    }

    fn rebuild_snapshot(&mut self) {
        let Some(manager) = &self.manager else {
            return;
        };
        let installed = manager.installed();
        let mut checks = Checks::default();
        for core in &installed {
            if matches!(core.origin, Origin::Buildbot { .. })
                && let Err(CoreError::Tampered { .. }) = manager.verified_path(&core.id)
            {
                checks.tampered.insert(core.id.clone());
            }
            checks
                .firmware
                .insert(core.id.clone(), manager.firmware_status(&core.id));
        }
        let known = KnownFirmware::builtin();
        let wanted: BTreeSet<&CoreId> = self.prefs.systems().flat_map(|(_, c)| c.iter()).collect();
        for id in wanted {
            // No game here: the system of the core's table (Mega CD for genesis_plus_gx), default variant.
            let Some(system) = crate::launch_firmware::firmware_system_for(&self.prefs, id, None)
            else {
                continue;
            };
            match check_core_firmware(&self.system_dir, &self.prefs, id, &system, None, &known) {
                Some(Ok(check)) => {
                    checks.required.insert(id.clone(), check);
                }
                Some(Err(e)) => tracing::warn!("contrôle du BIOS de {id} : {e}"),
                None => {}
            }
        }
        self.snapshot = Some(Snapshot {
            installed,
            infos: manager.infos(),
            catalog: manager.catalog(),
            checks,
        });
        self.dirty = false;
        if let Some(id) = self.detail.clone() {
            self.detail_firmware = self.firmware_for(&id);
        }
    }

    fn apply(&mut self, action: RowAction, ctx: &egui::Context) {
        match action {
            RowAction::Install(id) => self.start_install(id, ctx),
            RowAction::AskRemove(id) => {
                self.confirm_remove = Some(id);
                self.focus_confirm = true;
            }
            RowAction::CancelRemove => self.confirm_remove = None,
            RowAction::Remove(id) => {
                self.confirm_remove = None;
                self.start_op(id, OpKind::Remove, ctx);
            }
            RowAction::Rollback(id) => self.start_op(id, OpKind::Rollback, ctx),
            RowAction::Pin(id, pinned) => self.start_op(id, OpKind::Pin(pinned), ctx),
            RowAction::Details(id) => {
                self.detail_firmware = self.firmware_for(&id);
                self.detail = Some(id);
                self.detail_note = None;
                self.focus_first = true;
            }
            RowAction::CancelInstall(id) => {
                if let Some(run) = self.installs.get(&id) {
                    run.job.cancel();
                }
            }
        }
    }

    /// Draws the screen; returns what the host must do.
    pub(crate) fn show(&mut self, ui: &mut Ui) -> Option<CoresEvent> {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        if self.dirty {
            self.rebuild_snapshot();
        }
        let mut event = None;
        let escape = !self.modal_open
            && !self.import.is_active()
            && ui.input(|i| i.key_pressed(egui::Key::Escape));

        ui.horizontal(|ui| {
            ui.heading(text::TITLE);
        });
        if self.manager.is_none() {
            ui.colored_label(ERROR, text::NO_MANAGER);
            if let Some(e) = &self.init_error {
                ui.label(e);
            }
            if ui.button(text::BACK).clicked() || escape {
                event = Some(CoresEvent::Back);
            }
            return event;
        }
        self.header(ui, &ctx, &mut event);

        if let Some(id) = self.detail.clone() {
            if escape {
                self.detail = None;
            } else {
                self.detail_view(ui, &id);
                if let Some((core, key, value)) = self.detail_option.take() {
                    event = Some(CoresEvent::SetCoreOption { core, key, value });
                }
            }
        } else {
            if escape && self.confirm_remove.is_some() {
                self.confirm_remove = None;
            } else if escape {
                event = Some(CoresEvent::Back);
            }
            self.bios.show(ui, &ctx);
            if let Some((core, key, value)) = self.bios.use_for_all.take() {
                event = Some(CoresEvent::SetCoreOption { core, key, value });
            }
            ui.separator();
            if let Some(e) = self.core_choice(ui) {
                event = Some(e);
            }
            ui.separator();
            self.filter_bar(ui);
            ui.separator();
            let actions = self.list(ui);
            for a in actions {
                self.apply(a, &ctx);
            }
        }
        ui.separator();
        ui.label(RichText::new(text::NAV_HINT).small().weak());
        event
    }

    fn visible_rows(&self) -> Vec<CoreRow> {
        let Some(s) = &self.snapshot else {
            return Vec::new();
        };
        rows(
            &s.installed,
            s.catalog.as_ref(),
            &s.infos,
            &self.prefs,
            &self.filter,
            self.in_use.as_ref(),
            &s.checks,
        )
    }

    fn header(&mut self, ui: &mut Ui, ctx: &egui::Context, event: &mut Option<CoresEvent>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        let source = manager.source();
        if source.is_official() {
            ui.label(RichText::new(text::OFFICIAL_SOURCE).small().weak());
        } else {
            ui.colored_label(WARNING, text::source_custom(source.base()));
        }
        let fetched = self
            .snapshot
            .as_ref()
            .and_then(|s| s.catalog.as_ref())
            .map(|c| c.fetched_unix);
        let checked = match fetched {
            Some(t) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                text::checked_ago(now.saturating_sub(t))
            }
            None => text::NEVER_CHECKED.to_owned(),
        };
        ui.label(RichText::new(checked).small());

        let rows = self.visible_rows();
        let updatable: Vec<CoreId> = rows
            .iter()
            .filter(|r| r.can_update() && !self.installs.contains_key(&r.id))
            .map(|r| r.id.clone())
            .collect();
        let recommended = self
            .snapshot
            .as_ref()
            .map(|s| recommended_missing(&self.prefs, &s.installed))
            .unwrap_or_default();
        let refreshing = self.refresh.is_some();
        ui.horizontal_wrapped(|ui| {
            let back = ui.button(text::BACK);
            if std::mem::take(&mut self.focus_first) {
                back.request_focus();
            }
            if back.clicked() {
                *event = Some(CoresEvent::Back);
            }
            if ui
                .add_enabled(!refreshing, Button::new(text::REFRESH))
                .clicked()
            {
                self.start_refresh(ctx);
            }
            if ui.button(text::IMPORT_BUTTON).clicked() {
                self.import.start(None);
            }
            if ui
                .add_enabled(
                    !updatable.is_empty(),
                    Button::new(text::update_all(updatable.len())),
                )
                .clicked()
            {
                for id in updatable {
                    self.start_install(id, ctx);
                }
            }
            if ui
                .add_enabled(
                    !recommended.is_empty() && !refreshing,
                    Button::new(text::INSTALL_RECOMMENDED),
                )
                .clicked()
            {
                for id in recommended {
                    self.request_install(id, ctx);
                }
            }
        });
        if refreshing {
            ui.horizontal(|ui| {
                ui.label(text::REFRESHING);
                match self.refresh_progress {
                    Some(p) => ui.label(text::progress(p.done, p.total)),
                    None => ui.spinner(),
                };
            });
            if !self.queued.is_empty() {
                ui.label(RichText::new(text::queued_waiting(self.queued.len())).small());
            }
        }
        if let Some(e) = &self.refresh_error {
            ui.colored_label(ERROR, e);
        }
    }

    /// 3.0 check of a row: fallback in blue (never red), problems in orange, with the import button.
    /// `notice`: licence notice of ROMs built into the core (`builtin_firmware`), shown under a `Builtin` state.
    /// `files`: every file of a region table (PlayStation: one BIOS per region), empty otherwise.
    fn required_line(
        &mut self,
        ui: &mut Ui,
        check: &emu_assets::cores::FirmwareCheck,
        notice: Option<&str>,
        files: &[String],
    ) {
        let wanted = check.wanted.as_deref().unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            let importable = match &check.state {
                FirmwareState::Ok | FirmwareState::Free { .. } | FirmwareState::Unverified => {
                    ui.label(RichText::new(text::required_ok(wanted)).small().weak());
                    false
                }
                FirmwareState::Builtin { label } => {
                    ui.label(
                        RichText::new(text::firmware_state(&FirmwareState::Builtin {
                            label: label.clone(),
                        }))
                        .small()
                        .weak(),
                    );
                    false
                }
                FirmwareState::CoreFallback { label } => {
                    ui.colored_label(
                        INFO,
                        RichText::new(text::core_fallback_files(
                            label,
                            wanted,
                            files,
                            &check.others_present,
                        ))
                        .small(),
                    );
                    true
                }
                state => {
                    ui.colored_label(
                        WARNING,
                        RichText::new(text::required_problem(wanted, state)).small(),
                    );
                    true
                }
            };
            if let (FirmwareState::Builtin { .. }, Some(notice)) = (&check.state, notice) {
                ui.label(RichText::new(text::builtin_notice(notice)).small().weak());
            }
            if importable {
                let label = if wanted.to_ascii_lowercase().starts_with("kick") {
                    text::IMPORT_KICKSTART
                } else {
                    text::IMPORT_BUTTON
                };
                if ui.small_button(label).clicked() {
                    self.import.start(Some(wanted.to_owned()));
                }
            }
        });
    }

    /// « Core utilisé » for each system with several cores (design v3.0 §3.3). Writes nothing itself: the library
    /// saves `[cores.preferred]`. A core that is not installed can be chosen; its row offers « Installer ».
    fn core_choice(&mut self, ui: &mut Ui) -> Option<CoresEvent> {
        let systems: Vec<(String, Vec<CoreId>)> = self
            .prefs
            .systems()
            .filter(|(_, cores)| cores.len() > 1)
            .map(|(s, c)| (s.to_owned(), c.to_vec()))
            .collect();
        if systems.is_empty() {
            return None;
        }
        let installed: BTreeSet<CoreId> = self
            .snapshot
            .as_ref()
            .map(|s| s.installed.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default();
        let mut event = None;
        // Folded (keyboard: the header is focusable, Entrée opens it) so the core list stays visible, unless an
        // alternative is recommended on this OS (puae2021 on a Mac).
        let open = systems.iter().any(|(_, cores)| {
            cores
                .iter()
                .any(|c| crate::platform::recommended_here(&self.prefs.recommended_on(c)))
        });
        egui::CollapsingHeader::new(RichText::new(text::CORE_CHOICE_TITLE).strong())
            .id_salt("core-choice")
            .default_open(open)
            .show(ui, |ui| {
                for (system, cores) in systems {
                    let name = emu_library::systems()
                        .iter()
                        .find(|s| s.database == system)
                        .map_or_else(|| system.clone(), |s| s.name.clone());
                    let current = self.prefs.for_system(&system).first().cloned();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{name} :"));
                        let mut sorted = cores.clone();
                        sorted.sort();
                        for core in sorted {
                            let display = self
                                .snapshot
                                .as_ref()
                                .and_then(|s| s.infos.get(&core))
                                .map_or_else(
                                    || core.to_string(),
                                    |i| sanitize_core_text(&i.display_name),
                                );
                            let mut label = display;
                            if let Some(hint) = text::core_choice_hint(core.as_str()) {
                                label.push_str(&format!(" ({hint})"));
                            }
                            let on = self.prefs.recommended_on(&core);
                            if let Some(r) = text::recommended_on(&on) {
                                label.push_str(&format!(" ({r})"));
                            }
                            if !installed.contains(&core) {
                                label.push_str(&format!(" — {}", text::CORE_NOT_INSTALLED));
                            }
                            let mut rich = RichText::new(label);
                            if crate::platform::recommended_here(&on) {
                                rich = rich.strong();
                            }
                            let selected = current.as_ref() == Some(&core);
                            if ui.selectable_label(selected, rich).clicked() && !selected {
                                event = Some(CoresEvent::PreferCore {
                                    system: system.clone(),
                                    core: core.clone(),
                                });
                            }
                        }
                    });
                    // The chosen core is not installed: install it from here (7.0: one click for SwanStation or
                    // Beetle PSX), its row below offers the same.
                    if let Some(core) = current.filter(|c| !installed.contains(c))
                        && !self.installs.contains_key(&core)
                        && ui
                            .small_button(text::install_chosen(core.as_str()))
                            .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.request_install(core, &ctx);
                    }
                }
            });
        event
    }

    fn filter_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.filter.scope, Scope::Release, text::SCOPE_RELEASE);
            ui.selectable_value(&mut self.filter.scope, Scope::All, text::SCOPE_ALL);
            if self.filter.scope == Scope::All {
                ui.label(text::SEARCH);
                ui.text_edit_singleline(&mut self.filter.search);
            }
        });
    }

    fn list(&mut self, ui: &mut Ui) -> Vec<RowAction> {
        let rows = self.visible_rows();
        let mut actions = Vec::new();
        if rows.is_empty() {
            ui.label(text::NOTHING_TO_SHOW);
        }
        if self.snapshot.as_ref().is_some_and(|s| s.catalog.is_none()) {
            ui.label(RichText::new(text::NO_CATALOG_HINT).small().weak());
        }
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &rows {
                    self.row_ui(ui, row, &mut actions);
                }
            });
        actions
    }

    fn row_ui(&mut self, ui: &mut Ui, row: &CoreRow, actions: &mut Vec<RowAction>) {
        let running = self.installs.get(&row.id).map(|r| r.progress);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&row.name).strong());
                let (label, color) = match row.state {
                    CoreState::NotInstalled => (text::STATE_NOT_INSTALLED, Color32::GRAY),
                    CoreState::UpToDate => (text::STATE_UP_TO_DATE, GOOD),
                    CoreState::UpdateAvailable => (text::STATE_UPDATE, INFO),
                    CoreState::Manual => (text::STATE_MANUAL, Color32::LIGHT_GRAY),
                    CoreState::Tampered => (text::STATE_TAMPERED, ERROR),
                };
                ui.colored_label(color, label);
                if row.pinned {
                    ui.colored_label(WARNING, text::STATE_PINNED);
                }
                if row.recommended {
                    ui.label(RichText::new(text::RECOMMENDED).small().weak());
                }
                if row.in_use {
                    ui.colored_label(WARNING, text::IN_USE);
                }
            });
            ui.horizontal_wrapped(|ui| {
                let mut parts: Vec<String> = Vec::new();
                if !row.systems.is_empty() {
                    parts.push(row.systems.join(", "));
                }
                if let Some(l) = &row.license {
                    parts.push(l.clone());
                }
                match (row.state, row.installed) {
                    (CoreState::Manual, _) => parts.push(text::MANUAL.to_owned()),
                    (_, Some(d)) => parts.push(text::installed(d)),
                    _ => {}
                }
                if let Some(d) = row.available {
                    parts.push(text::available(d));
                }
                ui.label(RichText::new(parts.join(" · ")).small().weak());
            });
            let fw = text::firmware_summary(row.firmware_missing, row.firmware_wrong);
            if !fw.is_empty() {
                ui.colored_label(WARNING, RichText::new(fw).small());
            }
            if let Some(check) = &row.required {
                let notice = self.prefs.builtin_firmware(&row.id).map(|b| b.notice);
                let files = region_files(&self.prefs, &row.id);
                self.required_line(ui, check, notice.as_deref(), &files);
            }
            if let Some(e) = self.errors.get(&row.id) {
                ui.colored_label(ERROR, e);
            }
            let op = self.ops.get(&row.id).map(|o| o.kind);
            if op.is_none()
                && running.is_none()
                && self.offer_reinstall.contains(&row.id)
                && ui.button(text::REINSTALL).clicked()
            {
                actions.push(RowAction::Install(row.id.clone()));
            }
            if let Some(progress) = running {
                ui.horizontal(|ui| {
                    match progress {
                        Some(Progress {
                            done,
                            total: Some(t),
                        }) if t > 0 => {
                            #[allow(clippy::cast_precision_loss)] // display only
                            let f = (done as f32 / t as f32).clamp(0.0, 1.0);
                            ui.add(
                                egui::ProgressBar::new(f)
                                    .desired_width(240.0)
                                    .text(text::progress(done, Some(t))),
                            );
                        }
                        Some(p) => {
                            ui.label(text::progress(p.done, p.total));
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                    if ui.button(text::CANCEL).clicked() {
                        actions.push(RowAction::CancelInstall(row.id.clone()));
                    }
                });
            } else if let Some(kind) = op {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(text::op_running(match kind {
                        OpKind::Remove => text::OP_REMOVING,
                        OpKind::Rollback => text::OP_ROLLING_BACK,
                        OpKind::Pin(_) => text::OP_PINNING,
                    }));
                });
            } else if self.confirm_remove.as_ref() == Some(&row.id) {
                ui.horizontal(|ui| {
                    ui.label(text::confirm_remove(&row.name));
                    let no = ui.button(text::CONFIRM_REMOVE_NO);
                    if std::mem::take(&mut self.focus_confirm) {
                        no.request_focus();
                    }
                    if no.clicked() {
                        actions.push(RowAction::CancelRemove);
                    }
                    if ui.button(text::CONFIRM_REMOVE_YES).clicked() {
                        actions.push(RowAction::Remove(row.id.clone()));
                    }
                });
            } else {
                self.row_buttons(ui, row, actions);
            }
        });
    }

    fn row_buttons(&self, ui: &mut Ui, row: &CoreRow, actions: &mut Vec<RowAction>) {
        let idle = !row.in_use;
        ui.horizontal_wrapped(|ui| {
            let primary = match row.state {
                CoreState::NotInstalled => Some((text::INSTALL, row.downloadable)),
                CoreState::UpdateAvailable => Some((text::UPDATE, true)),
                CoreState::Tampered => Some((text::REINSTALL, row.downloadable)),
                CoreState::Manual => Some((text::INSTALL, row.downloadable)),
                CoreState::UpToDate => None,
            };
            if let Some((label, available)) = primary
                && ui
                    .add_enabled(available && idle, Button::new(label))
                    .clicked()
            {
                actions.push(RowAction::Install(row.id.clone()));
            }
            if row.is_installed() {
                if row.has_previous && ui.add_enabled(idle, Button::new(text::ROLLBACK)).clicked() {
                    actions.push(RowAction::Rollback(row.id.clone()));
                }
                if matches!(
                    row.state,
                    CoreState::UpToDate | CoreState::UpdateAvailable | CoreState::Tampered
                ) {
                    let label = if row.pinned { text::UNPIN } else { text::PIN };
                    if ui.button(label).clicked() {
                        actions.push(RowAction::Pin(row.id.clone(), !row.pinned));
                    }
                }
                if ui.add_enabled(idle, Button::new(text::REMOVE)).clicked() {
                    actions.push(RowAction::AskRemove(row.id.clone()));
                }
            }
            if ui.button(text::DETAILS).clicked() {
                actions.push(RowAction::Details(row.id.clone()));
            }
        });
    }

    fn detail_view(&mut self, ui: &mut Ui, id: &CoreId) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        let info = manager.info(id);
        let name = info.as_ref().map_or_else(
            || id.as_str().to_owned(),
            |i| sanitize_core_text(&i.display_name),
        );
        let close = ui.button(text::DETAIL_CLOSE);
        if std::mem::take(&mut self.focus_first) {
            close.request_focus();
        }
        if close.clicked() {
            self.detail = None;
        }
        ui.heading(name);
        let Some(info) = info else {
            ui.label(text::NO_CATALOG_HINT);
            return;
        };
        if let Some(d) = &info.description {
            ui.label(sanitize_core_text(d));
        }
        if !info.extensions.is_empty() {
            ui.label(format!(
                "{} : {}",
                text::DETAIL_EXTENSIONS,
                info.extensions.join(", ")
            ));
        }
        ui.separator();
        ui.label(RichText::new(text::DETAIL_FIRMWARE).strong());
        let statuses = &self.detail_firmware;
        if statuses.is_empty() {
            ui.label(text::DETAIL_NO_FIRMWARE);
        }
        for f in statuses {
            let kind = if f.spec.optional {
                text::FIRMWARE_OPTIONAL
            } else {
                text::FIRMWARE_REQUIRED
            };
            let color = match f.state {
                emu_assets::cores::FirmwareState::Ok => GOOD,
                emu_assets::cores::FirmwareState::Unverified => Color32::LIGHT_GRAY,
                _ if f.spec.optional => WARNING,
                _ => ERROR,
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(sanitize_core_text(&f.spec.path)).monospace());
                ui.label(
                    RichText::new(format!(
                        "({kind}) {}",
                        sanitize_core_text(&f.spec.description)
                    ))
                    .small()
                    .weak(),
                );
                ui.colored_label(color, text::firmware_state(&f.state));
                if !matches!(f.state, FirmwareState::Ok)
                    && ui.small_button(text::IMPORT_ONE).clicked()
                {
                    self.import.start(Some(f.spec.path.clone()));
                }
            });
        }
        let jiffy_present = statuses.iter().any(|f| {
            f.spec.path.starts_with("vice/JiffyDOS_") && matches!(f.state, FirmwareState::Ok)
        });
        if jiffy_present && id.as_str().starts_with("vice_x64") {
            if ui.button(text::JIFFYDOS_USE_FOR_ALL).clicked() {
                self.detail_option = Some((
                    id.as_str().to_owned(),
                    JIFFYDOS_OPTION.to_owned(),
                    "enabled".to_owned(),
                ));
                self.detail_note = Some(text::JIFFYDOS_USED_FOR_ALL.to_owned());
            }
            if let Some(note) = &self.detail_note {
                ui.colored_label(GOOD, note);
            }
        }
        ui.label(format!(
            "{} : {}",
            text::DETAIL_SYSTEM_DIR,
            self.system_dir.display()
        ));
        ui.label(RichText::new(text::DETAIL_FIRMWARE_NOTE).small().weak());
    }
}

/// Distinct files of the core's region table (Europe, USA, Japan order), when there are several (PlayStation);
/// empty for a table with one file (3DO) or keyed by model (Amiga).
pub(crate) fn region_files(prefs: &CorePreferences, core: &CoreId) -> Vec<String> {
    let Some(system) = crate::launch_firmware::firmware_system_for(prefs, core, None) else {
        return Vec::new();
    };
    let Some(req) = prefs.required_firmware(core, &system) else {
        return Vec::new();
    };
    if req.variant != emu_assets::cores::FirmwareVariant::Region {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for key in ["europe", "usa", "japan"] {
        if let Some(f) = req.files.get(key)
            && !out.contains(f)
        {
            out.push(f.clone());
        }
    }
    for f in req.files.values() {
        if !out.contains(f) {
            out.push(f.clone());
        }
    }
    if out.len() > 1 { out } else { Vec::new() }
}

#[cfg(test)]
mod tests;
