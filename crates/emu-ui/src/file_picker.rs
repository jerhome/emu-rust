//! File selection (design v3.0 §2.2, §4): the system dialog through `rfd` where it works, a built-in egui browser
//! under Docker and as a fallback. This file holds the dialog and the browser's model (listing, navigation,
//! typed path) and its egui drawing (`FileBrowser::show`, keyboard-first).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use crate::platform::running_in_container;

/// Entries shown at most in one folder (a pathological folder must not freeze the interface).
const MAX_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerBackend {
    /// System dialog (`rfd`).
    Native,
    /// Built-in egui browser.
    Builtin,
}

/// Container: always the built-in browser (no portal, no desktop). Linux without a session bus and without
/// `zenity` has no working native dialog either. Windows and macOS always have one.
pub(crate) fn picker_backend() -> PickerBackend {
    backend_for(
        running_in_container(),
        cfg!(target_os = "linux"),
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty()),
        || zenity_in_path(std::env::var_os("PATH").as_deref()),
    )
}

fn backend_for(
    in_container: bool,
    linux: bool,
    has_session_bus: bool,
    zenity: impl FnOnce() -> bool,
) -> PickerBackend {
    if in_container || (linux && !has_session_bus && !zenity()) {
        PickerBackend::Builtin
    } else {
        PickerBackend::Native
    }
}

fn zenity_in_path(path: Option<&std::ffi::OsStr>) -> bool {
    path.is_some_and(|p| {
        std::env::split_paths(p).any(|dir| {
            let candidate = dir.join("zenity");
            candidate.is_file()
        })
    })
}

/// Native "open file" dialog (`rfd`) on a worker thread, so the UI thread never blocks; no parent window (the
/// portal and Cocoa parent the dialog to the application anyway). The receiver yields exactly one value: the file, or `None` if cancelled or if the thread
/// could not start.
pub(crate) fn pick_file_in_background(
    title: &str,
    filters: &[(&str, &[&str])],
) -> Receiver<Option<PathBuf>> {
    let (tx, rx) = mpsc::channel();
    let dialog = dialog(title, filters);
    let tx2 = tx.clone();
    let spawned = std::thread::Builder::new()
        .name("file-dialog".into())
        .spawn(move || {
            let _ = tx.send(dialog.pick_file());
        });
    if spawned.is_err() {
        let _ = tx2.send(None);
    }
    rx
}

