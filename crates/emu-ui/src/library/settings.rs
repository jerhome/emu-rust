//! Library settings: ROM folders, scan, DAT import, cover sources, credentials, link to the cores screen.
//! Secrets are never pre-filled nor shown: « défini / non défini » with « Modifier » (masked field) and
//! « Effacer ». Changes are written by `Config::save` (0600, atomic) on « Enregistrer » and when leaving.

use std::path::PathBuf;

use egui::{Button, Key, RichText, ScrollArea, TextEdit, Ui};
use emu_assets::Secret;
use emu_library::systems;

use super::{Level, LibraryUi, Page, text, weak};
use crate::config_library::{IgdbConfig, ScreenScraperConfig};

const KNOWN_SOURCES: [&str; 3] = ["libretro", "screenscraper", "igdb"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SecretField {
    SsDevPassword,
    SsPassword,
    IgdbSecret,
}

#[derive(Default)]
pub(super) struct State {
    pub focus_first: bool,
    new_root: String,
    dat_path: String,
    /// 0 = automatic detection, else `systems()[n - 1]`.
    dat_system: usize,
    pub(super) editing: Option<SecretField>,
    /// Masked input of the secret being edited; cleared as soon as it is stored or cancelled.
    pub(super) secret_buf: String,
    focus_secret: bool,
    pub(super) dirty: bool,
    /// File system and database reads, refreshed when the page opens and after each action (not per frame).
    root_exists: Vec<bool>,
    /// `(system name, « n entrées, importé le … »)`, `None` = no DAT.
    dat_rows: Vec<(String, Option<String>)>,
    /// Last context the page was drawn with: lets `leave_settings` wipe the secret field's egui state.
    ctx: Option<egui::Context>,
}

const SECRET_FIELDS: [SecretField; 3] = [
    SecretField::SsDevPassword,
    SecretField::SsPassword,
    SecretField::IgdbSecret,
];

pub(super) fn secret_edit_id(field: SecretField) -> egui::Id {
    egui::Id::unique(("library-secret-edit", field))
}

/// egui keeps a `TextEditState` per field (cursor and undo history, i.e. copies of the typed text): dropped as
/// soon as the input is validated or cancelled.
pub(super) fn forget_secret_edits(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        for field in SECRET_FIELDS {
            d.remove::<egui::text_edit::TextEditState>(secret_edit_id(field));
        }
    });
}

/// Every known source, in configured order, with its enabled flag (unknown names dropped).
pub(super) fn source_rows(configured: &[String]) -> Vec<(String, bool)> {
    let mut rows: Vec<(String, bool)> = configured
        .iter()
        .filter(|s| KNOWN_SOURCES.contains(&s.as_str()))
        .map(|s| (s.clone(), true))
        .collect();
    rows.dedup();
    for k in KNOWN_SOURCES {
        if !rows.iter().any(|(s, _)| s == k) {
            rows.push((k.to_owned(), false));
        }
    }
    rows
}

pub(super) fn sources_from_rows(rows: &[(String, bool)]) -> Vec<String> {
    rows.iter()
        .filter(|(_, on)| *on)
        .map(|(s, _)| s.clone())
        .collect()
}

pub(super) fn secret_label(s: &Secret) -> &'static str {
    if s.is_empty() {
        text::SECRET_UNSET
    } else {
        text::SECRET_SET
    }
}

impl LibraryUi {
    pub(super) fn secret_mut(&mut self, field: SecretField) -> &mut Secret {
        let creds = &mut self.config.credentials;
        match field {
            SecretField::SsDevPassword => {
                &mut creds
                    .screenscraper
                    .get_or_insert_with(ScreenScraperConfig::default)
                    .dev_password
            }
            SecretField::SsPassword => {
                &mut creds
                    .screenscraper
                    .get_or_insert_with(ScreenScraperConfig::default)
                    .password
            }
            SecretField::IgdbSecret => {
                &mut creds
                    .igdb
                    .get_or_insert_with(IgdbConfig::default)
                    .client_secret
            }
        }
    }

    fn end_secret_edit(&mut self) {
        self.settings.secret_buf.clear();
        self.settings.editing = None;
        if let Some(ctx) = &self.settings.ctx {
            forget_secret_edits(ctx);
        }
    }

    pub(super) fn leave_settings(&mut self) {
        self.end_secret_edit();
        if std::mem::take(&mut self.settings.dirty) {
            self.save_config();
        }
        self.covers.configure(&self.config);
    }

