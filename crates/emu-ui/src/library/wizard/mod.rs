//! First-run wizard (design v7.3 §4): ROM folder and first scan, cores of the systems found, keyboard and pad.
//! Drawn in place of the home grid while active; the manual, Réglages (pad learning), the cores page and the
//! library dialogs keep working over or instead of it, and the wizard resumes at the same step when the user
//! comes back home. Nothing is downloaded without a click; never a game nor a BIOS.

#[cfg(test)]
mod tests;
mod text;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Button, Key, RichText, Ui};
use emu_assets::cores::{CoreId, CorePreferences};

use super::pad_settings::LearnPad;
use super::{LibraryUi, Page};
use crate::config::Config;
use crate::cores_ui::{self, InstallState};
use crate::file_picker::{self, BrowserOutcome, FileBrowser, PickerBackend};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Welcome,
    Roms,
    Cores,
    Controls,
}

impl Step {
    pub(crate) const ALL: [Step; 4] = [Step::Welcome, Step::Roms, Step::Cores, Step::Controls];

    /// 1-based, for « Étape n sur 4 ».
    pub(crate) fn number(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1
    }

    fn title(self) -> &'static str {
        match self {
            Step::Welcome => text::STEP_WELCOME,
            Step::Roms => text::STEP_ROMS,
            Step::Cores => text::STEP_CORES,
            Step::Controls => text::STEP_CONTROLS,
        }
    }

    /// Manual section opened by « Aide » (anchors of `docs/manuel.md`, checked by a test).
    pub(crate) fn help_anchor(self) -> &'static str {
        match self {
            Step::Welcome => "premiers-pas",
            Step::Roms => "le-dossier-des-roms",
            Step::Cores => "les-cores",
            Step::Controls => "clavier",
        }
    }

    fn next(self) -> Option<Step> {
        Self::ALL.get(self.number()).copied()
    }

    fn prev(self) -> Option<Step> {
        self.number().checked_sub(2).map(|i| Self::ALL[i])
    }
}

/// Button of Réglages > Application.
pub(crate) const RELAUNCH: &str = text::RELAUNCH;

/// Starts by itself only on a fresh install: no ROM folder configured and the wizard never finished or left.
pub(crate) fn should_start(config: &Config) -> bool {
    config.library.roots.is_empty() && !config.ui.wizard_done
}

/// One system found by the scan and what the wizard offers for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoreProposal {
    pub system_id: String,
    pub system_name: String,
    /// An installed core of this system (preferred order), if any: nothing to install.
    pub installed: Option<CoreId>,
    /// The core to install (first preferred); `None` when one is installed or none is known.
    pub core: Option<CoreId>,
    /// Display name of `installed` or `core` (`.info`), the id otherwise.
    pub label: String,
    /// « Installer » ticked: every missing core, by default.
    pub selected: bool,
}

/// Systems found (library ids, repeats allowed) → one proposal per known system, sorted by name.
pub(crate) fn propose_cores<'a>(
    systems: impl IntoIterator<Item = &'a str>,
    prefs: &CorePreferences,
    installed: &BTreeSet<CoreId>,
) -> Vec<CoreProposal> {
    let ids: BTreeSet<&str> = systems.into_iter().collect();
    let mut out: Vec<CoreProposal> = ids
        .into_iter()
        .filter_map(emu_library::system)
        .map(|info| {
            let cores = prefs.for_system(&info.database);
            let have = cores.iter().find(|c| installed.contains(*c)).cloned();
            let core = if have.is_some() {
                None
            } else {
                cores.first().cloned()
            };
            let label = have
                .as_ref()
                .or(core.as_ref())
                .map_or_else(String::new, ToString::to_string);
            CoreProposal {
                system_id: info.id.clone(),
                system_name: info.name.clone(),
                selected: core.is_some(),
                installed: have,
                core,
                label,
            }
        })
        .collect();
    out.sort_by(|a, b| a.system_name.cmp(&b.system_name));
    out
}

/// Ticked cores, once each (Game Boy and Game Boy Color share one).
pub(crate) fn selected_cores(proposals: &[CoreProposal]) -> BTreeSet<CoreId> {
    proposals
        .iter()
        .filter(|p| p.selected)
        .filter_map(|p| p.core.clone())
        .collect()
}

