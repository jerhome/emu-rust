//! Switch to an official system (ADR 0025 §10): the analysis of the preview, the settings it changes (keys and
//! core, written without any question from the journal header or the choice made in the preview) and the switch
//! that moves nothing. Drawing is in `view`; the run is `exec::Job::Switch`.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use emu_assets::cores::CorePreferences;
use emu_library::{
    Catalog, KeysChoice, Library, LibraryError, PendingSwitch, ScanProgress, SwitchChoices,
    SwitchDirection, SwitchKind, SwitchMove, SwitchProposal, SystemInfo, TidyOptions, TidyPlan,
    open_switch,
};

use super::exec::{self, LastTidy};
use super::text;
use crate::config::{Config, DataDirs};
use crate::config_library::SwitchTarget;
use crate::library::worker::{Notify, Task};

/// Names listed in the preview at most (the rest is a count).
const LISTED: usize = 10;

/// What the preview shows and the run does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwitchMode {
    /// Same id and database, or same id and the user says it is the same machine: no file moves, definitive.
    InPlace,
    /// Other id, no unit to move: rows rewritten, definition retired, definitive.
    NoUnits,
    /// Other id: the units go through the tidy (journal version 2), annulable.
    WithMoves,
    /// Other id, a unit is blocked, duplicate or conflicting: not proposed until it is solved.
    Blocked,
}

pub(crate) fn mode(kind: SwitchKind, plan: Option<&TidyPlan>) -> SwitchMode {
    match (kind, plan) {
        (SwitchKind::NeedsMove, Some(plan)) if plan.units.is_empty() => SwitchMode::NoUnits,
        (SwitchKind::NeedsMove, Some(plan)) if plan.switchable() => SwitchMode::WithMoves,
        (SwitchKind::NeedsMove, Some(_)) => SwitchMode::Blocked,
        _ => SwitchMode::InPlace,
    }
}

/// What the analysis worker found.
pub(crate) struct SwitchAnalysis {
    pub proposal: SwitchProposal,
    /// `plan_switch` (other id only).
    pub plan: Option<TidyPlan>,
    /// Rows the official system does not accept: kept, hidden (`TidyPlan::switch_frozen` or `switch_frozen_rows`).
    pub frozen: Vec<String>,
    /// Files that stay in `roms/<id perso>/` (`TidyPlan::ignored_paths`), shown as `roms/…`.
    pub ignored: Vec<String>,
    pub last: LastTidy,
}

pub(crate) type SwitchPlanTask = Task<ScanProgress, Result<SwitchAnalysis, String>>;

/// The preview being shown.
pub(crate) struct SwitchPreview {
    pub analysis: SwitchAnalysis,
    pub mode: SwitchMode,
    /// The official keys exist and differ from the user's: the question is asked.
    pub conflict: bool,
    pub keys: KeysChoice,
}

impl SwitchPreview {
    pub(crate) fn new(analysis: SwitchAnalysis, config: &Config, dirs: &DataDirs) -> Self {
        let mode = mode(analysis.proposal.kind, analysis.plan.as_ref());
        let cores = definition_cores(&dirs.custom_systems, &analysis.proposal.custom_id, None);
        let target = target_of(
            &analysis.proposal.custom_id,
            &analysis.proposal.official,
            &cores,
        );
        let conflict = config.switch_keys_conflict(&target);
        Self {
            analysis,
            mode,
            conflict,
            // « Garder vos touches » is the default.
            keys: if conflict {
                KeysChoice::KeepUser
            } else {
                KeysChoice::Copy
            },
        }
    }

    pub(crate) fn can_start(&self) -> bool {
        self.mode != SwitchMode::Blocked && !self.analysis.last.blocked
    }

    /// The choices of the journal header (and what the undo gives back), read from the settings now.
    pub(crate) fn choices(&self, config: &Config, dirs: &DataDirs) -> SwitchChoices {
        let proposal = &self.analysis.proposal;
        let cores = definition_cores(&dirs.custom_systems, &proposal.custom_id, None);
        let target = target_of(&proposal.custom_id, &proposal.official, &cores);
        let (prior_preference, prior_profile) = config.switch_prior(&target);
        SwitchChoices {
            keys: self.keys,
            prior_preference,
            prior_profile,
        }
    }

