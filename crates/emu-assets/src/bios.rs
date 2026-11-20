//! Free BIOS installer (ADR 0015 §2): EmuTOS for the Atari ST in 2.0, AROS for the Amiga in 3.0.
//! Catalogue embedded from `data/free_bios.toml`; every archive and every installed file is pinned by SHA-256.
//! Only free software is ever downloaded here (ADR 0005): never a commercial TOS or Kickstart.
//!
//! Default variant: EmuTOS 192 KB **French** (user decision recorded in ADR 0015). Callers pass the system
//! language to [`FreeBios::preferred_variant`]; when no 192 KB image matches it, `default_variant` (`192fr`)
//! is used. A variant chosen in the configuration (`[computer] emutos_variant`) always wins (caller side).
//!
//! Safety of the install (reviewed by `reviewer`):
//! - the archive is read in memory, bounded by its pinned size, and its SHA-256 checked before anything else;
//! - the image is found by **comparing** entry names (never used as a path, see `cores::archive`), bounded by
//!   its pinned size, then checked by size, SHA-256 and md5;
//! - the target name comes from the catalogue and must be a plain file name;
//! - a file emu-rust did not install is never overwritten: [`ReplacePolicy::BackupAndReplace`] renames it first;
//! - manifest then image are written atomically (temporary file + rename); a crash in between leaves a state
//!   (`Missing` / `Modified`) that a new install repairs without touching any user file.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use md5::Md5;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cores::{ArchiveError, ExtractLimits, MAX_FIRMWARE_SIZE, extract_core};
use crate::dat::write_atomic;
use crate::error::AssetError;
use crate::http::{CancelToken, HttpClient, HttpConfig, Progress};

/// Largest free BIOS archive accepted (EmuTOS 512k zip: 3.7 MB).
pub const MAX_BIOS_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
/// Largest extracted image (EmuTOS 1024k: 1 MiB; AROS ROMs: 512 KiB each).
pub const MAX_BIOS_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
/// Manifest of the files installed by emu-rust, in the system directory.
pub const MANIFEST_FILE_NAME: &str = ".emu-rust-bios.toml";
/// Lower bound applied to the client's timeout for BIOS downloads (slow SourceForge mirrors).
pub const BIOS_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

const BUILTIN: &str = include_str!("../data/free_bios.toml");
/// EmuTOS 192k zip: 49 entries.
const MAX_ARCHIVE_ENTRIES: usize = 512;
/// `.tmp-*` files older than this are leftovers of an interrupted write.
const STALE_TEMP_AGE: Duration = Duration::from_secs(3600);

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
    /// BCP 47 language tag used to preselect the variant (`fr`, `en-GB`), `None` for multilingual images.
    pub language: Option<String>,
    pub archive: String,
    /// Exact entry name in the archive (compared, never used as a path).
    pub entry: String,
    pub size: u64,
    pub sha256: [u8; 32],
    pub md5: [u8; 16],
}

impl FreeBios {
    pub fn variant(&self, id: &str) -> Option<&BiosVariant> {
        self.variants.iter().find(|v| v.id == id)
    }

    pub fn archive(&self, name: &str) -> Option<&BiosArchive> {
        self.archives.iter().find(|a| a.name == name)
    }

    /// Variant matching `locale` (`fr-FR`, `fr_FR.UTF-8`, `de-CH`, `en`…: full tag first, then the primary
    /// subtag; `nb` / `nn` count as `no`), else `default_variant`. `None` only for an empty catalogue entry.
    pub fn preferred_variant(&self, locale: Option<&str>) -> Option<&BiosVariant> {
        locale
            .and_then(|l| self.variant_for_locale(l))
            .or_else(|| self.variant(&self.default_variant))
            .or_else(|| self.variants.first())
    }

    fn variant_for_locale(&self, locale: &str) -> Option<&BiosVariant> {
        let tag = locale
            .split(['.', '@'])
            .next()
            .unwrap_or("")
            .replace('_', "-")
            .to_ascii_lowercase();
        let primary = match tag.split('-').next().unwrap_or("") {
            "nb" | "nn" => "no",
            p => p,
        };
        if primary.is_empty() {
            return None;
        }
        let with_language = |wanted: &str| {
            self.variants.iter().find(|v| {
                v.language
                    .as_deref()
                    .is_some_and(|l| l.eq_ignore_ascii_case(wanted))
            })
        };
        with_language(&tag).or_else(|| with_language(primary))
    }