/// State of a wizard session; the drawing lives in `impl LibraryUi` below.
pub(crate) struct Wizard {
    step: Step,
    /// Move the keyboard focus to the step's main button on the next frame.
    focus: bool,
    typed: String,
    browser: Option<FileBrowser>,
    native: Option<Receiver<Option<PathBuf>>>,
    /// Folder added by this session.
    folder: Option<PathBuf>,
    folder_error: Option<String>,
    /// The folder was added while another scan ran: scan again once it ends.
    rescan: bool,
    scan_started: bool,
    proposals: Vec<CoreProposal>,
    /// Recompute `proposals` (entering the step, an install finished).
    proposals_stale: bool,
    requested: BTreeSet<CoreId>,
    /// A requested install was still running on the previous frame.
    installing: bool,
    pads: Vec<LearnPad>,
}

impl Wizard {
    pub(crate) fn new() -> Self {
        Self {
            step: Step::Welcome,
            focus: true,
            typed: String::new(),
            browser: None,
            native: None,
            folder: None,
            folder_error: None,
            rescan: false,
            scan_started: false,
            proposals: Vec::new(),
            proposals_stale: true,
            requested: BTreeSet::new(),
            installing: false,
            pads: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn step(&self) -> Step {
        self.step
    }

    fn go(&mut self, step: Step) {
        self.step = step;
        self.focus = true;
        self.browser = None;
        if step == Step::Cores {
            self.proposals_stale = true;
        }
    }

    /// Next step (« Suivant », « Passer »); `true` when the last one is passed: the wizard is over.
    pub(crate) fn forward(&mut self) -> bool {
        match self.step.next() {
            Some(s) => {
                self.go(s);
                false
            }
            None => true,
        }
    }

    /// Previous step (« Retour », Échap); `false` on the first one.
    pub(crate) fn back(&mut self) -> bool {
        match self.step.prev() {
            Some(s) => {
                self.go(s);
                true
            }
            None => false,
        }
    }

    /// The step's own action was done, so moving on is « Suivant » rather than « Passer ».
    fn step_done(&self) -> bool {
        match self.step {
            Step::Welcome | Step::Controls => true,
            Step::Roms => self.folder.is_some(),
            Step::Cores => !self.requested.is_empty(),
        }
    }

    pub(crate) fn forward_label(&self) -> &'static str {
        match self.step {
            Step::Welcome => text::START,
            Step::Controls => text::FINISH,
            _ if self.step_done() => text::NEXT,
            _ => text::SKIP,
        }
    }
}

/// What the navigation row asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WizardNav {
    Forward,
    Back,
    Quit,
}

impl LibraryUi {
    /// Opens the wizard at its first step (automatic start, Réglages, `--wizard`, `--screen wizard`).
    pub(crate) fn open_wizard(&mut self) {
        if self.page == Page::Settings {
            self.leave_settings();
        }
        self.wizard = Some(Wizard::new());
        self.set_page(Page::Home);
    }

    #[cfg(test)]
    pub(crate) fn wizard_step(&self) -> Option<Step> {
        self.wizard.as_ref().map(Wizard::step)
    }

    /// Host → wizard: the pads that can be configured (same list as the learning screen).
    pub(crate) fn set_wizard_pads(&mut self, pads: &[LearnPad]) {
        if let Some(w) = &mut self.wizard
            && w.pads != pads
        {
            w.pads = pads.to_vec();
        }
    }

    /// Applies one navigation; finishing (or leaving) writes `[ui] wizard_done = true` and shows the library.
    pub(crate) fn wizard_nav(&mut self, nav: WizardNav) {
        let Some(w) = &mut self.wizard else {
            return;
        };
        let done = match nav {
            WizardNav::Forward => w.forward(),
            WizardNav::Back => {
                w.back();
                false
            }
            WizardNav::Quit => true,
        };
        if done {
            self.finish_wizard();
        }
    }