    pub(crate) fn switch_move(&self) -> SwitchMove {
        SwitchMove {
            custom_id: self.analysis.proposal.custom_id.clone(),
            official_id: self.analysis.proposal.official.id.clone(),
        }
    }

    /// Names shown for the frozen rows and the first ones that stay in `roms/<id perso>/`.
    pub(crate) fn listed(items: &[String]) -> (&[String], usize) {
        let shown = items.len().min(LISTED);
        (&items[..shown], items.len() - shown)
    }
}

fn target_of<'a>(
    custom_id: &'a str,
    official: &'a SystemInfo,
    cores: &'a [String],
) -> SwitchTarget<'a> {
    SwitchTarget {
        custom_id,
        official,
        cores,
        official_first_core: None,
    }
}

/// The official system of the catalog of this process, masked or active.
fn official_info(id: &str) -> Option<SystemInfo> {
    let catalog = emu_library::catalog();
    catalog
        .system(id)
        .or_else(|| catalog.masked_official().iter().find(|s| s.id == id))
        .cloned()
}

fn builtin_first_core(official: &SystemInfo) -> Option<String> {
    CorePreferences::builtin()
        .for_system(official.core_key())
        .first()
        .map(ToString::to_string)
}

/// Cores of the definition `custom_id`: the active file, else the retired one (`retired_name` in `.retired/`).
/// Bounded read; whatever is unreadable gives no core (the keys and core steps are then skipped).
pub(crate) fn definition_cores(
    systems_dir: &Path,
    custom_id: &str,
    retired_name: Option<&str>,
) -> Vec<String> {
    let mut files = vec![systems_dir.join(format!("{custom_id}.toml"))];
    files.extend(retired_name.map(|name| systems_dir.join(".retired").join(name)));
    for file in files {
        let mut raw = String::new();
        let read = std::fs::File::open(&file)
            .and_then(|f| {
                f.take(emu_library::MAX_DEFINITION_SIZE)
                    .read_to_string(&mut raw)
            })
            .is_ok();
        let Some(table) = read.then(|| raw.parse::<toml::Table>().ok()).flatten() else {
            continue;
        };
        let cores = table
            .get("cores")
            .and_then(|cores| cores.get("list"))
            .and_then(toml::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(toml::Value::as_str)
                    .filter(|core| emu_assets::cores::CoreId::new(core).is_some())
                    .take(8)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !cores.is_empty() {
            return cores;
        }
    }
    Vec::new()
}

/// Steps (a) keys and (b) core of a switch, or what its undo gives back, on `config`. `Ok(false)`: nothing to do
/// (no official system or no core known). Idempotent.
pub(crate) fn settle_config(
    config: &mut Config,
    dirs: &DataDirs,
    pending: &PendingSwitch,
) -> Result<bool, String> {
    let Some(official) = official_info(&pending.switch.official_id) else {
        return Err(text::SWITCH_NO_OFFICIAL.to_owned());
    };
    let retired = (!pending.retired_name.is_empty()).then_some(pending.retired_name.as_str());
    let cores = definition_cores(&dirs.custom_systems, &pending.switch.custom_id, retired);
    if cores.is_empty() {
        return Ok(false);
    }
    let first = builtin_first_core(&official);
    let target = SwitchTarget {
        custom_id: &pending.switch.custom_id,
        official: &official,
        cores: &cores,
        official_first_core: first.as_deref(),
    };
    let before = config.clone();
    match pending.direction {
        SwitchDirection::Undo => config.undo_switch(
            &target,
            pending.choices.prior_preference.as_deref(),
            pending.choices.prior_profile.as_deref(),
        ),
        _ => config.apply_switch(&target, pending.choices.keys),
    }
    Ok(*config != before)
}

/// [`settle_config`] on the settings file. `Err`: French reason (unreadable or unwritable file); the end of the
/// switch waits for the next start.
pub(crate) fn settle_on_disk(dirs: &DataDirs, pending: &PendingSwitch) -> Result<(), String> {
    let mut config = Config::load(&dirs.config_file).map_err(|e| config_reason(&e))?;
    if settle_config(&mut config, dirs, pending)? {
        config
            .save(&dirs.config_file)
            .map_err(|e| config_reason(&e))?;
    }
    Ok(())
}

/// Reason shown for a settings file that cannot be read or written: never the absolute path of `config.toml`.
fn config_reason(e: &crate::config::ConfigError) -> String {
    use crate::config::ConfigError;
    match e {
        ConfigError::Io { source, .. } => format!("fichier des réglages inaccessible : {source}"),
        ConfigError::Locked { .. } => text::SWITCH_CONFIG_LOCKED.to_owned(),
        ConfigError::TooLarge { .. } => text::SWITCH_CONFIG_TOO_LARGE.to_owned(),
        other => other.to_string(),
    }
}

/// Why the end of a switch cannot be done (what the banner says, ADR 0025 §10.4, ADR 0023): read from the names in
/// `systems/`, never from the text of the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndBlock {
    /// `systems/<id>.toml` is in the way (forward: the definition and its retired copy differ; undo: it was recreated).
    PlaceTaken,
    /// `systems/.retired/<name>` is missing: the user puts it back.
    RetiredMissing,
    /// Anything else (disk, database): the « Journal » has the detail.
    Other,
}

