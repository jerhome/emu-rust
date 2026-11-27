//! Dry run of a core in a child process (design 8.0 §2.4, ADR 0025 §7): load, a few frames, a report.
//!
//! Runs in-process and is meant for the child only: no window, no GL, no audio output, no pacing. Everything the
//! core and the frontend write (saves, states, working copies, extracted content) goes under `scratch_dir`; the
//! only other place is `system_dir`, which is the real one: the core reads its BIOS from it and the frontend
//! writes nothing there itself.

use std::path::{Path, PathBuf};

use emu_libretro::{Core, CoreConfig, HwRenderRequest, InputDescriptor, SystemInfo};

use crate::audio::NullSink;
use crate::content::Limits;
use crate::emulator::{Emulator, EmulatorOptions};
use crate::error::RuntimeError;
use crate::input::NullInput;
use crate::media::ContentSpec;

/// What the child is asked to do. `content: None` = probe only (no game).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunRequest {
    pub core: PathBuf,
    pub content: Option<PathBuf>,
    pub system_dir: PathBuf,
    pub scratch_dir: PathBuf,
    /// Where a private copy of a resident library may be written (ADR 0022). Accepted only strictly inside
    /// `scratch_dir` (canonical paths, equality refused), else the dry run is `LoadFailed("copy_dir outside
    /// scratch_dir")` before anything is loaded; `None`: a content run copies under `scratch_dir/cache`.
    pub copy_dir: Option<PathBuf>,
    pub options: Vec<(String, String)>,
    pub frames: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DryRunOutcome {
    /// Core loaded, no content asked.
    Probed,
    /// Content loaded and `frames` frames run (fewer if the core asked to shut down).
    Ran,
    LoadFailed(String),
    HardwareRenderRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunReport {
    pub outcome: DryRunOutcome,
    pub frames_run: u32,
    pub library_name: String,
    pub library_version: String,
    pub extensions: Vec<String>,
    pub need_fullpath: bool,
    pub supports_no_game: bool,
    pub hw_render: Option<HwRenderRequest>,
    pub descriptors: Vec<InputDescriptor>,
}

impl DryRunReport {
    fn empty(outcome: DryRunOutcome) -> Self {
        Self::new(outcome, 0, None, false, None, Vec::new())
    }

    /// The one place a report is built (probe and run alike).
    fn new(
        outcome: DryRunOutcome,
        frames_run: u32,
        info: Option<&SystemInfo>,
        supports_no_game: bool,
        hw_render: Option<HwRenderRequest>,
        descriptors: Vec<InputDescriptor>,
    ) -> Self {
        Self {
            outcome,
            frames_run,
            library_name: info.map(|i| i.library_name.clone()).unwrap_or_default(),
            library_version: info.map(|i| i.library_version.clone()).unwrap_or_default(),
            extensions: info.map(|i| i.valid_extensions.clone()).unwrap_or_default(),
            need_fullpath: info.is_some_and(|i| i.need_fullpath),
            supports_no_game,
            hw_render,
            descriptors,
        }
    }
}

/// The report of a `copy_dir` that is not under `scratch_dir`.
const COPY_DIR_REFUSED: &str = "copy_dir outside scratch_dir";

/// In-process, child only. No window, no GL, no audio output, no pacing.
///
/// A failure of the core to load, or of the game, is a report (`LoadFailed`), never a panic or an `Err`; the
/// names of a core that could not be loaded are empty. A core that crashes or hangs is the parent's business.
/// A `copy_dir` that is not strictly inside `scratch_dir` is refused before anything is loaded.
pub fn dry_run(request: &DryRunRequest) -> DryRunReport {
    let copy_dir = match checked_copy_dir(request) {
        Ok(dir) => dir,
        Err(message) => return DryRunReport::empty(DryRunOutcome::LoadFailed(message)),
    };
    match &request.content {
        None => probe(request, copy_dir),
        Some(content) => run(request, content, copy_dir),
    }
}

/// `copy_dir` made absolute and checked to lie strictly under the canonical `scratch_dir` (which may not exist
/// yet; nothing is created here, so a refused request writes nothing at all). `..` components and symlinks cannot get out: the longest existing
/// ancestor is canonicalised and the rest must be plain names. Nothing outside `scratch_dir` is created.
fn checked_copy_dir(request: &DryRunRequest) -> Result<Option<PathBuf>, String> {
    let Some(copy_dir) = &request.copy_dir else {
        return Ok(None);
    };
    let scratch =
        canonicalize_lenient(&request.scratch_dir).ok_or_else(|| COPY_DIR_REFUSED.to_owned())?;
    let resolved = canonicalize_lenient(copy_dir).ok_or_else(|| COPY_DIR_REFUSED.to_owned())?;
    if resolved != scratch && resolved.starts_with(&scratch) {
        Ok(Some(resolved))
    } else {
        Err(COPY_DIR_REFUSED.to_owned())
    }
}

/// Canonical form of a path that may not exist yet; `None` if the missing part holds anything but plain names.
fn canonicalize_lenient(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut existing = path;
    let mut base = loop {
        if let Ok(canonical) = existing.canonicalize() {
            break canonical;
        }
        missing.push(existing.file_name()?);
        existing = existing.parent()?;
    };
    for name in missing.into_iter().rev() {
        base.push(name);
    }
    Some(base)
}

fn probe(request: &DryRunRequest, copy_dir: Option<PathBuf>) -> DryRunReport {
    let config = CoreConfig {
        system_dir: request.system_dir.clone(),
        save_dir: request.scratch_dir.join("saves"),
        assets_dir: request.system_dir.clone(),
        variables: request.options.clone(),
        provide_vfs: false,
        copy_dir,
        fastforward_override: false,
        variable_state_size: false,
    };
    if let Err(e) = std::fs::create_dir_all(&config.save_dir) {
        return DryRunReport::empty(DryRunOutcome::LoadFailed(e.to_string()));
    }
    match Core::load(&request.core, config) {
        Ok(core) => DryRunReport::new(
            DryRunOutcome::Probed,
            0,
            Some(core.system_info()),
            core.supports_no_game(),
            core.hw_render_request().cloned(),
            core.input_descriptors().to_vec(),
        ),
        Err(e) => DryRunReport::empty(DryRunOutcome::LoadFailed(e.to_string())),
    }
}

fn run(request: &DryRunRequest, content: &Path, copy_dir: Option<PathBuf>) -> DryRunReport {
    let scratch = &request.scratch_dir;
    let options = EmulatorOptions {
        system_dir: request.system_dir.clone(),
        save_dir: scratch.join("saves"),
        cache_dir: scratch.join("cache"),
        provide_vfs: false,
        state_dir: scratch.join("states"),
        assets_dir: request.system_dir.clone(),
        core_variables: request.options.clone(),
        restore_added_disks: false,
        core_save_scope: crate::identity::SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices: Vec::new(),
    };
    let mut emulator = match Emulator::load_with_limits(
        &request.core,
        ContentSpec::File(content.to_path_buf()),
        options,
        Limits::DEFAULT,
        copy_dir,
    ) {
        Ok(emulator) => emulator,
        Err(RuntimeError::HardwareRenderRequired(hw)) => {
            // The core was dropped with the failed load: only the request survives.
            let mut report = DryRunReport::empty(DryRunOutcome::HardwareRenderRequired);
            report.hw_render = Some(hw);
            return report;
        }
        Err(e) => return DryRunReport::empty(DryRunOutcome::LoadFailed(e.to_string())),
    };

    let mut outcome = DryRunOutcome::Ran;
    let mut frames_run = 0;
    let (mut input, mut audio) = (NullInput, NullSink);
    while frames_run < request.frames {
        match emulator.run_frame(&mut input, &mut audio) {
            Ok(status) => {
                frames_run += 1;
                if status.shutdown_requested {
                    break;
                }
            }
            Err(RuntimeError::HardwareRenderRequired(_)) => {
                outcome = DryRunOutcome::HardwareRenderRequired;
                break;
            }
            Err(e) => {
                outcome = DryRunOutcome::LoadFailed(format!("run failed: {e}"));
                break;
            }
        }
    }
    DryRunReport::new(
        outcome,
        frames_run,
        Some(emulator.system_info()),
        emulator.supports_no_game(),
        emulator.hw_render_request().cloned(),
        emulator.input_descriptors().to_vec(),
    )
}

/// Windows: `SetErrorMode` (no system crash dialog in the child); no-op elsewhere.
pub fn suppress_crash_dialogs() {
    #[cfg(windows)]
    apply_error_mode();
}

/// Adds the three crash-dialog flags to the error mode in force.
#[cfg(windows)]
fn apply_error_mode() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        GetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX,
        SetErrorMode,
    };
    let wanted = SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX;
    // SAFETY: `GetErrorMode` takes no argument and only reads this process's error mode.
    let previous = unsafe { GetErrorMode() };
    // SAFETY: `SetErrorMode` takes plain flags and only changes this process's error mode; the previous flags
    // are OR-ed in so none set earlier by someone else is lost.
    unsafe { SetErrorMode(previous | wanted) };
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        GetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX,
    };

    #[test]
    fn error_mode_has_the_three_flags() {
        suppress_crash_dialogs();
        let wanted = SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX;
        // SAFETY: `GetErrorMode` only reads this process's error mode.
        let now = unsafe { GetErrorMode() };
        assert_eq!(now & wanted, wanted);
    }
}
