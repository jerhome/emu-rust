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
//! - a file recognised by hash as a PlayStation BIOS revision also gets a **region alias** (copy under
//!   `scph5500/5501/5502.bin`, ADR 0021 amendment 2026-11-23): never over a file that is not our own alias, the
//!   first alias of a region is kept, `alias_of` is recorded in the manifest;
//! - `[imported."<name>"]` (`source` = original file name without its folder, `md5`, `verified`) is recorded in
//!   `system/.emu-rust-bios.toml`; the original path is never logged above `debug`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use sha1::Sha1;

use super::{
    BiosError, FreeBios, ImportedEntry, ReplacePolicy, backup_file, hash_file, hex,
    is_temporary_name, read_manifest, unix_now, update_manifest,
};
use crate::cores::{
    CoreId, CoreInfo, KnownFirmware, PS1_BIOS, Ps1Bios, Ps1Region, is_safe_relative_path,
    ps1_lookup,
};

/// Largest file accepted by the import (largest known BIOS: 4 MiB; Amiga extended ROMs: 1 MiB). Distinct from
/// `cores::MAX_FIRMWARE_SIZE` (64 MiB), the bound of the 1.0 check of files already in the system directory.
pub const MAX_FIRMWARE_BYTES: u64 = 16 << 20;

/// Header of the Amiga Forever encrypted ROMs (decrypted by the core with `rom.key`).
const AMIGA_FOREVER_HEADER: &[u8; 11] = b"AMIROMTYPE1";
/// Target names: bytes, and folder depth (`.info` sub-folders are one or two levels deep).
const MAX_TARGET_NAME_BYTES: usize = 255;
const MAX_TARGET_DEPTH: usize = 4;

/// md5 → expected file names, from `data/firmware.toml`, the `.info` of the installed cores (md5 of their
/// `notes`, read at run time and never copied into our data: design §3.5.6) and the free BIOS catalogue.
#[derive(Debug, Clone, Default)]
pub struct FirmwareCatalogue {
    by_md5: BTreeMap<[u8; 16], Vec<ExpectedFirmware>>,
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
    /// Names that would be refused by [`import_firmware`] are left out. The first source naming a file gives its
    /// label (a `.info` description is better than the bare name); `system` and `size` are filled by any source.
    pub fn build(known: &KnownFirmware, infos: &[(CoreId, CoreInfo)], free: &[FreeBios]) -> Self {
        let mut expected: BTreeMap<String, ExpectedFirmware> = BTreeMap::new();
        let mut names_by_md5: BTreeMap<[u8; 16], BTreeSet<String>> = BTreeMap::new();
        let mut add = |name: &str,
                       label: &str,
                       system: Option<&str>,
                       size: Option<u64>,
                       md5s: &[[u8; 16]]| {
            if validate_target_name(name).is_err() {
                return;
            }
            let entry = expected
                .entry(name.to_owned())
                .or_insert_with(|| ExpectedFirmware {
                    name: name.to_owned(),
                    label: if label.trim().is_empty() { name } else { label }.to_owned(),
                    system: None,
                    size: None,
                });
            if entry.system.is_none() {
                entry.system = system.map(str::to_owned);
            }
            if entry.size.is_none() {
                entry.size = size;
            }
            for md5 in md5s {
                names_by_md5
                    .entry(*md5)
                    .or_default()
                    .insert(name.to_owned());
            }
        };
        for (_, info) in infos {
            let system = info.databases.first().map(String::as_str);
            for spec in &info.firmware {
                let md5: Vec<[u8; 16]> = spec.md5.into_iter().collect();
                add(
                    &spec.path,
                    &spec.description,
                    system,
                    known.size_for(&spec.path),
                    &md5,
                );
            }
        }
        for path in known.paths() {
            add(path, path, None, known.size_for(path), known.md5_for(path));
        }
        // A SoundFont is not a firmware: never an import target (design v4.0 §3.3).
        for bios in free.iter().filter(|b| b.kind == super::FreeBiosKind::Bios) {
            for variant in &bios.variants {
                add(
                    &bios.install_as,
                    &bios.name,
                    Some(&bios.system),
                    None,
                    &[variant.md5],
                );
            }
        }
        let by_md5 = names_by_md5
            .into_iter()
            .map(|(md5, names)| {
                let targets = names
                    .iter()
                    .filter_map(|n| expected.get(n).cloned())
                    .collect();
                (md5, targets)
            })
            .collect();
        Self {
            by_md5,
            expected: expected.into_values().collect(),
        }
    }

    /// Every expected file, sorted by name without duplicates: choices offered for an unverified import.
    pub fn expected_names(&self) -> Vec<ExpectedFirmware> {
        self.expected.clone()
    }

    /// Expected files for a md5 (empty when unknown), sorted by name.
    pub fn targets_for(&self, md5: &[u8; 16]) -> &[ExpectedFirmware] {
        self.by_md5.get(md5).map_or(&[], Vec::as_slice)
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
    /// A free BIOS installed by emu-rust, or a PlayStation region alias made by the import, unchanged
    /// (manifest): replaced without backup.
    FreeInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    pub installed: PathBuf,
    /// `<name>.user-<unix>` when a user file was moved aside.
    pub backup: Option<PathBuf>,
    /// md5 recomputed on the installed copy: the caller compares it with the one of [`identify_firmware`] (a
    /// source changed between the two calls then shows; a change during the copy is refused).
    pub md5: [u8; 16],
    /// Region aliases created by this import (PlayStation revisions only; empty otherwise, and when the
    /// canonical name was taken). A failure to make one is logged, never an error of the import.
    pub aliases: Vec<FirmwareAlias>,
}

/// A copy of a PlayStation BIOS made under the canonical name of its region (`scph5501.bin`...), so that cores
/// that only look that name up find it (ADR 0021 amendment 2026-11-23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareAlias {
    /// Canonical file name created, relative to the system directory.
    pub name: String,
    /// Name the imported file was installed under (the alias is a copy of it, and goes with it).
    pub source_name: String,
    /// Console model recognised by hash (`SCPH-7001`).
    pub revision: String,
    pub region: Ps1Region,
}

