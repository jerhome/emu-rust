//! « Ranger les ROMs… » (ADR 0023 §4, design v7.1 §2.2): analysis in a worker (`Library::plan_tidy`, read only),
//! then a preview where the user checks units and resolves ambiguous ones. States: Analyse → Aperçu (FU2a);
//! Exécution → Rapport (FU2b, `exec`). Pure parts are unit-tested here; drawing is in `view`.

pub(crate) mod exec;
pub(super) mod text;
mod view;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use emu_assets::bios::{FirmwareCatalogue, Identification, identify_firmware};
use emu_assets::cores::{CoreManager, CorePreferences};
use emu_library::{
    Library, LibraryError, ScanProgress, TidyOptions, TidyPlan, TidyStatus, TidyUnit, systems,
};

use super::worker::{Notify, Task};

/// Single files at most this big are hashed against the known BIOS (PlayStation 512 KiB, 3DO 1 MiB).
const BIOS_PROBE_MAX: u64 = 1024 * 1024;
/// Bound of the BIOS identification of one analysis (md5 of each candidate).
const BIOS_PROBE_COUNT: usize = 200;

/// A file of the roms folder whose md5 is a known BIOS (ADR 0023 §4.2: « signalé avec Importer un BIOS… »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BiosFound {
    /// Relative to the roms root.
    pub path: PathBuf,
    pub absolute: PathBuf,
    /// Names it is expected under in `system/`.
    pub names: Vec<String>,
    /// Unit of the plan made of this file only (left unchecked), `None` for an ignored file.
    pub unit: Option<u32>,
}

pub(super) struct Analysis {
    pub plan: TidyPlan,
    pub bios: Vec<BiosFound>,
    /// Drives « Annuler le dernier rangement » and the unreadable-journal block.
    pub last: exec::LastTidy,
}

pub(super) type PlanTask = Task<ScanProgress, Result<Analysis, String>>;

/// What may refuse to start the analysis (ADR 0023 §4.5: no tidying during a scan, a download or a game).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Busy {
    pub scan: bool,
    pub download: bool,
    pub game: bool,
    pub no_db: bool,
}

pub(super) fn refusal(b: Busy) -> Option<&'static str> {
    if b.no_db {
        Some(text::REFUSED_NO_DB)
    } else if b.game {
        Some(text::REFUSED_GAME)
    } else if b.scan {
        Some(text::REFUSED_SCAN)
    } else if b.download {
        Some(text::REFUSED_DOWNLOAD)
    } else {
        None
    }
}

/// How a unit is shown (design v7.1 §2.2 table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowKind {
    /// Checked by default.
    Sure,
    /// Unchecked by default, with the reason.
    Suggested,
    /// Several systems possible, none chosen yet: candidate buttons.
    Choose,
    /// Ambiguous unit whose system was chosen: « résolu ».
    Resolved,
    /// Blocked or unknown: stays in place, reason in French.
    Left,
}

pub(super) fn row_kind(u: &TidyUnit) -> RowKind {
    match &u.status {
        TidyStatus::Sure(_) => RowKind::Sure,
        TidyStatus::Suggested(_) => RowKind::Suggested,
        TidyStatus::Ambiguous if u.system.is_some() => RowKind::Resolved,
        TidyStatus::Ambiguous => RowKind::Choose,
        TidyStatus::Unknown | TidyStatus::Blocked(_) => RowKind::Left,
    }
}

/// May be checked: a known system and something to move.
pub(super) fn selectable(u: &TidyUnit) -> bool {
    matches!(
        row_kind(u),
        RowKind::Sure | RowKind::Suggested | RowKind::Resolved
    ) && u.system.is_some()
        && !u.moves.is_empty()
}

/// Sure units are checked, except a known BIOS (`bios_units`); everything else starts unchecked.
pub(super) fn initial_checked(plan: &TidyPlan, bios_units: &BTreeSet<u32>) -> BTreeSet<u32> {
    plan.units
        .iter()
        .filter(|u| row_kind(u) == RowKind::Sure && selectable(u) && !bios_units.contains(&u.id))
        .map(|u| u.id)
        .collect()
}

/// After a choice re-ran the checks: units that can no longer move are unchecked; `chosen` (just resolved by the
/// user) are checked when they can move.
pub(super) fn sync_checked(plan: &TidyPlan, checked: &mut BTreeSet<u32>, chosen: &[u32]) {
    let movable: BTreeSet<u32> = plan
        .units
        .iter()
        .filter(|u| selectable(u))
        .map(|u| u.id)
        .collect();
    checked.retain(|id| movable.contains(id));
    checked.extend(chosen.iter().filter(|id| movable.contains(id)));
}

