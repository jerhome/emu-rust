//! Stored-zip snapshot of an unpacked game folder (DOS, ADR 0018, design v4.0 §3.2): the core gets
//! `<cache>/<core_id>/<save_name>.zip`, never the user's folder. Filled by DR1.
//!
//! Contract for DR1:
//! - zip **stored** (Zip64 when needed), written to `<snapshot>.tmp-*` then renamed, with `<snapshot>.manifest`
//!   = SHA-256 of the sorted list `relative path \0 size \0 mtime`;
//! - entry names: relative, `/`-separated, no `..`, no empty component, no control character (else
//!   [`RuntimeError::UnrepresentableName`]); symbolic links ignored (neither followed nor copied); depth, file
//!   count and size bounded **while** walking ([`RuntimeError::SnapshotTooLarge`]);
//! - `cancel` checked between two files ([`RuntimeError::SnapshotCancelled`], temporary removed);
//! - the source folder is **never opened for writing** (a read-only folder is accepted).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::error::RuntimeError;

/// Bounds of one snapshot, enforced during the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotLimits {
    pub max_bytes: u64,
    pub max_files: u32,
    pub max_depth: u32,
}

impl SnapshotLimits {
    /// 4 GiB, 65 535 files, 16 levels.
    pub const DEFAULT: Self = Self {
        max_bytes: 4 << 30,
        max_files: 65_535,
        max_depth: 16,
    };
}

/// State of a snapshot against its folder (manifest comparison; nothing is hashed but the file list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStatus {
    /// Snapshot and manifest present, manifest equal to the folder's.
    Current,
    /// Snapshot present but the folder changed (or the manifest is missing / unreadable).
    Stale,
    Missing,
}

/// Outcome of [`snapshot_folder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotReport {
    pub files: u32,
    pub bytes: u64,
    /// `false` when the snapshot was already `Current` (nothing written).
    pub rebuilt: bool,
}

/// `<cache_core_dir>/<name>.zip`, `name` sanitized as a single file name, `#` removed (it separates the start
/// program in the path handed to the core). `name` = `GameIdentity::save_name`.
pub fn snapshot_path(cache_core_dir: &Path, name: &str) -> PathBuf {
    let _ = (cache_core_dir, name);
    todo!("DR1: sanitize_component, strip '#', append .zip")
}

pub fn snapshot_status(dir: &Path, snapshot: &Path) -> Result<SnapshotStatus, RuntimeError> {
    let _ = (dir, snapshot);
    todo!("DR1: compare <snapshot>.manifest with the folder's file list")
}

/// Builds (or keeps, if `Current`) the snapshot of `dir`. `progress(done_bytes, total_bytes)`.
pub fn snapshot_folder(
    dir: &Path,
    snapshot: &Path,
    limits: &SnapshotLimits,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<SnapshotReport, RuntimeError> {
    let _ = (dir, snapshot, limits, progress, cancel);
    todo!("DR1: stored zip, temporary + rename, manifest")
}

/// Identity fingerprint of a game folder (design v4.0 §10 n° 4): total size in bytes and CRC32 of the sorted list
/// `relative path \0 size` (no dates), same bounds as the snapshot. Computed only when an identity comparison
/// needs it. DR1 wires it into `identity.rs` (`identity_request_for(Folder)`: base name = folder name, primary =
/// the folder) once the 2.1 identity is merged into this branch.
#[allow(dead_code)] // Used by `identity.rs` after the 2.1 merge (DR1).
pub(crate) fn folder_fingerprint(
    dir: &Path,
    limits: &SnapshotLimits,
) -> Result<(u64, u32), RuntimeError> {
    let _ = (dir, limits);
    todo!("DR1: walk with the snapshot rules, CRC32 of the sorted list")
}