    /// Re-reads what the page shows from disk and the database.
    pub(super) fn refresh_settings_cache(&mut self) {
        self.refresh_dos_cache();
        self.settings.root_exists = self
            .config
            .library
            .roots
            .iter()
            .map(|r| r.is_dir())
            .collect();
        self.settings.dat_rows = match &self.db {
            Some(db) => systems()
                .iter()
                .map(|s| {
                    let info = db.dat_info(&s.id).ok().flatten().map(|info| {
                        let date = crate::ui::text::format_unix_utc(
                            u64::try_from(info.imported_unix).unwrap_or(0),
                        );
                        text::dat_info(info.entries, &date)
                    });
                    (s.name.clone(), info)
                })
                .collect(),
            None => Vec::new(),
        };
    }

    pub(super) fn settings_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.settings.ctx = Some(ctx.clone());
        let escape = self.dialog.is_none() && ui.input(|i| i.key_pressed(Key::Escape));
        egui::Panel::bottom("settings-status").show(ui, |ui| {
            let enabled = self.dialog.is_none();
            self.pad_notice_line(ui, enabled);
            self.status_line(ui);
            weak(ui, text::SETTINGS_HINT);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                let back = ui.button(text::BACK);
                if std::mem::take(&mut self.settings.focus_first) {
                    back.request_focus();
                }
                if back.clicked() {
                    self.leave_settings();
                    self.set_page(Page::Home);
                }
                if ui
                    .add_enabled(!self.capture, Button::new(text::SAVE))
                    .clicked()
                {
                    self.settings.dirty = false;
                    self.save_config();
                }
                if ui.button(text::CORES_LINK).clicked() {
                    self.leave_settings();
                    self.set_page(Page::Cores);
                }
                ui.heading(text::SETTINGS_TITLE);
            });
            if self.config.is_locked() {
                ui.colored_label(super::WARNING, text::LOCKED);
            }
            ui.separator();
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.roots_section(ui, &ctx);
                    ui.separator();
                    self.dat_section(ui, &ctx);
                    ui.separator();
                    self.covers_section(ui);
                    ui.separator();
                    self.credentials_section(ui);
                    ui.separator();
                    self.gamepad_section(ui);
                    ui.separator();
                    self.computers_section(ui);
                    ui.separator();
                    self.dos_cache_section(ui);
                    ui.separator();
                    self.app_section(ui);
                    ui.separator();
                    journal_section(ui);
                });
        });
        if escape {
            if self.cancel_pad_learning() {
                // Remap cancelled: stay on the page.
            } else if self.settings.editing.is_some() {
                self.end_secret_edit();
            } else {
                self.leave_settings();
                self.set_page(Page::Home);
            }
        }
    }

    fn roots_section(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.label(RichText::new(text::ROOTS).strong());
        let fixed = self.dirs.root.join("roms");
        ui.horizontal(|ui| {
            ui.label(RichText::new(fixed.display().to_string()).monospace());
            weak(ui, text::ROOT_FIXED);
        });
        let mut remove = None;
        for (i, root) in self.config.library.roots.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(root.display().to_string()).monospace());
                if !self.settings.root_exists.get(i).copied().unwrap_or(true) {
                    ui.colored_label(super::WARNING, text::ROOT_MISSING);
                }
                if ui.button(text::ROOT_REMOVE).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.config.library.roots.remove(i);
            self.settings.dirty = true;
            self.refresh_settings_cache();
        }
        ui.horizontal(|ui| {
            let edit = ui.add(
                TextEdit::singleline(&mut self.settings.new_root)
                    .hint_text(text::ROOT_HINT)
                    .desired_width(380.0),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let add = ui.add_enabled(
                !self.settings.new_root.trim().is_empty(),
                Button::new(text::ROOT_ADD),
            );
            if (add.clicked() || enter) && !self.settings.new_root.trim().is_empty() {
                let path = PathBuf::from(self.settings.new_root.trim());
                let path = std::path::absolute(&path).unwrap_or(path);
                if path == fixed || self.config.library.roots.contains(&path) {
                    self.set_status(
                        text::root_duplicate(&path.display().to_string()),
                        Level::Warn,
                    );
                } else {
                    self.config.library.roots.push(path);
                    self.settings.dirty = true;
                    self.refresh_settings_cache();
                }
                self.settings.new_root.clear();
            }
        });
        if ui
            .checkbox(&mut self.config.library.scan_on_start, text::SCAN_ON_START)
            .changed()
        {
            self.settings.dirty = true;
        }
        if ui
            .add_enabled(
                self.scan.is_none() && self.db.is_some() && !self.capture,
                Button::new(text::SCAN_NOW),
            )
            .clicked()
        {
            if std::mem::take(&mut self.settings.dirty) {
                self.save_config();
            }
            self.start_scan(ctx);
        }
    }

    fn dat_section(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.label(RichText::new(text::DAT_TITLE).strong());
        if !self.settings.dat_rows.is_empty() {
            egui::Grid::new("dat-info").num_columns(2).show(ui, |ui| {
                for (name, info) in &self.settings.dat_rows {
                    ui.label(name);
                    match info {
                        Some(info) => {
                            ui.label(info);
                        }
                        None => weak(ui, text::DAT_NONE),
                    }
                    ui.end_row();
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.settings.dat_path)
                    .hint_text(text::DAT_PATH_HINT)
                    .desired_width(380.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label(format!("{} :", text::DAT_SYSTEM));
            let n = systems().len() + 1;
            if ui.button("<").clicked() {
                self.settings.dat_system = (self.settings.dat_system + n - 1) % n;
            }
            let current = match self.settings.dat_system {
                0 => text::DAT_AUTO.to_owned(),
                i => systems()[i - 1].name.clone(),
            };
            ui.label(current);
            if ui.button(">").clicked() {
                self.settings.dat_system = (self.settings.dat_system + 1) % n;
            }
        });
        ui.horizontal(|ui| {
            let ready = !self.settings.dat_path.trim().is_empty()
                && self.dat.is_none()
                && self.db.is_some()
                && !self.capture;
            if ui
                .add_enabled(ready, Button::new(text::DAT_IMPORT))
                .clicked()
            {
                let system = match self.settings.dat_system {
                    0 => None,
                    i => Some(systems()[i - 1].id.clone()),
                };
                let path = PathBuf::from(self.settings.dat_path.trim());
                self.start_dat_import(ctx, path, system);
            }
            if self.dat.is_some() {
                ui.spinner();
            }
            if ui
                .add_enabled(
                    self.dat_download.is_none() && self.db.is_some() && !self.capture,
                    Button::new(text::DAT_DOWNLOAD),
                )
                .clicked()
            {
                self.start_dat_download(ctx);
            }
            if self.dat_download.is_some() {
                ui.spinner();
            }
        });
        weak(ui, text::DAT_DOWNLOAD_NOTE);
    }

    fn covers_section(&mut self, ui: &mut Ui) {
        ui.label(RichText::new(text::COVERS_TITLE).strong());
        if ui
            .checkbox(&mut self.config.covers.download, text::COVERS_DOWNLOAD)
            .changed()
        {
            self.settings.dirty = true;
        }
        let mut rows = source_rows(&self.config.covers.sources);
        let mut changed = false;
        let count = rows.len();
        let mut swap = None;
        for (i, (name, on)) in rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let label = text::source_name(name).to_owned();
                if ui.checkbox(on, label).changed() {
                    changed = true;
                }
                if ui
                    .add_enabled(i > 0, Button::new(text::SOURCE_UP))
                    .clicked()
                {
                    swap = Some((i - 1, i));
                }
                if ui
                    .add_enabled(i + 1 < count, Button::new(text::SOURCE_DOWN))
                    .clicked()
                {
                    swap = Some((i, i + 1));
                }
            });
        }
        if let Some((a, b)) = swap {
            rows.swap(a, b);
            changed = true;
        }
        if changed {
            self.config.covers.sources = sources_from_rows(&rows);
            self.settings.dirty = true;
        }
        if ui
            .add_enabled(!self.capture, Button::new(text::COVERS_RETRY))
            .clicked()
        {
            self.covers.retry_missing();
            self.set_status(text::COVERS_RETRY_DONE.to_owned(), Level::Info);
        }
    }

    /// Computers (design v5.0 §4): initial input mode of a game without a remembered one, automatic fast loading.
    fn app_section(&mut self, ui: &mut Ui) {
        ui.label(RichText::new(text::APP_TITLE).strong());
        if ui
            .checkbox(&mut self.config.app.show_terminal, text::SHOW_TERMINAL)
            .changed()
        {
            self.settings.dirty = true;
        }
    }

    fn computers_section(&mut self, ui: &mut Ui) {
        use crate::config_computer::KeyboardModeSetting as Mode;
        ui.label(RichText::new(text::COMPUTERS_TITLE).strong());
        ui.horizontal_wrapped(|ui| {
            ui.label(text::DEFAULT_INPUT_MODE);
            for (mode, label) in [
                (Mode::Auto, text::INPUT_MODE_AUTO),
                (Mode::Keyboard, text::INPUT_MODE_KEYBOARD),
                (Mode::Joystick, text::INPUT_MODE_JOYSTICK),
            ] {
                let current = self.config.computer.keyboard_mode == mode;
                if ui.selectable_label(current, label).clicked() && !current {
                    self.config.computer.keyboard_mode = mode;
                    self.settings.dirty = true;
                }
            }
        });
        weak(ui, text::DEFAULT_INPUT_MODE_NOTE);
        if ui
            .checkbox(
                &mut self.config.computer.auto_fast_load,
                text::AUTO_FAST_LOAD,
            )
            .changed()
        {
            self.settings.dirty = true;
        }
        weak(ui, text::AUTO_FAST_LOAD_NOTE);
    }

    fn credentials_section(&mut self, ui: &mut Ui) {
        ui.label(RichText::new(text::CREDENTIALS_TITLE).strong());
        weak(ui, text::CREDENTIALS_NOTE);
        let mut dirty = false;
        {
            let ss = self
                .config
                .credentials
                .screenscraper
                .get_or_insert_with(ScreenScraperConfig::default);
            dirty |= plain_field(ui, text::SS_DEV_ID, &mut ss.dev_id);
        }
        self.secret_row(ui, text::SS_DEV_PASSWORD, SecretField::SsDevPassword);
        {
            let ss = self
                .config
                .credentials
                .screenscraper
                .get_or_insert_with(ScreenScraperConfig::default);
            dirty |= plain_field(ui, text::SS_USER, &mut ss.user);
        }
        self.secret_row(ui, text::SS_PASSWORD, SecretField::SsPassword);
        {
            let igdb = self
                .config
                .credentials
                .igdb
                .get_or_insert_with(IgdbConfig::default);
            dirty |= plain_field(ui, text::IGDB_CLIENT_ID, &mut igdb.client_id);
        }
        self.secret_row(ui, text::IGDB_SECRET, SecretField::IgdbSecret);
        // Empty sections are not written (no `[credentials.x]` with blanks).
        let creds = &mut self.config.credentials;
        if creds
            .screenscraper
            .as_ref()
            .is_some_and(|s| *s == ScreenScraperConfig::default())
        {
            creds.screenscraper = None;
        }
        if creds
            .igdb
            .as_ref()
            .is_some_and(|s| *s == IgdbConfig::default())
        {
            creds.igdb = None;
        }
        if dirty {
            self.settings.dirty = true;
        }
    }

    fn secret_row(&mut self, ui: &mut Ui, label: &str, field: SecretField) {
        let state = secret_label(self.secret_mut(field));
        let editing = self.settings.editing == Some(field);
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label(format!("{label} :"));
            if editing {
                let edit = ui.add(
                    TextEdit::singleline(&mut self.settings.secret_buf)
                        .id(secret_edit_id(field))
                        .password(true)
                        .desired_width(220.0),
                );
                if std::mem::take(&mut self.settings.focus_secret) {
                    edit.request_focus();
                }
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if ui.button(text::SECRET_OK).clicked() || enter {
                    action = Some(true);
                }
                if ui.button(text::SECRET_CANCEL).clicked() {
                    action = Some(false);
                }
            } else {
                ui.label(RichText::new(state).italics());
                if ui.button(text::SECRET_EDIT).clicked() {
                    self.settings.editing = Some(field);
                    self.settings.secret_buf.clear();
                    self.settings.focus_secret = true;
                }
                if ui
                    .add_enabled(state == text::SECRET_SET, Button::new(text::SECRET_CLEAR))
                    .clicked()
                {
                    *self.secret_mut(field) = Secret::default();
                    self.settings.dirty = true;
                }
            }
        });
        match action {
            Some(true) => {
                let value = std::mem::take(&mut self.settings.secret_buf);
                *self.secret_mut(field) = Secret::new(value);
                self.end_secret_edit();
                self.settings.dirty = true;
            }
            Some(false) => self.end_secret_edit(),
            None => {}
        }
    }
}

