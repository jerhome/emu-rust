//! Dry run of a core in a child process (design 8.0 §2.4, ADR 0025 §7): load, a few frames, a report.
//! Contract stubs (PX0): the bodies belong to lot PR1.

use std::path::PathBuf;

use emu_libretro::{HwRenderRequest, InputDescriptor};

/// What the child is asked to do. `content: None` = probe only (no game).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunRequest {
    pub core: PathBuf,
    pub content: Option<PathBuf>,
    pub system_dir: PathBuf,
    pub scratch_dir: PathBuf,
    pub copy_dir: Option<PathBuf>,
    pub options: Vec<(String, String)>,
    pub frames: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DryRunOutcome {
    Probed,
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

/// In-process, child only. No window, no GL, no audio output, no pacing.
#[allow(unused_variables)]
pub fn dry_run(request: &DryRunRequest) -> DryRunReport {
    todo!("lot PR1")
}

/// Windows: `SetErrorMode` (no system crash dialog in the child); no-op elsewhere.
pub fn suppress_crash_dialogs() {
    todo!("lot PR1")
}
