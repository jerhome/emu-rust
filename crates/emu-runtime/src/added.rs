//! Disks added from the disk menu (2.1, `docs/design/v2.1-identite-disques.md` §2.3): restored at each session,
//! removable, and the working-copy folder's unreferenced files (orphans) listed for an explicit clean-up. Never
//! deletes anything without an explicit call; only stale `.tmp-*` files older than an hour go automatically.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::RuntimeError;
use crate::media::Disks;

/// One disk added from the menu (manifest entry `Origin::Added`) of the running game's working-copy folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedDisk {
    /// Working copy handed to the core.
    pub copy: PathBuf,
    /// Recorded source; `None` for an adopted orphan (« Reprendre »).
    pub source: Option<PathBuf>,
    /// The recorded source still exists (the copy is used either way: progress lives in it).
    pub source_present: bool,
    /// Size of the copy, in bytes.
    pub size: u64,
    /// Index in the core's image list when restored or added this session; `None` if not in the list.
    pub index: Option<u32>,
}

/// Why a file of the working-copy folder is listed by [`orphan_copies`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanKind {
    /// A disk image the manifest does not reference (rc.1 copies `02 X.st`, copies forgotten beyond 32).
    Copy,
    /// A generated `.m3u` that is not the folder's current playlist (old `<save_name>.m3u`).
    StalePlaylist,
}

/// One unreferenced file of a working-copy folder (« Nettoyer les copies… »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanCopy {
    /// Plain file name inside the folder.
    pub name: String,
    pub kind: OrphanKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl Disks<'_> {
    /// Added disks of the session's working-copy folder, in order of addition.
    pub fn added(&mut self) -> Vec<AddedDisk> {
        todo!("I2: manifest Origin::Added entries + core indices")
    }

    /// Removes `copy` from the core's list and from the manifest; with `delete_copy`, also deletes the file.
    /// `DiskInserted` if it is the inserted disk; `RemovalRefused` if `copy` is not an added disk of the folder.
    pub fn forget_added(&mut self, copy: &Path, delete_copy: bool) -> Result<(), RuntimeError> {
        let _ = (copy, delete_copy);
        todo!("I2: design §2.3 « Retrait »")
    }

    /// « Reprendre »: orphan `name` becomes an added disk (entry `Added` without source) and is added to the
    /// core's list (not inserted); returns its index.
    pub fn adopt_orphan(&mut self, name: &str) -> Result<u32, RuntimeError> {
        let _ = name;
        todo!("I2: design §2.3 « Nettoyage »")
    }
}

/// Unreferenced files of `work_dir`, which must be a `*.disks` folder directly under `saves_core_dir`
/// (`RemovalRefused` otherwise). The manifest, referenced copies and links are never listed.
pub fn orphan_copies(
    saves_core_dir: &Path,
    work_dir: &Path,
) -> Result<Vec<OrphanCopy>, RuntimeError> {
    let _ = (saves_core_dir, work_dir);
    todo!("I2: design §2.3 « Nettoyage »")
}

/// Deletes the orphans `names` of `work_dir` (checked selection of the clean-up dialog); returns the bytes freed.
/// Refuses (`RemovalRefused`, nothing deleted) any name that is not a plain file name of the folder, a referenced
/// entry, the manifest, a link, or a `work_dir` that is not a `*.disks` directly under `saves_core_dir`.
pub fn remove_orphan_copies(
    saves_core_dir: &Path,
    work_dir: &Path,
    names: &[String],
) -> Result<u64, RuntimeError> {
    let _ = (saves_core_dir, work_dir, names);
    todo!("I2: guards of reset_work_dir + manifest check")
}