    fn finish_wizard(&mut self) {
        self.wizard = None;
        if !self.config.ui.wizard_done {
            self.config.ui.wizard_done = true;
            self.save_config();
        }
        self.set_page(Page::Home);
    }

    /// Adds `path` to the ROM folders (saved at once) and scans. `false`: not a folder (message shown).
    pub(crate) fn wizard_add_folder(&mut self, path: PathBuf, ctx: &egui::Context) -> bool {
        let path = std::path::absolute(&path).unwrap_or(path);
        let Some(w) = &mut self.wizard else {
            return false;
        };
        if !path.is_dir() {
            w.folder_error = Some(text::roms_not_a_folder(&path.display().to_string()));
            return false;
        }
        w.folder_error = None;
        w.browser = None;
        w.folder = Some(path.clone());
        w.scan_started = true;
        w.rescan = self.scan.is_some();
        let fixed = self.dirs.root.join("roms");
        if path != fixed && !self.config.library.roots.contains(&path) {
            self.config.library.roots.push(path);
            self.save_config();
        }
        self.start_scan(ctx);
        true
    }

    /// Recomputes the proposals from the games of the library and the installed cores; ticks are kept.
    fn refresh_proposals(&mut self) {
        let installed: BTreeSet<CoreId> = self
            .cores
            .manager()
            .map(|m| m.installed().into_iter().map(|c| c.id).collect())
            .unwrap_or_default();
        let prefs = cores_ui::preferences(&self.config.cores);
        let mut fresh = propose_cores(
            self.games.iter().map(|g| g.system.as_str()),
            &prefs,
            &installed,
        );
        if let Some(m) = self.cores.manager() {
            for p in &mut fresh {
                if let Some(info) = p
                    .installed
                    .as_ref()
                    .or(p.core.as_ref())
                    .and_then(|c| m.info(c))
                {
                    p.label = crate::ui::text::sanitize_core_text(&info.display_name);
                }
            }
        }
        let Some(w) = &mut self.wizard else {
            return;
        };
        for p in &mut fresh {
            if let Some(old) = w.proposals.iter().find(|o| o.system_id == p.system_id)
                && p.core.is_some()
                && old.core == p.core
            {
                p.selected = old.selected;
            }
        }
        w.proposals = fresh;
        w.proposals_stale = false;
    }

    fn wizard_install(&mut self, ctx: &egui::Context) {
        if self.capture {
            return;
        }
        let Some(w) = &mut self.wizard else {
            return;
        };
        let wanted = selected_cores(&w.proposals);
        w.requested.extend(wanted.iter().cloned());
        w.installing = true;
        for id in wanted {
            self.cores.request_install(id, ctx);
        }
    }

