//! Launch witness (design 8.0 §2.5, ADR 0025 §7 « témoin »): a marker file that survives a crash and is reported
//! at the next start. Contract stubs (PX0): bodies in lot PU2. Not wired yet, hence the allowances.
#![allow(unused_variables, dead_code)]

use std::path::Path;

/// What was being launched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchWhat {
    pub core: String,
    pub system: String,
    pub game: String,
}

/// An orphan marker found at start-up: the previous run stopped during this launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrashNotice {
    pub what: LaunchWhat,
    pub started_unix: u64,
}

/// `.tmp` locked then renamed to `.marker`; removed on `Drop`.
pub(crate) struct LaunchMarker {
    _private: (),
}

/// `None`: warned, never blocking.
pub(crate) fn begin_launch(cache: &Path, what: &LaunchWhat) -> Option<LaunchMarker> {
    todo!("lot PU2")
}

pub(crate) fn orphan_markers(cache: &Path) -> Vec<CrashNotice> {
    todo!("lot PU2")
}