fn dialog(title: &str, filters: &[(&str, &[&str])]) -> rfd::FileDialog {
    let mut d = rfd::FileDialog::new().set_title(title);
    for (name, extensions) in filters {
        d = d.add_filter(*name, extensions);
    }
    d
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrowserOutcome {
    Picked(PathBuf),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Model of the built-in browser: current folder, sorted entries, extension filter, typed path.
#[derive(Debug)]
pub(crate) struct FileBrowser {
    dir: PathBuf,
    /// Lower-case extensions without the dot; empty = every file.
    filter: Vec<String>,
    entries: Vec<BrowserEntry>,
    /// Why the folder could not be listed (shown instead of the entries).
    error: Option<String>,
    /// Content of the path text field.
    pub typed: String,
    /// Give the keyboard focus to the first entry on the next frame (opening, folder change).
    focus_first: bool,
}

impl FileBrowser {
    /// `start` falls back to its first existing ancestor, then to the working directory.
    pub fn new(start: PathBuf, filter: Vec<String>) -> Self {
        let dir = start
            .ancestors()
            .find(|p| p.is_dir())
            .map_or_else(|| PathBuf::from("."), Path::to_owned);
        let mut b = Self {
            dir,
            filter: filter.into_iter().map(|e| e.to_ascii_lowercase()).collect(),
            entries: Vec::new(),
            error: None,
            typed: String::new(),
            focus_first: true,
        };
        b.refresh();
        b
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn entries(&self) -> &[BrowserEntry] {
        &self.entries
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Folders first, then files, case-insensitive; hidden entries and files rejected by the filter are skipped.
    pub fn refresh(&mut self) {
        self.typed = self.dir.display().to_string();
        self.focus_first = true;
        self.entries.clear();
        self.error = None;
        let read = match std::fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        for entry in read.flatten().take(MAX_ENTRIES * 4) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            // `metadata` follows symlinks: a link to a folder is a folder, a broken link is skipped.
            let Ok(meta) = std::fs::metadata(entry.path()) else {
                continue;
            };
            let is_dir = meta.is_dir();
            if !is_dir && !self.accepts(&name) {
                continue;
            }
            self.entries.push(BrowserEntry { name, is_dir });
            if self.entries.len() >= MAX_ENTRIES {
                break;
            }
        }
        self.entries.sort_by(compare);
    }

    fn accepts(&self, name: &str) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| self.filter.iter().any(|f| f.eq_ignore_ascii_case(e)))
    }

    /// Goes to the parent folder; `false` at the root.
    pub fn go_up(&mut self) -> bool {
        match self.dir.parent().map(Path::to_owned) {
            Some(p) if !p.as_os_str().is_empty() => {
                self.dir = p;
                self.refresh();
                true
            }
            _ => false,
        }
    }

    /// Activates the entry `index`: a folder is entered, a file is picked.
    pub fn activate(&mut self, index: usize) -> Option<BrowserOutcome> {
        let entry = self.entries.get(index)?.clone();
        let path = self.dir.join(&entry.name);
        if entry.is_dir {
            self.dir = path;
            self.refresh();
            None
        } else {
            Some(BrowserOutcome::Picked(path))
        }
    }

    /// Validates the typed path: a folder is entered, an existing file (accepted by the filter) is picked,
    /// anything else leaves the browser as is and returns `None`.
    pub fn submit_typed(&mut self) -> Option<BrowserOutcome> {
        let text = self.typed.trim();
        if text.is_empty() {
            return None;
        }
        let path = self.dir.join(text); // an absolute path replaces the base
        if path.is_dir() {
            self.dir = path;
            self.refresh();
            None
        } else if path.is_file()
            && path
                .file_name()
                .is_some_and(|n| self.accepts(&n.to_string_lossy()))
        {
            Some(BrowserOutcome::Picked(path))
        } else {
            self.error = Some(format!("introuvable : {text}"));
            None
        }
    }
}

impl FileBrowser {
    /// Draws the browser inside `ui` (a modal of the caller). Keyboard: Tab / arrows move between entries, Entrée
    /// opens a folder or picks a file, Retour arrière goes up (outside the path field), Échap cancels.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<BrowserOutcome> {
        use crate::ui::text;
        let mut outcome = None;
        let typing = ui.ctx().text_edit_focused();
        let (escape, back) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                !typing && i.key_pressed(egui::Key::Backspace),
            )
        });
        ui.label(egui::RichText::new(self.dir().display().to_string()).monospace());
        let mut go_up = back;
        let mut submit = false;
        ui.horizontal(|ui| {
            go_up |= ui.button(text::BROWSER_UP).clicked();
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.typed)
                    .desired_width(360.0)
                    .hint_text(text::BROWSER_PATH_HINT),
            );
            submit |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            submit |= ui.button(text::BROWSER_GO).clicked();
        });
        if let Some(e) = self.error() {
            ui.colored_label(egui::Color32::from_rgb(255, 110, 110), e);
        }
        if !self.filter.is_empty() {
            ui.label(
                egui::RichText::new(text::browser_filter(&self.filter))
                    .small()
                    .weak(),
            );
        }
        let mut activate = None;
        let focus_first = std::mem::take(&mut self.focus_first);
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if self.entries().is_empty() && self.error().is_none() {
                    ui.label(text::BROWSER_EMPTY);
                }
                for (i, entry) in self.entries().iter().enumerate() {
                    let label = if entry.is_dir {
                        format!("{}/", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let r = ui.add(egui::Button::new(label).frame(false));
                    if focus_first && i == 0 {
                        r.request_focus();
                    }
                    if r.clicked() {
                        activate = Some(i);
                    }
                }
            });
        ui.add_space(6.0);
        let cancel = ui.button(text::BROWSER_CANCEL);
        if focus_first && self.entries.is_empty() {
            cancel.request_focus();
        }
        if let Some(i) = activate {
            outcome = self.activate(i);
        } else if submit {
            outcome = self.submit_typed();
        } else if go_up {
            self.go_up();
        }
        if cancel.clicked() || (escape && outcome.is_none()) {
            outcome = Some(BrowserOutcome::Cancelled);
        }
        outcome
    }
}