/// A switch whose end is due and keeps failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndDue {
    pub switch: SwitchMove,
    pub retired_name: String,
    pub block: EndBlock,
}

pub(crate) fn end_block(dirs: &DataDirs, pending: &PendingSwitch) -> EndBlock {
    let active = dirs
        .custom_systems
        .join(format!("{}.toml", pending.switch.custom_id))
        .exists();
    let retired = dirs
        .custom_systems
        .join(".retired")
        .join(&pending.retired_name)
        .exists();
    match pending.direction {
        SwitchDirection::Undo if active => EndBlock::PlaceTaken,
        SwitchDirection::Undo if !retired => EndBlock::RetiredMissing,
        SwitchDirection::Undo => EndBlock::Other,
        _ if active && retired => EndBlock::PlaceTaken,
        _ if !active && !retired => EndBlock::RetiredMissing,
        _ => EndBlock::Other,
    }
}

/// The switch whose end is due, if any (read on the interface thread: a few small journals).
pub(crate) fn read_end_due(
    library: &Library,
    options: &TidyOptions,
    dirs: &DataDirs,
) -> Option<EndDue> {
    let pending = library.pending_switch(options).ok()??;
    Some(EndDue {
        block: end_block(dirs, &pending),
        retired_name: pending.retired_name.clone(),
        switch: pending.switch,
    })
}

/// The names of the rows kept as they are, shown like the units of the tidy: `roms/…` relative to the first of
/// `roots` that holds the path (canonical root first, then the plain one). A path under none of them shows its file
/// name only, never an absolute path.
pub(crate) fn relative_names(items: &[String], roots: &[&Path]) -> Vec<String> {
    items
        .iter()
        .map(|item| {
            let path = Path::new(item);
            if let Some(rel) = roots.iter().find_map(|root| exec::strip_root(path, root)) {
                return format!("roms/{}", text::path(&rel));
            }
            let shown = text::path(path);
            let absolute = item.starts_with(['/', '\\']) || item.as_bytes().get(1) == Some(&b':');
            if absolute {
                shown.rsplit('/').next().unwrap_or_default().to_owned()
            } else {
                shown
            }
        })
        .collect()
}

/// `TidyPlan::ignored_paths` (relative to the roms root) shown as `roms/…`.
pub(crate) fn ignored_names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| format!("roms/{}", text::path(p)))
        .collect()
}