/// Section of the preview.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Group {
    /// Destination system id, catalogue order.
    System(usize, String),
    Choose,
    Left,
}

/// Indices into `plan.units`, grouped by destination system, then « à choisir », then « laissés en place ».
pub(super) fn groups(plan: &TidyPlan) -> Vec<(Group, Vec<usize>)> {
    let order: BTreeMap<&str, usize> = systems()
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();
    let mut out: BTreeMap<Group, Vec<usize>> = BTreeMap::new();
    for (i, u) in plan.units.iter().enumerate() {
        let group = match (row_kind(u), &u.system) {
            (RowKind::Left, _) => Group::Left,
            (RowKind::Choose, _) | (_, None) => Group::Choose,
            (_, Some(s)) => Group::System(
                order.get(s.as_str()).copied().unwrap_or(usize::MAX),
                s.clone(),
            ),
        };
        out.entry(group).or_default().push(i);
    }
    out.into_iter().collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Totals {
    pub units: usize,
    pub files: usize,
    pub bytes: u64,
    /// Distinct destination folders (`psx`, `psx/Game (Europe)`).
    pub dirs: usize,
}

pub(super) fn totals(plan: &TidyPlan, checked: &BTreeSet<u32>) -> Totals {
    let mut t = Totals::default();
    let mut dirs: BTreeSet<&Path> = BTreeSet::new();
    for u in plan.units.iter().filter(|u| checked.contains(&u.id)) {
        t.units += 1;
        t.files += u.moves.len();
        t.bytes += u.moves.iter().map(|m| m.size).sum::<u64>();
        dirs.extend(u.moves.iter().filter_map(|m| m.to.parent()));
    }
    t.dirs = dirs.len();
    t
}

/// One line of the bulk choice: still ambiguous units of `extension` and the systems they list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BulkChoice {
    pub extension: String,
    pub count: usize,
    pub systems: Vec<String>,
}

/// Extensions with at least two unresolved ambiguous units (a single one is chosen on its row).
pub(super) fn bulk_choices(plan: &TidyPlan) -> Vec<BulkChoice> {
    let mut out: Vec<BulkChoice> = Vec::new();
    for u in plan.units.iter().filter(|u| row_kind(u) == RowKind::Choose) {
        if u.extension.is_empty() {
            continue;
        }
        let i = match out.iter().position(|b| b.extension == u.extension) {
            Some(i) => i,
            None => {
                out.push(BulkChoice {
                    extension: u.extension.clone(),
                    count: 0,
                    systems: Vec::new(),
                });
                out.len() - 1
            }
        };
        let b = &mut out[i];
        b.count += 1;
        for c in &u.candidates {
            if !b.systems.contains(c) {
                b.systems.push(c.clone());
            }
        }
    }
    out.retain(|b| b.count > 1);
    out.sort_by(|a, b| a.extension.cmp(&b.extension));
    out
}

/// A system choice of the preview. Applied in a worker: re-placing the units may CRC large files to tell a
/// collision from a duplicate (FV0 I6), which must not freeze the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Choice {
    Unit { unit: u32, system: String },
    Extension { extension: String, system: String },
}

#[derive(Debug)]
pub(super) enum ChoiceOutcome {
    Unit(Result<(), LibraryError>),
    /// Units resolved.
    Extension(u32),
    /// The preview was left before the worker started this choice.
    Cancelled,
}

/// Result: the plan with the choice applied (left unused when the choice is refused).
pub(super) type ChoiceTask = Task<(), (TidyPlan, ChoiceOutcome)>;

/// `TidyPlan::choose_system*` on `plan` (worker side).
pub(super) fn run_choice(plan: &mut TidyPlan, choice: &Choice) -> ChoiceOutcome {
    match choice {
        Choice::Unit { unit, system } => ChoiceOutcome::Unit(plan.choose_system(*unit, system)),
        Choice::Extension { extension, system } => {
            ChoiceOutcome::Extension(plan.choose_system_for_extension(extension, system))
        }
    }
}

/// Worker body: nothing is computed once the preview was left (`cancel`).
pub(super) fn choice_job(
    cancel: &std::sync::atomic::AtomicBool,
    mut plan: TidyPlan,
    choice: &Choice,
) -> (TidyPlan, ChoiceOutcome) {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return (plan, ChoiceOutcome::Cancelled);
    }
    let outcome = run_choice(&mut plan, choice);
    (plan, outcome)
}

