//! Free BIOS installer (ADR 0015): EmuTOS for the Atari ST in 2.0, AROS for the Amiga in 3.0.
//! Catalogue embedded from `data/free_bios.toml`; every archive and every installed file is pinned by SHA-256.
//!
//! Contract only (version 2.0): bodies are filled by the `library-assets` agent
//! (`docs/design/v2.0-atari-st.md` §3.3).

use std::path::{Path, PathBuf};

use crate::error::AssetError;
use crate::http::{CancelToken, HttpClient, Progress};

/// Largest free BIOS archive accepted (EmuTOS 512k zip: 3.7 MB).
pub const MAX_BIOS_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
/// Largest extracted image (EmuTOS 1024k: 1 MiB; AROS ROMs: 512 KiB each).
pub const MAX_BIOS_IMAGE_BYTES: u64 = 4 * 1024 * 1024;

/// One free BIOS family (`emutos`, later `aros`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeBios {
    /// `[A-Za-z0-9_-]{1,32}`, stable (config, manifest).
    pub id: String,
    pub name: String,
    pub version: String,
    /// SPDX expression (`GPL-2.0-or-later`).
    pub license: String,
    pub homepage: String,
    /// libretro database name of the system (`Atari - ST`), as in `SystemInfo::database` and `cores.toml`.
    pub system: String,
    /// File name expected by the cores, relative to the system directory (`tos.img`).
    pub install_as: String,
    pub archives: Vec<BiosArchive>,
    pub variants: Vec<BiosVariant>,
    pub default_variant: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiosArchive {
    pub name: String,
    /// HTTPS; redirections to other HTTPS hosts are followed (SourceForge mirrors): integrity comes from the
    /// pinned SHA-256, not from the host.
    pub url: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiosVariant {
    /// `192fr`, `192us`, `1024k`…
    pub id: String,
    /// Shown in the UI (« EmuTOS 192 Ko — français (ST/STE) »).
    pub label: String,
    /// BCP 47 primary language subtag used to preselect the variant (`fr`), `None` for multilingual images.
    pub language: Option<String>,
    pub archive: String,
    /// Exact entry name in the archive (compared, never used as a path).
    pub entry: String,
    pub size: u64,
    pub sha256: [u8; 32],
    pub md5: [u8; 16],
}

/// Catalogue embedded in the binary. Panics only if the embedded file is invalid (unit-tested).
pub fn builtin_free_bios() -> Vec<FreeBios> {
    todo!("library-assets (2.0): parse data/free_bios.toml")
}

/// State of `<system>/<install_as>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeBiosState {
    Missing,
    /// Installed by emu-rust and unchanged since (SHA-256 of the manifest).
    Installed {
        variant: String,
    },
    /// A file emu-rust did not install (a TOS dumped by the user): never overwritten without consent.
    UserFile {
        md5: [u8; 16],
    },
    /// Installed by emu-rust, then modified or replaced outside the application.
    Modified,
    Unreadable(String),
}

/// What to do when the target file is not ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacePolicy {
    /// Fail with `BiosError::UserFileExists` (default; the UI then asks).
    Refuse,
    /// Rename the user's file to `<install_as>.user-<unix>` first, then install.
    BackupAndReplace,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BiosError {
    #[error("BIOS libre inconnu : {0}")]
    UnknownBios(String),
    #[error("variante inconnue : {0}")]
    UnknownVariant(String),
    #[error("un fichier fourni par l'utilisateur existe déjà : {0}")]
    UserFileExists(PathBuf),
    #[error("empreinte SHA-256 inattendue pour {0}")]
    HashMismatch(String),
    #[error("entrée absente de l'archive : {0}")]
    MissingEntry(String),
    #[error(transparent)]
    Asset(#[from] AssetError),
    #[error("archive invalide : {0}")]
    Archive(String),
    #[error("E/S sur {path} : {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Installs, checks and removes free BIOS files in the system directory. Manifest:
/// `<system>/.emu-rust-bios.toml` (`file`, `bios`, `variant`, `version`, `sha256`, `installed_unix`).
/// `Send + Sync`; methods take `&self`; network only in `install`.
pub struct FreeBiosManager {
    // Read once the stubs are implemented (library-assets, version 2.0).
    #[allow(dead_code)]
    system_dir: PathBuf,
    #[allow(dead_code)]
    http: HttpClient,
}

impl FreeBiosManager {
    /// No network. Creates the system directory if needed; cleans `.tmp-*` leftovers older than 1 h.
    pub fn open(system_dir: &Path, http: HttpClient) -> Result<Self, BiosError> {
        let _ = (system_dir, http);
        todo!("library-assets (2.0)")
    }

    pub fn state(&self, bios: &FreeBios) -> FreeBiosState {
        let _ = bios;
        todo!("library-assets (2.0)")
    }

    /// Downloads the archive (pinned SHA-256), extracts `variant.entry` in memory (pinned SHA-256), writes
    /// `<system>/<install_as>` atomically and updates the manifest. Replaces an `Installed` / `Modified` file
    /// without asking (it is ours); a `UserFile` follows `policy`.
    pub fn install(
        &self,
        bios: &FreeBios,
        variant: &str,
        policy: ReplacePolicy,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, BiosError> {
        let _ = (bios, variant, policy, progress, cancel);
        todo!("library-assets (2.0)")
    }

    /// Offline install from an archive the user downloaded himself; same checks as `install`.
    pub fn install_from_archive(
        &self,
        bios: &FreeBios,
        variant: &str,
        archive: &Path,
        policy: ReplacePolicy,
    ) -> Result<PathBuf, BiosError> {
        let _ = (bios, variant, archive, policy);
        todo!("library-assets (2.0)")
    }

    /// Removes the file only when it is ours and unchanged (`Installed`); returns `false` otherwise.
    pub fn remove(&self, bios: &FreeBios) -> Result<bool, BiosError> {
        let _ = bios;
        todo!("library-assets (2.0)")
    }

    /// md5 of every installed free BIOS file, for `check_firmware` (state `FirmwareState::Free`).
    pub fn installed_md5(&self) -> Vec<(String, [u8; 16], String)> {
        todo!("library-assets (2.0): (relative path, md5, label)")
    }
}