    /// Text for `FirmwareState::Free` and the UI (« EmuTOS 192 Ko — français (ST/STE), version 1.4 »).
    pub fn label(&self, variant: &BiosVariant) -> String {
        format!("{}, version {}", variant.label, self.version)
    }
}

/// Catalogue embedded in the binary. Panics only if the embedded file is invalid (unit-tested).
pub fn builtin_free_bios() -> Vec<FreeBios> {
    match parse_free_bios(BUILTIN) {
        Ok(list) => list,
        Err(e) => panic!("data/free_bios.toml invalide : {e}"),
    }
}

/// Free BIOS of the embedded catalogue by id (`--install-bios emutos`).
pub fn find_free_bios(id: &str) -> Result<FreeBios, BiosError> {
    builtin_free_bios()
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(|| BiosError::UnknownBios(id.to_owned()))
}

/// `(install_as, md5, label)` of every variant: the input of `cores::check_firmware`, so that a free image under
/// the expected name is `FirmwareState::Free` whoever copied it there.
pub fn free_firmware_md5(catalogue: &[FreeBios]) -> Vec<(String, [u8; 16], String)> {
    catalogue
        .iter()
        .flat_map(|b| {
            b.variants
                .iter()
                .map(|v| (b.install_as.clone(), v.md5, b.label(v)))
        })
        .collect()
}