/// Hashes `src` (md5 for the lookup, sha1 for display only) and looks it up. `BiosError::TooLarge` beyond
/// [`MAX_FIRMWARE_BYTES`].
pub fn identify_firmware(
    src: &Path,
    catalogue: &FirmwareCatalogue,
) -> Result<Identification, BiosError> {
    let (file, _) = open_source(src)?;
    let mut header = Vec::with_capacity(AMIGA_FOREVER_HEADER.len());
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let size = stream(file, src, |chunk| {
        if header.len() < AMIGA_FOREVER_HEADER.len() {
            let take = (AMIGA_FOREVER_HEADER.len() - header.len()).min(chunk.len());
            header.extend_from_slice(&chunk[..take]);
        }
        md5.update(chunk);
        sha1.update(chunk);
        Ok(())
    })?;
    if header == AMIGA_FOREVER_HEADER {
        return Ok(Identification::AmigaForeverEncrypted {
            size,
            suggested: amiga_forever_name(src),
        });
    }
    let md5: [u8; 16] = md5.finalize().into();
    let sha1: [u8; 20] = sha1.finalize().into();
    let targets = catalogue.targets_for(&md5);
    Ok(if targets.is_empty() {
        Identification::Unknown { md5, sha1, size }
    } else {
        Identification::Known {
            md5,
            sha1,
            size,
            targets: targets.to_vec(),
        }
    })
}

/// State of `name` in `system_dir`, compared with the md5 of the file about to be imported
/// (`Identification::{Known, Unknown}::md5`).
pub fn target_state(
    system_dir: &Path,
    name: &str,
    md5: &[u8; 16],
) -> Result<TargetState, BiosError> {
    validate_target_name(name)?;
    let path = target_path(system_dir, name);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(TargetState::Absent),
        Err(source) => return Err(BiosError::Io { path, source }),
    }
    // A link (or a dangling one, a folder, a huge file) is read through: anything unreadable is an error, never
    // `Absent`, so that it is never replaced silently.
    let (sha256, current) = hash_file(&path).map_err(|source| BiosError::Io {
        path: path.clone(),
        source,
    })?;
    if current == *md5 {
        return Ok(TargetState::Same);
    }
    let manifest = read_manifest(system_dir);
    let ours = manifest
        .installed
        .iter()
        .any(|e| e.file == name && e.sha256 == hex(&sha256))
        || manifest
            .imported
            .get(name)
            .is_some_and(|e| e.alias_of.is_some() && e.md5 == hex(&current));
    Ok(if ours {
        TargetState::FreeInstalled
    } else {
        TargetState::Other { md5: current }
    })
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
    import_firmware_expecting(system_dir, src, name, policy, verified, None)
}

/// Like [`import_firmware`], with the md5 given by [`identify_firmware`]: when the md5 recomputed on the copy
/// differs (the source changed in between), `BiosError::Changed` is returned before anything is installed or
/// moved aside; the temporary file is removed.
pub fn import_firmware_expecting(
    system_dir: &Path,
    src: &Path,
    name: &str,
    policy: ReplacePolicy,
    verified: bool,
    expected_md5: Option<[u8; 16]>,
) -> Result<ImportOutcome, BiosError> {
    import_with_table(
        system_dir,
        src,
        name,
        policy,
        verified,
        expected_md5,
        PS1_BIOS,
    )
}

