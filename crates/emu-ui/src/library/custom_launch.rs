//! Launch of a game of a custom system (ADR 0025 §5 to §7, design 8.0 §1): hardware rendering refused, the
//! unverified content of a core accepted once, then, at the first launch of each game, the « non testé » warning
//! and the dry run in a child process. Every step is a dialog polled once per frame; the dry run runs in a job,
//! so the interface never waits for it, and abandoning the wait (or closing the window) kills the child.
//!
//! The gate sits on the launch events of the library (`gate_custom_launches`), after every other check: a launch
//! that comes back from a BIOS dialog goes through it too. Official systems pass untouched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{Key, ProgressBar, RichText};
use emu_assets::jobs::{Job, JobEvent};
use emu_library::SystemInfo;
use emu_runtime::DryRunRequest;

use super::{Dialog, Focus, LaunchRequest, Level, LibraryEvent, LibraryUi, notify, text};
use crate::GameContent;
use crate::cores_ui::{self, CustomLaunchState, CustomStep, custom_launch_steps};
use crate::crash_guard::CrashNotice;
use crate::dry_run::{
    DRY_RUN_FRAMES, DRY_RUN_TIMEOUT, DryRunCommand, DryRunDir, DryRunVerdict, GameCopyError,
    run_dry_run_until,
};
use crate::dry_run_log::{DryRunKey, DryRunLog, core_sha256};

/// What a dry run came to.
#[derive(Debug)]
pub(super) enum Tried {
    Verdict(DryRunVerdict),
    /// The game was not tried (the reason is shown); the launch goes on after the warning.
    NotTried(&'static str),
}

pub(super) struct TryTask {
    request: LaunchRequest,
    /// The try and the SHA-256 of the core it was made with (hashed on the job thread, empty when unreadable).
    job: Job<(Tried, String)>,
    key: DryRunKey,
    focus: bool,
}

/// State of the custom launches of the library.
#[derive(Default)]
pub(super) struct State {
    task: Option<TryTask>,
    /// `(core, title)` of the launches the user or the dry run cleared: not gated a second time.
    pub(super) cleared: Vec<(String, String)>,
    /// SHA-256 of cores already hashed, by (path, size, modification time): a core is hashed once, not at every launch.
    sha_cache: HashMap<(PathBuf, u64, SystemTime), String>,
    /// The dry-run child: this executable unless a test gives its own (`test_support`).
    pub(super) command: Option<DryRunCommand>,
}

impl State {
    /// SHA-256 of the core file (`None` when unreadable), from the cache while the file has not changed.
    fn core_sha256(&mut self, path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let key = (path.to_owned(), meta.len(), meta.modified().ok()?);
        if let Some(sha) = self.sha_cache.get(&key) {
            return Some(sha.clone());
        }
        let sha = core_sha256(path)?;
        self.sha_cache.insert(key, sha.clone());
        Some(sha)
    }
}

/// The button pressed in a dialog of [`CustomDialog`]: the first one, the second one, or « Annuler ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pick {
    Primary,
    Secondary,
    Cancel,
}

/// A dialog of the launch of a custom system.
pub(super) enum CustomDialog {
    /// « Contenu non vérifié » (the core is not in `cores.toml`).
    Accept { request: LaunchRequest },
    /// « Système non testé »: `tried` is the key of the dry run, or why the game cannot be tried.
    Untested {
        request: LaunchRequest,
        tried: Result<DryRunKey, &'static str>,
    },
    /// The dry run failed: another core (default) or the launch anyway.
    Failed {
        request: LaunchRequest,
        reason: String,
    },
    /// The previous run stopped during a launch.
    Crash { text: String, untested: bool },
}

/// The custom system of a library key (libretro name, else id); `None` for an official one.
fn custom_system(key: &str) -> Option<&'static SystemInfo> {
    emu_library::systems()
        .iter()
        .find(|s| s.core_key() == key)
        .filter(|s| s.is_custom())
}