fn compare(a: &BrowserEntry, b: &BrowserEntry) -> Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_decision() {
        let never = || panic!("zenity must not be looked up");
        assert_eq!(backend_for(true, true, true, never), PickerBackend::Builtin);
        assert_eq!(backend_for(false, true, true, never), PickerBackend::Native);
        assert_eq!(
            backend_for(false, false, false, never),
            PickerBackend::Native
        );
        assert_eq!(
            backend_for(false, true, false, || false),
            PickerBackend::Builtin
        );
        assert_eq!(
            backend_for(false, true, false, || true),
            PickerBackend::Native
        );
    }

    #[test]
    fn zenity_lookup_in_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert!(!zenity_in_path(Some(&path)));
        std::fs::write(dir.path().join("zenity"), "").unwrap();
        assert!(zenity_in_path(Some(&path)));
        assert!(!zenity_in_path(None));
    }

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for d in ["beta", "Alpha", ".hidden"] {
            std::fs::create_dir(dir.path().join(d)).unwrap();
        }
        for f in ["kick.ROM", "game.adf", "notes.txt", ".dot.rom", "Zed.rom"] {
            std::fs::write(dir.path().join(f), "x").unwrap();
        }
        dir
    }

    fn names(b: &FileBrowser) -> Vec<&str> {
        b.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn lists_folders_first_filtered_and_sorted() {
        let dir = tree();
        let b = FileBrowser::new(dir.path().to_owned(), vec!["rom".into()]);
        assert_eq!(names(&b), ["Alpha", "beta", "kick.ROM", "Zed.rom"]);
        let all = FileBrowser::new(dir.path().to_owned(), Vec::new());
        assert_eq!(
            names(&all),
            [
                "Alpha",
                "beta",
                "game.adf",
                "kick.ROM",
                "notes.txt",
                "Zed.rom"
            ]
        );
    }

    #[test]
    fn navigation_and_picking() {
        let dir = tree();
        std::fs::write(dir.path().join("Alpha").join("a.rom"), "x").unwrap();
        let mut b = FileBrowser::new(dir.path().to_owned(), vec!["rom".into()]);
        assert_eq!(b.activate(0), None);
        assert_eq!(b.dir(), dir.path().join("Alpha"));
        assert_eq!(
            b.activate(0),
            Some(BrowserOutcome::Picked(
                dir.path().join("Alpha").join("a.rom")
            ))
        );
        assert!(b.go_up());
        assert_eq!(b.dir(), dir.path());
        assert_eq!(b.activate(99), None);
    }

    #[test]
    fn typed_path() {
        let dir = tree();
        let mut b = FileBrowser::new(dir.path().to_owned(), vec!["rom".into()]);
        b.typed = "beta".into();
        assert_eq!(b.submit_typed(), None);
        assert_eq!(b.dir(), dir.path().join("beta"));
        b.typed = dir.path().join("Zed.rom").display().to_string();
        assert_eq!(
            b.submit_typed(),
            Some(BrowserOutcome::Picked(dir.path().join("Zed.rom")))
        );
        b.typed = dir.path().join("notes.txt").display().to_string();
        assert_eq!(b.submit_typed(), None);
        assert!(b.error().is_some());
        b.typed = "  ".into();
        assert_eq!(b.submit_typed(), None);
    }

    #[test]
    fn missing_start_falls_back_to_an_ancestor() {
        let dir = tree();
        let b = FileBrowser::new(dir.path().join("nope/deeper"), Vec::new());
        assert_eq!(b.dir(), dir.path());
    }

    #[test]
    fn root_has_no_parent() {
        let mut b = FileBrowser::new(PathBuf::from("/"), Vec::new());
        assert!(!b.go_up());
    }
}