/// [`import_firmware_expecting`] with the PlayStation table as a parameter (tests inject fingerprints of fake
/// files: no real BIOS is ever needed).
fn import_with_table(
    system_dir: &Path,
    src: &Path,
    name: &str,
    policy: ReplacePolicy,
    verified: bool,
    expected_md5: Option<[u8; 16]>,
    ps1: &[Ps1Bios],
) -> Result<ImportOutcome, BiosError> {
    validate_target_name(name)?;
    fs::create_dir_all(system_dir).map_err(|source| BiosError::Io {
        path: system_dir.to_path_buf(),
        source,
    })?;
    let parent = prepare_parent(system_dir, name)?;
    let target = target_path(system_dir, name);
    let file_name = name.rsplit('/').next().unwrap_or(name);
    let io_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| BiosError::Io { path, source }
    };

    let (file, before) = open_source(src)?;
    let mut temp = tempfile::Builder::new()
        .prefix(&format!(".{file_name}.tmp-"))
        .tempfile_in(&parent)
        .map_err(io_err(&parent))?;
    let mut streamed = Md5::new();
    let copied = {
        let out = temp.as_file_mut();
        stream(file, src, |chunk| {
            streamed.update(chunk);
            out.write_all(chunk)
        })?
    };
    temp.as_file_mut().flush().map_err(io_err(&target))?;
    temp.as_file().sync_all().map_err(io_err(&target))?;
    // Changed while being copied (size or modification time): the bytes may be a mix of two files.
    let after = fs::metadata(src).map_err(source_err(src))?;
    if after.len() != copied
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
    {
        return Err(source_err(src)(io::Error::other(
            "fichier source modifié pendant la copie",
        )));
    }
    // Hash recomputed on the copy itself: what is recorded is what is installed.
    let md5 = {
        let mut hasher = Md5::new();
        let copy = temp.reopen().map_err(io_err(&target))?;
        let read = io::copy(&mut copy.take(MAX_FIRMWARE_BYTES + 1), &mut hasher)
            .map_err(io_err(&target))?;
        let md5: [u8; 16] = hasher.finalize().into();
        if read != copied || md5 != <[u8; 16]>::from(streamed.finalize()) {
            return Err(io_err(&target)(io::Error::other("copie altérée")));
        }
        md5
    };
    if expected_md5.is_some_and(|expected| expected != md5) {
        return Err(BiosError::Changed);
    }

    // Same lock as the free BIOS manager: the manifest update below must not interleave with an install.
    let _guard = super::lock_manifests();
    let mut backup = None;
    let mut noclobber = true;
    let replaces_free = match target_state(system_dir, name, &md5)? {
        TargetState::Same => {
            tracing::info!("BIOS déjà installé : {name}");
            // The user's own copy of an alias we made: it is a real file now (kept if the source goes away).
            let promoted = ImportedEntry {
                source: source_name(src),
                md5: hex(&md5),
                verified,
                imported_unix: unix_now(),
                alias_of: None,
            };
            if let Err(e) = update_manifest(system_dir, |m| {
                if let Some(e) = m.imported.get_mut(name)
                    && e.alias_of.is_some()
                    && e.md5 == promoted.md5
                {
                    *e = promoted;
                }
            }) {
                tracing::warn!("manifeste des BIOS non mis à jour : {e}");
            }
            let aliases = install_region_alias(system_dir, src, name, &target, &md5, ps1);
            return Ok(ImportOutcome {
                installed: target,
                backup: None,
                md5,
                aliases,
            });
        }
        TargetState::Absent => false,
        TargetState::FreeInstalled => {
            noclobber = false;
            true
        }
        TargetState::Other { .. } => match policy {
            ReplacePolicy::Refuse => return Err(BiosError::TargetExists(name.to_owned())),
            ReplacePolicy::BackupAndReplace => {
                backup = Some(backup_file(&target)?);
                false
            }
        },
    };
    // A file that appeared since `target_state` (copied by hand meanwhile) is never overwritten; the temporary
    // is removed when `PersistError` drops.
    let persisted = if noclobber {
        temp.persist_noclobber(&target)
    } else {
        temp.persist(&target)
    };
    persisted.map_err(|e| {
        if noclobber && e.error.kind() == io::ErrorKind::AlreadyExists {
            BiosError::TargetExists(name.to_owned())
        } else {
            BiosError::Io {
                path: target.clone(),
                source: e.error,
            }
        }
    })?;
    crate::http::sync_dir(&parent);
    tracing::info!(
        "BIOS importé : {name} ({})",
        if verified {
            "vérifié"
        } else {
            "non vérifié"
        }
    );
    tracing::debug!("source de l'import : {}", src.display());

    let entry = ImportedEntry {
        source: source_name(src),
        md5: hex(&md5),
        verified,
        imported_unix: unix_now(),
        alias_of: None,
    };
    // The file is in place: a manifest that cannot be written only loses the record. A replaced free BIOS whose
    // entry stays reads `Modified`, protected like a user file (the safe side).
    if let Err(e) = update_manifest(system_dir, |m| {
        if replaces_free {
            m.installed.retain(|e| e.file != name);
        }
        m.imported.insert(name.to_owned(), entry);
    }) {
        tracing::warn!("manifeste des BIOS non mis à jour : {e}");
    }
    let aliases = install_region_alias(system_dir, src, name, &target, &md5, ps1);
    Ok(ImportOutcome {
        installed: target,
        backup,
        md5,
        aliases,
    })
}

fn source_name(src: &Path) -> String {
    src.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Copies the installed `name` under the canonical file name of its PlayStation region when `md5` is a known
/// revision (never for an unknown hash). The canonical name must be free: an existing file (the user's, or an
/// alias made earlier from another revision) is left alone. Called with the manifest lock held. Best effort:
/// the import already succeeded, so a failure is only logged.
fn install_region_alias(
    system_dir: &Path,
    src: &Path,
    name: &str,
    installed: &Path,
    md5: &[u8; 16],
    ps1: &[Ps1Bios],
) -> Vec<FirmwareAlias> {
    let Some(revision) = ps1_lookup(ps1, md5) else {
        return Vec::new();
    };
    let canonical = revision.region.canonical_file();
    if name == canonical {
        return Vec::new();
    }
    let target = system_dir.join(canonical);
    match fs::symlink_metadata(&target) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => {
            tracing::info!("alias {canonical} non créé : le nom est déjà pris");
            return Vec::new();
        }
        Err(e) => {
            tracing::warn!("alias {canonical} non créé : {e}");
            return Vec::new();
        }
    }
    let made = (|| -> io::Result<()> {
        let mut temp = tempfile::Builder::new()
            .prefix(&format!(".{canonical}.tmp-"))
            .tempfile_in(system_dir)?;
        let mut hasher = Md5::new();
        let mut reader = File::open(installed)?.take(MAX_FIRMWARE_BYTES + 1);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            hasher.update(&buf[..n]);
            temp.as_file_mut().write_all(&buf[..n])?;
        }
        if <[u8; 16]>::from(hasher.finalize()) != *md5 {
            return Err(io::Error::other("copie altérée"));
        }
        temp.as_file().sync_all()?;
        temp.persist_noclobber(&target).map_err(|e| e.error)?;
        Ok(())
    })();
    if let Err(e) = made {
        tracing::warn!("alias {canonical} non créé : {e}");
        return Vec::new();
    }
    crate::http::sync_dir(system_dir);
    tracing::info!("BIOS PlayStation : copie sous {canonical} (alias de {name})");
    let entry = ImportedEntry {
        source: source_name(src),
        md5: hex(md5),
        verified: true,
        imported_unix: unix_now(),
        alias_of: Some(name.to_owned()),
    };
    if let Err(e) = update_manifest(system_dir, |m| {
        m.imported.insert(canonical.to_owned(), entry);
    }) {
        // Without its record the copy reads as a user file: kept, never replaced silently (the safe side).
        tracing::warn!("manifeste des BIOS non mis à jour : {e}");
    }
    vec![FirmwareAlias {
        name: canonical.to_owned(),
        source_name: name.to_owned(),
        revision: revision.model(),
        region: revision.region,
    }]
}