/// The dry run of one game in a folder of its own: the game is copied there (the original is never given to the
/// child), the core writes only there, and the folder goes away when the child is over. Runs on a job thread.
pub(super) fn try_game(
    command: &DryRunCommand,
    cache: &Path,
    system_dir: &Path,
    core: &Path,
    game: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Tried {
    let dir = match DryRunDir::create(cache) {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!("dossier d'essai non créé : {e}");
            return Tried::NotTried(text::NOT_TRIED_FAILED);
        }
    };
    let copy = match dir.copy_game(game) {
        Ok(copy) => copy,
        Err(GameCopyError::TooBig(_)) => return Tried::NotTried(text::NOT_TRIED_TOO_BIG),
        Err(e) => {
            tracing::warn!("copie du jeu pour l'essai impossible : {e:?}");
            return Tried::NotTried(text::NOT_TRIED_FAILED);
        }
    };
    let request = DryRunRequest {
        core: core.to_owned(),
        content: Some(copy),
        system_dir: system_dir.to_owned(),
        scratch_dir: dir.path().to_owned(),
        copy_dir: Some(dir.copy_dir()),
        options: Vec::new(),
        frames: DRY_RUN_FRAMES,
    };
    Tried::Verdict(run_dry_run_until(
        command,
        &request,
        DRY_RUN_TIMEOUT,
        cancelled,
    ))
}

/// Key of the dry run of `request` (its `core_sha256` empty: the try fills it in, off the interface thread), or why the game cannot be tried (an archive may need extracting, several
/// files or a folder are not one file to copy).
fn try_key(request: &LaunchRequest, system: &SystemInfo) -> Result<DryRunKey, &'static str> {
    let game = match &request.content {
        GameContent::Files(files) if files.len() == 1 => &files[0],
        _ => return Err(text::NOT_TRIED_SEVERAL_FILES),
    };
    let archive = game
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("7z"));
    if archive {
        return Err(text::NOT_TRIED_ARCHIVE);
    }
    let game = game.to_str().ok_or(text::NOT_TRIED_FAILED)?;
    Ok(DryRunKey {
        system: system.id.clone(),
        core: request.core.to_string(),
        core_sha256: String::new(),
        game: game.to_owned(),
    })
}

impl LibraryUi {
    /// Launch events of custom systems go through the steps first; the others pass as they are.
    pub(super) fn gate_custom_launches(&mut self) {
        let events = std::mem::take(&mut self.events);
        for event in events {
            match event {
                LibraryEvent::Launch(request) => {
                    if let Some(request) = self.custom_gate(*request) {
                        self.events.push(LibraryEvent::Launch(Box::new(request)));
                    }
                }
                other => self.events.push(other),
            }
        }
        // A cleared launch is filtered in this very pass; one that did not reach the gate must not clear another
        // game of the same title later.
        self.custom.cleared.clear();
    }

