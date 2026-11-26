//! Dry run of a core in a child process (design 8.0 §2.5, ADR 0025 §7). Contract stubs (PX0): bodies in lot PU1.
//! Serialisable (toml) mirror structures of `DryRunRequest` / `DryRunReport` stay private to this module.
#![allow(unused_variables)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use emu_runtime::{DryRunReport, DryRunRequest, HwRenderRequest};

/// Injectable command: tests pass the real binary (`CARGO_BIN_EXE_emu-rust`) and `MOCK_*` variables (no
/// `set_var`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DryRunVerdict {
    Passed(DryRunReport),
    Crashed { code: Option<i32> },
    TimedOut,
    HardwareRender(HwRenderRequest),
    LoadFailed(String),
    Skipped(SkipReason),
    SpawnFailed(String),
}

/// Why no dry run was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkipReason {
    /// « Ça marche chez moi » (ADR 0025 §7).
    WorksForMe,
    /// The pair (core, content) already passed.
    AlreadyPassed,
}

/// Runs the child with `request` and waits at most `timeout`.
pub fn run_dry_run(
    command: &DryRunCommand,
    request: &DryRunRequest,
    timeout: Duration,
) -> DryRunVerdict {
    todo!("lot PU1")
}

/// Child side, called by `emu-app` before any other initialisation: bounded I/O, watchdog, no dialog.
/// Returns the process exit code.
pub fn dry_run_child_main(request_file: &Path) -> i32 {
    todo!("lot PU1")
}
