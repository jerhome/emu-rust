//! The probe of an installed core (ADR 0025 §4, §5): `retro_get_system_info` and the first calls, in the
//! dry-run child, without a game. Used when a system is added and by « Recalculer depuis le core ».

use std::path::Path;

use emu_runtime::DryRunRequest;

use crate::dry_run::{
    DRY_RUN_FRAMES, DRY_RUN_TIMEOUT, DryRunCommand, DryRunDir, DryRunVerdict, run_dry_run_until,
};

/// Why a probe gave nothing the definition can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeFailure {
    /// The core asks for hardware rendering: planned for 9.0, nothing is written.
    HardwareRender,
    /// The child could not start: the `.info` alone is used (the core is not at fault).
    NotRun(String),
    /// The core crashed, hung or refused to load.
    Failed(String),
    /// The wait was given up.
    Cancelled,
}

/// Runs the probe of `core` in a dry-run folder of its own, removed afterwards. Runs on a job thread.
pub(crate) fn probe_core(
    command: &DryRunCommand,
    cache: &Path,
    system_dir: &Path,
    core: &Path,
    cancelled: &dyn Fn() -> bool,
) -> DryRunVerdict {
    let dir = match DryRunDir::create(cache) {
        Ok(dir) => dir,
        Err(e) => return DryRunVerdict::SpawnFailed(format!("dossier d'essai : {e}")),
    };
    let request = DryRunRequest {
        core: core.to_owned(),
        content: None,
        system_dir: system_dir.to_owned(),
        scratch_dir: dir.path().to_owned(),
        copy_dir: Some(dir.copy_dir()),
        options: Vec::new(),
        frames: DRY_RUN_FRAMES,
    };
    run_dry_run_until(command, &request, DRY_RUN_TIMEOUT, cancelled)
}

/// What a probe verdict gives: the report of a core that ran, or why it gives nothing.
pub(crate) fn probe_outcome(
    verdict: DryRunVerdict,
) -> Result<emu_runtime::DryRunReport, ProbeFailure> {
    match verdict {
        DryRunVerdict::Passed(report) => Ok(report),
        DryRunVerdict::HardwareRender(_) => Err(ProbeFailure::HardwareRender),
        DryRunVerdict::SpawnFailed(why) => Err(ProbeFailure::NotRun(why)),
        DryRunVerdict::Cancelled => Err(ProbeFailure::Cancelled),
        DryRunVerdict::Crashed { code } => Err(ProbeFailure::Failed(
            crate::library::text::custom_failed_crashed(code),
        )),
        DryRunVerdict::TimedOut => Err(ProbeFailure::Failed(
            crate::library::text::CUSTOM_FAILED_TIMEOUT.to_owned(),
        )),
        DryRunVerdict::LoadFailed(message) => Err(ProbeFailure::Failed(message)),
        other => Err(ProbeFailure::NotRun(format!("{other:?}"))),
    }
}
