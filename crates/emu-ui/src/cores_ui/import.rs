//! « Importer un BIOS… » dialog (design v3.0 §2.2): file picker (native `rfd` dialog on a worker thread, or the
//! built-in egui browser under Docker / without a portal), identification and import on worker threads, then a
//! summary. Every step is a modal usable with the keyboard alone; the UI thread never hashes nor copies.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use egui::{Color32, Key, RichText};
use emu_assets::bios::{ExpectedFirmware, TargetState};
use emu_assets::cores::CoreManager;

use super::text;
use crate::bios_import::{
    BiosIdentified, BiosImportItem, BiosImportReport, firmware_catalogue, identify_bios,
    import_bios, known_import_items, unverified_item,
};
use crate::file_picker::{
    BrowserOutcome, FileBrowser, PickerBackend, pick_file_in_background, picker_backend,
};

const WARNING: Color32 = crate::theme::WARN;
const ERROR: Color32 = crate::theme::ERROR;
/// Names listed at most in the « non vérifié » choice (the filter field narrows the list).
const MAX_LISTED_NAMES: usize = 60;

enum Work {
    Identified(PathBuf, Result<BiosIdentified, String>),
    Imported(BiosImportReport),
}

enum State {
    Idle,
    Native(Receiver<Option<PathBuf>>),
    Browser(FileBrowser),
    Working(Receiver<Work>),
    Review {
        src: PathBuf,
        identified: BiosIdentified,
        /// « Importer quand même (non vérifié) » was chosen (unknown file): the name list is shown.
        forcing: bool,
        filter: String,
        choice: Option<String>,
    },
    Failed(String),
    Done(BiosImportReport),
}

/// Outcome of a closed dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImportClosed {
    /// At least one file was written to the system folder.
    pub installed: bool,
}