/// Removes a file recorded by [`import_firmware`] when it is still unchanged, together with the region aliases
/// made from it that are themselves unchanged. A file the user modified, or one that was never imported, is
/// left alone. Returns the names removed (the file first); empty when nothing was.
pub fn remove_imported_firmware(system_dir: &Path, name: &str) -> Result<Vec<String>, BiosError> {
    validate_target_name(name)?;
    let _guard = super::lock_manifests();
    let manifest = read_manifest(system_dir);
    let Some(entry) = manifest.imported.get(name) else {
        return Ok(Vec::new());
    };
    let unchanged = |file: &str, entry: &ImportedEntry| {
        hash_file(&target_path(system_dir, file)).is_ok_and(|(_, md5)| hex(&md5) == entry.md5)
    };
    let mut forget = Vec::new();
    let mut removed = Vec::new();
    if unchanged(name, entry) {
        remove_file_of(system_dir, name)?;
        removed.push(name.to_owned());
        forget.push(name.to_owned());
        for (alias, e) in &manifest.imported {
            if e.alias_of.as_deref() != Some(name) {
                continue;
            }
            if unchanged(alias, e) {
                remove_file_of(system_dir, alias)?;
                removed.push(alias.clone());
            }
            // Modified or already gone: nothing of ours is left to protect, the record only goes.
            forget.push(alias.clone());
        }
    } else if !target_path(system_dir, name).exists() {
        forget.push(name.to_owned());
    }
    if !forget.is_empty() {
        crate::http::sync_dir(system_dir);
        update_manifest(system_dir, |m| {
            m.imported.retain(|k, _| !forget.contains(k))
        })?;
    }
    Ok(removed)
}

fn remove_file_of(system_dir: &Path, name: &str) -> Result<(), BiosError> {
    let path = target_path(system_dir, name);
    match fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(BiosError::Io { path, source: e }),
        _ => Ok(()),
    }
}

/// Rules of the module doc. `/` is accepted (sub-folders declared by `.info` files, `vice/JiffyDOS_C64.bin`),
/// so callers must only pass names from the firmware catalogue (`expected_names()`), never free user input;
/// every component must be safe on every target, and the file name must not be hidden (manifest, temporary
/// files).
fn validate_target_name(name: &str) -> Result<(), BiosError> {
    let invalid = || BiosError::InvalidTargetName(name.to_owned());
    if name.len() > MAX_TARGET_NAME_BYTES
        || !is_safe_relative_path(name)
        || name.split('/').count() > MAX_TARGET_DEPTH
    {
        return Err(invalid());
    }
    let file_name = name.rsplit('/').next().unwrap_or(name);
    if file_name.starts_with('.') || is_temporary_name(file_name) {
        return Err(invalid());
    }
    Ok(())
}

fn target_path(system_dir: &Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(system_dir.to_path_buf(), |p, c| p.join(c))
}

/// Creates the sub-folders of `name` inside `system_dir`; an existing component must be a real folder (a link
/// could lead outside the system directory).
fn prepare_parent(system_dir: &Path, name: &str) -> Result<PathBuf, BiosError> {
    let mut dir = system_dir.to_path_buf();
    let components: Vec<&str> = name.split('/').collect();
    for component in &components[..components.len() - 1] {
        dir.push(component);
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(BiosError::InvalidTargetName(name.to_owned())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&dir).map_err(|source| BiosError::Io {
                    path: dir.clone(),
                    source,
                })?;
            }
            Err(source) => return Err(BiosError::Io { path: dir, source }),
        }
    }
    Ok(dir)
}

/// Errors about the source name only its file name (the user's folders stay out of messages and logs).
fn source_err(src: &Path) -> impl Fn(io::Error) -> BiosError {
    let path = PathBuf::from(src.file_name().unwrap_or(src.as_os_str()));
    move |source| BiosError::Io {
        path: path.clone(),
        source,
    }
}

/// Opens the source, links followed (users link BIOS collections), regular file only, size checked first.
fn open_source(src: &Path) -> Result<(File, fs::Metadata), BiosError> {
    let meta = fs::metadata(src).map_err(source_err(src))?;
    if !meta.is_file() {
        return Err(source_err(src)(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pas un fichier",
        )));
    }
    if meta.len() > MAX_FIRMWARE_BYTES {
        return Err(BiosError::TooLarge(meta.len()));
    }
    let file = File::open(src).map_err(source_err(src))?;
    Ok((file, meta))
}

/// Reads `file` by chunks, never beyond [`MAX_FIRMWARE_BYTES`] (the file may grow after `open_source`).
fn stream(
    file: File,
    src: &Path,
    mut each: impl FnMut(&[u8]) -> io::Result<()>,
) -> Result<u64, BiosError> {
    let mut reader = file.take(MAX_FIRMWARE_BYTES + 1);
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(source_err(src)(e)),
        };
        total += n as u64;
        if total > MAX_FIRMWARE_BYTES {
            return Err(BiosError::TooLarge(total));
        }
        each(&buf[..n]).map_err(source_err(src))?;
    }
    Ok(total)
}

/// Amiga Forever names (`amiga-os-130.rom`), lower-cased, when the source carries one.
fn amiga_forever_name(src: &Path) -> Option<String> {
    let name = src.file_name()?.to_str()?.to_ascii_lowercase();
    (name.starts_with("amiga-os-") && name.ends_with(".rom") && validate_target_name(&name).is_ok())
        .then_some(name)
}

#[cfg(test)]
mod tests {
    use super::super::{Manifest, ManifestEntry, write_manifest};
    use super::*;
    use crate::cores::parse_info;
    use sha2::Sha256;