fn plain_field(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    ui.horizontal(|ui| {
        ui.label(format!("{label} :"));
        ui.add(TextEdit::singleline(value).desired_width(220.0))
            .changed()
    })
    .inner
}

/// « Journal » pane: the latest log lines, warnings and errors highlighted, keyboard-reachable copy button.
fn journal_section(ui: &mut Ui) {
    let lines = crate::LogBuffer::global().snapshot();
    egui::CollapsingHeader::new(RichText::new(text::JOURNAL_TITLE).strong())
        .id_salt("journal")
        .show(ui, |ui| {
            if ui.button(text::JOURNAL_COPY).clicked() {
                let all = lines
                    .iter()
                    .map(|l| format!("{} {} {}", l.level, l.target, l.message))
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.ctx().copy_text(all);
            }
            if lines.is_empty() {
                weak(ui, text::JOURNAL_EMPTY);
                return;
            }
            ScrollArea::vertical()
                .id_salt("journal-lines")
                .max_height(240.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for l in &lines {
                        let line =
                            format!("{:<5} {}", l.level, crate::sanitize_core_line(&l.message));
                        let rich = RichText::new(line).monospace();
                        match l.level {
                            tracing::Level::ERROR => {
                                ui.label(rich.color(egui::Color32::LIGHT_RED));
                            }
                            tracing::Level::WARN => {
                                ui.label(rich.color(super::WARNING));
                            }
                            _ => {
                                ui.label(rich);
                            }
                        }
                    }
                });
        });
}