/// Parses and validates a catalogue in the `data/free_bios.toml` format (tests inject their own).
pub fn parse_free_bios(text: &str) -> Result<Vec<FreeBios>, String> {
    let file: CatalogueFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut ids = BTreeSet::new();
    let mut out = Vec::new();
    for entry in file.bios {
        let bios = entry.into_bios()?;
        validate(&bios)?;
        // At run time the client enforces HTTPS on every hop; the data file itself must say so too.
        if let Some(a) = bios
            .archives
            .iter()
            .find(|a| !a.url.starts_with("https://"))
        {
            return Err(format!("{} : URL non HTTPS", a.name));
        }
        if !ids.insert(bios.id.clone()) {
            return Err(format!("BIOS en double : {}", bios.id));
        }
        out.push(bios);
    }
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogueFile {
    #[serde(default)]
    bios: Vec<BiosEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BiosEntry {
    id: String,
    name: String,
    version: String,
    license: String,
    homepage: String,
    system: String,
    install_as: String,
    default_variant: String,
    #[serde(rename = "archive")]
    archives: Vec<ArchiveEntry>,
    #[serde(rename = "variant")]
    variants: Vec<VariantEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveEntry {
    name: String,
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantEntry {
    id: String,
    label: String,
    #[serde(default)]
    language: Option<String>,
    archive: String,
    entry: String,
    size: u64,
    md5: String,
    sha256: String,
}

impl BiosEntry {
    fn into_bios(self) -> Result<FreeBios, String> {
        let archives = self
            .archives
            .into_iter()
            .map(|a| {
                Ok(BiosArchive {
                    sha256: parse_hex(&a.sha256)
                        .ok_or_else(|| format!("sha256 invalide : {}", a.name))?,
                    name: a.name,
                    url: a.url,
                    size: a.size,
                })
            })
            .collect::<Result<_, String>>()?;
        let variants = self
            .variants
            .into_iter()
            .map(|v| {
                Ok(BiosVariant {
                    sha256: parse_hex(&v.sha256)
                        .ok_or_else(|| format!("sha256 invalide : {}", v.id))?,
                    md5: parse_hex(&v.md5).ok_or_else(|| format!("md5 invalide : {}", v.id))?,
                    id: v.id,
                    label: v.label,
                    language: v.language,
                    archive: v.archive,
                    entry: v.entry,
                    size: v.size,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(FreeBios {
            id: self.id,
            name: self.name,
            version: self.version,
            license: self.license,
            homepage: self.homepage,
            system: self.system,
            install_as: self.install_as,
            archives,
            variants,
            default_variant: self.default_variant,
        })
    }
}

fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    // `from_str_radix` alone would accept a leading `+`.
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_id(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A single plain file name: no separator, no drive, not hidden (the manifest and temporaries are), not `..`.
fn is_plain_file_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.starts_with('.')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// The fields are public: re-checked before every file system or network use, not only at parse time.
/// URL schemes are left to `HttpClient` (HTTPS only, loopback HTTP in tests).
fn validate(bios: &FreeBios) -> Result<(), String> {
    if !is_id(&bios.id) {
        return Err(format!("id invalide : {}", bios.id));
    }
    if !is_plain_file_name(&bios.install_as) {
        return Err(format!("{} : install_as refusé", bios.id));
    }
    let mut names = BTreeSet::new();
    for a in &bios.archives {
        if a.size == 0 || a.size > MAX_BIOS_ARCHIVE_BYTES {
            return Err(format!("{} : taille hors limites", a.name));
        }
        if !names.insert(a.name.as_str()) {
            return Err(format!("archive en double : {}", a.name));
        }
    }
    let mut ids = BTreeSet::new();
    for v in &bios.variants {
        if !is_id(&v.id) || !ids.insert(v.id.as_str()) {
            return Err(format!("variante invalide ou en double : {}", v.id));
        }
        if !names.contains(v.archive.as_str()) {
            return Err(format!("{} : archive inconnue {}", v.id, v.archive));
        }
        if v.size == 0 || v.size > MAX_BIOS_IMAGE_BYTES || v.entry.is_empty() {
            return Err(format!("{} : taille ou entrée invalide", v.id));
        }
    }
    if !ids.contains(bios.default_variant.as_str()) {
        return Err(format!(
            "{} : variante par défaut inconnue {}",
            bios.id, bios.default_variant
        ));
    }
    Ok(())
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
    /// A `FreeBios` built by hand (fields are public) that does not pass the catalogue checks.
    #[error("catalogue de BIOS invalide : {0}")]
    InvalidCatalogue(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    installed: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestEntry {
    file: String,
    bios: String,
    variant: String,
    version: String,
    label: String,
    sha256: String,
    md5: String,
    installed_unix: u64,
}

/// Installs, checks and removes free BIOS files in the system directory. Manifest:
/// `<system>/.emu-rust-bios.toml` (`[[installed]]`: `file`, `bios`, `variant`, `version`, `label`, `sha256`,
/// `md5`, `installed_unix`). `Send + Sync`; methods take `&self`; network only in `install` (blocking: worker
/// thread, never the UI thread). Writes are serialised inside one process; two processes sharing a data
/// directory are not coordinated (same as the 1.0 configuration files).
pub struct FreeBiosManager {
    system_dir: PathBuf,
    http: HttpClient,
    write: Mutex<()>,
}

impl FreeBiosManager {
    /// No network. Creates the system directory if needed; cleans `.tmp-*` leftovers older than 1 h.
    ///
    /// The client is rebuilt from `http.config()` with `same_host_redirects = false` (SourceForge answers with
    /// a redirection to a mirror; every hop is still HTTPS-only) and a timeout of at least
    /// [`BIOS_DOWNLOAD_TIMEOUT`].
    pub fn open(system_dir: &Path, http: HttpClient) -> Result<Self, BiosError> {
        let io_err = |source| BiosError::Io {
            path: system_dir.to_path_buf(),
            source,
        };
        fs::create_dir_all(system_dir).map_err(io_err)?;
        let config = HttpConfig {
            same_host_redirects: false,
            timeout: http.config().timeout.max(BIOS_DOWNLOAD_TIMEOUT),
            ..http.config().clone()
        };
        let http = if config == *http.config() {
            http
        } else {
            HttpClient::new(config)?
        };
        let manager = Self {
            system_dir: system_dir.to_path_buf(),
            http,
            write: Mutex::new(()),
        };
        manager.clean_stale_temporaries();
        Ok(manager)
    }

    pub fn state(&self, bios: &FreeBios) -> FreeBiosState {
        if validate(bios).is_err() {
            return FreeBiosState::Unreadable("catalogue invalide".into());
        }
        let path = self.system_dir.join(&bios.install_as);
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return FreeBiosState::Missing,
            Err(e) => return FreeBiosState::Unreadable(e.to_string()),
        }
        let (sha256, md5) = match hash_file(&path) {
            Ok(h) => h,
            Err(e) => return FreeBiosState::Unreadable(e.to_string()),
        };
        match self
            .read_manifest()
            .installed
            .into_iter()
            .find(|e| e.file == bios.install_as)
        {
            Some(e) if e.bios == bios.id && e.sha256 == hex(&sha256) => {
                FreeBiosState::Installed { variant: e.variant }
            }
            Some(_) => FreeBiosState::Modified,
            None => FreeBiosState::UserFile { md5 },
        }
    }

    /// Downloads the archive (pinned SHA-256), extracts `variant.entry` in memory (pinned SHA-256), writes
    /// `<system>/<install_as>` atomically and updates the manifest. Replaces an `Installed` / `Modified` file
    /// without asking (it is ours); a `UserFile` (or an unreadable file) follows `policy`, checked before the
    /// download and again before writing.
    pub fn install(
        &self,
        bios: &FreeBios,
        variant: &str,
        policy: ReplacePolicy,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, BiosError> {
        let (variant, archive) = resolve(bios, variant)?;
        let _guard = self.lock();
        self.check_policy(bios, policy)?;
        tracing::info!("téléchargement de {} ({})", archive.name, bios.license);
        let data = self
            .http
            .get_with_progress(&archive.url, archive.size, progress, cancel)?;
        if cancel.is_cancelled() {
            return Err(AssetError::Cancelled.into());
        }
        self.finish(bios, variant, archive, &data, policy)
    }

    /// Offline install from an archive the user downloaded himself; same checks as `install`.
    pub fn install_from_archive(
        &self,
        bios: &FreeBios,
        variant: &str,
        archive_path: &Path,
        policy: ReplacePolicy,
    ) -> Result<PathBuf, BiosError> {
        let (variant, archive) = resolve(bios, variant)?;
        let _guard = self.lock();
        self.check_policy(bios, policy)?;
        let io_err = |source| BiosError::Io {
            path: archive_path.to_path_buf(),
            source,
        };
        let file = File::open(archive_path).map_err(io_err)?;
        let mut data = Vec::new();
        file.take(MAX_BIOS_ARCHIVE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(io_err)?;
        if data.len() as u64 > MAX_BIOS_ARCHIVE_BYTES {
            return Err(AssetError::TooLarge {
                limit: MAX_BIOS_ARCHIVE_BYTES,
            }
            .into());
        }
        self.finish(bios, variant, archive, &data, policy)
    }

    /// Removes the file only when it is ours and unchanged (`Installed`); returns `false` otherwise.
    pub fn remove(&self, bios: &FreeBios) -> Result<bool, BiosError> {
        let _guard = self.lock();
        let state = self.state(bios);
        let path = self.system_dir.join(&bios.install_as);
        match state {
            FreeBiosState::Installed { .. } => {
                fs::remove_file(&path).map_err(|source| BiosError::Io {
                    path: path.clone(),
                    source,
                })?;
                crate::http::sync_dir(&self.system_dir);
                self.update_manifest(|m| m.installed.retain(|e| e.file != bios.install_as))?;
                Ok(true)
            }
            FreeBiosState::Missing => {
                // Stale entry (file deleted by hand): forget it.
                self.update_manifest(|m| m.installed.retain(|e| e.file != bios.install_as))?;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    /// md5 of every installed free BIOS file still intact, for `check_firmware` (state `FirmwareState::Free`):
    /// `(path relative to the system directory, md5, label)`.
    pub fn installed_md5(&self) -> Vec<(String, [u8; 16], String)> {
        self.read_manifest()
            .installed
            .into_iter()
            .filter(|e| is_plain_file_name(&e.file))
            .filter_map(|e| {
                let (sha256, md5) = hash_file(&self.system_dir.join(&e.file)).ok()?;
                (hex(&sha256) == e.sha256 && parse_hex::<16>(&e.md5) == Some(md5))
                    .then_some((e.file, md5, e.label))
            })
            .collect()
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.write.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn check_policy(&self, bios: &FreeBios, policy: ReplacePolicy) -> Result<(), BiosError> {
        match self.state(bios) {
            FreeBiosState::UserFile { .. } | FreeBiosState::Unreadable(_)
                if policy == ReplacePolicy::Refuse =>
            {
                Err(BiosError::UserFileExists(
                    self.system_dir.join(&bios.install_as),
                ))
            }
            _ => Ok(()),
        }
    }

    fn finish(
        &self,
        bios: &FreeBios,
        variant: &BiosVariant,
        archive: &BiosArchive,
        data: &[u8],
        policy: ReplacePolicy,
    ) -> Result<PathBuf, BiosError> {
        if data.len() as u64 != archive.size || Sha256::digest(data).as_slice() != archive.sha256 {
            return Err(BiosError::HashMismatch(archive.name.clone()));
        }
        let limits = ExtractLimits {
            max_entry_size: variant.size,
            max_total_size: variant.size,
            max_entries: MAX_ARCHIVE_ENTRIES,
        };
        let image = extract_core(data, &variant.entry, limits).map_err(|e| match e {
            ArchiveError::MissingEntry(name) => BiosError::MissingEntry(name),
            other => BiosError::Archive(other.to_string()),
        })?;
        if image.len() as u64 != variant.size
            || Sha256::digest(&image).as_slice() != variant.sha256
            || Md5::digest(&image).as_slice() != variant.md5
        {
            return Err(BiosError::HashMismatch(variant.entry.clone()));
        }

        let target = self.system_dir.join(&bios.install_as);
        // Authoritative check: a file may have appeared during the download.
        match self.state(bios) {
            FreeBiosState::UserFile { .. } | FreeBiosState::Unreadable(_) => match policy {
                ReplacePolicy::Refuse => return Err(BiosError::UserFileExists(target)),
                ReplacePolicy::BackupAndReplace => self.backup_user_file(&target)?,
            },
            FreeBiosState::Missing | FreeBiosState::Installed { .. } | FreeBiosState::Modified => {}
        }

        // Manifest first: if the image write fails or the process dies in between, the old file (ours or none:
        // a user file has just been moved away) reads as `Modified` / `Missing`, never as a user file lost.
        let previous = self.read_manifest();
        let entry = ManifestEntry {
            file: bios.install_as.clone(),
            bios: bios.id.clone(),
            variant: variant.id.clone(),
            version: bios.version.clone(),
            label: bios.label(variant),
            sha256: hex(&variant.sha256),
            md5: hex(&variant.md5),
            installed_unix: unix_now(),
        };
        self.update_manifest(|m| {
            m.installed.retain(|e| e.file != entry.file);
            m.installed.push(entry);
        })?;
        if let Err(e) = write_atomic(&target, &image) {
            if let Err(e2) = self.write_manifest(&previous) {
                tracing::warn!("manifeste des BIOS non restauré : {e2}");
            }
            return Err(e.into());
        }
        tracing::info!("{} installé : {}", bios.label(variant), target.display());
        Ok(target)
    }

    /// `<install_as>.user-<unix>` (plus `-<n>` if taken), in the same directory.
    fn backup_user_file(&self, target: &Path) -> Result<(), BiosError> {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stamp = unix_now();
        let mut backup = self.system_dir.join(format!("{name}.user-{stamp}"));
        let mut n = 1;
        while fs::symlink_metadata(&backup).is_ok() {
            backup = self.system_dir.join(format!("{name}.user-{stamp}-{n}"));
            n += 1;
        }
        fs::rename(target, &backup).map_err(|source| BiosError::Io {
            path: target.to_path_buf(),
            source,
        })?;
        crate::http::sync_dir(&self.system_dir);
        tracing::info!("BIOS de l'utilisateur conservé sous {}", backup.display());
        Ok(())
    }

    fn manifest_path(&self) -> PathBuf {
        self.system_dir.join(MANIFEST_FILE_NAME)
    }

    /// Missing or invalid manifest = nothing installed by us: every file then reads as a user file, the safe side.
    fn read_manifest(&self) -> Manifest {
        let path = self.manifest_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Manifest::default(),
            Err(e) => {
                tracing::warn!("{} illisible : {e}", path.display());
                return Manifest::default();
            }
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("{} invalide : {e}", path.display());
            Manifest::default()
        })
    }

    fn update_manifest(&self, change: impl FnOnce(&mut Manifest)) -> Result<(), BiosError> {
        let mut manifest = self.read_manifest();
        change(&mut manifest);
        self.write_manifest(&manifest)
    }

    fn write_manifest(&self, manifest: &Manifest) -> Result<(), BiosError> {
        let text = toml::to_string(manifest).map_err(|e| BiosError::Archive(e.to_string()))?;
        let text = format!(
            "# Free BIOS installed by emu-rust (ADR 0015). Do not edit: files listed here may be replaced.\n{text}"
        );
        write_atomic(&self.manifest_path(), text.as_bytes())?;
        Ok(())
    }

    /// Only names written by `write_atomic` (`.<name>.tmp-<6 alphanumerics>`), only files, only old ones.
    fn clean_stale_temporaries(&self) {
        let Ok(entries) = fs::read_dir(&self.system_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !is_temporary_name(name) {
                continue;
            }
            let stale = entry.metadata().is_ok_and(|m| {
                m.is_file()
                    && m.modified()
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > STALE_TEMP_AGE)
            });
            if stale {
                match fs::remove_file(entry.path()) {
                    Ok(()) => tracing::debug!("fichier temporaire supprimé : {name}"),
                    Err(e) => tracing::debug!("{name} non supprimé : {e}"),
                }
            }
        }
    }
}

fn is_temporary_name(name: &str) -> bool {
    name.starts_with('.')
        && name.rsplit_once(".tmp-").is_some_and(|(base, suffix)| {
            base.len() > 1 && suffix.len() == 6 && suffix.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

fn resolve<'a>(
    bios: &'a FreeBios,
    variant: &str,
) -> Result<(&'a BiosVariant, &'a BiosArchive), BiosError> {
    validate(bios).map_err(BiosError::InvalidCatalogue)?;
    let v = bios
        .variant(variant)
        .ok_or_else(|| BiosError::UnknownVariant(variant.to_owned()))?;
    let a = bios
        .archive(&v.archive)
        .ok_or_else(|| BiosError::InvalidCatalogue(format!("archive inconnue : {}", v.archive)))?;
    Ok((v, a))
}

/// SHA-256 and md5 of a file, bounded by `MAX_FIRMWARE_SIZE` (a user may have anything under that name).
fn hash_file(path: &Path) -> io::Result<([u8; 32], [u8; 16])> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::other("pas un fichier"));
    }
    let mut file = File::open(path)?.take(MAX_FIRMWARE_SIZE + 1);
    let mut sha = Sha256::new();
    let mut md5 = Md5::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        total += n as u64;
        if total > MAX_FIRMWARE_SIZE {
            return Err(io::Error::other("fichier trop volumineux"));
        }
        sha.update(&buf[..n]);
        md5.update(&buf[..n]);
    }
    Ok((sha.finalize().into(), md5.finalize().into()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalogue() {
        let list = builtin_free_bios();
        assert_eq!(list.len(), 1);
        let emutos = &list[0];
        assert_eq!(emutos.id, "emutos");
        assert_eq!(emutos.install_as, "tos.img");
        assert_eq!(emutos.system, "Atari - ST");
        assert_eq!(emutos.license, "GPL-2.0-or-later");
        // User decision (ADR 0015): French 192 KB by default.
        assert_eq!(emutos.default_variant, "192fr");
        assert_eq!(emutos.variants.len(), 20);
        for v in &emutos.variants {
            let a = emutos.archive(&v.archive).unwrap();
            assert!(
                a.url
                    .starts_with("https://downloads.sourceforge.net/project/emutos/emutos/1.4/")
            );
            assert!(a.url.ends_with(&a.name));
            assert!(v.entry.starts_with(a.name.trim_end_matches(".zip")));
            if v.id.starts_with("192") {
                assert_eq!(v.size, 196_608);
                assert_eq!(v.entry, format!("emutos-192k-1.4/etos{}.img", v.id));
                assert!(v.language.is_some());
            }
        }
        let fr = emutos.variant("192fr").unwrap();
        assert_eq!(
            hex(&fr.sha256),
            "9fe2fc9c4458818bd0d401fc7a5ad5d487c83a5d5501d75f7737fbb84b9a7692"
        );
        assert_eq!(hex(&fr.md5), "cf2847c15e5251653cd7e6c69d4a2d7f");
        assert_eq!(
            hex(&emutos.archive("emutos-192k-1.4.zip").unwrap().sha256),
            "59abac06a2d29b0864c5a7cfb2af65f022c337aed34188e174a9a08cc737e4bc"
        );
        assert_eq!(emutos.variant("1024k").unwrap().language, None);
        assert_eq!(find_free_bios("emutos").unwrap(), *emutos);
        assert!(matches!(
            find_free_bios("tos"),
            Err(BiosError::UnknownBios(_))
        ));
        let free = free_firmware_md5(&list);
        assert_eq!(free.len(), 20);
        assert!(free.iter().all(|(p, _, _)| p == "tos.img"));
    }

    #[test]
    fn preferred_variant_by_locale() {
        let list = builtin_free_bios();
        let emutos = &list[0];
        let pick = |l: Option<&str>| emutos.preferred_variant(l).unwrap().id.clone();
        assert_eq!(pick(None), "192fr");
        assert_eq!(pick(Some("fr-FR")), "192fr");
        assert_eq!(pick(Some("fr_CA.UTF-8")), "192fr");
        assert_eq!(pick(Some("en-US")), "192us");
        assert_eq!(pick(Some("en")), "192us");
        assert_eq!(pick(Some("en-GB")), "192uk");
        assert_eq!(pick(Some("de-CH")), "192sg");
        assert_eq!(pick(Some("de-AT")), "192de");
        assert_eq!(pick(Some("nb-NO")), "192no");
        assert_eq!(pick(Some("sv_SE")), "192se");
        assert_eq!(pick(Some("C")), "192fr");
        assert_eq!(pick(Some("pt-BR")), "192fr");
        assert_eq!(pick(Some("")), "192fr");
    }

    #[test]
    fn catalogue_validation() {
        let base = BUILTIN.to_owned();
        let bad = |from: &str, to: &str| {
            assert!(base.contains(from), "{from}");
            parse_free_bios(&base.replacen(from, to, 1)).unwrap_err()
        };
        bad("install_as = \"tos.img\"", "install_as = \"../tos.img\"");
        bad(
            "install_as = \"tos.img\"",
            "install_as = \".emu-rust-bios.toml\"",
        );
        bad("install_as = \"tos.img\"", "install_as = \"sub/tos.img\"");
        bad("url = \"https://", "url = \"http://");
        bad("size = 2260230", "size = 99999999");
        bad("default_variant = \"192fr\"", "default_variant = \"192xx\"");
        bad(
            "archive = \"emutos-1024k-1.4.zip\"",
            "archive = \"other.zip\"",
        );
        bad("id = \"emutos\"", "id = \"emu tos\"");
        bad("md5 = \"f187", "md5 = \"g187");
        bad("size = 1048576", "size = 99999999");
        bad(
            "[[bios.variant]]\nid = \"1024k\"",
            "[[bios.variant]]\nid = \"192fr\"",
        );
        assert!(parse_free_bios(&format!("{base}\nextra = 1\n")).is_err());
        assert!(parse_free_bios("").unwrap().is_empty());
    }

    #[test]
    fn temporary_names() {
        assert!(is_temporary_name(".tos.img.tmp-AbC123"));
        assert!(is_temporary_name(".emu-rust-bios.toml.tmp-zzzzzz"));
        assert!(!is_temporary_name("tos.img.tmp-AbC123"));
        assert!(!is_temporary_name(".tos.img.tmp-AbC12"));
        assert!(!is_temporary_name(".tos.img.tmp-AbC1_3"));
        assert!(!is_temporary_name(".tmp-AbC123"));
        assert!(!is_temporary_name(".emu-rust-bios.toml"));
    }

    #[test]
    fn hex_round_trip() {
        let h: [u8; 4] = parse_hex("00ff10Ab").unwrap();
        assert_eq!(hex(&h), "00ff10ab");
        assert!(parse_hex::<4>("00ff10").is_none());
        assert!(parse_hex::<2>("+1ff").is_none());
    }
}
