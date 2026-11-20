//! Import of a BIOS file provided by the user (ADR 0017 §3, `docs/design/v3.0-amiga.md` §2.2 and §3.3): any
//! system (Kickstart, TOS, Game Boy boot ROM…). Nothing is ever downloaded here (ADR 0005).
//!
//! Rules the implementation must keep (reviewed by `reviewer`, task V3):
//! - the source is read as a stream, never beyond [`MAX_FIRMWARE_BYTES`]; a symbolic link is followed (users
//!   link BIOS collections) but only a regular file is accepted;
//! - the target name is validated (non-empty, no `..`, not absolute, no `\\`, no drive prefix; `/` only for a
//!   sub-folder declared by a `.info`) and the file never lands outside the system directory;
//! - temporary file in the system directory, `sync_all`, **hash recomputed on the copy**, then atomic rename;
//! - an existing file with another content is never overwritten silently: `ReplacePolicy::Refuse` fails with
//!   `BiosError::TargetExists`, `BackupAndReplace` renames it `<name>.user-<unix>` first; a free BIOS installed
//!   by emu-rust (manifest) is replaced without backup and leaves the manifest;
//! - `[imported."<name>"]` (`source` = original file name without its folder, `md5`, `verified`) is recorded in
//!   `system/.emu-rust-bios.toml`; the original path is never logged above `debug`.

use std::path::{Path, PathBuf};

use super::{BiosError, FreeBios, ReplacePolicy};
use crate::cores::{CoreId, CoreInfo, KnownFirmware};

/// Largest file accepted by the import (largest known BIOS: 4 MiB; Amiga extended ROMs: 1 MiB).
pub const MAX_FIRMWARE_BYTES: u64 = 16 << 20;

/// md5 → expected file names, from `data/firmware.toml`, the `.info` of the installed cores (md5 of their
/// `notes`, read at run time and never copied into our data: design §3.5.6) and the free BIOS catalogue.
#[derive(Debug, Clone, Default)]
pub struct FirmwareCatalogue {
    by_md5: std::collections::BTreeMap<[u8; 16], Vec<ExpectedFirmware>>,
    expected: Vec<ExpectedFirmware>,
}

/// One file a core (or the catalogue) expects in the system directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedFirmware {
    /// Relative to the system directory (`kick34005.A500`).
    pub name: String,
    /// Shown to the user (`.info` description, or the name).
    pub label: String,
    /// libretro database name, when known.
    pub system: Option<String>,
    pub size: Option<u64>,
}

impl FirmwareCatalogue {
    pub fn build(known: &KnownFirmware, infos: &[(CoreId, CoreInfo)], free: &[FreeBios]) -> Self {
        let _ = (known, infos, free);
        todo!("A3: design v3.0 §3.3")
    }

    /// Every expected file, sorted by name without duplicates: choices offered for an unverified import.
    pub fn expected_names(&self) -> Vec<ExpectedFirmware> {
        let _ = (&self.by_md5, &self.expected);
        todo!("A3: design v3.0 §3.3")
    }
}

/// What [`identify_firmware`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identification {
    /// md5 known: to be installed under every target name.
    Known {
        md5: [u8; 16],
        sha1: [u8; 20],
        size: u64,
        targets: Vec<ExpectedFirmware>,
    },
    /// Amiga Forever encrypted ROM (header `AMIROMTYPE1`): imported unverified under its Amiga Forever name
    /// (`suggested`), with `rom.key`.
    AmigaForeverEncrypted {
        size: u64,
        suggested: Option<String>,
    },
    /// Refused by default; the UI may import it unverified under a name chosen among `expected_names()`.
    Unknown {
        md5: [u8; 16],
        sha1: [u8; 20],
        size: u64,
    },
}

/// What is at a target name before an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetState {
    Absent,
    /// Same content as the file about to be imported: nothing to do.
    Same,
    /// A user file with another content.
    Other {
        md5: [u8; 16],
    },
    /// A free BIOS installed by emu-rust, unchanged (manifest): replaced without backup.
    FreeInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    pub installed: PathBuf,
    /// `<name>.user-<unix>` when a user file was moved aside.
    pub backup: Option<PathBuf>,
}

/// Hashes `src` (md5 for the lookup, sha1 for display only) and looks it up. `BiosError::TooLarge` beyond
/// [`MAX_FIRMWARE_BYTES`].
pub fn identify_firmware(
    src: &Path,
    catalogue: &FirmwareCatalogue,
) -> Result<Identification, BiosError> {
    let _ = (src, catalogue);
    todo!("A3: design v3.0 §3.3")
}

/// State of `name` in `system_dir`, compared with the md5 of the file about to be imported
/// (`Identification::{Known, Unknown}::md5`).
pub fn target_state(
    system_dir: &Path,
    name: &str,
    md5: &[u8; 16],
) -> Result<TargetState, BiosError> {
    let _ = (system_dir, name, md5);
    todo!("A3: design v3.0 §3.3")
}

/// Copies `src` to `<system_dir>/<name>` (module rules above). `verified = false` records an import the user
/// forced for an unknown hash. Same content already there: `Ok`, nothing written.
pub fn import_firmware(
    system_dir: &Path,
    src: &Path,
    name: &str,
    policy: ReplacePolicy,
    verified: bool,
) -> Result<ImportOutcome, BiosError> {
    let _ = (system_dir, src, name, policy, verified);
    todo!("A3: design v3.0 §3.3")
}