/// Analysis in a worker: `plan_switch` for another id (read only), the frozen rows for the same id.
pub(crate) fn spawn_switch_plan(
    dirs: DataDirs,
    proposal: SwitchProposal,
    notify: Notify,
) -> SwitchPlanTask {
    Task::spawn_pinning_catalog("switch-plan", notify, move |cancel, progress| {
        let failed = |e: &LibraryError| text::analysis_failed(&exec::error_text(e));
        let roms = dirs.root.join("roms");
        if !roms.is_dir() {
            return Err(text::no_roms_folder(&roms));
        }
        let options = TidyOptions::new(&roms, &dirs.tidy_journals)
            .map_err(|e| failed(&e))?
            .with_systems_dir(&dirs.custom_systems);
        if open_switch(&options).map_err(|e| failed(&e))?.is_some() {
            return Err(text::SWITCH_JOURNAL_OPEN.to_owned());
        }
        let library = Library::open_existing(&dirs.library_db).map_err(|e| failed(&e))?;
        let custom_id = proposal.custom_id.clone();
        let mut ignored = Vec::new();
        let (plan, frozen) = if proposal.kind == SwitchKind::NeedsMove {
            let after = Catalog::load_without(&dirs.custom_systems, &custom_id).catalog;
            let switch = SwitchMove {
                custom_id: custom_id.clone(),
                official_id: proposal.official.id.clone(),
            };
            let plan = match library.plan_switch(&options, &after, &switch, cancel, &mut |p| {
                progress(p.clone());
            }) {
                Ok(plan) => plan,
                Err(LibraryError::Cancelled) => return Err(text::ANALYSIS_STOPPED.to_owned()),
                Err(e) => return Err(failed(&e)),
            };
            let frozen = relative_names(&plan.switch_frozen, &[options.roms_root(), &roms]);
            ignored = ignored_names(&plan.ignored_paths);
            (Some(plan), frozen)
        } else {
            let frozen = library
                .switch_frozen_rows(&custom_id, &proposal.official)
                .map_err(|e| failed(&e))?;
            let frozen = relative_names(&frozen, &[options.roms_root(), &roms]);
            (None, frozen)
        };
        let last = exec::read_last(&options, Some(&library), &dirs.custom_systems);
        Ok(SwitchAnalysis {
            proposal,
            plan,
            frozen,
            ignored,
            last,
        })
    })
}

/// A switch that moves no file: the keys and the core, then (other id) the rows the official system accepts, then
/// the retirement of the definition — in that order, so that a cut leaves the definition active and the switch
/// proposed again. Nothing is changed when the settings cannot be written.
pub(crate) fn run_direct(
    dirs: &DataDirs,
    library: &mut Library,
    options: &TidyOptions,
    switch: &SwitchMove,
    official: &SystemInfo,
    choices: &SwitchChoices,
    rows_move: bool,
) -> exec::Outcome {
    let mut out = exec::Outcome::new(exec::RunKind::Switch);
    let mut pending = PendingSwitch {
        journal: PathBuf::new(),
        switch: switch.clone(),
        retired_name: String::new(),
        choices: choices.clone(),
        direction: SwitchDirection::Forward,
    };
    if let Err(reason) = settle_on_disk(dirs, &pending) {
        out.messages.push(text::switch_settings_failed(&reason));
        return out;
    }
    if rows_move
        && let Err(e) = library.reassign_system(
            &switch.custom_id,
            &switch.official_id,
            emu_library::ReassignScope::AcceptedBy(official),
        )
    {
        out.messages.push(exec::error_text(&e));
        return out;
    }
    let unix = u64::try_from(crate::library::now_unix()).unwrap_or(0);
    match emu_library::retire_custom(
        &dirs.custom_systems,
        &switch.custom_id,
        emu_library::Retirement::Replaced,
        unix,
    ) {
        Ok(file) => {
            pending.retired_name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.switched = Some(pending);
        }
        Err(e) => out.messages.push(exec::error_text(&e)),
    }
    out.last = LastTidy::from_result(emu_library::last_tidy(options));
    out
}

#[cfg(test)]
mod tests;
