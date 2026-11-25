//! Drawing of « Ranger les ROMs… »: every action is a button or a check box (keyboard and, later, gamepad focus
//! navigation; nothing needs hovering). Changes are collected during the frame and applied afterwards.

use std::path::PathBuf;

use egui::{Button, Key, RichText, ScrollArea, Ui};

use super::exec::{self, Job, Outcome, RunKind};
use super::{Choice, Group, Preview, RowKind, Stage, UndoAsk, row_kind, selectable, text};
use crate::library::{ERROR, Level, LibraryUi, Page, WARNING, notify, weak};

const INFO: egui::Color32 = crate::theme::ACCENT;

enum Action {
    Back,
    Stop,
    Retry,
    Check(u32, bool),
    Choose(u32, String),
    Bulk(String, String),
    ToggleOther(u32),
    ImportBios(PathBuf),
    Apply,
    AskUndo(UndoAsk),
    ConfirmUndo,
    CancelUndo,
    StopRun,
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

    /// Window closed during « Ranger » or its undo (rv-fu2b m11): a run stops after the game being moved, an undo
    /// is let finish (stopping it would make it final); both within a bound, the journal covers the rest at the
    /// next start. The window is already gone: only the process lingers.
    pub(crate) fn finish_tidy_before_exit(&mut self) {
        const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
        if let Stage::Running { task, undo, .. } = &mut self.tidy.stage {
            if !*undo {
                task.cancel();
            }
            if !task.wait_end(EXIT_WAIT) {
                tracing::warn!("{}", text::EXIT_UNFINISHED);
            }
            self.tidy.stage = Stage::Idle;
        }
    }

    /// « Ranger » / « Annuler le dernier rangement »: same refusals as the analysis, then the worker.
    fn start_run(&mut self, job: Job, ctx: &egui::Context) {
        self.tidy.confirm_undo = None;
        if self.tidy.busy() {
            return;
        }
        if let Some(msg) = super::refusal(self.tidy_busy_state()) {
            self.set_status(msg.to_owned(), Level::Warn);
            return;
        }
        let undo = matches!(job, Job::Undo { .. });
        let task = exec::spawn_run(self.dirs.clone(), job, notify(ctx));
        self.tidy.stage = Stage::Running {
            task,
            progress: None,
            undo,
        };
        self.tidy.focus_first = true;
    }

    fn leave_tidy(&mut self) {
        self.tidy.close();
        self.set_page(Page::Settings);
    }