/// The preview being edited.
pub(super) struct Preview {
    pub plan: TidyPlan,
    pub checked: BTreeSet<u32>,
    pub bios: Vec<BiosFound>,
    /// Units whose « Autre système… » is open (known games).
    pub other_open: BTreeSet<u32>,
    /// Result of the last bulk choice.
    pub note: Option<String>,
    /// Choice being applied in a worker, and the ones made meanwhile (applied in order, none is lost).
    pub choosing: Option<(Choice, ChoiceTask)>,
    pub queued: std::collections::VecDeque<Choice>,
    pub last: exec::LastTidy,
}

impl Preview {
    pub(super) fn new(analysis: Analysis) -> Self {
        let bios_units: BTreeSet<u32> = analysis.bios.iter().filter_map(|b| b.unit).collect();
        let checked = initial_checked(&analysis.plan, &bios_units);
        Self {
            plan: analysis.plan,
            checked,
            bios: analysis.bios,
            other_open: BTreeSet::new(),
            note: None,
            choosing: None,
            queued: std::collections::VecDeque::new(),
            last: analysis.last,
        }
    }

    /// A choice is being applied or waits (« Mise à jour de l'aperçu… »; FU2b: no execution meanwhile).
    pub(super) fn updating(&self) -> usize {
        usize::from(self.choosing.is_some()) + self.queued.len()
    }

    /// Queues `choice`; the worker starts at once when none runs.
    pub(super) fn request(&mut self, choice: Choice, notify: Notify) {
        self.queued.push_back(choice);
        self.start_next(notify);
    }

    fn start_next(&mut self, notify: Notify) {
        if self.choosing.is_some() {
            return;
        }
        let Some(choice) = self.queued.pop_front() else {
            return;
        };
        let plan = self.plan.clone();
        let job = choice.clone();
        // Leaving the preview drops the task (cancel flag) and the queue with it: no further choice runs. A
        // choice already in `choose_system*` runs to its end: emu-library offers no cancellation there.
        let task = Task::spawn("tidy-choice", notify, move |cancel, _progress| {
            choice_job(cancel, plan, &job)
        });
        self.choosing = Some((choice, task));
    }

    /// Drains the choice worker: `Some(Err(message))` for a refused or interrupted choice, `Some(Ok)` when the
    /// plan changed. The next queued choice starts on the new plan.
    pub(super) fn poll_choice(&mut self, notify: Notify) -> Option<Result<(), String>> {
        let (choice, task) = self.choosing.as_mut()?;
        let (_, done) = task.poll();
        let done = done?;
        let choice = choice.clone();
        self.choosing = None;
        let result = match done {
            Some((plan, outcome)) => self.apply(&choice, plan, outcome),
            None => Err(text::CHOICE_INTERRUPTED.to_owned()),
        };
        self.start_next(notify);
        Some(result)
    }

    /// UI side of a choice computed by [`run_choice`] on a copy of `self.plan`.
    pub(super) fn apply(
        &mut self,
        choice: &Choice,
        plan: TidyPlan,
        outcome: ChoiceOutcome,
    ) -> Result<(), String> {
        match (choice, outcome) {
            (_, ChoiceOutcome::Unit(Err(e))) => Err(e.to_string()),
            (_, ChoiceOutcome::Cancelled) => Err(text::CHOICE_INTERRUPTED.to_owned()),
            (Choice::Unit { unit, .. }, ChoiceOutcome::Unit(Ok(()))) => {
                self.plan = plan;
                self.other_open.remove(unit);
                sync_checked(&self.plan, &mut self.checked, &[*unit]);
                Ok(())
            }
            (Choice::Extension { extension, system }, ChoiceOutcome::Extension(n)) => {
                let before: BTreeSet<u32> = self
                    .plan
                    .units
                    .iter()
                    .filter(|u| row_kind(u) == RowKind::Choose)
                    .map(|u| u.id)
                    .collect();
                self.plan = plan;
                let chosen: Vec<u32> = self
                    .plan
                    .units
                    .iter()
                    .filter(|u| before.contains(&u.id) && u.system.is_some())
                    .map(|u| u.id)
                    .collect();
                sync_checked(&self.plan, &mut self.checked, &chosen);
                self.note = Some(text::bulk_done(n, extension, system));
                Ok(())
            }
            // `run_choice` pairs each choice with its outcome kind.
            (Choice::Unit { .. }, ChoiceOutcome::Extension(_))
            | (Choice::Extension { .. }, ChoiceOutcome::Unit(Ok(()))) => Ok(()),
        }
    }