pub(crate) struct ImportUi {
    system_dir: PathBuf,
    /// Folder the built-in browser opens on.
    start_dir: PathBuf,
    manager: Option<Arc<CoreManager>>,
    state: State,
    /// File the launch was missing (`kick34005.A500`), shown in the title.
    wanted: Option<String>,
    focus: bool,
    installed: bool,
    /// Tests: forces the built-in browser.
    backend: Option<PickerBackend>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl ImportUi {
    pub(crate) fn new(system_dir: PathBuf, manager: Option<Arc<CoreManager>>) -> Self {
        let start_dir = home_dir().unwrap_or_else(|| system_dir.clone());
        Self {
            system_dir,
            start_dir,
            manager,
            state: State::Idle,
            wanted: None,
            focus: false,
            installed: false,
            backend: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_builtin_browser(mut self, start: PathBuf) -> Self {
        self.backend = Some(PickerBackend::Builtin);
        self.start_dir = start;
        self
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// Opens the picker. `wanted`: name of the missing file (dialog of a launch), shown as a hint.
    pub(crate) fn start(&mut self, wanted: Option<String>) {
        if self.is_active() {
            return;
        }
        self.wanted = wanted;
        self.installed = false;
        self.focus = true;
        self.state = match self.backend.unwrap_or_else(picker_backend) {
            PickerBackend::Native => {
                State::Native(pick_file_in_background(text::IMPORT_TITLE, &[]))
            }
            PickerBackend::Builtin => {
                State::Browser(FileBrowser::new(self.start_dir.clone(), Vec::new()))
            }
        };
    }

    fn identify(&mut self, src: PathBuf, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let system_dir = self.system_dir.clone();
        let manager = self.manager.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("bios-identify".into())
            .spawn(move || {
                let catalogue = firmware_catalogue(manager.as_deref());
                let result = identify_bios(&system_dir, &src, &catalogue);
                let _ = tx.send(Work::Identified(src, result));
                ctx.request_repaint();
            });
        self.state = match spawned {
            Ok(_) => State::Working(rx),
            Err(e) => State::Failed(e.to_string()),
        };
    }

    fn import(
        &mut self,
        src: PathBuf,
        items: Vec<BiosImportItem>,
        md5: Option<[u8; 16]>,
        ctx: &egui::Context,
    ) {
        let (tx, rx) = mpsc::channel();
        let system_dir = self.system_dir.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("bios-import".into())
            .spawn(move || {
                let report = import_bios(&system_dir, &src, &items, md5);
                let _ = tx.send(Work::Imported(report));
                ctx.request_repaint();
            });
        self.state = match spawned {
            Ok(_) => State::Working(rx),
            Err(e) => State::Failed(e.to_string()),
        };
        self.focus = true;
    }

    /// Advances the background steps (picker answer, identification, import).
    fn poll(&mut self, ctx: &egui::Context) {
        if let State::Native(rx) = &mut self.state {
            match rx.try_recv() {
                Ok(Some(path)) => self.identify(path, ctx),
                Ok(None) | Err(TryRecvError::Disconnected) => self.state = State::Idle,
                Err(TryRecvError::Empty) => {}
            }
        }
        let work = match &mut self.state {
            State::Working(rx) => match rx.try_recv() {
                Ok(w) => Some(Some(w)),
                Err(TryRecvError::Disconnected) => Some(None),
                Err(TryRecvError::Empty) => None,
            },
            _ => None,
        };
        match work {
            Some(Some(Work::Identified(src, Ok(identified)))) => {
                self.focus = true;
                let choice = identified.unverified_names().first().cloned();
                self.state = State::Review {
                    src,
                    identified,
                    forcing: false,
                    filter: String::new(),
                    choice,
                };
            }
            Some(Some(Work::Identified(_, Err(e)))) => {
                self.focus = true;
                self.state = State::Failed(e);
            }
            Some(Some(Work::Imported(report))) => {
                self.focus = true;
                self.installed |= report.installed > 0;
                self.state = State::Done(report);
            }
            Some(None) => self.state = State::Failed(text::IMPORT_INTERRUPTED.to_owned()),
            None => {}
        }
    }

    /// Draws the dialog when active. `Some` once it is closed.
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<ImportClosed> {
        self.poll(ctx);
        if !self.is_active() {
            return None;
        }
        let focus = std::mem::take(&mut self.focus);
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut close = false;
        let mut action = None;
        let wanted = self.wanted.clone();
        egui::Modal::new(egui::Id::unique("bios-import-dialog")).show(ctx, |ui| {
            ui.set_max_width(640.0);
            ui.heading(text::IMPORT_TITLE);
            if let Some(w) = &wanted {
                ui.label(text::import_wanted(w));
            }
            match &mut self.state {
                State::Idle => {}
                State::Native(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(text::IMPORT_NATIVE_OPEN);
                    });
                }
                State::Browser(browser) => match browser.show(ui) {
                    Some(BrowserOutcome::Picked(p)) => action = Some(Action::Identify(p)),
                    Some(BrowserOutcome::Cancelled) => close = true,
                    None => {}
                },
                State::Working(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(text::IMPORT_WORKING);
                    });
                }
                State::Review {
                    identified,
                    forcing,
                    filter,
                    choice,
                    ..
                } => {
                    action = review_ui(ui, identified, forcing, filter, choice, focus, escape);
                }
                State::Failed(e) => {
                    crate::copyable::message(ui, ERROR, &text::import_failed(e));
                    let ok = ui.button(text::OK);
                    if focus {
                        ok.request_focus();
                    }
                    close |= ok.clicked() || escape;
                }
                State::Done(report) => {
                    let color = if report.failed > 0 {
                        WARNING
                    } else {
                        ui.visuals().text_color()
                    };
                    if report.lines.is_empty() && report.warnings.is_empty() {
                        ui.label(text::IMPORT_NOTHING);
                    }
                    for line in &report.lines {
                        ui.colored_label(color, line);
                    }
                    for warning in &report.warnings {
                        crate::copyable::message(ui, WARNING, warning);
                    }
                    let ok = ui.button(text::OK);
                    if focus {
                        ok.request_focus();
                    }
                    close |= ok.clicked() || escape;
                }
            }
        });
        if let Some(action) = action {
            self.apply(action, ctx);
        }
        if close {
            self.state = State::Idle;
            self.wanted = None;
            return Some(ImportClosed {
                installed: std::mem::take(&mut self.installed),
            });
        }
        None
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Identify(p) => self.identify(p, ctx),
            Action::Cancel => {
                self.state = State::Idle;
            }
            Action::Force => {
                if let State::Review { forcing, .. } = &mut self.state {
                    *forcing = true;
                    self.focus = true;
                }
            }
            Action::ImportKnown { replace } => {
                if let State::Review {
                    src,
                    identified: BiosIdentified::Known { md5, targets },
                    ..
                } = &self.state
                {
                    let items = known_import_items(targets, replace);
                    let (src, md5) = (src.clone(), *md5);
                    self.import(src, items, Some(md5), ctx);
                }
            }
            Action::ImportUnverified(name) => {
                if let State::Review {
                    src, identified, ..
                } = &self.state
                    && let Some(item) = unverified_item(identified, &name)
                {
                    let src = src.clone();
                    self.import(src, vec![item], None, ctx);
                }
            }
        }
        if matches!(self.state, State::Idle) {
            self.wanted = None;
        }
    }
}