    pub(in crate::library) fn tidy_page(&mut self, ui: &mut Ui) {
        let was_running = matches!(self.tidy.stage, Stage::Running { .. });
        self.tidy.poll();
        if was_running && matches!(self.tidy.stage, Stage::Report(_)) {
            // Rows were relocated by the run: reload them, then rescan so that the library matches the disk.
            self.reload_games();
            self.start_scan(ui.ctx());
        }
        let running = matches!(self.tidy.stage, Stage::Running { .. });
        if let Stage::Preview(p) = &mut self.tidy.stage
            && let Some(Err(e)) = p.poll_choice(notify(ui.ctx()))
        {
            self.set_status(e, Level::Warn);
        }
        let escape = !running
            && self.dialog.is_none()
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
                let back = ui.add_enabled(!running, Button::new(text::BACK));
                if focus_first && !running {
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
                    super::copyable::message(ui, WARNING, e);
                    if ui.button(text::RETRY).clicked() {
                        actions.push(Action::Retry);
                    }
                }
                Stage::Preview(p) => preview(ui, p, &mut self.tidy.confirm_undo, &mut actions),
                Stage::Running {
                    progress,
                    undo,
                    task,
                } => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        let (done, total) = progress
                            .as_ref()
                            .map_or((0, 0), |p| (p.done_units, p.total_units));
                        ui.label(text::running(*undo, done, total));
                    });
                    if let Some(current) = progress.as_ref().and_then(|p| p.current.as_deref()) {
                        weak(ui, text::path(current));
                    }
                    // An undo is never stopped: a stopped undo is final (design v7.1 §3.4.1). The stop button of
                    // a run never gets the focus (a held Enter would press it).
                    if !*undo
                        && ui
                            .add_enabled(!task.is_cancelled(), Button::new(text::STOP_RUN))
                            .clicked()
                    {
                        actions.push(Action::StopRun);
                    }
                }
                Stage::Report(outcome) => report(
                    ui,
                    outcome,
                    focus_first,
                    &mut self.tidy.confirm_undo,
                    &mut actions,
                ),
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
                    // Refused (scan, download…): the report stays on screen.
                    if let Some(msg) = super::refusal(self.tidy_busy_state()) {
                        self.set_status(msg.to_owned(), Level::Warn);
                    } else {
                        let ctx = ui.ctx().clone();
                        self.tidy.close();
                        self.start_tidy(&ctx);
                    }
                }
                Action::Check(id, on) => {
                    if let Stage::Preview(p) = &mut self.tidy.stage {
                        p.set_checked(id, on);
                    }
                }
                Action::Choose(unit, system) => {
                    if let Stage::Preview(p) = &mut self.tidy.stage {
                        p.request(Choice::Unit { unit, system }, notify(ui.ctx()));
                    }
                }
                Action::Bulk(extension, system) => {
                    if let Stage::Preview(p) = &mut self.tidy.stage {
                        p.request(Choice::Extension { extension, system }, notify(ui.ctx()));
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
                Action::Apply => {
                    if let Stage::Preview(p) = &self.tidy.stage
                        && p.can_apply()
                    {
                        let job = Job::Apply {
                            plan: p.plan.clone(),
                            selected: p.checked.iter().copied().collect(),
                        };
                        let ctx = ui.ctx().clone();
                        self.start_run(job, &ctx);
                    }
                }
                Action::AskUndo(ask) => self.tidy.confirm_undo = Some(ask),
                Action::CancelUndo => self.tidy.confirm_undo = None,
                Action::ConfirmUndo => {
                    if self.tidy.may_undo()
                        && let Some(ask) = self.tidy.confirm_undo.take()
                    {
                        let ctx = ui.ctx().clone();
                        self.start_run(
                            Job::Undo {
                                journal: ask.journal,
                            },
                            &ctx,
                        );
                    }
                }
                Action::StopRun => {
                    if let Stage::Running { task, .. } = &self.tidy.stage {
                        task.cancel();
                    }
                }
            }
        }
    }
}

fn preview(ui: &mut Ui, p: &mut Preview, confirm: &mut Option<UndoAsk>, actions: &mut Vec<Action>) {
    weak(ui, text::INTRO);
    ui.colored_label(WARNING, RichText::new(text::SECOND_INSTANCE).small());
    let t = super::totals(&p.plan, &p.checked);
    ui.label(RichText::new(text::summary(t.units, t.files, t.bytes, t.dirs)).strong());
    weak(ui, text::counts(p.plan.already_tidy, p.plan.ignored));
    let updating = p.updating();
    ui.horizontal(|ui| {
        // Disabled while a choice is pending: the plan may still change.
        if ui
            .add_enabled(p.can_apply(), Button::new(text::apply(p.checked.len())))
            .clicked()
        {
            actions.push(Action::Apply);
        }
        if ui.button(text::CLOSE_WITHOUT_TIDY).clicked() {
            actions.push(Action::Back);
        }
    });
    undo_section(ui, &p.last, confirm, actions);
    if updating > 0 {
        weak(ui, text::APPLY_UPDATING);
    }
    last_tidy_lines(ui, &p.last);
    if updating > 0 {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(text::updating(updating));
        });
    }
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
    weak(ui, text::NEVER_DOWNLOADED);
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