    /// « Ranger » is enabled ([`exec::can_apply`]).
    pub(super) fn can_apply(&self) -> bool {
        exec::can_apply(self.updating(), self.checked.len(), self.last.blocked)
    }

    pub(super) fn is_bios_unit(&self, id: u32) -> bool {
        self.bios.iter().any(|b| b.unit == Some(id))
    }

    /// [`run_choice`] then [`Preview::apply`] on this thread; a refused choice (`TidyChoice`) changes nothing.
    #[cfg(test)]
    pub(super) fn choose(&mut self, unit: u32, system: &str) -> Result<(), String> {
        let choice = Choice::Unit {
            unit,
            system: system.to_owned(),
        };
        let mut plan = self.plan.clone();
        let outcome = run_choice(&mut plan, &choice);
        self.apply(&choice, plan, outcome)
    }

    #[cfg(test)]
    pub(super) fn choose_for_extension(&mut self, extension: &str, system: &str) -> u32 {
        let choice = Choice::Extension {
            extension: extension.to_owned(),
            system: system.to_owned(),
        };
        let mut plan = self.plan.clone();
        let outcome = run_choice(&mut plan, &choice);
        let n = match outcome {
            ChoiceOutcome::Extension(n) => n,
            ChoiceOutcome::Unit(_) | ChoiceOutcome::Cancelled => 0,
        };
        let _ = self.apply(&choice, plan, outcome);
        n
    }

    pub(super) fn set_checked(&mut self, id: u32, on: bool) {
        let can = self.plan.units.iter().any(|u| u.id == id && selectable(u));
        if on && can {
            self.checked.insert(id);
        } else {
            self.checked.remove(&id);
        }
    }
}

#[derive(Default)]
pub(super) enum Stage {
    #[default]
    Idle,
    Analysing {
        task: PlanTask,
        progress: Option<ScanProgress>,
    },
    Preview(Box<Preview>),
    Failed(String),
    /// `apply_tidy` / `undo_tidy` then the identity step, in a worker. Not left before the end; a window closed
    /// meanwhile waits for it at exit (bounded), the journal makes the next start finish the rest.
    Running {
        task: exec::RunTask,
        progress: Option<emu_library::TidyProgress>,
        undo: bool,
    },
    Report(Box<exec::Outcome>),
}

/// « Annuler le dernier rangement… » waiting for its confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UndoAsk {
    pub journal: PathBuf,
    pub moved: u64,
    pub unix: i64,
    /// Focus « Non » on the next frame (the safe default for a held key).
    pub focus: bool,
}

#[derive(Default)]
pub(super) struct TidyUi {
    pub stage: Stage,
    pub confirm_undo: Option<UndoAsk>,
    pub focus_first: bool,
    /// Start the analysis on the next frame (capture mode).
    pub pending: bool,
}

impl TidyUi {
    pub(super) fn busy(&self) -> bool {
        matches!(self.stage, Stage::Analysing { .. } | Stage::Running { .. })
    }

    /// Drains the workers; `true` when something changed.
    pub(super) fn poll(&mut self) -> bool {
        if let Stage::Running {
            task,
            progress,
            undo,
        } = &mut self.stage
        {
            let (steps, done) = task.poll();
            let changed = !steps.is_empty() || done.is_some();
            if let Some(p) = steps.into_iter().last() {
                *progress = Some(p);
            }
            if let Some(done) = done {
                let outcome = done.unwrap_or_else(|| {
                    let mut out = exec::Outcome::new(if *undo {
                        exec::RunKind::Undo
                    } else {
                        exec::RunKind::Tidy
                    });
                    out.messages.push(text::RUN_INTERRUPTED.to_owned());
                    out
                });
                self.stage = Stage::Report(Box::new(outcome));
                self.focus_first = true;
            }
            return changed;
        }
        let Stage::Analysing { task, progress } = &mut self.stage else {
            return false;
        };
        let (steps, done) = task.poll();
        let changed = !steps.is_empty() || done.is_some();
        if let Some(p) = steps.into_iter().last() {
            *progress = Some(p);
        }
        match done {
            Some(Some(Ok(analysis))) => {
                self.stage = Stage::Preview(Box::new(Preview::new(analysis)));
                self.focus_first = true;
            }
            Some(Some(Err(e))) => self.stage = Stage::Failed(e),
            Some(None) => self.stage = Stage::Failed(text::analysis_failed("tâche interrompue")),
            None => {}
        }
        changed
    }

