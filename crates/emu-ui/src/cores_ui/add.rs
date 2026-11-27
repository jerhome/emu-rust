//! « Ajouter un système non testé… » (ADR 0025 §9, design 8.0 §1): the list of the cores libretro offers for
//! machines emu-rust has not tested, the sheet of one core, the acceptance of unexamined content (§7), the install
//! of the core (the existing manager), then the probe in a child process. The definitions are built here and
//! handed to the host as an [`AddEvent`]: it writes them, publishes the catalog, creates `roms/<id>/`, scans.
//! Nothing here blocks the interface thread: the install and the probe are jobs, polled once per frame.

use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{Button, RichText, ScrollArea, TextEdit, Ui};
use emu_assets::cores::{CoreId, CoreInfo, CorePreferences};
use emu_assets::jobs::{Job, JobEvent};
use emu_library::CustomSystemDraft;
use emu_runtime::DryRunReport;

use super::{CoresUi, InstallState, RowAction, text};
use crate::custom::{
    AddRow, AddWarning, Addability, Machine, MachineState, ProbeFailure, add_rows, drafts_for,
    probe_core, probe_outcome,
};
use crate::dry_run::DryRunCommand;
use crate::ui::text::sanitize_core_text;

/// What the add flow asks of its host.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AddEvent {
    /// The user accepted the unexamined content of this core: `[custom_systems] content_accepted` to write.
    ContentAccepted(CoreId),
    /// The definitions to write (one per machine), the core installed and probed.
    Drafts(Vec<CustomSystemDraft>),
}

type ProbeJob = Job<Result<DryRunReport, ProbeFailure>>;

enum Stage {
    Closed,
    List,
    /// One core: `checked` follows `AddRow::machines`.
    Sheet {
        core: CoreId,
        checked: Vec<bool>,
    },
    Accept {
        core: CoreId,
        machines: Vec<Machine>,
    },
    Installing {
        core: CoreId,
        machines: Vec<Machine>,
    },
    Probing {
        core: CoreId,
        machines: Vec<Machine>,
        job: ProbeJob,
    },
}

pub(super) struct AddUi {
    stage: Stage,
    show_all: bool,
    search: String,
    focus: bool,
    error: Option<String>,
    /// `content_accepted` of the configuration, kept in step by the host.
    accepted: BTreeSet<String>,
    events: Vec<AddEvent>,
    /// The probe command: this executable with the hidden child argument; tests give their own.
    command: Option<DryRunCommand>,
    /// `<data>/cache`: where the probe's folder is made. Without it the probe is not run.
    cache_dir: Option<PathBuf>,
}

impl Default for AddUi {
    fn default() -> Self {
        Self {
            stage: Stage::Closed,
            show_all: false,
            search: String::new(),
            focus: false,
            error: None,
            accepted: BTreeSet::new(),
            events: Vec::new(),
            command: None,
            cache_dir: None,
        }
    }
}

/// The first available machine is ticked, the others are not.
fn default_checked(machines: &[Machine]) -> Vec<bool> {
    let mut first = true;
    machines
        .iter()
        .map(|m| {
            let on = first && m.state == MachineState::Available;
            first &= !on;
            on
        })
        .collect()
}

/// A core `cores.toml` does not list, and that the user has not accepted yet, asks for the acceptance first
/// (ADR 0025 §7).
fn needs_accept(prefs: &CorePreferences, accepted: &BTreeSet<String>, core: &CoreId) -> bool {
    !prefs.is_examined(core) && !accepted.contains(core.as_str())
}

/// The machines of `row` ticked and still available.
fn chosen(machines: &[Machine], checked: &[bool]) -> Vec<Machine> {
    machines
        .iter()
        .zip(checked)
        .filter(|(m, on)| **on && m.state == MachineState::Available)
        .map(|(m, _)| m.clone())
        .collect()
}

impl CoresUi {
    pub(crate) fn open_add(&mut self) {
        self.add.stage = Stage::List;
        self.add.focus = true;
        self.add.error = None;
    }

    pub(crate) fn add_open(&self) -> bool {
        !matches!(self.add.stage, Stage::Closed)
    }