    /// `Some` when the launch can go on; `None` when a dialog or the dry run took over (or the launch ended).
    fn custom_gate(&mut self, request: LaunchRequest) -> Option<LaunchRequest> {
        let Some(system) = request.system.as_deref().and_then(custom_system) else {
            return Some(request);
        };
        let who = (request.core.to_string(), request.title.clone());
        if let Some(i) = self.custom.cleared.iter().position(|c| *c == who) {
            self.custom.cleared.remove(i);
            return Some(request);
        }
        if self.custom.task.is_some() {
            self.abort_launch(text::CUSTOM_BUSY.to_owned());
            return None;
        }
        let info = self.cores.manager().and_then(|m| m.info(&request.core));
        if let Some(check) = cores_ui::hardware_check(info.as_ref(), &request.core) {
            let message = check.blocking_message().unwrap_or_default();
            self.abort_launch(message);
            return None;
        }
        let prefs = cores_ui::preferences(&self.config.cores);
        let custom = &self.config.custom_systems;
        let tried = try_key(&request, system);
        let works_for_me = custom
            .works_for_me
            .contains(&format!("{}@{}", request.core, system.id));
        let state = CustomLaunchState {
            content_accepted: prefs.is_examined(&request.core)
                || custom.content_accepted.contains(request.core.as_str()),
            works_for_me,
            dry_run_passed: !works_for_me && self.dry_run_passed(&request, &tried),
        };
        for step in custom_launch_steps(system, &request.core, &state) {
            match step {
                CustomStep::AcceptContent => {
                    self.open_dialog(Dialog::Custom(CustomDialog::Accept { request }));
                    return None;
                }
                CustomStep::UntestedWarning => {
                    self.open_dialog(Dialog::Custom(CustomDialog::Untested { request, tried }));
                    return None;
                }
                // Made by « Essayer puis lancer » of the warning.
                CustomStep::DryRun => {}
                CustomStep::Launch => return Some(request),
            }
        }
        Some(request)
    }

    /// Whether the game already passed with this very core file. The core is hashed (once, see [`State`]) only
    /// when a pass of this game and core is remembered; otherwise the hash is made by the try itself.
    fn dry_run_passed(
        &mut self,
        request: &LaunchRequest,
        tried: &Result<DryRunKey, &'static str>,
    ) -> bool {
        let Ok(key) = tried else {
            return false;
        };
        let log = DryRunLog::load(&self.dirs.cache_dir());
        if !log.knows(key) {
            return false;
        }
        let Some(sha) = self.custom.core_sha256(&request.core_path) else {
            return false;
        };
        log.passed(&DryRunKey {
            core_sha256: sha,
            ..key.clone()
        })
    }

    /// The launch of `request` goes on, without the gate asking again.
    fn clear_and_launch(&mut self, request: LaunchRequest) {
        self.custom
            .cleared
            .push((request.core.to_string(), request.title.clone()));
        self.events.push(LibraryEvent::Launch(Box::new(request)));
    }

    fn start_try(&mut self, request: LaunchRequest, key: DryRunKey, ctx: &egui::Context) {
        let cache = self.dirs.cache_dir();
        let system_dir = self.dirs.system.clone();
        let core = request.core_path.clone();
        let game = std::path::PathBuf::from(&key.game);
        let command = self.custom.command.clone();
        let job = Job::spawn("dry-run", notify(ctx), move |_, cancel| {
            let Some(sha) = core_sha256(&core) else {
                return (Tried::NotTried(text::NOT_TRIED_FAILED), String::new());
            };
            let command = match command {
                Some(command) => Ok(command),
                None => DryRunCommand::current_exe(),
            };
            let tried = match command {
                Ok(command) => try_game(&command, &cache, &system_dir, &core, &game, &|| {
                    cancel.is_cancelled()
                }),
                Err(e) => Tried::Verdict(DryRunVerdict::SpawnFailed(e.to_string())),
            };
            (tried, sha)
        });
        self.custom.task = Some(TryTask {
            request,
            job,
            key,
            focus: true,
        });
    }

    pub(super) fn custom_trying(&self) -> bool {
        self.custom.task.is_some()
    }