    /// An undo may start: confirmed, nothing running, from the preview or the report.
    pub(super) fn may_undo(&self) -> bool {
        self.confirm_undo.is_some() && matches!(self.stage, Stage::Preview(_) | Stage::Report(_))
    }

    pub(super) fn close(&mut self) {
        self.confirm_undo = None;
        // Dropping the task cancels it.
        self.stage = Stage::Idle;
    }
}

/// File names never planned as games (`TidyOptions::with_firmware_names`): the BIOS catalogue (`firmware.toml`,
/// installed `.info`, free BIOS), every file of the `required_firmware` tables and the `optional_firmware` entries.
pub(super) fn firmware_names(
    catalogue: &FirmwareCatalogue,
    prefs: &CorePreferences,
) -> Vec<String> {
    let mut names: BTreeSet<String> = catalogue
        .expected_names()
        .into_iter()
        .map(|e| e.name)
        .collect();
    for (database, cores) in prefs.systems() {
        for core in cores {
            if let Some(req) = prefs.required_firmware(core, database) {
                names.extend(req.files.values().cloned());
            }
            names.extend(
                prefs
                    .optional_firmware(core, database)
                    .iter()
                    .map(|o| o.file.clone()),
            );
        }
    }
    names.into_iter().collect()
}

fn file_name_lower(p: &Path) -> Option<String> {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_lowercase)
}

/// Files worth an md5 against the known BIOS: ignored files named like a BIOS, then single-file units of at most
/// [`BIOS_PROBE_MAX`] bytes (`.bin`, `.rom`); [`BIOS_PROBE_COUNT`] at most. `(relative path, unit)`.
pub(super) fn bios_candidates(plan: &TidyPlan, names: &[String]) -> Vec<(PathBuf, Option<u32>)> {
    let names: BTreeSet<String> = names
        .iter()
        .filter_map(|n| file_name_lower(Path::new(n)))
        .collect();
    let mut out: Vec<(PathBuf, Option<u32>)> = plan
        .ignored_paths
        .iter()
        .filter(|p| file_name_lower(p).is_some_and(|n| names.contains(&n)))
        .map(|p| (p.clone(), None))
        .collect();
    for u in &plan.units {
        if let [m] = u.moves.as_slice()
            && m.size <= BIOS_PROBE_MAX
            && matches!(u.extension.as_str(), "bin" | "rom")
        {
            out.push((m.from.clone(), Some(u.id)));
        }
    }
    out.truncate(BIOS_PROBE_COUNT);
    out
}

/// Runs the analysis in a worker: own SQLite connection, `plan_tidy` (read only), then the md5 of the few BIOS
/// candidates. Nothing is written.
pub(super) fn spawn_plan(
    db: PathBuf,
    roms: PathBuf,
    journals: PathBuf,
    manager: Option<Arc<CoreManager>>,
    prefs: CorePreferences,
    notify: Notify,
) -> PlanTask {
    Task::spawn("tidy-plan", notify, move |cancel, progress| {
        if !roms.is_dir() {
            return Err(text::no_roms_folder(&roms));
        }
        let library =
            Library::open_existing(&db).map_err(|e| text::analysis_failed(&e.to_string()))?;
        let catalogue = crate::bios_import::firmware_catalogue(manager.as_deref());
        let names = firmware_names(&catalogue, &prefs);
        let options = TidyOptions::new(&roms, &journals)
            .map_err(|e| text::analysis_failed(&e.to_string()))?
            .with_firmware_names(&names);
        let plan = match library.plan_tidy(&options, cancel, &mut |p: &ScanProgress| {
            progress(p.clone());
        }) {
            Ok(plan) => plan,
            Err(LibraryError::Cancelled) => return Err(text::ANALYSIS_STOPPED.to_owned()),
            Err(e) => return Err(text::analysis_failed(&e.to_string())),
        };
        let mut bios = Vec::new();
        for (path, unit) in bios_candidates(&plan, &names) {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(text::ANALYSIS_STOPPED.to_owned());
            }
            let absolute = options.roms_root().join(&path);
            if let Ok(Identification::Known { targets, .. }) =
                identify_firmware(&absolute, &catalogue)
                && !targets.is_empty()
            {
                bios.push(BiosFound {
                    path,
                    absolute,
                    names: targets.into_iter().map(|t| t.name).collect(),
                    unit,
                });
            }
        }
        let last = exec::read_last(&options);
        Ok(Analysis { plan, bios, last })
    })
}

#[cfg(test)]
mod tests;