    /// One frame of the wizard (page Home while it is active).
    pub(super) fn wizard_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.wizard_background(&ctx);
        let Some(mut w) = self.wizard.take() else {
            return;
        };
        let modal = self.dialog.is_some()
            || self.cores.import.is_active()
            || self.snapshot_busy()
            || self.programs_open();
        let escape = !modal
            && w.browser.is_none()
            && !ctx.text_edit_focused()
            && ui.input(|i| i.key_pressed(Key::Escape));
        let mut nav = escape.then_some(WizardNav::Back);
        let mut action = Action::None;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(theme::SP_S);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(text::TITLE)
                        .size(theme::FONT_TITLE)
                        .color(theme::TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(text::QUIT).clicked() {
                        nav = Some(WizardNav::Quit);
                    }
                });
            });
            step_strip(ui, w.step);
            ui.separator();
            ui.add_space(theme::SP_S);
            let mut primary = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height((ui.available_height() - 96.0).max(120.0))
                .show(ui, |ui| {
                    ui.set_max_width(760.0);
                    match w.step {
                        Step::Welcome => welcome(ui),
                        Step::Roms => self.roms_step(ui, &mut w, &mut action, &mut primary),
                        Step::Cores => self.cores_step(ui, &mut w, &mut action, &mut primary),
                        Step::Controls => self.controls_step(ui, &w, &mut action, &mut primary),
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(w.step != Step::Welcome, Button::new(text::BACK))
                    .clicked()
                {
                    nav = Some(WizardNav::Back);
                }
                let forward = ui.add(
                    Button::new(RichText::new(w.forward_label()).strong()).fill(theme::ACCENT_DIM),
                );
                if forward.clicked() {
                    nav = Some(WizardNav::Forward);
                }
                if ui.button(text::HELP).clicked() {
                    action = Action::Help(w.step.help_anchor());
                }
                if std::mem::take(&mut w.focus) {
                    primary.unwrap_or(forward).request_focus();
                }
            });
            ui.label(
                RichText::new(text::KEYS_HINT)
                    .size(theme::FONT_SMALL)
                    .color(theme::TEXT_WEAK),
            );
            self.status_line(ui);
        });
        self.wizard = Some(w);
        match action {
            Action::None => {}
            Action::Choose => self.wizard_choose(),
            Action::Add(path) => {
                self.wizard_add_folder(path, &ctx);
            }
            Action::Install => self.wizard_install(&ctx),
            Action::Help(anchor) => self.open_help(Some(anchor)),
            Action::CoresPage => self.set_page(Page::Cores),
            Action::Pad(id) => {
                self.open_pad_learning(id);
            }
        }
        if let Some(nav) = nav {
            self.wizard_nav(nav);
        }
    }

    /// Native dialog, folder scans and installs that move on by themselves.
    fn wizard_background(&mut self, ctx: &egui::Context) {
        let scan_idle = self.scan.is_none();
        let Some(w) = &mut self.wizard else {
            return;
        };
        let mut picked = None;
        if let Some(rx) = &w.native {
            match rx.try_recv() {
                Ok(p) => {
                    w.native = None;
                    picked = p;
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(TryRecvError::Disconnected) => w.native = None,
            }
        }
        let rescan = w.rescan && scan_idle;
        if rescan {
            w.rescan = false;
        }
        let running = w.requested.iter().any(|id| {
            matches!(
                self.cores.install_state(id),
                InstallState::Running(_) | InstallState::Queued
            )
        });
        if w.installing && !running {
            w.proposals_stale = true;
        }
        w.installing = running;
        let refresh = w.step == Step::Cores && w.proposals_stale && scan_idle && !rescan;
        if let Some(path) = picked {
            self.wizard_add_folder(path, ctx);
        }
        if rescan {
            self.start_scan(ctx);
        }
        if refresh {
            self.refresh_proposals();
        }
    }

    /// « Choisir le dossier… »: the system dialog, or the built-in browser where there is none.
    fn wizard_choose(&mut self) {
        let start = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.dirs.root.clone());
        let Some(w) = &mut self.wizard else {
            return;
        };
        if w.native.is_some() {
            return;
        }
        match file_picker::picker_backend() {
            _ if self.capture => w.browser = Some(FileBrowser::new(start, Vec::new())),
            PickerBackend::Native => {
                w.native = Some(file_picker::pick_folder_in_background(
                    text::ROMS_DIALOG_TITLE,
                ));
            }
            PickerBackend::Builtin => w.browser = Some(FileBrowser::new(start, Vec::new())),
        }
    }

    fn roms_step(
        &self,
        ui: &mut Ui,
        w: &mut Wizard,
        action: &mut Action,
        primary: &mut Option<egui::Response>,
    ) {
        section(ui, text::STEP_ROMS);
        ui.label(text::ROMS_INTRO);
        ui.add_space(theme::SP_S);
        ui.horizontal_wrapped(|ui| {
            weak(ui, text::ROMS_FIXED);
            ui.label(RichText::new(self.dirs.root.join("roms").display().to_string()).monospace());
        });
        ui.add_space(theme::SP_M);
        if let Some(browser) = &mut w.browser {
            let mut close = false;
            egui::Frame::group(ui.style())
                .fill(theme::PANEL)
                .show(ui, |ui| {
                    let outcome = browser.show(ui);
                    let use_this = ui.button(text::ROMS_USE_THIS).clicked();
                    match outcome {
                        // A file picked: its folder is meant.
                        Some(BrowserOutcome::Picked(file)) => {
                            let dir = file
                                .parent()
                                .map_or_else(|| file.clone(), std::path::Path::to_path_buf);
                            *action = Action::Add(dir);
                        }
                        Some(BrowserOutcome::Cancelled) => close = true,
                        None if use_this => *action = Action::Add(browser.dir().to_owned()),
                        None => {}
                    }
                });
            if close {
                w.browser = None;
            }
        } else if w.native.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text::ROMS_DIALOG_OPEN);
            });
        } else {
            let choose = ui.button(RichText::new(text::ROMS_CHOOSE).strong());
            if choose.clicked() {
                *action = Action::Choose;
            }
            *primary = Some(choose);
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut w.typed)
                        .hint_text(text::ROMS_TYPED_HINT)
                        .desired_width(420.0),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let typed = w.typed.trim().to_owned();
                let add = ui.add_enabled(!typed.is_empty(), Button::new(text::ROMS_ADD));
                if (add.clicked() || enter) && !typed.is_empty() {
                    *action = Action::Add(PathBuf::from(typed));
                    w.typed.clear();
                }
            });
        }
        if let Some(e) = &w.folder_error {
            ui.colored_label(theme::ERROR, e);
        }
        if let Some(folder) = &w.folder {
            ui.add_space(theme::SP_S);
            ui.label(text::roms_folder(&folder.display().to_string()));
            if self.scan.is_some() || w.rescan {
                ui.horizontal(|ui| {
                    ui.spinner();
                    match &self.scan_progress {
                        Some(p) => ui.label(super::text::scan_running(p.files_seen, p.hashed)),
                        None => ui.label(text::ROMS_SCAN_WAIT),
                    };
                });
            } else if w.scan_started {
                let systems: BTreeSet<&str> =
                    self.games.iter().map(|g| g.system.as_str()).collect();
                ui.colored_label(
                    if self.games.is_empty() {
                        theme::WARN
                    } else {
                        theme::OK
                    },
                    text::roms_found(self.games.len(), systems.len()),
                );
            }
        }
    }

    fn cores_step(
        &self,
        ui: &mut Ui,
        w: &mut Wizard,
        action: &mut Action,
        primary: &mut Option<egui::Response>,
    ) {
        section(ui, text::STEP_CORES);
        ui.label(text::CORES_INTRO);
        ui.add_space(theme::SP_S);
        if self.cores.manager().is_none() {
            ui.colored_label(theme::WARN, text::CORES_NO_MANAGER);
            return;
        }
        if self.scan.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text::ROMS_SCAN_WAIT);
            });
            return;
        }
        if w.proposals.is_empty() {
            weak(ui, text::CORES_NO_GAMES);
        }
        let mut offline = false;
        egui::Grid::new("wizard-cores")
            .num_columns(2)
            .spacing([theme::SP_L, theme::SP_S])
            .show(ui, |ui| {
                for p in &mut w.proposals {
                    let state = p.core.as_ref().map(|c| self.cores.install_state(c));
                    let busy =
                        matches!(state, Some(InstallState::Running(_) | InstallState::Queued));
                    let row = text::cores_row(&p.system_name, &p.label);
                    let r = if p.core.is_some() {
                        ui.add_enabled(!busy, egui::Checkbox::new(&mut p.selected, row))
                    } else {
                        ui.label(row)
                    };
                    if primary.is_none() && p.core.is_some() {
                        *primary = Some(r);
                    }
                    match (&p.installed, state) {
                        (Some(_), _) => ui.colored_label(theme::OK, text::CORES_INSTALLED),
                        (None, None) => ui.colored_label(theme::TEXT_WEAK, text::CORES_UNKNOWN),
                        (None, Some(InstallState::Queued)) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(text::CORES_QUEUED);
                            })
                            .response
                        }
                        (None, Some(InstallState::Running(progress))) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(text::CORES_DOWNLOADING);
                                if let Some((done, total)) = progress {
                                    ui.label(cores_ui::text::progress(done, total));
                                }
                            })
                            .response
                        }
                        (None, Some(InstallState::Failed(e))) => {
                            ui.colored_label(theme::ERROR, text::cores_failed(&e))
                        }
                        (None, Some(InstallState::Idle)) => {
                            offline |= p.core.as_ref().is_some_and(|c| w.requested.contains(c));
                            ui.label("")
                        }
                    };
                    ui.end_row();
                }
            });
        ui.add_space(theme::SP_M);
        let n = selected_cores(&w.proposals)
            .iter()
            .filter(|c| {
                !matches!(
                    self.cores.install_state(c),
                    InstallState::Running(_) | InstallState::Queued
                )
            })
            .count();
        ui.horizontal(|ui| {
            let install = ui.add_enabled(
                n > 0 && !self.capture,
                Button::new(RichText::new(text::cores_install(n)).strong()),
            );
            if install.clicked() {
                *action = Action::Install;
            }
            if primary.is_none() {
                *primary = Some(install);
            }
            if ui.button(text::CORES_OPEN_PAGE).clicked() {
                *action = Action::CoresPage;
            }
        });
        if self.capture {
            weak(ui, text::CORES_CAPTURE);
        }
        if let Some(e) = self.cores.refresh_error()
            && offline
        {
            ui.colored_label(theme::ERROR, text::cores_offline(e));
            if ui.button(text::CORES_RETRY).clicked() {
                *action = Action::Install;
            }
        }
        ui.add_space(theme::SP_S);
        weak(ui, text::CORES_NOTE);
    }

    fn controls_step(
        &self,
        ui: &mut Ui,
        w: &Wizard,
        action: &mut Action,
        primary: &mut Option<egui::Response>,
    ) {
        section(ui, text::KEYBOARD_TITLE);
        egui::Grid::new("wizard-keys")
            .num_columns(2)
            .spacing([theme::SP_L, theme::SP_XS])
            .show(ui, |ui| {
                for (key, what) in text::KEYBOARD_ROWS {
                    ui.label(RichText::new(key).strong());
                    ui.label(what);
                    ui.end_row();
                }
            });
        ui.add_space(theme::SP_XS);
        let host = crate::ui::text::key_label(self.config.hotkeys.host_key);
        ui.label(text::host_key(&host));
        weak(ui, text::KEYBOARD_CHANGE);
        ui.add_space(theme::SP_M);
        section(ui, text::PAD_TITLE);
        if !self.config.input.gamepad.enabled {
            weak(ui, text::PAD_DISABLED);
        } else if w.pads.is_empty() {
            weak(ui, text::PAD_NONE);
        } else {
            for pad in &w.pads {
                ui.horizontal(|ui| {
                    ui.label(crate::ui::text::sanitize_core_text(&pad.name));
                    let r = ui.button(text::PAD_CONFIGURE);
                    if r.clicked() {
                        *action = Action::Pad(pad.id);
                    }
                    if primary.is_none() {
                        *primary = Some(r);
                    }
                });
            }
            weak(ui, text::PAD_NOTE);
        }
        ui.add_space(theme::SP_M);
        weak(ui, text::BIOS_REMINDER);
    }
}