    pub(crate) fn set_content_accepted(&mut self, accepted: BTreeSet<String>) {
        self.add.accepted = accepted;
    }

    pub(crate) fn set_cache_dir(&mut self, dir: PathBuf) {
        self.add.cache_dir = Some(dir);
    }

    #[cfg(all(test, unix))]
    pub(crate) fn set_probe_command(&mut self, command: DryRunCommand) {
        self.add.command = Some(command);
    }

    pub(crate) fn take_add_events(&mut self) -> Vec<AddEvent> {
        std::mem::take(&mut self.add.events)
    }

    /// Rows of the list with the screen's filter, or (`all`) every row of every core for the sheet.
    fn add_rows_now(&self, all: bool) -> Vec<AddRow> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let installed: BTreeSet<&CoreId> = snapshot.installed.iter().map(|c| &c.id).collect();
        let downloadable = |id: &CoreId| {
            installed.contains(id)
                || snapshot
                    .catalog
                    .as_ref()
                    .is_some_and(|c| c.cores.contains_key(id))
        };
        add_rows(
            &snapshot.infos,
            &downloadable,
            emu_library::catalog(),
            &self.prefs,
            self.add.show_all || all,
            if all { "" } else { &self.add.search },
        )
    }

    fn core_installed(&self, core: &CoreId) -> bool {
        self.manager
            .as_ref()
            .is_some_and(|m| m.installed().iter().any(|c| &c.id == core))
    }

    /// Once per frame (from `poll`): the install in progress, the probe in progress.
    pub(super) fn drive_add(&mut self, ctx: &egui::Context) {
        let stage = std::mem::replace(&mut self.add.stage, Stage::Closed);
        self.add.stage = match stage {
            Stage::Installing { core, machines } => self.drive_installing(core, machines, ctx),
            Stage::Probing {
                core,
                machines,
                job,
            } => self.drive_probing(core, machines, job),
            other => other,
        };
    }

    fn back_to_list(&mut self, error: Option<String>) -> Stage {
        self.add.error = error;
        self.add.focus = true;
        Stage::List
    }

    fn drive_installing(
        &mut self,
        core: CoreId,
        machines: Vec<Machine>,
        ctx: &egui::Context,
    ) -> Stage {
        match self.install_state(&core) {
            InstallState::Queued | InstallState::Running(_) => Stage::Installing { core, machines },
            InstallState::Failed(why) => self.back_to_list(Some(text::add_install_failed(&why))),
            InstallState::Idle if self.core_installed(&core) => {
                self.start_probe(core, machines, ctx)
            }
            // Cancelled by the user, or refused without a message.
            InstallState::Idle => self.back_to_list(None),
        }
    }

    fn start_probe(&mut self, core: CoreId, machines: Vec<Machine>, ctx: &egui::Context) -> Stage {
        let Some(manager) = self.manager.clone() else {
            return self.back_to_list(None);
        };
        let cache = self.add.cache_dir.clone();
        let command = self.add.command.clone();
        let system_dir = self.system_dir.clone();
        let target = core.clone();
        let job = Job::spawn(
            &format!("probe-{core}"),
            Self::notify(ctx),
            move |_, cancel| {
                let path = manager
                    .verified_path(&target)
                    .map_err(|e| ProbeFailure::Failed(text::error(&e)))?;
                let Some(cache) = cache else {
                    return Err(ProbeFailure::NotRun(text::ADD_NO_CACHE.to_owned()));
                };
                let command = match command {
                    Some(command) => command,
                    None => DryRunCommand::current_exe()
                        .map_err(|e| ProbeFailure::NotRun(e.to_string()))?,
                };
                probe_outcome(probe_core(&command, &cache, &system_dir, &path, &|| {
                    cancel.is_cancelled()
                }))
            },
        );
        self.add.focus = true;
        Stage::Probing {
            core,
            machines,
            job,
        }
    }

    fn drive_probing(&mut self, core: CoreId, machines: Vec<Machine>, mut job: ProbeJob) -> Stage {
        let mut result = None;
        for event in job.poll() {
            if let JobEvent::Done(done) = event {
                result = Some(done);
            }
        }
        if result.is_none() && job.is_finished() {
            result = Some(Err(ProbeFailure::Failed(
                text::ADD_TASK_INTERRUPTED.to_owned(),
            )));
        }
        let Some(result) = result else {
            return Stage::Probing {
                core,
                machines,
                job,
            };
        };
        let report = match result {
            Ok(report) => Some(report),
            // The child could not even start: the `.info` alone is used, the core is not at fault.
            Err(ProbeFailure::NotRun(why)) => {
                tracing::warn!("sonde du core {core} non lancée : {why}");
                None
            }
            Err(ProbeFailure::HardwareRender) => {
                return self.back_to_list(Some(text::hardware_render(core.as_str())));
            }
            Err(ProbeFailure::Failed(why)) => {
                return self.back_to_list(Some(text::add_failed_probe(&why)));
            }
            Err(ProbeFailure::Cancelled) => return self.back_to_list(None),
        };
        let Some(info) = self.manager.as_ref().and_then(|m| m.info(&core)) else {
            return self.back_to_list(Some(text::add_failed_probe(text::ADD_NO_SHEET)));
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let drafts = drafts_for(
            &info,
            &machines,
            report.as_ref(),
            emu_library::catalog(),
            now,
            &format!("emu-rust {}", crate::VERSION),
        );
        self.add.events.push(AddEvent::Drafts(drafts));
        Stage::Closed
    }

    /// The add view, in place of the core list. `escape`: Échap went unused by a dialog.
    pub(super) fn add_view(&mut self, ui: &mut Ui, escape: bool) {
        let ctx = ui.ctx().clone();
        let stage = std::mem::replace(&mut self.add.stage, Stage::Closed);
        self.add.stage = match stage {
            Stage::Closed => Stage::Closed,
            Stage::List => self.add_list(ui, escape),
            Stage::Sheet { core, checked } => self.add_sheet(ui, escape, core, checked, &ctx),
            Stage::Accept { core, machines } => self.add_accept(ui, escape, core, machines, &ctx),
            Stage::Installing { core, machines } => self.add_installing(ui, escape, core, machines),
            Stage::Probing {
                core,
                machines,
                job,
            } => self.add_probing(ui, escape, core, machines, job),
        };
    }

    fn add_list(&mut self, ui: &mut Ui, escape: bool) -> Stage {
        ui.heading(text::ADD_TITLE);
        ui.label(RichText::new(text::ADD_INTRO).small().weak());
        if let Some(error) = &self.add.error {
            crate::copyable::message(ui, super::ERROR, error);
        }
        let mut next = None;
        ui.horizontal_wrapped(|ui| {
            let close = ui.button(text::ADD_CLOSE);
            if std::mem::take(&mut self.add.focus) {
                close.request_focus();
            }
            if close.clicked() {
                next = Some(Stage::Closed);
            }
            ui.checkbox(&mut self.add.show_all, text::ADD_SHOW_ALL);
            ui.label(text::SEARCH);
            ui.add(TextEdit::singleline(&mut self.add.search).desired_width(220.0));
        });
        ui.separator();
        let rows = self.add_rows_now(false);
        if self.snapshot.as_ref().is_none_or(|s| s.infos.is_empty()) {
            ui.label(text::ADD_NO_INFO);
        } else if rows.is_empty() {
            ui.label(text::ADD_NO_MATCH);
        }
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &rows {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(sanitize_core_text(&row.name)).strong());
                            let names: Vec<&str> =
                                row.machines.iter().map(|m| m.name.as_str()).collect();
                            ui.label(
                                RichText::new(sanitize_core_text(&names.join(", ")))
                                    .small()
                                    .weak(),
                            );
                            ui.label(
                                RichText::new(text::add_row_line(row.license.as_deref()))
                                    .small()
                                    .weak(),
                            );
                        });
                        ui.horizontal_wrapped(|ui| {
                            let (label, ok) = match &row.verdict {
                                Addability::Ok { .. } => (None, true),
                                Addability::HardwareRender => (Some(text::ADD_HARDWARE), false),
                                Addability::DiscOnly => (Some(text::ADD_DISC_ONLY), false),
                                Addability::Arcade => (Some(text::ADD_ARCADE), false),
                                _ => (None, false),
                            };
                            if let Some(label) = label {
                                ui.colored_label(super::WARNING, label);
                            }
                            let any = row
                                .machines
                                .iter()
                                .any(|m| m.state == MachineState::Available);
                            if ui
                                .add_enabled(ok && any, Button::new(text::ADD_CHOOSE))
                                .clicked()
                            {
                                next = Some(Stage::Sheet {
                                    core: row.core.clone(),
                                    checked: default_checked(&row.machines),
                                });
                            }
                        });
                    });
                }
            });
        if escape && next.is_none() {
            next = Some(Stage::Closed);
        }
        match next {
            Some(Stage::Sheet { core, checked }) => {
                self.add.focus = true;
                self.add.error = None;
                Stage::Sheet { core, checked }
            }
            Some(stage) => stage,
            None => Stage::List,
        }
    }

    fn add_sheet(
        &mut self,
        ui: &mut Ui,
        escape: bool,
        core: CoreId,
        mut checked: Vec<bool>,
        ctx: &egui::Context,
    ) -> Stage {
        let Some(row) = self.add_rows_now(true).into_iter().find(|r| r.core == core) else {
            return Stage::List;
        };
        let info: Option<CoreInfo> = self.manager.as_ref().and_then(|m| m.info(&core));
        ui.heading(sanitize_core_text(&row.name));
        if let Some(license) = &row.license {
            ui.label(format!(
                "{} : {}",
                text::ADD_LICENSE,
                sanitize_core_text(license)
            ));
        }
        if let Addability::Ok { warnings } = &row.verdict {
            for warning in warnings {
                ui.colored_label(
                    super::WARNING,
                    match warning {
                        AddWarning::Experimental => text::WARN_EXPERIMENTAL,
                        AddWarning::KeyboardNeeded => text::WARN_KEYBOARD,
                        AddWarning::NoSavestates => text::WARN_NO_SAVESTATES,
                        AddWarning::UnverifiedContent => text::WARN_UNVERIFIED,
                    },
                );
            }
        }
        ui.label(RichText::new(text::ADD_MACHINES).strong());
        let mut next = None;
        for (i, machine) in row.machines.iter().enumerate() {
            let note = match machine.state {
                MachineState::Available => None,
                MachineState::AlreadySupported => Some(text::MACHINE_SUPPORTED),
                MachineState::AlreadyAdded => Some(text::MACHINE_ADDED),
                MachineState::DiscPlanned => Some(text::MACHINE_DISC),
                MachineState::ArcadePlanned => Some(text::MACHINE_ARCADE),
            };
            ui.horizontal_wrapped(|ui| {
                let enabled = machine.state == MachineState::Available;
                let mut on = checked.get(i).copied().unwrap_or(false) && enabled;
                let first = ui.add_enabled(
                    enabled,
                    egui::Checkbox::new(&mut on, sanitize_core_text(&machine.name)),
                );
                if i == 0 && std::mem::take(&mut self.add.focus) {
                    first.request_focus();
                }
                if let Some(slot) = checked.get_mut(i) {
                    *slot = on;
                }
                if let Some(note) = note {
                    ui.label(RichText::new(note).small().weak());
                }
            });
        }
        if let Some(info) = &info
            && !info.firmware.is_empty()
        {
            ui.label(RichText::new(text::ADD_FIRMWARE).strong());
            for file in &info.firmware {
                let kind = if file.optional {
                    text::ADD_OPTIONAL
                } else {
                    text::ADD_REQUIRED
                };
                ui.label(format!("{} ({kind})", sanitize_core_text(&file.path)));
            }
            ui.label(RichText::new(text::ADD_FIRMWARE_NOTE).small().weak());
        }
        let picked = chosen(&row.machines, &checked);
        ui.horizontal(|ui| {
            if ui.button(text::ADD_BACK).clicked() {
                next = Some(Stage::List);
            }
            if ui
                .add_enabled(!picked.is_empty(), Button::new(text::ADD_CONFIRM))
                .clicked()
            {
                next = Some(if needs_accept(&self.prefs, &self.add.accepted, &core) {
                    Stage::Accept {
                        core: core.clone(),
                        machines: picked.clone(),
                    }
                } else {
                    self.begin_install(core.clone(), picked.clone(), ctx)
                });
            }
        });
        if escape && next.is_none() {
            next = Some(Stage::List);
        }
        match next {
            Some(Stage::Accept { core, machines }) => {
                self.add.focus = true;
                Stage::Accept { core, machines }
            }
            Some(Stage::List) => self.back_to_list(None),
            Some(stage) => stage,
            None => Stage::Sheet { core, checked },
        }
    }

    /// Installs the core when it is not there, then probes it.
    fn begin_install(
        &mut self,
        core: CoreId,
        machines: Vec<Machine>,
        ctx: &egui::Context,
    ) -> Stage {
        self.add.error = None;
        self.add.focus = true;
        if self.core_installed(&core) {
            return self.start_probe(core, machines, ctx);
        }
        self.request_install(core.clone(), ctx);
        Stage::Installing { core, machines }
    }

    fn add_accept(
        &mut self,
        ui: &mut Ui,
        escape: bool,
        core: CoreId,
        machines: Vec<Machine>,
        ctx: &egui::Context,
    ) -> Stage {
        ui.heading(text::ACCEPT_TITLE);
        ui.label(text::accept_body(core.as_str()));
        let mut next = None;
        ui.horizontal(|ui| {
            // The safe choice first.
            let cancel = ui.button(text::ADD_CANCEL);
            if std::mem::take(&mut self.add.focus) {
                cancel.request_focus();
            }
            if cancel.clicked() {
                next = Some(false);
            }
            if ui.button(text::ACCEPT_YES).clicked() {
                next = Some(true);
            }
        });
        if escape && next.is_none() {
            next = Some(false);
        }
        match next {
            Some(true) => {
                self.add.accepted.insert(core.to_string());
                self.add
                    .events
                    .push(AddEvent::ContentAccepted(core.clone()));
                self.begin_install(core, machines, ctx)
            }
            Some(false) => self.back_to_list(None),
            None => Stage::Accept { core, machines },
        }
    }

    fn add_installing(
        &mut self,
        ui: &mut Ui,
        escape: bool,
        core: CoreId,
        machines: Vec<Machine>,
    ) -> Stage {
        ui.heading(text::ADD_TITLE);
        ui.horizontal(|ui| {
            match self.install_state(&core) {
                InstallState::Queued => {
                    ui.spinner();
                    ui.label(text::ADD_WAITING_CATALOG);
                }
                InstallState::Running(Some((done, total))) => {
                    ui.label(text::ADD_INSTALLING);
                    ui.label(text::progress(done, total));
                }
                _ => {
                    ui.spinner();
                    ui.label(text::ADD_INSTALLING);
                }
            };
        });
        let cancel = ui.button(text::ADD_CANCEL);
        if std::mem::take(&mut self.add.focus) {
            cancel.request_focus();
        }
        if cancel.clicked() || escape {
            let ctx = ui.ctx().clone();
            self.queued.retain(|c| c != &core);
            self.apply(RowAction::CancelInstall(core), &ctx);
            return self.back_to_list(None);
        }
        Stage::Installing { core, machines }
    }

    fn add_probing(
        &mut self,
        ui: &mut Ui,
        escape: bool,
        core: CoreId,
        machines: Vec<Machine>,
        job: ProbeJob,
    ) -> Stage {
        ui.heading(text::ADD_TITLE);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(text::ADD_PROBING);
        });
        let cancel = ui.button(text::ADD_CANCEL);
        if std::mem::take(&mut self.add.focus) {
            cancel.request_focus();
        }
        if cancel.clicked() || escape {
            job.cancel();
            return self.back_to_list(None);
        }
        Stage::Probing {
            core,
            machines,
            job,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(name: &str, state: MachineState) -> Machine {
        Machine {
            database: Some(name.to_owned()),
            name: name.to_owned(),
            state,
        }
    }

    #[test]
    fn the_first_available_machine_is_ticked() {
        let machines = [
            machine("Disc", MachineState::DiscPlanned),
            machine("A", MachineState::Available),
            machine("B", MachineState::Available),
        ];
        assert_eq!(default_checked(&machines), [false, true, false]);
        assert_eq!(chosen(&machines, &[true, true, true]).len(), 2);
        assert!(chosen(&machines, &[true, false, false]).is_empty());
    }

    #[test]
    fn only_a_core_outside_cores_toml_asks_for_the_acceptance_once() {
        let prefs = CorePreferences::builtin();
        let listed = prefs
            .systems()
            .flat_map(|(_, cores)| cores.iter())
            .next()
            .unwrap()
            .clone();
        let unknown = CoreId::new("fictional").unwrap();
        let mut accepted = BTreeSet::new();
        assert!(!needs_accept(&prefs, &accepted, &listed));
        assert!(needs_accept(&prefs, &accepted, &unknown));
        accepted.insert("fictional".to_owned());
        assert!(!needs_accept(&prefs, &accepted, &unknown));
    }

    #[cfg(unix)]
    mod with_shell {
        use std::fs;

        use emu_assets::cores::Platform;

        use super::*;
        use crate::Config;
        use crate::config::DataDirs;

        /// A data root with one core installed by hand (`fictional`) and its `.info`: no network.
        fn fixture() -> Option<(tempfile::TempDir, DataDirs)> {
            let platform = Platform::current()?;
            let tmp = tempfile::tempdir().unwrap();
            let dirs = DataDirs::under(tmp.path());
            dirs.create_all().unwrap();
            fs::write(
                dirs.cores.join(format!(
                    "fictional_libretro.{}",
                    platform.library_extension()
                )),
                b"x",
            )
            .unwrap();
            let info = dirs.cores.join("info");
            fs::create_dir_all(&info).unwrap();
            fs::write(
                info.join("fictional_libretro.info"),
                "display_name = \"Acme - Fictional Console (Fictional)\"\ncategories = \"Emulator\"\n\
                 database = \"Acme - Fictional Console\"\nsupported_extensions = \"fic|zip\"\n\
                 hw_render = \"false\"\n",
            )
            .unwrap();
            Some((tmp, dirs))
        }

        fn run_pass(ctx: &egui::Context, ui: &mut CoresUi) {
            let mut out = ctx.run_ui(egui::RawInput::default(), |root| {
                ui.show(root);
            });
            out.textures_delta.clear();
        }

        fn shell(script: &str) -> DryRunCommand {
            DryRunCommand {
                program: PathBuf::from("sh"),
                args: ["-c", script, "child"]
                    .map(std::ffi::OsString::from)
                    .to_vec(),
                env: Vec::new(),
            }
        }

        /// Opens the add view on the fixture, starts the add of its only machine with `child` as the probe, and
        /// drives the screen until the probe ends (or gives up after a few seconds).
        fn add_with(child: &str) -> Option<(tempfile::TempDir, CoresUi, Vec<AddEvent>)> {
            let (tmp, dirs) = fixture()?;
            let mut ui = CoresUi::open(&dirs, &Config::default());
            ui.set_probe_command(shell(child));
            let ctx = egui::Context::default();
            run_pass(&ctx, &mut ui);
            ui.open_add();
            let rows = ui.add_rows_now(false);
            assert_eq!(rows.len(), 1, "{rows:?}");
            let core = rows[0].core.clone();
            let machines = chosen(&rows[0].machines, &default_checked(&rows[0].machines));
            assert_eq!(machines.len(), 1);
            ui.add.stage = ui.begin_install(core, machines, &ctx);
            let mut events = Vec::new();
            for _ in 0..500 {
                ui.drive_add(&ctx);
                events.extend(ui.take_add_events());
                if !matches!(ui.add.stage, Stage::Probing { .. }) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Some((tmp, ui, events))
        }

        #[test]
        fn a_probed_core_hands_over_a_definition_with_the_extensions_it_reports() {
            // `$1` is the request file: the result goes next to it.
            let child = r#"d="$(dirname "$1")"; printf 'outcome = "probed"\nextensions = ["fic", "new"]\n' > "$d/result.toml""#;
            let Some((_tmp, ui, events)) = add_with(child) else {
                return;
            };
            let [AddEvent::Drafts(drafts)] = events.as_slice() else {
                panic!("{events:?}");
            };
            assert_eq!(drafts.len(), 1);
            let draft = &drafts[0];
            assert_eq!(draft.database.as_deref(), Some("Acme - Fictional Console"));
            assert_eq!(
                draft.extensions,
                ["fic", "new"],
                "the probe replaces the .info"
            );
            assert_eq!(draft.cores, ["fictional"]);
            assert!(
                draft.dats.is_empty(),
                "no DAT is asked for before the user searches"
            );
            assert!(matches!(ui.add.stage, Stage::Closed));
            assert!(ui.add.error.is_none());
        }

        #[test]
        fn a_core_that_asks_for_hardware_rendering_adds_nothing() {
            let child = r#"d="$(dirname "$1")"; printf 'outcome = "hardware_render_required"\n' > "$d/result.toml""#;
            let Some((_tmp, ui, events)) = add_with(child) else {
                return;
            };
            assert!(events.is_empty(), "{events:?}");
            assert!(matches!(ui.add.stage, Stage::List));
            assert!(ui.add.error.as_deref().unwrap().contains("9.0"));
        }

        /// The view draws in every stage, with the keyboard focus set once and Échap stepping back.
        #[test]
        fn every_stage_of_the_add_view_draws_and_escape_steps_back() {
            let Some((_tmp, dirs)) = fixture() else {
                return;
            };
            let mut ui = CoresUi::open(&dirs, &Config::default());
            ui.set_probe_command(shell("sleep 5"));
            let ctx = egui::Context::default();
            run_pass(&ctx, &mut ui);
            ui.open_add();
            run_pass(&ctx, &mut ui);
            let rows = ui.add_rows_now(false);
            let core = rows[0].core.clone();
            let machines = chosen(&rows[0].machines, &default_checked(&rows[0].machines));
            ui.add.stage = Stage::Sheet {
                core: core.clone(),
                checked: default_checked(&rows[0].machines),
            };
            run_pass(&ctx, &mut ui);
            ui.add.stage = Stage::Accept {
                core: core.clone(),
                machines: machines.clone(),
            };
            run_pass(&ctx, &mut ui);
            ui.add.stage = ui.begin_install(core, machines, &ctx);
            assert!(matches!(ui.add.stage, Stage::Probing { .. }));
            run_pass(&ctx, &mut ui);
            // Échap while probing gives up: the child is killed, back to the list, nothing handed over.
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..egui::RawInput::default()
                },
                |root| {
                    ui.show(root);
                },
            );
            out.textures_delta.clear();
            assert!(matches!(ui.add.stage, Stage::List));
            assert!(ui.take_add_events().is_empty());
        }

        #[test]
        fn a_core_that_crashes_in_the_probe_adds_nothing() {
            let Some((_tmp, ui, events)) = add_with("exit 3") else {
                return;
            };
            assert!(events.is_empty(), "{events:?}");
            assert!(matches!(ui.add.stage, Stage::List));
            assert!(ui.add.error.is_some());
        }

        /// The whole path against a simulated buildbot: catalog and `.info` fetched, the core installed by the
        /// manager (nothing is loaded), probed by a shell child, the definitions handed over. Linux x86-64 only: the
        /// URLs of the buildbot are those of that platform.
        #[cfg(all(unix, target_os = "linux", target_arch = "x86_64"))]
        #[test]
        fn adding_a_core_the_buildbot_offers_installs_it_then_probes_it() {
            use std::io::{Cursor, Write};
            use std::sync::Arc;
            use std::time::Duration;

            use emu_assets::cores::{BuildbotSource, CoreDirs, CoreManager};
            use emu_assets::http::{HttpClient, HttpConfig};
            use wiremock::matchers::{method, path};
            use wiremock::{Mock, MockServer, ResponseTemplate};
            use zip::write::SimpleFileOptions;

            fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
                let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
                for (name, data) in entries {
                    writer
                        .start_file(*name, SimpleFileOptions::default())
                        .unwrap();
                    writer.write_all(data).unwrap();
                }
                writer.finish().unwrap().into_inner()
            }

            let core_bytes: &[u8] = b"not a real core: the probe is a shell script";
            let core_zip = zip_of(&[("fictional_libretro.so", core_bytes)]);
            let crc = zip::ZipArchive::new(Cursor::new(core_zip.clone()))
                .unwrap()
                .by_index(0)
                .unwrap()
                .crc32();
            let info = "display_name = \"Acme - Fictional Console (Fictional)\"\ncategories = \"Emulator\"\n\
                        database = \"Acme - Fictional Console\"\nsupported_extensions = \"fic|zip\"\nlicense = \"MIT\"\n";
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap();
            let server = rt.block_on(MockServer::start());
            let serve = |at: &str, response: ResponseTemplate| {
                rt.block_on(
                    Mock::given(method("GET"))
                        .and(path(at.to_owned()))
                        .respond_with(response)
                        .mount(&server),
                );
            };
            serve(
                "/assets/frontend/info.zip",
                ResponseTemplate::new(200)
                    .set_body_bytes(zip_of(&[("fictional_libretro.info", info.as_bytes())])),
            );
            serve(
                "/nightly/linux/x86_64/latest/.index-extended",
                ResponseTemplate::new(200)
                    .set_body_string(format!("2026-11-20 {crc:08x} fictional_libretro.so.zip\n")),
            );
            serve(
                "/nightly/linux/x86_64/latest/fictional_libretro.so.zip",
                ResponseTemplate::new(200).set_body_bytes(core_zip),
            );

            let tmp = tempfile::tempdir().unwrap();
            let dirs = DataDirs::under(tmp.path());
            dirs.create_all().unwrap();
            let http = HttpClient::new(HttpConfig {
                allow_loopback_http: true,
                timeout: Duration::from_secs(20),
                ..HttpConfig::default()
            })
            .unwrap();
            let manager = CoreManager::open(
                CoreDirs {
                    cores: dirs.cores.clone(),
                    system: dirs.system.clone(),
                },
                Platform::LinuxX86_64,
                BuildbotSource::custom(&server.uri()).unwrap(),
                http,
            )
            .unwrap();
            let mut ui = CoresUi::new(
                Some(Arc::new(manager)),
                None,
                dirs.system.clone(),
                CorePreferences::builtin(),
            );
            ui.set_cache_dir(dirs.cache_dir());
            ui.set_probe_command(shell(
                r#"d="$(dirname "$1")"; printf 'outcome = "probed"\nextensions = ["fic"]\n' > "$d/result.toml""#,
            ));
            let ctx = egui::Context::default();
            let wait = |ui: &mut CoresUi, until: &dyn Fn(&CoresUi) -> bool| {
                for _ in 0..1000 {
                    run_pass(&ctx, ui);
                    if until(ui) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                panic!("the screen never got there");
            };
            // Nothing is offered before the catalog is known.
            run_pass(&ctx, &mut ui);
            ui.open_add();
            assert!(ui.add_rows_now(false).is_empty());
            ui.start_refresh(&ctx);
            wait(&mut ui, &|ui| ui.refresh.is_none());
            run_pass(&ctx, &mut ui);
            let rows = ui.add_rows_now(false);
            assert_eq!(rows.len(), 1, "{rows:?}");
            assert!(!ui.core_installed(&rows[0].core));

            // The core is not examined by cores.toml: the acceptance comes first, once.
            assert!(needs_accept(&ui.prefs, &ui.add.accepted, &rows[0].core));
            let machines = chosen(&rows[0].machines, &default_checked(&rows[0].machines));
            ui.add.stage = ui.begin_install(rows[0].core.clone(), machines, &ctx);
            assert!(matches!(ui.add.stage, Stage::Installing { .. }));
            wait(&mut ui, &|ui| matches!(ui.add.stage, Stage::Closed));
            assert!(ui.core_installed(&rows[0].core));
            let events = ui.take_add_events();
            let [AddEvent::Drafts(drafts)] = events.as_slice() else {
                panic!("{events:?}");
            };
            assert_eq!(
                drafts[0].database.as_deref(),
                Some("Acme - Fictional Console")
            );
            assert_eq!(drafts[0].extensions, ["fic"]);
            assert_eq!(drafts[0].cores, ["fictional"]);
            assert!(ui.add.error.is_none());
        }
    }
}
