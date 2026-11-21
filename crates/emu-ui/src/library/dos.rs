//! PC-DOS in the library (design v4.0 §2.1-§2.2, §4): folder snapshot built before the launch (job « Préparation
//! du jeu… », progress, cancel), « Lancer un autre programme… » for a folder game, DOS cache of the settings page.
//! Nothing here blocks the UI thread: the snapshot and the cache size run on worker threads.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Button, Key, ProgressBar, RichText};
use emu_assets::cores::CoreId;
use emu_assets::http::Progress;
use emu_assets::jobs::{Job, JobEvent};
use emu_library::Game;
use emu_runtime::{RuntimeError, SnapshotLimits, SnapshotReport, snapshot_folder};

use super::{Dialog, Focus, LaunchRequest, LibraryEvent, LibraryUi, notify, text};
use crate::GameContent;
use crate::computer::dos::{self, DosPlan};
use crate::cores_ui;

/// Snapshot being built for a launch.
pub(super) struct SnapshotTask {
    request: LaunchRequest,
    job: Job<Result<SnapshotReport, RuntimeError>>,
    progress: Option<Progress>,
    focus: bool,
}

/// « Lancer un autre programme… » of a folder game.
pub(super) struct ProgramsDialog {
    game: Game,
    pub(super) programs: Vec<String>,
    /// Program picked, waiting for « pour cette fois » / « par défaut ».
    pub(super) chosen: Option<String>,
    /// The core remembers a boot program (`AUTOBOOT.DBP`), which wins over `[dos] start`.
    autoboot: bool,
    /// `[dos] start` of the game, if any.
    current: Option<String>,
    pub(super) options_path: PathBuf,
    focus: bool,
}

/// DOS cache size shown by the settings page, measured off the UI thread.
#[derive(Default)]
pub(super) struct DosCache {
    pub size: Option<u64>,
    rx: Option<Receiver<u64>>,
    pub message: Option<String>,
}

impl LibraryUi {
    /// Launch events of folder games go through the snapshot first; the others pass as they are.
    pub(super) fn gate_dos_launches(&mut self, ctx: &egui::Context) {
        let events = std::mem::take(&mut self.events);
        for event in events {
            match event {
                LibraryEvent::Launch(request)
                    if matches!(request.content, GameContent::Folder { .. }) =>
                {
                    if let Some(request) = self.dos_gate(*request, ctx) {
                        self.events.push(LibraryEvent::Launch(Box::new(request)));
                    }
                }
                other => self.events.push(other),
            }
        }
    }

    /// `Some` when the snapshot is current (launch now); `None` when a job or an error dialog took over.
    fn dos_gate(&mut self, request: LaunchRequest, ctx: &egui::Context) -> Option<LaunchRequest> {
        if self.snapshot.is_some() {
            self.abort_launch(text::DOS_BUSY.to_owned());
            return None;
        }
        let prefs = cores_ui::preferences(&self.config.cores);
        let spec = match dos::spec_for_content(&request.content, &prefs, &request.core) {
            Ok(spec) => spec,
            Err(e) => {
                self.abort_launch(text::game_failed(&request.title, &e));
                return None;
            }
        };
        // Same identity as `App::build`: the snapshot is named after its `save_name`.
        let resolved = crate::game_identity::resolve_game(request.core.as_str(), spec, &self.dirs);
        let plan = dos::snapshot_plan(
            resolved.spec,
            &resolved.identity.save_name,
            &self.dirs.cache_dir(),
            &request.core,
        );
        match plan {
            Ok(DosPlan::Ready(_)) => Some(request),
            Ok(DosPlan::NeedsSnapshot { dir, snapshot, .. }) => {
                tracing::info!(
                    "instantané de {} vers {}",
                    dir.display(),
                    snapshot.display()
                );
                let job = Job::spawn("dos-snapshot", notify(ctx), move |progress, cancel| {
                    let stop = AtomicBool::new(false);
                    snapshot_folder(
                        &dir,
                        &snapshot,
                        &SnapshotLimits::DEFAULT,
                        &mut |done, total| {
                            if cancel.is_cancelled() {
                                stop.store(true, Ordering::Relaxed);
                            }
                            progress(Progress {
                                done,
                                total: Some(total),
                            });
                        },
                        &stop,
                    )
                });
                self.snapshot = Some(SnapshotTask {
                    request,
                    job,
                    progress: None,
                    focus: true,
                });
                None
            }
            Err(e) => {
                self.abort_launch(text::game_failed(&request.title, &e));
                None
            }
        }
    }

    /// The launch stops before the game: the core marked in use by `plan_launch` is released.
    fn abort_launch(&mut self, message: String) {
        if let Some(m) = self.cores.manager() {
            cores_ui::release(m);
        }
        self.open_dialog(Dialog::Message(message));
    }