    fn md5_of(data: &[u8]) -> [u8; 16] {
        Md5::digest(data).into()
    }

    /// Catalogue of fake files (never a real ROM): `ks13` expected as `kick.A500` and `sub/kick.A500`.
    fn catalogue() -> FirmwareCatalogue {
        let known = KnownFirmware::default();
        let text = format!(
            "database = \"Commodore - Amiga\"\nfirmware0_path = \"kick.A500\"\nfirmware0_desc = \"Kickstart 1.3\"\n\
             firmware1_path = \"sub/kick.A500\"\nfirmware2_path = \"other.rom\"\n\
             notes = \"(!) kick.A500 (md5): {0}|(!) sub/kick.A500 (md5): {0}\"\n",
            hex(&md5_of(b"ks13"))
        );
        let info = parse_info(CoreId::new("puae").unwrap(), &text).unwrap();
        FirmwareCatalogue::build(&known, &[(info.id.clone(), info)], &[])
    }

    fn write(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, data).unwrap();
        path
    }

    fn imported(system: &Path) -> BTreeMap<String, ImportedEntry> {
        read_manifest(system).imported
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn known_file_is_installed_under_every_name() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let src = write(src_dir.path(), "amiga-os-130.bin", b"ks13");
        let Identification::Known {
            md5, size, targets, ..
        } = identify_firmware(&src, &catalogue()).unwrap()
        else {
            panic!("not known")
        };
        assert_eq!(size, 4);
        let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["kick.A500", "sub/kick.A500"]);
        assert_eq!(targets[0].label, "Kickstart 1.3");
        assert_eq!(targets[0].system.as_deref(), Some("Commodore - Amiga"));
        for t in &targets {
            assert_eq!(
                target_state(system.path(), &t.name, &md5).unwrap(),
                TargetState::Absent
            );
            let out =
                import_firmware(system.path(), &src, &t.name, ReplacePolicy::Refuse, true).unwrap();
            assert_eq!(out.md5, md5);
            assert_eq!(out.backup, None);
            assert_eq!(fs::read(&out.installed).unwrap(), b"ks13");
            assert_eq!(
                target_state(system.path(), &t.name, &md5).unwrap(),
                TargetState::Same
            );
        }
        assert_eq!(
            fs::read(system.path().join("sub").join("kick.A500")).unwrap(),
            b"ks13"
        );
        let record = &imported(system.path())["kick.A500"];
        assert_eq!(record.source, "amiga-os-130.bin");
        assert_eq!(record.md5, hex(&md5));
        assert!(record.verified);
        // Same content again: nothing written, Ok.
        let out = import_firmware(
            system.path(),
            &src,
            "kick.A500",
            ReplacePolicy::Refuse,
            true,
        )
        .unwrap();
        assert_eq!(out.backup, None);
        assert_eq!(
            listing(system.path()),
            [".emu-rust-bios.toml", "kick.A500", "sub"]
        );
    }

    #[test]
    fn other_file_is_never_overwritten_silently() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let src = write(src_dir.path(), "ks.rom", b"ks13");
        write(system.path(), "kick.A500", b"mine");
        assert_eq!(
            target_state(system.path(), "kick.A500", &md5_of(b"ks13")).unwrap(),
            TargetState::Other {
                md5: md5_of(b"mine")
            }
        );
        let err = import_firmware(
            system.path(),
            &src,
            "kick.A500",
            ReplacePolicy::Refuse,
            true,
        )
        .unwrap_err();
        assert!(matches!(err, BiosError::TargetExists(ref n) if n == "kick.A500"));
        assert_eq!(
            listing(system.path()),
            ["kick.A500"],
            "nothing written, no temporary left"
        );
        assert_eq!(fs::read(system.path().join("kick.A500")).unwrap(), b"mine");

        let out = import_firmware(
            system.path(),
            &src,
            "kick.A500",
            ReplacePolicy::BackupAndReplace,
            true,
        )
        .unwrap();
        let backup = out.backup.unwrap();
        let backup_name = backup.file_name().unwrap().to_string_lossy().into_owned();
        assert!(backup_name.starts_with("kick.A500.user-"), "{backup_name}");
        assert_eq!(backup.parent(), Some(system.path()));
        assert_eq!(fs::read(&backup).unwrap(), b"mine");
        assert_eq!(fs::read(system.path().join("kick.A500")).unwrap(), b"ks13");
        // An imported file is a user file: replacing it again needs consent.
        let src2 = write(src_dir.path(), "ks2.rom", b"ks20");
        assert!(matches!(
            import_firmware(
                system.path(),
                &src2,
                "kick.A500",
                ReplacePolicy::Refuse,
                true
            ),
            Err(BiosError::TargetExists(_))
        ));
    }

    #[test]
    fn free_bios_installed_by_us_is_replaced_without_backup() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let emutos = b"emutos image";
        write(system.path(), "tos.img", emutos);
        let entry = ManifestEntry {
            file: "tos.img".into(),
            bios: "emutos".into(),
            variant: "192fr".into(),
            version: "1.4".into(),
            label: "EmuTOS".into(),
            sha256: hex(&Sha256::digest(emutos)),
            md5: hex(&md5_of(emutos)),
            installed_unix: 1,
        };
        write_manifest(
            system.path(),
            &Manifest {
                installed: vec![entry],
                ..Manifest::default()
            },
        )
        .unwrap();
        let src = write(src_dir.path(), "TOS 1.62.img", b"tos 1.62");
        assert_eq!(
            target_state(system.path(), "tos.img", &md5_of(b"tos 1.62")).unwrap(),
            TargetState::FreeInstalled
        );
        let out =
            import_firmware(system.path(), &src, "tos.img", ReplacePolicy::Refuse, true).unwrap();
        assert_eq!(out.backup, None);
        assert_eq!(
            fs::read(system.path().join("tos.img")).unwrap(),
            b"tos 1.62"
        );
        let manifest = read_manifest(system.path());
        assert!(manifest.installed.is_empty(), "no longer ours");
        assert_eq!(manifest.imported["tos.img"].source, "TOS 1.62.img");
        // Modified since our install: protected like a user file.
        write(system.path(), "tos.img", b"edited");
        assert!(matches!(
            target_state(system.path(), "tos.img", &md5_of(b"x")).unwrap(),
            TargetState::Other { .. }
        ));
    }

    #[test]
    fn unknown_file_only_unverified() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let src = write(src_dir.path(), "dump.bin", b"something");
        let id = identify_firmware(&src, &catalogue()).unwrap();
        assert!(matches!(id, Identification::Unknown { size: 9, .. }));
        let Identification::Unknown { sha1, .. } = id else {
            unreachable!()
        };
        assert_eq!(<[u8; 20]>::from(Sha1::digest(b"something")), sha1);
        let names: Vec<String> = catalogue()
            .expected_names()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["kick.A500", "other.rom", "sub/kick.A500"]);
        import_firmware(
            system.path(),
            &src,
            "other.rom",
            ReplacePolicy::Refuse,
            false,
        )
        .unwrap();
        let record = &imported(system.path())["other.rom"];
        assert!(!record.verified);
        assert_eq!(record.md5, hex(&md5_of(b"something")));
    }

    #[test]
    fn amiga_forever_encrypted_rom() {
        let dir = tempfile::tempdir().unwrap();
        let mut data = b"AMIROMTYPE1".to_vec();
        data.extend_from_slice(&[7u8; 64]);
        let src = write(dir.path(), "Amiga-OS-130.ROM", &data);
        assert_eq!(
            identify_firmware(&src, &catalogue()).unwrap(),
            Identification::AmigaForeverEncrypted {
                size: 75,
                suggested: Some("amiga-os-130.rom".into())
            }
        );
        let src = write(dir.path(), "kick.rom", &data);
        assert!(matches!(
            identify_firmware(&src, &catalogue()).unwrap(),
            Identification::AmigaForeverEncrypted {
                suggested: None,
                ..
            }
        ));
        // Header shorter than the magic: an ordinary unknown file.
        let src = write(dir.path(), "short.rom", b"AMIROM");
        assert!(matches!(
            identify_firmware(&src, &catalogue()).unwrap(),
            Identification::Unknown { .. }
        ));
    }

    #[test]
    fn hostile_target_names() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("system");
        let src = write(dir.path(), "x.rom", b"x");
        for name in [
            "",
            "../x",
            "/abs",
            "C:\\x",
            "C:x",
            "a\\b",
            "a/../b",
            "./x",
            "a//b",
            "x.",
            "x ",
            "NUL",
            "com1.rom",
            ".emu-rust-bios.toml",
            ".hidden",
            ".kick.A500.tmp-abcdef",
            "a/b/c/d/e",
        ] {
            assert!(
                matches!(
                    import_firmware(&system, &src, name, ReplacePolicy::BackupAndReplace, false),
                    Err(BiosError::InvalidTargetName(_))
                ),
                "{name:?}"
            );
            assert!(matches!(
                target_state(&system, name, &[0; 16]),
                Err(BiosError::InvalidTargetName(_))
            ));
        }
        let long = "a".repeat(256);
        assert!(matches!(
            import_firmware(&system, &src, &long, ReplacePolicy::Refuse, false),
            Err(BiosError::InvalidTargetName(_))
        ));
        assert_eq!(
            listing(dir.path()),
            ["x.rom"],
            "nothing created, not even the system directory"
        );
    }

    #[test]
    fn size_limit_and_source_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("system");
        let big = dir.path().join("big.bin");
        let file = File::create(&big).unwrap();
        file.set_len(MAX_FIRMWARE_BYTES + 1).unwrap();
        assert!(matches!(
            identify_firmware(&big, &catalogue()),
            Err(BiosError::TooLarge(n)) if n == MAX_FIRMWARE_BYTES + 1
        ));
        assert!(matches!(
            import_firmware(&system, &big, "big.bin", ReplacePolicy::Refuse, false),
            Err(BiosError::TooLarge(_))
        ));
        // Exactly at the limit: accepted.
        file.set_len(MAX_FIRMWARE_BYTES).unwrap();
        assert!(identify_firmware(&big, &catalogue()).is_ok());
        // A folder is not a BIOS; errors name the file only, never the user's folders.
        let err = identify_firmware(dir.path(), &catalogue()).unwrap_err();
        assert!(
            !err.to_string().contains(&*dir.path().to_string_lossy()),
            "{err}"
        );
        let err = identify_firmware(&dir.path().join("missing.rom"), &catalogue()).unwrap_err();
        assert!(
            !err.to_string().contains(&*dir.path().to_string_lossy()),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn links() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("system");
        fs::create_dir(&system).unwrap();
        let real = write(dir.path(), "real.rom", b"ks13");
        // A link as source is followed (BIOS collections), the content is copied, not the link.
        let link = dir.path().join("link.rom");
        symlink(&real, &link).unwrap();
        assert!(matches!(
            identify_firmware(&link, &catalogue()).unwrap(),
            Identification::Known { .. }
        ));
        let out =
            import_firmware(&system, &link, "kick.A500", ReplacePolicy::Refuse, true).unwrap();
        assert!(
            !fs::symlink_metadata(&out.installed)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(imported(&system)["kick.A500"].source, "link.rom");
        // A link to a folder is refused.
        let to_dir = dir.path().join("dir.rom");
        symlink(dir.path(), &to_dir).unwrap();
        assert!(matches!(
            identify_firmware(&to_dir, &catalogue()),
            Err(BiosError::Io { .. })
        ));
        // A sub-folder of the system directory that is a link (possibly outside): refused, nothing written.
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), system.join("sub")).unwrap();
        assert!(matches!(
            import_firmware(&system, &real, "sub/kick.A500", ReplacePolicy::Refuse, true),
            Err(BiosError::InvalidTargetName(_))
        ));
        assert!(listing(outside.path()).is_empty());
        // A dangling link at the target: an error, never `Absent`.
        symlink(dir.path().join("gone"), system.join("kick.A1200")).unwrap();
        assert!(matches!(
            target_state(&system, "kick.A1200", &md5_of(b"ks13")),
            Err(BiosError::Io { .. })
        ));
    }

    #[test]
    fn import_waits_for_the_manifest_lock() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("system");
        let src = write(dir.path(), "src.bin", b"ks13");
        let guard = crate::bios::lock_manifests();
        let sys = system.clone();
        let worker = std::thread::spawn(move || {
            import_firmware(&sys, &src, "kick.A500", ReplacePolicy::Refuse, true).unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            !system.join("kick.A500").exists(),
            "import ran under the lock"
        );
        drop(guard);
        worker.join().unwrap();
        assert!(system.join("kick.A500").exists());
        assert!(imported(&system).contains_key("kick.A500"));
    }

    #[test]
    fn hash_is_taken_from_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("system");
        let src = write(dir.path(), "ks.rom", b"ks13");
        let Identification::Known { md5, .. } = identify_firmware(&src, &catalogue()).unwrap()
        else {
            panic!()
        };
        // The source changes between identification and import: the outcome tells it, the record is the truth.
        write(dir.path(), "ks.rom", b"changed");
        let out = import_firmware(&system, &src, "kick.A500", ReplacePolicy::Refuse, true).unwrap();
        assert_ne!(out.md5, md5);
        assert_eq!(out.md5, md5_of(b"changed"));
        assert_eq!(imported(&system)["kick.A500"].md5, hex(&md5_of(b"changed")));
    }

    #[test]
    fn changed_source_is_refused_before_any_write() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("system");
        fs::create_dir(&system).unwrap();
        write(&system, "kick.A500", b"mine");
        let src = write(dir.path(), "ks.rom", b"ks13");
        let expected = md5_of(b"ks13");
        write(dir.path(), "ks.rom", b"changed");
        let err = import_firmware_expecting(
            &system,
            &src,
            "kick.A500",
            ReplacePolicy::BackupAndReplace,
            true,
            Some(expected),
        )
        .unwrap_err();
        assert!(matches!(err, BiosError::Changed));
        assert_eq!(listing(&system), ["kick.A500"], "no backup, no temporary");
        assert_eq!(fs::read(system.join("kick.A500")).unwrap(), b"mine");
        // Matching md5: installed as usual.
        write(dir.path(), "ks.rom", b"ks13");
        let out = import_firmware_expecting(
            &system,
            &src,
            "kick.A500",
            ReplacePolicy::BackupAndReplace,
            true,
            Some(expected),
        )
        .unwrap();
        assert!(out.backup.is_some());
        assert_eq!(out.md5, expected);
    }

    #[test]
    fn builtin_catalogue() {
        let known = KnownFirmware::builtin();
        let free = super::super::builtin_free_bios();
        let catalogue = FirmwareCatalogue::build(&known, &[], &free);
        let names = catalogue.expected_names();
        assert!(
            names.windows(2).all(|w| w[0].name < w[1].name),
            "sorted, no duplicates"
        );
        let kick = names.iter().find(|e| e.name == "kick34005.A500").unwrap();
        assert_eq!(kick.size, Some(262_144));
        let kick31 = names.iter().find(|e| e.name == "kick40068.A1200").unwrap();
        assert_eq!(kick31.size, Some(524_288));
        let targets = catalogue
            .targets_for(&crate::cores::KnownFirmware::builtin().md5_for("kick34005.A500")[0]);
        assert_eq!(targets.len(), 1);
        // Free BIOS images are recognised under their install name.
        let emutos = &free[0];
        let t = catalogue.targets_for(&emutos.variants[0].md5);
        assert!(
            t.iter()
                .any(|e| e.name == "tos.img" && e.system.as_deref() == Some("Atari - ST"))
        );
        // Same md5 for several names (Game Boy boot ROM): every name is a target.
        let gb = catalogue.targets_for(&known.md5_for("gb_bios.bin")[0]);
        let gb: Vec<&str> = gb.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(gb, ["dmg_boot.bin", "dmg_rom.bin", "gb_bios.bin"]);
    }

    // PlayStation region aliases (ADR 0021 amendment). Fake contents, fingerprints injected: no real BIOS.

    fn fake(file: &'static str, region: Ps1Region, data: &[u8]) -> Ps1Bios {
        Ps1Bios {
            file,
            region,
            crc32: 0,
            md5: Box::leak(hex(&md5_of(data)).into_boxed_str()),
            sha1: "",
        }
    }

    fn table() -> Vec<Ps1Bios> {
        vec![
            fake("scph7001.bin", Ps1Region::Usa, b"usa-a"),
            fake("scph5501.bin", Ps1Region::Usa, b"usa-b"),
            fake("scph7002.bin", Ps1Region::Europe, b"eu-a"),
        ]
    }

    fn import_ps1(system: &Path, src: &Path, name: &str) -> ImportOutcome {
        import_with_table(
            system,
            src,
            name,
            ReplacePolicy::Refuse,
            true,
            None,
            &table(),
        )
        .unwrap()
    }

    #[test]
    fn region_alias_is_created_and_recorded() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let src = write(src_dir.path(), "7001.bin", b"usa-a");
        let out = import_ps1(system.path(), &src, "scph7001.bin");
        assert_eq!(
            out.aliases,
            [FirmwareAlias {
                name: "scph5501.bin".into(),
                source_name: "scph7001.bin".into(),
                revision: "SCPH-7001".into(),
                region: Ps1Region::Usa,
            }]
        );
        assert_eq!(
            fs::read(system.path().join("scph5501.bin")).unwrap(),
            b"usa-a"
        );
        let record = &imported(system.path())["scph5501.bin"];
        assert_eq!(record.alias_of.as_deref(), Some("scph7001.bin"));
        assert_eq!(imported(system.path())["scph7001.bin"].alias_of, None);
        // Re-import: nothing new, the alias is kept.
        assert!(
            import_ps1(system.path(), &src, "scph7001.bin")
                .aliases
                .is_empty()
        );
        assert!(system.path().join("scph5501.bin").exists());
        // The alias is ours: shown as replaceable.
        assert_eq!(
            target_state(system.path(), "scph5501.bin", &md5_of(b"usa-b")).unwrap(),
            TargetState::FreeInstalled
        );
    }

    #[test]
    fn user_file_is_never_overwritten_by_an_alias() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        write(system.path(), "scph5501.bin", b"mine");
        let src = write(src_dir.path(), "7001.bin", b"usa-a");
        let out = import_ps1(system.path(), &src, "scph7001.bin");
        assert!(out.aliases.is_empty());
        assert_eq!(
            fs::read(system.path().join("scph5501.bin")).unwrap(),
            b"mine"
        );
        assert!(!imported(system.path()).contains_key("scph5501.bin"));
    }

    #[test]
    fn first_revision_keeps_the_alias() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let a = write(src_dir.path(), "a.bin", b"usa-a");
        let b = write(src_dir.path(), "b.bin", b"usa-b");
        assert_eq!(
            import_ps1(system.path(), &a, "scph7001.bin").aliases.len(),
            1
        );
        // A second USA revision (named otherwise than canonical) finds the name taken by our alias.
        assert!(
            import_ps1(system.path(), &b, "other.bin")
                .aliases
                .is_empty()
        );
        assert_eq!(
            fs::read(system.path().join("scph5501.bin")).unwrap(),
            b"usa-a"
        );
    }

    #[test]
    fn real_file_replaces_our_alias_without_backup() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let a = write(src_dir.path(), "a.bin", b"usa-a");
        let b = write(src_dir.path(), "b.bin", b"usa-b");
        import_ps1(system.path(), &a, "scph7001.bin");
        let out = import_ps1(system.path(), &b, "scph5501.bin");
        assert_eq!(out.backup, None);
        assert!(out.aliases.is_empty());
        assert_eq!(
            fs::read(system.path().join("scph5501.bin")).unwrap(),
            b"usa-b"
        );
        assert_eq!(imported(system.path())["scph5501.bin"].alias_of, None);
        assert!(listing(system.path()).iter().all(|n| !n.contains(".user-")));
        // The same content imported as the real file promotes the alias.
        let system2 = tempfile::tempdir().unwrap();
        import_ps1(system2.path(), &a, "scph7001.bin");
        import_ps1(system2.path(), &a, "scph5501.bin");
        assert_eq!(imported(system2.path())["scph5501.bin"].alias_of, None);
        // ...so it survives the removal of its former source.
        let removed = remove_imported_firmware(system2.path(), "scph7001.bin").unwrap();
        assert_eq!(removed, ["scph7001.bin"]);
        assert!(system2.path().join("scph5501.bin").exists());
    }

    #[test]
    fn alias_goes_with_its_source() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let a = write(src_dir.path(), "a.bin", b"eu-a");
        import_ps1(system.path(), &a, "scph7002.bin");
        assert!(system.path().join("scph5502.bin").exists());
        let removed = remove_imported_firmware(system.path(), "scph7002.bin").unwrap();
        assert_eq!(removed, ["scph7002.bin", "scph5502.bin"]);
        assert!(imported(system.path()).is_empty());
        assert_eq!(listing(system.path()), [".emu-rust-bios.toml"]);
        // A modified alias is kept: it is the user's now.
        import_ps1(system.path(), &a, "scph7002.bin");
        fs::write(system.path().join("scph5502.bin"), b"edited").unwrap();
        let removed = remove_imported_firmware(system.path(), "scph7002.bin").unwrap();
        assert_eq!(removed, ["scph7002.bin"]);
        assert_eq!(
            fs::read(system.path().join("scph5502.bin")).unwrap(),
            b"edited"
        );
    }

    #[test]
    fn region_is_crossed_never_and_unknown_hash_has_no_alias() {
        let src_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let eu = write(src_dir.path(), "eu.bin", b"eu-a");
        let out = import_ps1(system.path(), &eu, "scph7002.bin");
        assert_eq!(out.aliases[0].name, "scph5502.bin");
        assert!(!system.path().join("scph5501.bin").exists());
        let unknown = write(src_dir.path(), "x.bin", b"who knows");
        let out = import_with_table(
            system.path(),
            &unknown,
            "scph7000.bin",
            ReplacePolicy::Refuse,
            false,
            None,
            &table(),
        )
        .unwrap();
        assert!(out.aliases.is_empty());
        assert_eq!(
            listing(system.path()),
            [
                ".emu-rust-bios.toml",
                "scph5502.bin",
                "scph7000.bin",
                "scph7002.bin"
            ]
        );
    }

    #[test]
    fn real_table_names_cover_the_canonical_files() {
        for region in [Ps1Region::Japan, Ps1Region::Usa, Ps1Region::Europe] {
            let file = region.canonical_file();
            assert_eq!(Ps1Region::from_canonical_file(file), Some(region));
            assert!(
                PS1_BIOS
                    .iter()
                    .any(|b| b.file == file && b.region == region)
            );
        }
    }
}