/// « Annuler le dernier rangement… » on its own line, then its confirmation (date and number of files).
fn undo_section(
    ui: &mut Ui,
    last: &exec::LastTidy,
    confirm: &mut Option<UndoAsk>,
    actions: &mut Vec<Action>,
) {
    let (Some(_), Some(info)) = (last.undo_target(), last.info.as_ref()) else {
        return;
    };
    match confirm {
        Some(ask) if ask.journal == info.path => {
            ui.group(|ui| {
                ui.colored_label(WARNING, text::undo_confirm(ask.moved, ask.unix));
                ui.horizontal(|ui| {
                    let no = ui.button(text::UNDO_CONFIRM_NO);
                    if std::mem::take(&mut ask.focus) {
                        no.request_focus();
                    }
                    if no.clicked() {
                        actions.push(Action::CancelUndo);
                    }
                    if ui.button(text::UNDO_CONFIRM_YES).clicked() {
                        actions.push(Action::ConfirmUndo);
                    }
                });
            });
        }
        _ => {
            if ui.button(text::UNDO).clicked() {
                actions.push(Action::AskUndo(UndoAsk {
                    journal: info.path.clone(),
                    moved: info.moved,
                    unix: info.created_unix,
                    focus: true,
                }));
            }
        }
    }
}

fn last_tidy_lines(ui: &mut Ui, last: &exec::LastTidy) {
    if let Some(e) = &last.error {
        super::copyable::message(ui, WARNING, e);
    }
    if let Some(info) = last.info.as_ref().filter(|i| i.can_undo()) {
        weak(ui, text::last_tidy(info.moved, info.created_unix));
    }
}

fn report(
    ui: &mut Ui,
    out: &Outcome,
    focus_first: bool,
    confirm: &mut Option<UndoAsk>,
    actions: &mut Vec<Action>,
) {
    let title = match out.kind {
        RunKind::Tidy => text::REPORT_TIDY,
        RunKind::Undo => text::REPORT_UNDO,
        RunKind::Recovery => text::REPORT_RECOVERY,
    };
    ui.label(RichText::new(title).strong());
    ui.horizontal(|ui| {
        let again = ui.button(text::NEW_ANALYSIS);
        if focus_first {
            again.request_focus();
        }
        if again.clicked() {
            actions.push(Action::Retry);
        }
    });
    undo_section(ui, &out.last, confirm, actions);
    last_tidy_lines(ui, &out.last);
    for m in &out.messages {
        super::copyable::message(ui, WARNING, m);
    }
    let shown = |p: &std::path::Path| exec::shown_path(out, p);
    let sel = |ui: &mut Ui, color: egui::Color32, s: String| {
        ui.add(egui::Label::new(RichText::new(s).color(color)).selectable(true));
    };
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(r) = &out.report {
                let lines = exec::report_lines(out.kind, r);
                ui.add(egui::Label::new(lines.headline).selectable(true));
                if let Some(c) = lines.cancelled {
                    sel(ui, INFO, c.to_owned());
                }
                if !r.left.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(lines.left_title).strong());
                    for (p, b) in &r.left {
                        sel(ui, WARNING, text::left(&shown(p), b));
                    }
                }
                if !r.extra_names.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(text::EXTRA_NAMES_TITLE).strong());
                    for line in exec::extra_name_lines(out, r) {
                        sel(ui, WARNING, line);
                    }
                }
                if !r.errors.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(text::ERRORS_TITLE).strong());
                    for (p, m) in &r.errors {
                        sel(ui, ERROR, exec::error_entry(out, p, m));
                    }
                }
            }
            if !out.skipped.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(text::SKIPPED_TITLE).strong());
                for (p, m) in &out.skipped {
                    sel(ui, WARNING, text::error_line(&shown(p), m));
                }
            }
            ui.add_space(6.0);
            super::copyable::copy_button_labelled(
                ui,
                text::COPY_REPORT,
                &exec::report_text(out),
            );
        });
}