    /// « Préparation du jeu… » window; on success the launch goes on.
    pub(super) fn snapshot_dialog(&mut self, ctx: &egui::Context) {
        let Some(task) = &mut self.snapshot else {
            return;
        };
        let mut result = None;
        for ev in task.job.poll() {
            match ev {
                JobEvent::Progress(p) => task.progress = Some(p),
                JobEvent::Done(r) => result = Some(r),
            }
        }
        if result.is_none() && task.job.is_finished() {
            result = Some(Err(RuntimeError::SnapshotCancelled));
        }
        if let Some(result) = result {
            let Some(task) = self.snapshot.take() else {
                return;
            };
            match result {
                Ok(report) => {
                    tracing::info!(
                        "instantané prêt : {} fichiers, {} octets",
                        report.files,
                        report.bytes
                    );
                    self.events
                        .push(LibraryEvent::Launch(Box::new(task.request)));
                }
                Err(RuntimeError::SnapshotCancelled) => {
                    if let Some(m) = self.cores.manager() {
                        cores_ui::release(m);
                    }
                    self.set_status(text::DOS_SNAPSHOT_CANCELLED.to_owned(), super::Level::Info);
                    self.focus = Some(Focus::Grid);
                }
                Err(e) => self.abort_launch(text::game_failed(
                    &task.request.title,
                    &crate::ui::text::dos_snapshot_failed(&e),
                )),
            }
            return;
        }
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut task.focus);
        let mut cancel = escape;
        egui::Modal::new(egui::Id::unique("library-dos-snapshot")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(text::DOS_SNAPSHOT_TITLE);
            ui.label(text::dos_snapshot_body(&task.request.title));
            match task.progress.and_then(|p| p.total.map(|t| (p.done, t))) {
                #[allow(clippy::cast_precision_loss)] // display only
                Some((done, total)) if total > 0 => {
                    ui.add(
                        ProgressBar::new(done as f32 / total as f32)
                            .text(cores_ui::text::progress(done, Some(total))),
                    );
                }
                _ => {
                    ui.spinner();
                }
            }
            let c = ui.button(text::LAUNCH_CANCEL);
            if focus {
                c.request_focus();
            }
            cancel |= c.clicked();
            ui.label(RichText::new(text::DOS_SNAPSHOT_NOTE).small().weak());
        });
        if cancel {
            task.job.cancel();
        }
    }

    pub(super) fn snapshot_busy(&self) -> bool {
        self.snapshot.is_some()
    }

    /// Key P on a folder game: its root programs.
    pub(super) fn open_programs(&mut self) {
        let Some(game) = self.selected_game().filter(|g| g.is_dir).cloned() else {
            return;
        };
        let programs = dos::root_executables(&game.path);
        if programs.is_empty() {
            self.set_status(text::DOS_NO_PROGRAM.to_owned(), super::Level::Warn);
            return;
        }
        let prefs = cores_ui::preferences(&self.config.cores);
        let core = prefs
            .for_system("DOS")
            .first()
            .cloned()
            .or_else(|| CoreId::new("dosbox_pure"));
        let Some(core) = core else {
            return;
        };
        let content = GameContent::Folder {
            dir: game.path.clone(),
            start: None,
        };
        let spec = crate::content::content_spec(core.as_str(), &content, &prefs);
        let resolved = crate::game_identity::resolve_game(core.as_str(), spec, &self.dirs);
        let pure_zip = resolved
            .identity
            .core_save_dir
            .join(format!("{}.pure.zip", resolved.identity.save_name));
        let autoboot = emu_runtime::archive_entry_names(&pure_zip)
            .is_ok_and(|names| names.iter().any(|n| n.eq_ignore_ascii_case("AUTOBOOT.DBP")));
        let current = crate::game_options::GameOptions::load(&resolved.options_path)
            .options
            .dos
            .start;
        self.programs = Some(ProgramsDialog {
            game,
            programs,
            chosen: None,
            autoboot,
            current,
            options_path: resolved.options_path,
            focus: true,
        });
    }

    pub(super) fn programs_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.programs else {
            return;
        };
        enum Choice {
            Close,
            Pick(String),
            Once,
            Default,
            ClearDefault,
            Back,
        }
        let mut choice = None;
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut d.focus);
        egui::Modal::new(egui::Id::unique("library-dos-programs")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(text::DOS_PROGRAMS_TITLE);
            ui.label(&d.game.title);
            match &d.chosen {
                None => {
                    if let Some(current) = &d.current {
                        ui.label(text::dos_default_program(current));
                    }
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for (i, p) in d.programs.iter().enumerate() {
                                let b = ui.add(Button::new(p).min_size([240.0, 24.0].into()));
                                if focus && i == 0 {
                                    b.request_focus();
                                }
                                if b.clicked() {
                                    choice = Some(Choice::Pick(p.clone()));
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if d.current.is_some() && ui.button(text::DOS_CLEAR_DEFAULT).clicked() {
                            choice = Some(Choice::ClearDefault);
                        }
                        if ui.button(text::LAUNCH_CANCEL).clicked() || escape {
                            choice = Some(Choice::Close);
                        }
                    });
                }
                Some(p) => {
                    ui.label(RichText::new(p).strong());
                    let once = ui.button(text::DOS_ONCE);
                    if focus {
                        once.request_focus();
                    }
                    if once.clicked() {
                        choice = Some(Choice::Once);
                    }
                    if d.autoboot {
                        ui.label(RichText::new(text::DOS_AUTOBOOT_WINS).small().weak());
                    } else if ui.button(text::DOS_BY_DEFAULT).clicked() {
                        choice = Some(Choice::Default);
                    }
                    if ui.button(crate::ui::text::BACK).clicked() || escape {
                        choice = Some(Choice::Back);
                    }
                }
            }
            ui.label(RichText::new(text::DOS_PROGRAMS_HINT).small().weak());
        });
        let Some(choice) = choice else {
            return;
        };
        match choice {
            Choice::Pick(p) => {
                d.chosen = Some(p);
                d.focus = true;
            }
            Choice::Back => {
                d.chosen = None;
                d.focus = true;
            }
            Choice::Close => {
                self.programs = None;
                self.focus = Some(Focus::Grid);
            }
            Choice::ClearDefault => {
                let path = d.options_path.clone();
                self.programs = None;
                self.update_game_options(&path, |o| o.dos.start = None);
                self.set_status(text::DOS_DEFAULT_CLEARED.to_owned(), super::Level::Info);
                self.focus = Some(Focus::Grid);
            }
            Choice::Once | Choice::Default => {
                let Some(d) = self.programs.take() else {
                    return;
                };
                let Some(start) = d.chosen else {
                    return;
                };
                if matches!(choice, Choice::Default) {
                    let s = start.clone();
                    self.update_game_options(&d.options_path, move |o| o.dos.start = Some(s));
                }
                self.focus = Some(Focus::Grid);
                let game = d.game;
                self.launch(
                    Some(game.id),
                    game.title.clone(),
                    &game.system,
                    None,
                    GameContent::Folder {
                        dir: game.path.clone(),
                        start: Some(start),
                    },
                    true,
                );
            }
        }
    }

    pub(super) fn programs_open(&self) -> bool {
        self.programs.is_some()
    }

    /// Measures the DOS cache on a worker thread (settings page opened, cache cleared).
    pub(super) fn refresh_dos_cache(&mut self) {
        let prefs = cores_ui::preferences(&self.config.cores);
        let dirs = dos::clearable_cache_dirs(&prefs, &self.dirs.cache_dir());
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("emu-dos-cache".into())
            .spawn(move || {
                let _ = tx.send(dirs.iter().map(|d| dos::cache_size(d)).sum());
            });
        if spawned.is_ok() {
            self.dos_cache.rx = Some(rx);
        }
    }

    pub(super) fn poll_dos_cache(&mut self) {
        let Some(rx) = &self.dos_cache.rx else {
            return;
        };
        match rx.try_recv() {
            Ok(size) => {
                self.dos_cache.size = Some(size);
                self.dos_cache.rx = None;
            }
            Err(TryRecvError::Disconnected) => self.dos_cache.rx = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// « Vider le cache DOS » of the settings page. The library page is only shown with no game running, so no
    /// snapshot is open (Windows could not delete it otherwise); while a snapshot is being built it refuses (`DOS_BUSY`).
    pub(super) fn clear_dos_cache(&mut self) {
        if self.snapshot.is_some() {
            self.dos_cache.message = Some(text::DOS_BUSY.to_owned());
            return;
        }
        let prefs = cores_ui::preferences(&self.config.cores);
        let mut result = Ok(());
        for dir in dos::clearable_cache_dirs(&prefs, &self.dirs.cache_dir()) {
            if let Err(e) = dos::clear_cache(&dir) {
                result = Err(e);
            }
        }
        self.dos_cache.message = Some(match result {
            Ok(()) => text::DOS_CACHE_CLEARED.to_owned(),
            Err(e) => text::dos_cache_failed(&e.to_string()),
        });
        self.refresh_dos_cache();
    }

    pub(super) fn dos_cache_section(&mut self, ui: &mut egui::Ui) {
        self.poll_dos_cache();
        ui.label(RichText::new(text::DOS_CACHE_SECTION).strong());
        let size = self.dos_cache.size;
        let label = text::dos_cache_button(size);
        if ui
            .add_enabled(
                !self.capture && size.is_some_and(|s| s > 0),
                Button::new(label),
            )
            .clicked()
        {
            self.clear_dos_cache();
        }
        super::weak(ui, text::DOS_CACHE_NOTE);
        if let Some(m) = &self.dos_cache.message {
            super::weak(ui, m.clone());
        }
    }
}