/// What a click inside the step asked for, applied once drawing is over.
enum Action {
    None,
    Choose,
    Add(PathBuf),
    Install,
    Help(&'static str),
    CoresPage,
    Pad(crate::PadId),
}

fn step_strip(ui: &mut Ui, current: Step) {
    ui.horizontal_wrapped(|ui| {
        for step in Step::ALL {
            let label = format!("{}. {}", step.number(), step.title());
            let rich = if step == current {
                RichText::new(label).strong().color(theme::ACCENT)
            } else {
                RichText::new(label).color(theme::TEXT_WEAK)
            };
            ui.label(rich);
            if step != Step::Controls {
                ui.label(RichText::new("›").color(theme::TEXT_WEAK));
            }
        }
        ui.add_space(theme::SP_L);
        weak(ui, &text::step_counter(current.number(), Step::ALL.len()));
    });
}

fn welcome(ui: &mut Ui) {
    section(ui, text::STEP_WELCOME);
    ui.label(text::WELCOME);
    ui.add_space(theme::SP_S);
    weak(ui, text::WELCOME_LEGAL);
}

fn section(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .size(theme::FONT_SECTION)
            .color(theme::TEXT),
    );
    ui.add_space(theme::SP_XS);
}

fn weak(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).color(theme::TEXT_WEAK));
}
