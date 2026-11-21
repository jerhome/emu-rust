//! Drawing of « Ranger les ROMs… »: every action is a button or a check box (keyboard and, later, gamepad focus
//! navigation; nothing needs hovering). Changes are collected during the frame and applied afterwards.

use std::path::PathBuf;

use egui::{Button, Key, RichText, ScrollArea, Ui};

use super::{Group, Preview, RowKind, Stage, row_kind, selectable, text};
use crate::library::{ERROR, Level, LibraryUi, Page, WARNING, notify, weak};

const INFO: egui::Color32 = egui::Color32::from_rgb(130, 180, 255);

enum Action {
    Back,
    Stop,
    Retry,
    Check(u32, bool),
    Choose(u32, String),
    Bulk(String, String),
    ToggleOther(u32),
    ImportBios(PathBuf),
}

impl LibraryUi {
    /// « Créer les dossiers des systèmes » (ADR 0023 §4.1): nothing is moved. A few `mkdir`: done here.
    pub(in crate::library) fn create_system_folders(&mut self) {
        if self.capture {
            return;
        }
        match emu_library::create_system_folders(&self.dirs.root.join("roms")) {
            Ok(created) => self.set_status(text::folders_created(created.len()), Level::Info),
            Err(e) => self.set_status(text::folders_failed(&e.to_string()), Level::Warn),
        }
    }

    fn tidy_busy_state(&self) -> super::Busy {
        super::Busy {
            scan: self.scan.is_some(),
            download: self.dat.is_some() || self.dat_download.is_some() || self.cores.installing(),
            game: self.cores.in_use().is_some(),
            no_db: self.db.is_none(),
        }
    }

    /// « Ranger les ROMs… »: refused with a message during a scan, a download or a game; otherwise the analysis
    /// starts and the page opens.
    pub(in crate::library) fn start_tidy(&mut self, ctx: &egui::Context) {
        if let Some(msg) = super::refusal(self.tidy_busy_state()) {
            self.set_status(msg.to_owned(), Level::Warn);
            return;
        }
        let task = super::spawn_plan(
            self.dirs.library_db.clone(),
            self.dirs.root.join("roms"),
            self.dirs.tidy_journals.clone(),
            self.cores.manager().cloned(),
            crate::cores_ui::preferences(&self.config.cores),
            notify(ctx),
        );
        self.tidy.stage = Stage::Analysing {
            task,
            progress: None,
        };
        self.tidy.focus_first = true;
        self.set_page(Page::Tidy);
    }

    /// Capture mode is still analysing (the window is captured once the preview is there).
    pub(crate) fn tidy_busy(&self) -> bool {
        self.tidy.busy()
    }

    fn leave_tidy(&mut self) {
        self.tidy.close();
        self.set_page(Page::Settings);
    }

    pub(in crate::library) fn tidy_page(&mut self, ui: &mut Ui) {
        self.tidy.poll();
        let escape = self.dialog.is_none()
            && !self.cores.import.is_active()
            && ui.input(|i| i.key_pressed(Key::Escape));
        let mut actions: Vec<Action> = Vec::new();
        egui::Panel::bottom("tidy-status").show(ui, |ui| {
            self.status_line(ui);
            weak(ui, text::HINT);
        });
        let focus_first = std::mem::take(&mut self.tidy.focus_first);
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                let back = ui.button(text::BACK);
                if focus_first {
                    back.request_focus();
                }
                if back.clicked() {
                    actions.push(Action::Back);
                }
                ui.heading(text::TITLE);
            });
            ui.separator();
            match &mut self.tidy.stage {
                Stage::Idle => {}
                Stage::Analysing { progress, .. } => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(text::analysing(
                            progress.as_ref().map_or(0, |p| p.files_seen),
                        ));
                    });
                    if ui.button(text::STOP).clicked() {
                        actions.push(Action::Stop);
                    }
                }
                Stage::Failed(e) => {
                    ui.colored_label(WARNING, e.as_str());
                    if ui.button(text::RETRY).clicked() {
                        actions.push(Action::Retry);
                    }
                }
                Stage::Preview(p) => preview(ui, p, &mut actions),
            }
        });
        if escape {
            actions.push(Action::Back);
        }
        for a in actions {
            match a {
                Action::Back => {
                    self.leave_tidy();
                    return;
                }
                Action::Stop => {
                    if let Stage::Analysing { task, .. } = &self.tidy.stage {
                        task.cancel();
                    }
                }
                Action::Retry => {
                    let ctx = ui.ctx().clone();
                    self.tidy.close();
                    self.start_tidy(&ctx);
                }
                Action::Check(id, on) => {
                    if let Stage::Preview(p) = &mut self.tidy.stage {
                        p.set_checked(id, on);
                    }
                }
                Action::Choose(id, system) => {
                    let result = match &mut self.tidy.stage {
                        Stage::Preview(p) => p.choose(id, &system),
                        _ => Ok(()),
                    };
                    if let Err(e) = result {
                        self.set_status(e.to_string(), Level::Warn);
                    }
                }
                Action::Bulk(extension, system) => {
                    if let Stage::Preview(p) = &mut self.tidy.stage {
                        p.choose_for_extension(&extension, &system);
                    }
                }
                Action::ToggleOther(id) => {
                    if let Stage::Preview(p) = &mut self.tidy.stage
                        && !p.other_open.remove(&id)
                    {
                        p.other_open.insert(id);
                    }
                }
                Action::ImportBios(path) => {
                    let ctx = ui.ctx().clone();
                    self.cores.import.start_with_file(path, &ctx);
                }
            }
        }
    }
}

