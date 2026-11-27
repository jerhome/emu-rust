//! Headless driver of the library screen, for the integration tests of `crates/emu-ui/tests/`: those that need
//! the process-wide catalog to hold a real custom system live in files of their own (design 8.0 §4). It is not part
//! of the program.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Context, Event, Key, Modifiers, RawInput};
use emu_assets::cores::CoreId;

use super::custom_launch::{CustomDialog, Pick};
use super::custom_systems::{Confirm, ConfirmPick};
use super::{Dialog, LaunchRequest, LibraryEvent, LibraryUi};
use crate::GameContent;
use crate::config::{Config, DataDirs};
use crate::dry_run::DryRunCommand;

/// The open dialog of the screen, as far as the tests tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    None,
    /// « Contenu non vérifié ».
    Accept,
    /// « Système non testé »; `can_try`: the game has a dry run (a single plain file).
    Untested {
        can_try: bool,
    },
    /// The dry run failed.
    Failed,
    /// Deletion of a custom system.
    Delete,
    /// Deletion of a system with an official id: its games are forgotten first.
    DeleteUncertain,
    /// Games are still under `roms/<id>/`: the deletion is refused.
    Blocked,
    Forget,
    /// A system was added.
    Added,
    Other,
}

/// The button pressed: the first, the second one (« Ça marche chez moi »), or « Annuler ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Primary,
    Secondary,
    Cancel,
}

/// A launch the screen let through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    pub title: String,
    pub core: String,
    pub warnings: Vec<String>,
}

/// One game to launch the way the grid does, through the gate.
pub struct GameToLaunch<'a> {
    /// Library key of the system (`SystemInfo::core_key`).
    pub system_key: &'a str,
    pub title: &'a str,
    pub core: &'a str,
    pub core_path: &'a Path,
    pub file: &'a Path,
}

pub struct LibraryHarness {
    ctx: Context,
    ui: LibraryUi,
}

impl LibraryHarness {
    /// The library on `dirs` (the data folders exist). No scan at start, no cover download.
    pub fn open(dirs: DataDirs, mut config: Config) -> Self {
        config.library.scan_on_start = false;
        config.covers.download = false;
        let ctx = Context::default();
        crate::ui::apply_style(&ctx);
        Self {
            ctx,
            ui: LibraryUi::open(dirs, config, false),
        }
    }

    pub fn config(&self) -> &Config {
        &self.ui.config
    }

    /// The child of the dry runs: tests give a shell script instead of this executable.
    pub fn set_dry_run_command(&mut self, command: DryRunCommand) {
        self.ui.custom.command = Some(command);
    }

    /// One frame; the launches the screen let through.
    pub fn frame(&mut self) -> Vec<Launched> {
        self.frame_with(Vec::new())
    }

    fn frame_with(&mut self, events: Vec<Event>) -> Vec<Launched> {
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
        let mut run = self.ctx.run_ui(raw, |root| out = ui.show(root));
        run.textures_delta.clear();
        out.into_iter()
            .filter_map(|event| match event {
                LibraryEvent::Launch(request) => Some(Launched {
                    title: request.title.clone(),
                    core: request.core.to_string(),
                    warnings: request.warnings.clone(),
                }),
                LibraryEvent::Quit => None,
            })
            .collect()
    }

    /// Presses and releases a key.
    pub fn key(&mut self, key: Key) -> Vec<Launched> {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let mut out = self.frame_with(vec![event(true)]);
        out.extend(self.frame_with(vec![event(false)]));
        out
    }

    /// Launches a game through the gate and runs one frame.
    pub fn launch(&mut self, game: &GameToLaunch<'_>) -> Vec<Launched> {
        let Some(core) = CoreId::new(game.core) else {
            return Vec::new();
        };
        self.ui
            .events
            .push(LibraryEvent::Launch(Box::new(LaunchRequest {
                game: None,
                title: game.title.to_owned(),
                core,
                core_path: game.core_path.to_owned(),
                content: GameContent::Files(vec![game.file.to_owned()]),
                computer: false,
                warnings: Vec::new(),
                system: Some(game.system_key.to_owned()),
                region: None,
                spec: None,
            })));
        self.frame()
    }

    pub fn dialog(&self) -> DialogKind {
        match &self.ui.dialog {
            None => DialogKind::None,
            Some(Dialog::Custom(CustomDialog::Accept { .. })) => DialogKind::Accept,
            Some(Dialog::Custom(CustomDialog::Untested { tried, .. })) => DialogKind::Untested {
                can_try: tried.is_ok(),
            },
            Some(Dialog::Custom(CustomDialog::Failed { .. })) => DialogKind::Failed,
            Some(Dialog::CustomSystems(confirm)) => match confirm {
                Confirm::Delete { .. } => DialogKind::Delete,
                Confirm::DeleteUncertain { .. } => DialogKind::DeleteUncertain,
                Confirm::Blocked { .. } => DialogKind::Blocked,
                Confirm::Forget { .. } => DialogKind::Forget,
                Confirm::Added { .. } => DialogKind::Added,
            },
            Some(_) => DialogKind::Other,
        }
    }