enum Action {
    Identify(PathBuf),
    Cancel,
    Force,
    ImportKnown { replace: bool },
    ImportUnverified(String),
}

fn target_line(
    t: &ExpectedFirmware,
    state: &Result<TargetState, String>,
) -> (String, Option<Color32>) {
    match state {
        Ok(TargetState::Absent) => (text::target_new(&t.name, &t.label), None),
        Ok(TargetState::FreeInstalled) => (text::target_free(&t.name), None),
        Ok(TargetState::Same) => (text::target_same(&t.name), None),
        Ok(TargetState::Other { .. }) => (text::target_other(&t.name), Some(WARNING)),
        Err(e) => (format!("{} : {e}", t.name), Some(ERROR)),
    }
}

fn review_ui(
    ui: &mut egui::Ui,
    identified: &BiosIdentified,
    forcing: &mut bool,
    filter: &mut String,
    choice: &mut Option<String>,
    focus: bool,
    escape: bool,
) -> Option<Action> {
    let mut action = escape.then_some(Action::Cancel);
    match identified {
        BiosIdentified::Known { targets, .. } => {
            ui.label(text::IMPORT_KNOWN);
            for (t, state) in targets {
                let (line, color) = target_line(t, state);
                match color {
                    Some(c) => ui.colored_label(c, line),
                    None => ui.label(line),
                };
            }
            let writable = !known_import_items(targets, false).is_empty();
            let replaceable =
                known_import_items(targets, true).len() > known_import_items(targets, false).len();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let first = writable.then(|| {
                    let b = ui.button(text::IMPORT_DO);
                    if b.clicked() {
                        action = Some(Action::ImportKnown { replace: false });
                    }
                    b
                });
                let cancel = ui.button(text::CANCEL);
                if cancel.clicked() {
                    action = Some(Action::Cancel);
                }
                if replaceable && ui.button(text::IMPORT_REPLACE).clicked() {
                    action = Some(Action::ImportKnown { replace: true });
                }
                if focus {
                    first.unwrap_or(cancel).request_focus();
                }
            });
            if replaceable {
                ui.label(RichText::new(text::IMPORT_REPLACE_NOTE).small().weak());
            }
        }
        BiosIdentified::Encrypted { .. } => {
            ui.colored_label(WARNING, crate::bios_import::text::ROM_KEY_NOTE);
            action = names_ui(ui, identified.unverified_names(), filter, choice, focus).or(action);
        }
        BiosIdentified::Unknown {
            md5, size, rom_key, ..
        } => {
            if *rom_key {
                ui.label(text::IMPORT_ROM_KEY);
            } else {
                ui.colored_label(WARNING, text::import_unknown(md5, *size));
            }
            if *forcing || *rom_key {
                action =
                    names_ui(ui, identified.unverified_names(), filter, choice, focus).or(action);
            } else {
                ui.horizontal(|ui| {
                    let cancel = ui.button(text::CANCEL);
                    if focus {
                        cancel.request_focus();
                    }
                    if cancel.clicked() {
                        action = Some(Action::Cancel);
                    }
                    if ui.button(text::IMPORT_FORCE).clicked() {
                        action = Some(Action::Force);
                    }
                });
            }
        }
    }
    action
}