    /// « Essai du jeu » window while the child runs; the result is handled as soon as it arrives.
    pub(super) fn try_dialog(&mut self, ctx: &egui::Context) {
        let Some(task) = &mut self.custom.task else {
            return;
        };
        let mut result = None;
        for event in task.job.poll() {
            if let JobEvent::Done(done) = event {
                result = Some(Some(done));
            }
        }
        if result.is_none() && task.job.is_finished() {
            // The worker ended without a result (panic): a failed try.
            result = Some(None);
        }
        if let Some(result) = result {
            if let Some(task) = self.custom.task.take() {
                self.finish_try(task, result);
            }
            return;
        }
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut task.focus);
        let mut cancel = escape;
        egui::Modal::new(egui::Id::unique("library-custom-try")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(text::CUSTOM_TRY_TITLE);
            ui.label(text::custom_try_body(&task.request.title));
            ui.add(ProgressBar::new(0.0).animate(true));
            let c = ui.button(text::LAUNCH_CANCEL);
            if focus {
                c.request_focus();
            }
            cancel |= c.clicked();
            ui.label(RichText::new(text::CUSTOM_TRY_NOTE).small().weak());
        });
        if cancel {
            task.job.cancel();
        }
    }

    fn finish_try(&mut self, task: TryTask, result: Option<(Tried, String)>) {
        let TryTask {
            mut request,
            mut key,
            ..
        } = task;
        let failed = |request: LaunchRequest, reason: String| {
            Dialog::Custom(CustomDialog::Failed { request, reason })
        };
        let root = self.dirs.root.clone();
        let Some((tried, sha)) = result else {
            let dialog = failed(request, text::custom_failed_crashed(None));
            self.open_dialog(dialog);
            return;
        };
        key.core_sha256 = sha;
        match tried {
            Tried::NotTried(why) => {
                request.warnings.push(text::custom_not_tried(why));
                self.clear_and_launch(request);
            }
            Tried::Verdict(DryRunVerdict::Passed(_)) => {
                if !self.capture {
                    let mut log = DryRunLog::load(&self.dirs.cache_dir());
                    if let Err(e) = log.record(key, super::now_unix().try_into().unwrap_or(0)) {
                        tracing::warn!("essai à blanc non mémorisé : {e}");
                    }
                }
                self.clear_and_launch(request);
            }
            Tried::Verdict(DryRunVerdict::HardwareRender(_)) => {
                self.abort_launch(cores_ui::text::hardware_render(request.core.as_str()));
            }
            Tried::Verdict(DryRunVerdict::Crashed { code }) => {
                let dialog = failed(request, text::custom_failed_crashed(code));
                self.open_dialog(dialog);
            }
            Tried::Verdict(DryRunVerdict::TimedOut) => {
                let dialog = failed(request, text::CUSTOM_FAILED_TIMEOUT.to_owned());
                self.open_dialog(dialog);
            }
            Tried::Verdict(DryRunVerdict::LoadFailed(message)) => {
                let reason =
                    text::custom_failed_load(&crate::ui::text::display_message(&message, &root));
                let dialog = failed(request, reason);
                self.open_dialog(dialog);
            }
            Tried::Verdict(DryRunVerdict::Cancelled) => {
                if let Some(m) = self.cores.manager() {
                    cores_ui::release(m);
                }
                self.set_status(text::CUSTOM_TRY_CANCELLED.to_owned(), Level::Info);
                self.focus = Some(Focus::Grid);
            }
            // The child could not even start, or the verdict is one this version does not know: the warning was
            // given, the game is not tried.
            Tried::Verdict(other) => {
                tracing::warn!("essai à blanc sans résultat : {other:?}");
                request
                    .warnings
                    .push(text::custom_not_tried(text::NOT_TRIED_FAILED));
                self.clear_and_launch(request);
            }
        }
    }

    /// Dialogs of [`CustomDialog`]. The safe choice has the focus: the try, « Changer de core », « Annuler ».
    pub(super) fn custom_dialog(&mut self, ctx: &egui::Context) {
        let Some(Dialog::Custom(dialog)) = &self.dialog else {
            return;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let mut pick = escape.then_some(Pick::Cancel);
        egui::Modal::new(egui::Id::unique("library-custom-dialog")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            let (title, body) = match dialog {
                CustomDialog::Accept { request } => (
                    text::CUSTOM_ACCEPT_TITLE,
                    text::custom_accept_body(request.core.as_str()),
                ),
                CustomDialog::Untested { request, tried } => {
                    let system = request
                        .system
                        .as_deref()
                        .and_then(custom_system)
                        .map_or_else(String::new, |s| s.name.clone());
                    let mut body =
                        text::custom_untested_body(&request.title, &system, request.core.as_str());
                    match tried {
                        Ok(_) => {
                            body.push(' ');
                            body.push_str(text::CUSTOM_TRY_NOTE);
                        }
                        Err(why) => {
                            body.push(' ');
                            body.push_str(&text::custom_not_tried(why));
                        }
                    }
                    (text::CUSTOM_UNTESTED_TITLE, body)
                }
                CustomDialog::Failed { reason, .. } => (text::CUSTOM_FAILED_TITLE, reason.clone()),
                CustomDialog::Crash { text, .. } => (text::CRASH_NOTICE_TITLE, text.clone()),
            };
            ui.heading(title);
            // Selectable: a message can be copied into a report.
            ui.add(egui::Label::new(&body).selectable(true));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let (primary, secondary, cancel_label) = match dialog {
                    CustomDialog::Accept { .. } => {
                        (Some(text::CUSTOM_ACCEPT_YES), None, text::LAUNCH_CANCEL)
                    }
                    CustomDialog::Untested { tried, .. } => (
                        Some(if tried.is_ok() {
                            text::CUSTOM_TRY_AND_LAUNCH
                        } else {
                            text::CUSTOM_LAUNCH
                        }),
                        Some(text::CUSTOM_WORKS_FOR_ME),
                        text::LAUNCH_CANCEL,
                    ),
                    CustomDialog::Failed { .. } => (
                        Some(text::CUSTOM_CHANGE_CORE),
                        Some(text::LAUNCH_ANYWAY),
                        text::LAUNCH_CANCEL,
                    ),
                    CustomDialog::Crash { untested, .. } => {
                        (untested.then_some(text::CUSTOM_CHANGE_CORE), None, text::OK)
                    }
                };
                let first = primary.map(|label| {
                    let r = ui.button(label);
                    if r.clicked() {
                        pick = Some(Pick::Primary);
                    }
                    r
                });
                if let Some(label) = secondary
                    && ui.button(label).clicked()
                {
                    pick = Some(Pick::Secondary);
                }
                let cancel = ui.button(cancel_label);
                if cancel.clicked() {
                    pick = Some(Pick::Cancel);
                }
                if focus {
                    first.unwrap_or(cancel).request_focus();
                }
            });
        });
        if let Some(pick) = pick {
            self.resolve_custom(pick, ctx);
        }
    }

    /// The answer to the open dialog of [`CustomDialog`] (a button, or Échap = « Annuler »).
    pub(super) fn resolve_custom(&mut self, pick: Pick, ctx: &egui::Context) {
        let Some(Dialog::Custom(dialog)) = self.dialog.take() else {
            return;
        };
        self.focus = Some(Focus::Grid);
        match (dialog, pick) {
            (CustomDialog::Accept { request }, Pick::Primary) => {
                self.config
                    .custom_systems
                    .content_accepted
                    .insert(request.core.to_string());
                self.save_config();
                self.cores
                    .set_content_accepted(self.config.custom_systems.content_accepted.clone());
                self.events.push(LibraryEvent::Launch(Box::new(request)));
            }
            (CustomDialog::Untested { request, tried }, Pick::Primary) => match tried {
                Ok(key) => self.start_try(request, key, ctx),
                Err(why) => {
                    let mut request = request;
                    request.warnings.push(text::custom_not_tried(why));
                    self.clear_and_launch(request);
                }
            },
            (CustomDialog::Untested { request, .. }, Pick::Secondary) => {
                if let Some(system) = request.system.as_deref().and_then(custom_system) {
                    self.config
                        .custom_systems
                        .works_for_me
                        .insert(format!("{}@{}", request.core, system.id));
                    self.save_config();
                }
                self.clear_and_launch(request);
            }
            (CustomDialog::Failed { request, .. }, Pick::Primary) => {
                drop(request);
                self.release_core();
                self.change_core();
            }
            (CustomDialog::Failed { request, .. }, Pick::Secondary) => {
                self.clear_and_launch(request);
            }
            (CustomDialog::Crash { untested: true, .. }, Pick::Primary) => self.change_core(),
            (CustomDialog::Crash { .. }, _) => {}
            // « Annuler »: the launch stops, the core marked in use is released.
            (CustomDialog::Accept { .. } | CustomDialog::Untested { .. }, _)
            | (CustomDialog::Failed { .. }, Pick::Cancel) => self.release_core(),
        }
    }

    fn release_core(&mut self) {
        if let Some(m) = self.cores.manager() {
            cores_ui::release(m);
        }
    }

    /// « Changer de core »: the cores screen, where « Core utilisé » is chosen per system.
    fn change_core(&mut self) {
        self.set_page(super::Page::Cores);
        self.set_status(text::CUSTOM_CHANGE_CORE_HINT.to_owned(), Level::Info);
    }

    /// At start-up: the notice of a previous run that stopped during a launch (the most recent one).
    pub(super) fn show_crash_notices(&mut self) {
        if self.capture {
            return;
        }
        let notices: Vec<CrashNotice> = crate::crash_guard::take_crash_notices();
        if let Some(last) = notices.last() {
            let text = text::crash_notice(&last.what.game, &last.what.core, last.what.untested);
            self.open_dialog(Dialog::Custom(CustomDialog::Crash {
                text,
                untested: last.what.untested,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use emu_assets::cores::CoreId;
    use emu_library::{SystemInfo, SystemOrigin};

    use super::*;

    fn system() -> SystemInfo {
        SystemInfo::synthetic("acme", "Acme", "acm", SystemOrigin::Custom)
    }

    fn request(content: GameContent, core_path: PathBuf) -> LaunchRequest {
        LaunchRequest {
            game: None,
            title: "Jeu".to_owned(),
            core: CoreId::new("acme_core").unwrap(),
            core_path,
            content,
            computer: false,
            warnings: Vec::new(),
            system: Some("acme".to_owned()),
            region: None,
            spec: None,
        }
    }

    #[test]
    fn a_single_plain_file_is_tried_with_a_key_made_of_its_four_parts() {
        let dir = tempfile::tempdir().unwrap();
        let (core, game) = (dir.path().join("core.so"), dir.path().join("a.acm"));
        fs::write(&core, b"abc").unwrap();
        fs::write(&game, b"rom").unwrap();
        let key = try_key(
            &request(GameContent::Files(vec![game.clone()]), core),
            &system(),
        )
        .unwrap();
        assert_eq!(key.system, "acme");
        assert_eq!(key.core, "acme_core");
        assert_eq!(
            key.core_sha256, "",
            "the core is hashed by the try, not here"
        );
        assert_eq!(key.game, game.to_str().unwrap());
    }

    #[test]
    fn archives_several_files_folders_and_unreadable_cores_are_not_tried() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("core.so");
        fs::write(&core, b"abc").unwrap();
        let tried = |content: GameContent, core: &Path| {
            try_key(&request(content, core.to_owned()), &system())
        };
        for archive in ["a.zip", "a.ZIP", "a.7z"] {
            let content = GameContent::Files(vec![dir.path().join(archive)]);
            assert_eq!(
                tried(content, &core),
                Err(text::NOT_TRIED_ARCHIVE),
                "{archive}"
            );
        }
        let two = GameContent::Files(vec![dir.path().join("a.acm"), dir.path().join("b.acm")]);
        assert_eq!(tried(two, &core), Err(text::NOT_TRIED_SEVERAL_FILES));
        let folder = GameContent::Folder {
            dir: dir.path().to_owned(),
            start: None,
        };
        assert_eq!(tried(folder, &core), Err(text::NOT_TRIED_SEVERAL_FILES));
        assert_eq!(
            tried(GameContent::NoGame, &core),
            Err(text::NOT_TRIED_SEVERAL_FILES)
        );
    }

    #[test]
    fn the_core_hash_is_cached_until_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("core.so");
        fs::write(&core, b"abc").unwrap();
        let mut state = State::default();
        let first = state.core_sha256(&core).unwrap();
        assert_eq!(state.sha_cache.len(), 1);
        assert_eq!(state.core_sha256(&core), Some(first.clone()));
        assert_eq!(state.sha_cache.len(), 1, "second call from the cache");
        fs::write(&core, b"abcd").unwrap();
        assert_ne!(state.core_sha256(&core).unwrap(), first);
        assert_eq!(state.core_sha256(&dir.path().join("absent.so")), None);
    }

    #[cfg(unix)]
    mod with_shell {
        use std::ffi::OsString;

        use super::*;

        fn shell(script: &str) -> DryRunCommand {
            DryRunCommand {
                program: PathBuf::from("sh"),
                args: ["-c", script, "child"].map(OsString::from).to_vec(),
                env: Vec::new(),
            }
        }

        /// `<cache>/dry-run` entries left behind.
        fn left_over(cache: &Path) -> usize {
            fs::read_dir(cache.join("dry-run")).map_or(0, |d| d.flatten().count())
        }

        #[test]
        fn the_child_sees_a_copy_and_the_folder_is_gone_afterwards() {
            let root = tempfile::tempdir().unwrap();
            let base = root.path().canonicalize().unwrap();
            let game = base.join("a.acm");
            fs::write(&game, b"rom").unwrap();
            let cache = base.join("cache");
            // `$1` is the request file: the copy of the game is in `game/` next to it, and the result goes there.
            let script = r#"d="$(dirname "$1")"; test "$(cat "$d/game/a.acm")" = rom || exit 9;
                echo 'outcome = "ran"' > "$d/result.toml""#;
            let tried = try_game(
                &shell(script),
                &cache,
                &base.join("system"),
                &base.join("core.so"),
                &game,
                &|| false,
            );
            assert!(
                matches!(tried, Tried::Verdict(DryRunVerdict::Passed(_))),
                "{tried:?}"
            );
            assert_eq!(left_over(&cache), 0, "the dry-run folder was removed");
            assert_eq!(fs::read(&game).unwrap(), b"rom");
        }

        #[test]
        fn a_game_too_big_to_copy_is_not_tried_and_leaves_nothing() {
            let root = tempfile::tempdir().unwrap();
            let base = root.path().canonicalize().unwrap();
            let game = base.join("big.acm");
            fs::File::create(&game)
                .unwrap()
                .set_len(crate::dry_run::MAX_GAME_COPY_BYTES + 1)
                .unwrap();
            let cache = base.join("cache");
            let tried = try_game(
                &shell("exit 0"),
                &cache,
                &base,
                &base.join("core.so"),
                &game,
                &|| false,
            );
            assert!(matches!(tried, Tried::NotTried(m) if m == text::NOT_TRIED_TOO_BIG));
            assert_eq!(left_over(&cache), 0);
        }

        #[test]
        fn giving_up_the_wait_ends_the_try_without_waiting_for_the_timeout() {
            let root = tempfile::tempdir().unwrap();
            let base = root.path().canonicalize().unwrap();
            let game = base.join("a.acm");
            fs::write(&game, b"rom").unwrap();
            let cache = base.join("cache");
            let started = std::time::Instant::now();
            let tried = try_game(
                &shell("exec sleep 60"),
                &cache,
                &base,
                &base.join("core.so"),
                &game,
                &|| started.elapsed() > std::time::Duration::from_millis(300),
            );
            assert!(
                matches!(tried, Tried::Verdict(DryRunVerdict::Cancelled)),
                "{tried:?}"
            );
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            assert_eq!(left_over(&cache), 0);
        }
    }
}