    /// Answers the open dialog of a custom launch, then runs one frame.
    pub fn answer(&mut self, answer: Answer) -> Vec<Launched> {
        let pick = match answer {
            Answer::Primary => Pick::Primary,
            Answer::Secondary => Pick::Secondary,
            Answer::Cancel => Pick::Cancel,
        };
        self.ui.resolve_custom(pick, &self.ctx);
        self.frame()
    }

    /// Frames until the dry run is over (its launches are returned); panics after `timeout`.
    pub fn wait_try(&mut self, timeout: Duration) -> Vec<Launched> {
        let deadline = Instant::now() + timeout;
        let mut out = self.frame();
        while self.ui.custom_trying() {
            assert!(Instant::now() < deadline, "the dry run never ended");
            std::thread::sleep(Duration::from_millis(10));
            out.extend(self.frame());
        }
        out.extend(self.frame());
        out
    }

    /// Titles of the games the grid holds.
    pub fn games(&self) -> Vec<String> {
        self.ui.games.iter().map(|g| g.title.clone()).collect()
    }

    // ---- the settings --------------------------------------------------------------------------------------

    pub fn refresh_settings(&mut self) {
        self.ui.refresh_settings_cache();
    }

    /// « Supprimer » of a custom system: the dialog it opens.
    pub fn ask_delete(&mut self, id: &str) -> DialogKind {
        self.ui.refresh_custom_systems();
        self.ui.ask_delete(id);
        self.dialog()
    }

    /// The button of the open confirmation dialog (`Folder` is « Ouvrir le dossier » and opens nothing here).
    pub fn confirm(&mut self, yes: bool) {
        let pick = if yes {
            ConfirmPick::Yes
        } else {
            ConfirmPick::Cancel
        };
        self.ui.resolve_confirm(pick);
    }

    /// « Restaurer » of a retired system.
    pub fn restore(&mut self, id: &str) {
        self.ui.refresh_custom_systems();
        self.ui.restore_custom_system(id);
    }

    /// « Oublier ses jeux » of a retired system: the dialog it opens (`None` when the button is not offered).
    pub fn ask_forget(&mut self, id: &str) -> DialogKind {
        self.ui.refresh_custom_systems();
        if self.ui.can_forget_retired(id) {
            self.ui
                .open_dialog(Dialog::CustomSystems(Confirm::Forget { id: id.to_owned() }));
        }
        self.dialog()
    }

    /// Retired systems: `(id, « Restaurer » offered, « Oublier ses jeux » offered)`.
    pub fn retired(&mut self) -> Vec<(String, bool, bool)> {
        self.ui.refresh_custom_systems();
        self.ui.retired_buttons()
    }

    /// Lines of the problems the page shows, and the files grouped under « N définitions ignorées ».
    pub fn problems(&mut self) -> (Vec<String>, Vec<String>) {
        self.ui.refresh_custom_systems();
        self.ui.problem_lines()
    }

    /// A publication of the catalog waits for the end of a scan or a tidying.
    pub fn publication_waits(&self) -> bool {
        self.ui.publication_waits()
    }

    pub fn status(&self) -> Option<String> {
        self.ui.status.as_ref().map(|(m, _)| m.clone())
    }

    /// Frames until the due publication went through (panics after `timeout`).
    pub fn wait_publication(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while self.ui.publication_waits() {
            assert!(Instant::now() < deadline, "the catalog was never published");
            self.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
        self.frame();
    }

    /// Writes the definitions as « Ajouter » of the cores screen does.
    pub fn add_systems(&mut self, drafts: Vec<emu_library::CustomSystemDraft>) {
        self.ui.add_custom_systems(drafts);
    }

    /// The DAT server (a local one in the tests, plain `http://127.0.0.1` allowed).
    pub fn set_dat_source(&mut self, base_url: &str) {
        base_url.clone_into(&mut self.ui.dat_base_url);
        self.ui.dat_http.allow_loopback_http = true;
    }

    /// « Modifier » then « Chercher la base d'identification » of a custom system.
    pub fn search_database(&mut self, id: &str) {
        self.ui.refresh_custom_systems();
        self.ui.open_edit(id);
        self.ui.start_search(&self.ctx);
    }

    /// Frames until the search, the publication and the import that follow are over (panics after `timeout`).
    pub fn wait_dat_work(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            self.frame();
            if !self.ui.dat_work_pending() {
                break;
            }
            assert!(Instant::now() < deadline, "the DAT work never ended");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn root(&self) -> PathBuf {
        self.ui.dirs.root.clone()
    }
}

/// A scan or a tidying holding the catalog, as long as it lives: publications wait.
pub struct CatalogHold(#[allow(dead_code)] crate::CatalogPin);

pub fn hold_catalog() -> CatalogHold {
    CatalogHold(crate::CatalogPin::new())
}