fn preview(ui: &mut Ui, p: &mut Preview, actions: &mut Vec<Action>) {
    weak(ui, text::INTRO);
    ui.colored_label(WARNING, RichText::new(text::SECOND_INSTANCE).small());
    let t = super::totals(&p.plan, &p.checked);
    ui.label(RichText::new(text::summary(t.units, t.files, t.bytes, t.dirs)).strong());
    weak(ui, text::counts(p.plan.already_tidy, p.plan.ignored));
    ui.horizontal(|ui| {
        // FU2b enables it (execution, report, undo).
        ui.add_enabled(false, Button::new(text::APPLY));
        if ui.button(text::CANCEL).clicked() {
            actions.push(Action::Back);
        }
    });
    weak(ui, text::APPLY_LATER);
    if let Some(note) = &p.note {
        ui.colored_label(INFO, note.as_str());
    }
    ui.separator();
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if p.plan.units.is_empty() && p.bios.is_empty() {
                ui.label(text::NOTHING_TO_DO);
            }
            bios_section(ui, p, actions);
            bulk_section(ui, p, actions);
            for (group, indices) in super::groups(&p.plan) {
                let title = match &group {
                    Group::System(_, id) => {
                        text::group_system(&text::system_name(id), id, indices.len())
                    }
                    Group::Choose => text::group_count(text::GROUP_CHOOSE, indices.len()),
                    Group::Left => text::group_count(text::GROUP_LEFT, indices.len()),
                };
                ui.add_space(6.0);
                ui.label(RichText::new(title).strong());
                for i in indices {
                    unit_row(ui, p, i, actions);
                }
            }
        });
}

fn bios_section(ui: &mut Ui, p: &Preview, actions: &mut Vec<Action>) {
    if p.bios.is_empty() {
        return;
    }
    ui.label(RichText::new(text::BIOS_TITLE).strong());
    for b in &p.bios {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(INFO, text::bios_found(&b.path, &b.names));
            if ui.small_button(text::BIOS_IMPORT).clicked() {
                actions.push(Action::ImportBios(b.absolute.clone()));
            }
        });
    }
    weak(ui, crate::cores_ui::text::NEVER_DOWNLOADED);
    ui.separator();
}

fn bulk_section(ui: &mut Ui, p: &Preview, actions: &mut Vec<Action>) {
    let choices = super::bulk_choices(&p.plan);
    if choices.is_empty() {
        return;
    }
    ui.label(RichText::new(text::BULK_TITLE).strong());
    for b in choices {
        ui.horizontal_wrapped(|ui| {
            ui.label(text::bulk_line(&b.extension, b.count));
            for s in &b.systems {
                if ui.button(text::system_name(s)).clicked() {
                    actions.push(Action::Bulk(b.extension.clone(), s.clone()));
                }
            }
        });
    }
    ui.separator();
}

fn unit_row(ui: &mut Ui, p: &Preview, index: usize, actions: &mut Vec<Action>) {
    let u = &p.plan.units[index];
    let kind = row_kind(u);
    let title = text::path(std::path::Path::new(&u.title));
    ui.horizontal_wrapped(|ui| {
        if selectable(u) {
            let mut on = p.checked.contains(&u.id);
            if ui.checkbox(&mut on, title).changed() {
                actions.push(Action::Check(u.id, on));
            }
        } else {
            ui.label(title);
        }
        let first = u.whole_dir.as_ref().or(u.moves.first());
        if let (Some(m), true) = (first, u.system.is_some() && kind != RowKind::Left) {
            let to = if u.whole_dir.is_some() || u.moves.len() == 1 {
                m.to.clone()
            } else {
                m.to.parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default()
            };
            weak(ui, text::destination(&to));
        }
        match (&u.status, kind) {
            (emu_library::TidyStatus::Sure(r), _) => {
                weak(ui, format!("{} · {}", text::SURE, text::reason(*r)))
            }
            (emu_library::TidyStatus::Suggested(r), _) => {
                ui.colored_label(INFO, format!("{} · {}", text::SUGGESTED, text::reason(*r)));
            }
            (_, RowKind::Resolved) => {
                ui.colored_label(
                    INFO,
                    text::resolved(u.system.as_deref().unwrap_or_default()),
                );
            }
            (_, RowKind::Choose) => {
                ui.colored_label(INFO, text::AMBIGUOUS);
            }
            (emu_library::TidyStatus::Blocked(b), _) => {
                ui.colored_label(WARNING, text::block(b));
            }
            _ => weak(ui, text::UNKNOWN),
        }
    });
    if p.is_bios_unit(u.id) {
        ui.colored_label(INFO, RichText::new(text::BIOS_UNIT).small());
    }
    if let Some((from, to)) = &u.system_change {
        ui.colored_label(ERROR, RichText::new(text::system_change(from, to)).small());
    }
    let known_other = u.known_game.is_some() && !u.candidates.is_empty() && kind != RowKind::Left;
    let show_candidates = matches!(kind, RowKind::Choose | RowKind::Resolved)
        || (known_other && p.other_open.contains(&u.id));
    if !show_candidates && !known_other {
        return;
    }
    // A known game sent elsewhere can come back to the system of its row.
    let mut choices: Vec<&String> = u.system_change.iter().map(|(row, _)| row).collect();
    for c in &u.candidates {
        if !choices.contains(&c) {
            choices.push(c);
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.add_space(24.0);
        if show_candidates {
            for c in choices {
                let on = u.system.as_deref() == Some(c.as_str());
                if ui.selectable_label(on, text::system_name(c)).clicked() && !on {
                    actions.push(Action::Choose(u.id, c.clone()));
                }
            }
        }
        if known_other && ui.small_button(text::OTHER_SYSTEM).clicked() {
            actions.push(Action::ToggleOther(u.id));
        }
    });
}