/// Choice of the target name of an unverified import: filter field, list of buttons, then « Importer ».
fn names_ui(
    ui: &mut egui::Ui,
    names: &[String],
    filter: &mut String,
    choice: &mut Option<String>,
    focus: bool,
) -> Option<Action> {
    let mut action = None;
    ui.label(text::IMPORT_CHOOSE_NAME);
    if names.len() > 1 {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text(text::IMPORT_FILTER_HINT)
                .desired_width(280.0),
        );
    }
    let needle = filter.trim().to_lowercase();
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, name) in names
                .iter()
                .filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle))
                .take(MAX_LISTED_NAMES)
                .enumerate()
            {
                let on = choice.as_deref() == Some(name.as_str());
                let r = ui.selectable_label(on, RichText::new(name).monospace());
                if focus && i == 0 {
                    r.request_focus();
                }
                if r.clicked() {
                    *choice = Some(name.clone());
                }
            }
        });
    ui.horizontal(|ui| {
        let chosen = choice.clone().filter(|c| names.contains(c));
        let label = chosen.as_deref().map_or_else(
            || text::IMPORT_UNVERIFIED.to_owned(),
            text::import_unverified_as,
        );
        if ui
            .add_enabled(chosen.is_some(), egui::Button::new(label))
            .clicked()
            && let Some(c) = chosen
        {
            action = Some(Action::ImportUnverified(c));
        }
        if ui.button(text::CANCEL).clicked() {
            action = Some(Action::Cancel);
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, RawInput};

    fn frame(ctx: &egui::Context, ui: &mut ImportUi, events: Vec<Event>) -> Option<ImportClosed> {
        let raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            focused: true,
            events,
            ..RawInput::default()
        };
        let mut closed = None;
        let mut o = ctx.run_ui(raw, |_| closed = ui.show(ctx));
        o.textures_delta.clear();
        closed
    }

    fn key(ctx: &egui::Context, ui: &mut ImportUi, key: Key) -> Option<ImportClosed> {
        let ev = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let a = frame(ctx, ui, vec![ev(true)]);
        a.or(frame(ctx, ui, vec![ev(false)]))
    }

    fn wait(ctx: &egui::Context, ui: &mut ImportUi, until: impl Fn(&State) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            frame(ctx, ui, Vec::new());
            if until(&ui.state) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("state not reached");
    }

    /// Built-in browser (Docker), keyboard only: Entrée picks the file, the unknown file is refused, forced under
    /// an expected name, written unverified; Échap / OK close the dialog.
    #[test]
    fn unknown_file_through_the_builtin_browser() {
        let dir = tempfile::tempdir().unwrap();
        let pick = dir.path().join("pick");
        std::fs::create_dir_all(&pick).unwrap();
        std::fs::write(pick.join("my-kick.rom"), b"not a known kickstart").unwrap();
        let system = dir.path().join("system");
        let ctx = egui::Context::default();
        let mut ui = ImportUi::new(system.clone(), None).with_builtin_browser(pick);
        ui.start(Some("kick34005.A500".into()));
        assert!(matches!(ui.state, State::Browser(_)));
        // The folder is listed on a worker thread: wait for the list, then let the first entry take the focus.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            frame(&ctx, &mut ui, Vec::new());
            let State::Browser(b) = &mut ui.state else {
                panic!("browser expected");
            };
            if !b.poll() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "listing not ready");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        frame(&ctx, &mut ui, Vec::new());
        frame(&ctx, &mut ui, Vec::new());
        // The first entry has the focus: Entrée picks it.
        key(&ctx, &mut ui, Key::Enter);
        wait(&ctx, &mut ui, |s| matches!(s, State::Review { .. }));
        let State::Review { identified, .. } = &ui.state else {
            unreachable!()
        };
        assert!(matches!(
            identified,
            BiosIdentified::Unknown { rom_key: false, .. }
        ));
        assert!(
            identified
                .unverified_names()
                .contains(&"kick34005.A500".to_owned())
        );
        // Refused by default: nothing written.
        assert!(!system.join("kick34005.A500").exists());
        ui.apply(Action::Force, &ctx);
        ui.apply(Action::ImportUnverified("../evil".into()), &ctx);
        assert!(
            matches!(ui.state, State::Review { .. }),
            "name outside the catalogue ignored"
        );
        ui.apply(Action::ImportUnverified("kick34005.A500".into()), &ctx);
        wait(&ctx, &mut ui, |s| matches!(s, State::Done(_)));
        assert_eq!(
            std::fs::read(system.join("kick34005.A500")).unwrap(),
            b"not a known kickstart"
        );
        let closed = key(&ctx, &mut ui, Key::Escape).expect("closed");
        assert!(closed.installed);
        assert!(!ui.is_active());
    }

    #[test]
    fn escape_in_the_browser_cancels() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut ui = ImportUi::new(dir.path().join("system"), None)
            .with_builtin_browser(dir.path().to_owned());
        ui.start(None);
        frame(&ctx, &mut ui, Vec::new());
        let closed = key(&ctx, &mut ui, Key::Escape).expect("closed");
        assert!(!closed.installed);
        assert!(!ui.is_active());
    }
}
