//! Firmware (BIOS) checks. Nothing is ever downloaded here (ADR 0005): commercial BIOS are provided by the
//! user in the system directory; the free ones (EmuTOS, AROS) have their own installer (`crate::bios`).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::Deserialize;

use super::info::{is_safe_relative_path, parse_md5};
use super::select::RequiredFirmware;
use super::{CoreId, CoreInfo, CorePreferences, FirmwareSpec};

const BUILTIN: &str = include_str!("../../data/firmware.toml");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FirmwareFile {
    firmware: BTreeMap<String, FirmwareEntry>,
}

/// `"name" = ["md5", …]` (1.0 form) or `"name" = { md5 = "…" | ["…", …], size = 262144 }` (design v3.0 §3.5.6).
#[derive(Deserialize)]
#[serde(untagged)]
enum FirmwareEntry {
    List(Vec<String>),
    Table(FirmwareTable),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FirmwareTable {
    md5: OneOrMany,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// Files larger than this are reported `Unreadable` without hashing (largest 1.0-era BIOS: 128 KiB;
/// CD BIOS: 512 KiB).
pub const MAX_FIRMWARE_SIZE: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirmwareState {
    /// md5 matches one of the known hashes.
    Ok,
    Missing,
    WrongHash {
        md5: [u8; 16],
    },
    /// Present but no reference hash known.
    Unverified,
    /// md5 of a free BIOS from `data/free_bios.toml` (EmuTOS in place of TOS): no "missing / wrong hash"
    /// warning. `label` is shown as is (« EmuTOS 192 Ko — français (ST/STE), version 1.4 »).
    Free {
        label: String,
    },
    Unreadable(String),
    /// [`check_required`] only: the wanted file is absent but the core declares a fallback
    /// (`required_firmware.fallback` of `data/cores.toml`; puae: built-in AROS). Neither "missing" nor
    /// blocking; `label` is the `builtin_firmware` label of the core.
    CoreFallback {
        label: String,
    },
    /// [`check_required`] only: the core has `builtin_firmware` and no `required_firmware` table for the
    /// system (ROMs compiled into the core: cap32, VICE). Nothing is expected in the system directory.
    Builtin {
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareStatus {
    pub spec: FirmwareSpec,
    pub state: FirmwareState,
}

/// Reference hashes not given by the `.info` files (e.g. genesis_plus_gx lists 12 files, hashes for 2),
/// from libretro-database `dat/System.dat`, embedded from `data/firmware.toml`.
#[derive(Debug, Clone, Default)]
pub struct KnownFirmware {
    by_path: BTreeMap<String, Vec<[u8; 16]>>,
    sizes: BTreeMap<String, u64>,
}

impl KnownFirmware {
    /// Parses the embedded `data/firmware.toml`; a parse error is a bug caught by a unit test.
    pub fn builtin() -> Self {
        match Self::parse(BUILTIN) {
            Ok(known) => known,
            Err(e) => {
                tracing::error!("data/firmware.toml invalide : {e}");
                Self::default()
            }
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        let file: FirmwareFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut by_path = BTreeMap::new();
        let mut sizes = BTreeMap::new();
        for (path, entry) in file.firmware {
            if !is_safe_relative_path(&path) {
                return Err(format!("chemin refusé : {path}"));
            }
            let (hashes, size) = match entry {
                FirmwareEntry::List(hashes) => (hashes, None),
                FirmwareEntry::Table(FirmwareTable { md5, size }) => match md5 {
                    OneOrMany::One(h) => (vec![h], size),
                    OneOrMany::Many(hashes) => (hashes, size),
                },
            };
            if hashes.is_empty() && size.is_some() {
                return Err(format!("taille sans md5 pour {path}"));
            }
            if let Some(size) = size {
                if size == 0 || size > MAX_FIRMWARE_SIZE {
                    return Err(format!("taille invalide pour {path}"));
                }
                sizes.insert(path.clone(), size);
            }
            let hashes = hashes
                .iter()
                .map(|h| parse_md5(h).ok_or_else(|| format!("md5 invalide pour {path}")))
                .collect::<Result<Vec<_>, _>>()?;
            by_path.insert(path, hashes);
        }
        Ok(Self { by_path, sizes })
    }

    /// Accepted md5 for a relative path (case-sensitive, `/`-separated).
    pub fn md5_for(&self, path: &str) -> &[[u8; 16]] {
        self.by_path.get(path).map_or(&[], Vec::as_slice)
    }

    /// Expected size of `path`, when `data/firmware.toml` gives one (table form
    /// `"name" = { md5 = "…" | ["…", …], size = 262144 }`, design v3.0 §3.5.6). Only a quick reject at import and
    /// a display hint: the md5 stays the sole verification.
    pub fn size_for(&self, path: &str) -> Option<u64> {
        self.sizes.get(path).copied()
    }

    /// Every known relative path, sorted (import catalogue).
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }
}

/// Result of the single pre-launch firmware check ([`check_required`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareCheck {
    /// File wanted for the variant (relative to the system directory); `None` when nothing is expected
    /// (`Builtin`).
    pub wanted: Option<String>,
    /// `Ok` / `Free` / `Unverified` / `Builtin`: launch. `CoreFallback`: launch, the UI warns once per game.
    /// `Missing` (no fallback declared) / `WrongHash` / `Unreadable`: the UI asks.
    pub state: FirmwareState,
    /// Other files of the same table present in the system directory (« bios_CD_E.bin présent, mais ce jeu
    /// est USA »), sorted, without `wanted`.
    pub others_present: Vec<String>,
}

/// The single firmware check before a launch (design v3.0 §3.5.5), also used by `--check-bios`.
///
/// `variant` is the value the table is keyed by (`RequiredFirmware::variant`: resolved model for the Amiga,
/// disc region for Mega CD / PlayStation); `None` or a variant absent from `req.files` selects `req.default`.
/// The wanted file is looked up in `system_dir` and hashed like [`check_firmware`] does (md5 from `known`;
/// md5 that only exist in a core's `.info` are not known here: such a file is `Unverified`). `fallback_label`
/// is `CorePreferences::builtin_firmware(core).label` when `req.fallback` is set: a missing file is then
/// `CoreFallback { label }`, otherwise `Missing`. Nothing is written, nothing is downloaded.
pub fn check_required(
    system_dir: &Path,
    req: &RequiredFirmware,
    variant: Option<&str>,
    known: &KnownFirmware,
    fallback_label: Option<&str>,
) -> Result<FirmwareCheck, FirmwareError> {
    if let Some(path) = req.files.values().find(|p| !is_safe_relative_path(p)) {
        return Err(FirmwareError::UnsafePath(path.clone()));
    }
    let default = req
        .files
        .get(&req.default)
        .ok_or_else(|| FirmwareError::UnknownDefault(req.default.clone()))?;
    let wanted = variant.and_then(|v| req.files.get(v)).unwrap_or(default);
    let spec = FirmwareSpec {
        path: wanted.clone(),
        description: wanted.clone(),
        optional: false,
        md5: None,
    };
    let state = match check_one(system_dir, &spec, known, &[]) {
        FirmwareState::Missing if req.fallback.is_some() => FirmwareState::CoreFallback {
            label: fallback_label.unwrap_or(DEFAULT_FALLBACK_LABEL).to_owned(),
        },
        state => state,
    };
    let mut others_present: Vec<String> = req
        .files
        .values()
        .filter(|p| *p != wanted && is_regular_file(&join_relative(system_dir, p)))
        .cloned()
        .collect();
    others_present.sort();
    others_present.dedup();
    Ok(FirmwareCheck {
        wanted: Some(wanted.clone()),
        state,
        others_present,
    })
}

/// Label of a `CoreFallback` when the caller has none (a fallback without `builtin_firmware` is refused when
/// `data/cores.toml` is parsed, so only hand-built tables get it).
const DEFAULT_FALLBACK_LABEL: &str = "ROM intégrée au core";

impl FirmwareCheck {
    /// Result for a core with `builtin_firmware` and no `required_firmware` table for the system: nothing is
    /// expected in the system directory.
    pub fn builtin(label: &str) -> Self {
        Self {
            wanted: None,
            state: FirmwareState::Builtin {
                label: label.to_owned(),
            },
            others_present: Vec::new(),
        }
    }
}

/// The whole decision of design v3.0 §3.5.5 for one (core, system): the `required_firmware` table when there is
/// one ([`check_required`], with the core's `builtin_firmware` label as fallback label), else
/// [`FirmwareCheck::builtin`] when the core has ROMs built in, else `None` (the caller keeps the 1.0 `.info`
/// check, a non-blocking warning).
pub fn check_core_firmware(
    system_dir: &Path,
    prefs: &CorePreferences,
    core: &CoreId,
    system: &str,
    variant: Option<&str>,
    known: &KnownFirmware,
) -> Option<Result<FirmwareCheck, FirmwareError>> {
    let builtin = prefs.builtin_firmware(core);
    match prefs.required_firmware(core, system) {
        Some(req) => Some(check_required(
            system_dir,
            req,
            variant,
            known,
            builtin.as_ref().map(|b| b.label.as_str()),
        )),
        None => builtin.map(|b| Ok(FirmwareCheck::builtin(&b.label))),
    }
}

fn join_relative(system_dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(system_dir.to_path_buf(), |p, c| p.join(c))
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// Errors of [`check_required`]: data problems only (a missing or wrong file is a [`FirmwareState`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FirmwareError {
    /// `req.default` is not a key of `req.files` (hand-built table; the embedded one is checked by a test).
    #[error("table de firmware invalide : variante par défaut inconnue ({0})")]
    UnknownDefault(String),
    /// A file name of the table is not a safe relative path.
    #[error("table de firmware invalide : chemin refusé ({0})")]
    UnsafePath(String),
}

/// One status per `info.firmware` entry, same order. A spec's own md5 (from `notes`) and `known` are both
/// accepted. Symlinks are followed (users link BIOS collections); the target is hashed.
///
/// `free`: `(path relative to the system directory, md5, label)` of free BIOS images
/// (`crate::bios::free_firmware_md5`, or `FreeBiosManager::installed_md5`). A file that is not `Ok` but matches
/// one of them is `Free` rather than `WrongHash` / `Unverified`.
pub fn check_firmware(
    system_dir: &Path,
    info: &CoreInfo,
    known: &KnownFirmware,
    free: &[(String, [u8; 16], String)],
) -> Vec<FirmwareStatus> {
    info.firmware
        .iter()
        .map(|spec| FirmwareStatus {
            state: check_one(system_dir, spec, known, free),
            spec: spec.clone(),
        })
        .collect()
}

fn check_one(
    system_dir: &Path,
    spec: &FirmwareSpec,
    known: &KnownFirmware,
    free: &[(String, [u8; 16], String)],
) -> FirmwareState {
    check_one_with(system_dir, spec, known, free, super::PS1_BIOS)
}

/// [`check_one`] with the PlayStation revision table as a parameter (tests inject fake fingerprints).
fn check_one_with(
    system_dir: &Path,
    spec: &FirmwareSpec,
    known: &KnownFirmware,
    free: &[(String, [u8; 16], String)],
    ps1: &[super::Ps1Bios],
) -> FirmwareState {
    // `FirmwareSpec` fields are public: re-checked here, not only at parse time.
    if !is_safe_relative_path(&spec.path) {
        return FirmwareState::Unreadable("chemin refusé".into());
    }
    let path = join_relative(system_dir, &spec.path);
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return FirmwareState::Missing,
        Err(e) => return FirmwareState::Unreadable(e.to_string()),
    };
    if !meta.is_file() {
        return FirmwareState::Unreadable("pas un fichier".into());
    }
    if meta.len() > MAX_FIRMWARE_SIZE {
        return FirmwareState::Unreadable("fichier trop volumineux".into());
    }
    let md5 = match md5_file(&path) {
        Ok(md5) => md5,
        Err(e) => return FirmwareState::Unreadable(e.to_string()),
    };
    let mut accepted = known
        .md5_for(&spec.path)
        .iter()
        .chain(spec.md5.iter())
        .peekable();
    let has_reference = accepted.peek().is_some();
    if accepted.any(|h| *h == md5) {
        return FirmwareState::Ok;
    }
    // A PlayStation dump of the same region under the canonical name is an alias made by the import (ADR 0021,
    // amendment 2026-11-23); another region stays `WrongHash` (see `ps1_wrong_region`).
    if let Some(region) = super::Ps1Region::from_canonical_file(&spec.path)
        && super::ps1_lookup(ps1, &md5).is_some_and(|b| b.region == region)
    {
        return FirmwareState::Ok;
    }
    if let Some((_, _, label)) = free.iter().find(|(p, h, _)| *p == spec.path && *h == md5) {
        return FirmwareState::Free {
            label: label.clone(),
        };
    }
    if has_reference {
        FirmwareState::WrongHash { md5 }
    } else {
        FirmwareState::Unverified
    }
}

fn md5_file(path: &Path) -> io::Result<[u8; 16]> {
    let file = File::open(path)?;
    let mut hasher = Md5::new();
    // The size may have grown since `metadata`: bounded read anyway.
    let copied = io::copy(&mut file.take(MAX_FIRMWARE_SIZE + 1), &mut hasher)?;
    if copied > MAX_FIRMWARE_SIZE {
        return Err(io::Error::other("fichier trop volumineux"));
    }
    Ok(hasher.finalize().into())
}

/// Kind of a [`FirmwareEnhancement`] (ADR 0023 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnhancementKind {
    /// File of a `required_firmware` table that declares a `fallback`: the core runs a built-in replacement
    /// meanwhile (PlayStation HLE / OpenBIOS, 3DO, Amiga AROS).
    Fallback,
    /// `optional_firmware` entry of `data/cores.toml` listed by the installed `.info`.
    Optional,
}

/// An original file, provided by the user, that improves a core for a system. Never downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareEnhancement {
    /// Relative to the system directory.
    pub file: String,
    /// Key of the `required_firmware` table (`"europe"`, `"A500"`…); `None` for `Optional`.
    pub variant: Option<String>,
    pub kind: EnhancementKind,
    /// French, shown as is. `Fallback`: « BIOS d'origine : meilleure compatibilité que le remplaçant du core ».
    pub benefit: String,
    /// `Ok` / `Missing` / `WrongHash` / `Unverified` (md5 from `firmware.toml`, else the `.info`).
    pub state: FirmwareState,
}

/// Original files that improve `core` for `system`: every file of a `required_firmware` table that declares a
/// `fallback`, then the `optional_firmware` entries that the installed `.info` lists. Nothing is downloaded.
pub fn firmware_enhancements(
    prefs: &CorePreferences,
    core: &CoreId,
    system: &str,
    info: Option<&CoreInfo>,
    system_dir: &Path,
    known: &KnownFirmware,
) -> Result<Vec<FirmwareEnhancement>, FirmwareError> {
    let mut found: Vec<FirmwareEnhancement> = Vec::new();
    let info_md5 = |file: &str| {
        info.and_then(|i| i.firmware.iter().find(|s| s.path == file))
            .and_then(|s| s.md5)
    };
    let state_of = |file: &str| {
        let spec = FirmwareSpec {
            path: file.to_owned(),
            description: file.to_owned(),
            optional: true,
            md5: info_md5(file),
        };
        check_one(system_dir, &spec, known, &[])
    };
    if let Some(req) = prefs.required_firmware(core, system)
        && req.fallback.is_some()
    {
        if let Some(path) = req.files.values().find(|p| !is_safe_relative_path(p)) {
            return Err(FirmwareError::UnsafePath(path.clone()));
        }
        // `files` is a BTreeMap: variants come sorted, so the variant kept for a shared file is stable.
        for (variant, file) in &req.files {
            if found.iter().any(|e| e.file == *file) {
                continue;
            }
            found.push(FirmwareEnhancement {
                file: file.clone(),
                variant: Some(variant.clone()),
                kind: EnhancementKind::Fallback,
                benefit: FALLBACK_BENEFIT.to_owned(),
                state: state_of(file),
            });
        }
    }
    // Inert unless the installed `.info` lists the file (an unconfirmed name is never proposed), except for
    // entries marked `outside_info` whose name comes from the core's own documentation.
    for opt in prefs.optional_firmware(core, system) {
        if !is_safe_relative_path(&opt.file) {
            return Err(FirmwareError::UnsafePath(opt.file.clone()));
        }
        let listed = info.is_some_and(|i| i.firmware.iter().any(|s| s.path == opt.file));
        if !(listed || opt.outside_info) || found.iter().any(|e| e.file == opt.file) {
            continue;
        }
        found.push(FirmwareEnhancement {
            file: opt.file.clone(),
            variant: None,
            kind: EnhancementKind::Optional,
            benefit: opt.benefit.clone(),
            // Dumps of other revisions exist and only some md5 are known: never claim a wrong hash.
            state: match state_of(&opt.file) {
                FirmwareState::WrongHash { .. } if opt.outside_info => FirmwareState::Unverified,
                other => other,
            },
        });
    }
    Ok(found)
}

const FALLBACK_BENEFIT: &str = "BIOS d'origine : meilleure compatibilité que le remplaçant du core";

/// Why an [`AdviceFile`] is listed (ADR 0023 §5, design v7.5 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdviceNeed {
    /// File of a `required_firmware` table without `fallback`: the game does not run without it.
    Required,
    /// A file that makes the core better (`firmware_enhancements`: fallback tables, `optional_firmware`).
    Improves,
}

/// One original file shown by the firmware banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdviceFile {
    /// Relative to the system directory.
    pub name: String,
    pub need: AdviceNeed,
    /// French, shown as is.
    pub benefit: String,
    /// `Missing`, `WrongHash`, `Unverified`, `Unreadable` (`Ok` / `Free` files are never listed).
    pub state: FirmwareState,
}

/// Banner data for a core: what to ask the user for, never downloaded (ADR 0005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareAdvice {
    pub core: CoreId,
    /// `Required` files first, then `Improves`; never empty.
    pub files: Vec<AdviceFile>,
}

const REQUIRED_BENEFIT: &str = "BIOS indispensable : sans lui, le jeu ne démarre pas";

/// Banner of a core that `cores.toml` does not list (custom system, design 8.0 §2.2, ADR 0025 §6): files of
/// `info.firmware` minus `ignored` (`CustomSystem::ignored_firmware`); `optional = false` ⇒
/// [`AdviceNeed::Required`] (never blocking), otherwise `Improves`; md5 of the `notes` and `KnownFirmware` for
/// the state. `Ok(None)` when every file is `Ok` or `Unverified`. Unsafe paths ⇒ `Err`.
// Contract stub (PX0): the body belongs to lot PA1.
#[allow(unused_variables)]
pub fn firmware_advice_from_info(
    core: &CoreId,
    info: &CoreInfo,
    ignored: &[String],
    system_dir: &Path,
    known: &KnownFirmware,
) -> Result<Option<FirmwareAdvice>, FirmwareError> {
    todo!("lot PA1")
}

/// Banner data for `core` running a game of `system` (libretro database name) in region `variant`
/// (`"usa"`…, `None` or a key absent from the table = unknown).
///
/// - `Required`: files of a `required_firmware` table **without** `fallback`. Known variant: that variant's file,
///   listed unless it is satisfied (`Ok`, `Free`, or `Unverified`: present, no reference hash). Unknown variant:
///   nothing when one file of the table is satisfied, else every distinct file of the table.
/// - `Improves`: [`firmware_enhancements`] whose state is not `Ok` / `Free`.
///
/// `Ok(None)` when no file needs an action: nothing is listed, or only `Improves` files that are `Unverified`
/// (present, unknown revision: nothing to ask). Those are still listed when another file is actionable.
pub fn firmware_advice(
    prefs: &CorePreferences,
    core: &CoreId,
    system: &str,
    variant: Option<&str>,
    info: Option<&CoreInfo>,
    system_dir: &Path,
    known: &KnownFirmware,
) -> Result<Option<FirmwareAdvice>, FirmwareError> {
    let satisfied = |s: &FirmwareState| {
        matches!(
            s,
            FirmwareState::Ok | FirmwareState::Free { .. } | FirmwareState::Unverified
        )
    };
    let mut files: Vec<AdviceFile> = Vec::new();
    if let Some(req) = prefs.required_firmware(core, system)
        && req.fallback.is_none()
    {
        if let Some(path) = req.files.values().find(|p| !is_safe_relative_path(p)) {
            return Err(FirmwareError::UnsafePath(path.clone()));
        }
        if !req.files.contains_key(&req.default) {
            return Err(FirmwareError::UnknownDefault(req.default.clone()));
        }
        let state_of = |file: &str| {
            let md5 = info
                .and_then(|i| i.firmware.iter().find(|s| s.path == file))
                .and_then(|s| s.md5);
            let spec = FirmwareSpec {
                path: file.to_owned(),
                description: file.to_owned(),
                optional: false,
                md5,
            };
            check_one(system_dir, &spec, known, &[])
        };
        let mut candidates: Vec<(&String, FirmwareState)> = Vec::new();
        match variant.and_then(|v| req.files.get(v)) {
            Some(file) => candidates.push((file, state_of(file))),
            None => {
                for file in req.files.values() {
                    if !candidates.iter().any(|(f, _)| *f == file) {
                        candidates.push((file, state_of(file)));
                    }
                }
                if candidates.iter().any(|(_, s)| satisfied(s)) {
                    candidates.clear();
                }
            }
        }
        files.extend(
            candidates
                .into_iter()
                .filter(|(_, s)| !satisfied(s))
                .map(|(file, state)| AdviceFile {
                    name: file.clone(),
                    need: AdviceNeed::Required,
                    benefit: REQUIRED_BENEFIT.to_owned(),
                    state,
                }),
        );
    }
    for e in firmware_enhancements(prefs, core, system, info, system_dir, known)? {
        if matches!(e.state, FirmwareState::Ok | FirmwareState::Free { .. })
            || files.iter().any(|f| f.name == e.file)
        {
            continue;
        }
        files.push(AdviceFile {
            name: e.file,
            need: AdviceNeed::Improves,
            benefit: e.benefit,
            state: e.state,
        });
    }
    let actionable = files
        .iter()
        .any(|f| f.need == AdviceNeed::Required || f.state != FirmwareState::Unverified);
    Ok(actionable.then(|| FirmwareAdvice {
        core: core.clone(),
        files,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::{CoreId, parse_info};

    #[test]
    fn builtin_parses() {
        assert!(KnownFirmware::parse(BUILTIN).is_ok());
        let known = KnownFirmware::builtin();
        assert_eq!(
            known.md5_for("bios_MD.bin"),
            [parse_md5("d3293ebaaa7f4eb2a6766b68a0fb4609").unwrap()]
        );
        assert!(known.md5_for("nope.bin").is_empty());
        // 7.0 sections (System.dat: « Sony - PlayStation », « 3DO Company, The - 3DO »).
        for (name, md5, size) in [
            ("scph5500.bin", "8dd7d5296a650fac7319bce665a6a53c", 524_288),
            ("scph5501.bin", "490f666e1afb15b7362b406ed1cea246", 524_288),
            ("scph5502.bin", "32736f17079d0b2b7024407c39bd3050", 524_288),
            (
                "panafz10.bin",
                "51f2f43ae2f3508a14d9f56597e2d3ce",
                1_048_576,
            ),
        ] {
            assert_eq!(known.md5_for(name), [parse_md5(md5).unwrap()], "{name}");
            assert_eq!(known.size_for(name), Some(size), "{name}");
        }
        assert!(KnownFirmware::parse("[firmware]\n\"../x\" = []\n").is_err());
        assert!(KnownFirmware::parse("[firmware]\n\"x\" = [\"zz\"]\n").is_err());
    }

    #[test]
    fn states() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path();
        std::fs::write(system.join("good.bin"), b"abc").unwrap();
        std::fs::write(system.join("bad.bin"), b"xyz").unwrap();
        std::fs::write(system.join("free.bin"), b"abc").unwrap();
        std::fs::create_dir(system.join("dir.bin")).unwrap();
        std::fs::create_dir(system.join("sub")).unwrap();
        std::fs::write(system.join("sub/known.bin"), b"abc").unwrap();
        // md5("abc")
        let abc = "900150983cd24fb0d6963f7d28e17f72";
        let text = format!(
            "firmware0_path = \"good.bin\"\nfirmware1_path = \"bad.bin\"\nfirmware2_path = \"free.bin\"\n\
             firmware3_path = \"missing.bin\"\nfirmware4_path = \"dir.bin\"\nfirmware5_path = \"sub/known.bin\"\n\
             notes = \"(!) good.bin (md5): {abc}|(!) bad.bin (md5): {abc}\"\n"
        );
        let info = parse_info(CoreId::new("x").unwrap(), &text).unwrap();
        let known = KnownFirmware::parse(&format!("[firmware]\n\"sub/known.bin\" = [\"{abc}\"]\n"))
            .unwrap();
        let states: Vec<_> = check_firmware(system, &info, &known, &[])
            .into_iter()
            .map(|s| s.state)
            .collect();
        assert_eq!(states[0], FirmwareState::Ok);
        assert!(matches!(states[1], FirmwareState::WrongHash { .. }));
        assert_eq!(states[2], FirmwareState::Unverified);
        assert_eq!(states[3], FirmwareState::Missing);
        assert!(matches!(states[4], FirmwareState::Unreadable(_)));
        assert_eq!(states[5], FirmwareState::Ok);
    }

    #[test]
    fn free_bios_is_recognised() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tos.img"), b"abc").unwrap();
        std::fs::write(dir.path().join("other.img"), b"abc").unwrap();
        let abc = parse_md5("900150983cd24fb0d6963f7d28e17f72").unwrap();
        // .info md5 of a commercial TOS: EmuTOS would otherwise be `WrongHash`.
        let text = "firmware0_path = \"tos.img\"\nfirmware1_path = \"other.img\"\n\
                    firmware2_path = \"none.img\"\n\
                    notes = \"(!) tos.img (md5): c1c57ce48e8ee4135885cee9e63a68a2\"\n";
        let info = parse_info(CoreId::new("hatari").unwrap(), text).unwrap();
        let free = [
            ("tos.img".to_owned(), abc, "EmuTOS".to_owned()),
            ("none.img".to_owned(), abc, "X".to_owned()),
        ];
        let states: Vec<_> = check_firmware(dir.path(), &info, &KnownFirmware::default(), &free)
            .into_iter()
            .map(|s| s.state)
            .collect();
        assert_eq!(
            states[0],
            FirmwareState::Free {
                label: "EmuTOS".into()
            }
        );
        // Same content under another name: not the free BIOS's path.
        assert_eq!(states[1], FirmwareState::Unverified);
        assert_eq!(states[2], FirmwareState::Missing);
        let other = [("tos.img".to_owned(), [0u8; 16], "EmuTOS".to_owned())];
        assert!(matches!(
            check_firmware(dir.path(), &info, &KnownFirmware::default(), &other)[0].state,
            FirmwareState::WrongHash { .. }
        ));
    }

    #[test]
    fn unsafe_spec_is_not_read() {
        let spec = FirmwareSpec {
            path: "../etc/passwd".into(),
            description: String::new(),
            optional: true,
            md5: None,
        };
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            check_one(dir.path(), &spec, &KnownFirmware::default(), &[]),
            FirmwareState::Unreadable(_)
        ));
    }

    fn md5_of(data: &[u8]) -> [u8; 16] {
        Md5::digest(data).into()
    }

    fn hexs(md5: [u8; 16]) -> String {
        md5.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn both_entry_forms() {
        let a = "900150983cd24fb0d6963f7d28e17f72";
        let b = "d3293ebaaa7f4eb2a6766b68a0fb4609";
        let known = KnownFirmware::parse(&format!(
            "[firmware]\n\"list\" = [\"{a}\", \"{b}\"]\n\"one\" = {{ md5 = \"{a}\", size = 3 }}\n\
             \"many\" = {{ md5 = [\"{a}\", \"{b}\"] }}\n"
        ))
        .unwrap();
        assert_eq!(known.md5_for("list").len(), 2);
        assert_eq!(known.size_for("list"), None);
        assert_eq!(known.md5_for("one"), [parse_md5(a).unwrap()]);
        assert_eq!(known.size_for("one"), Some(3));
        assert_eq!(known.md5_for("many").len(), 2);
        assert_eq!(known.size_for("many"), None);
        assert_eq!(known.paths().collect::<Vec<_>>(), ["list", "many", "one"]);
        for bad in [
            format!("\"x\" = {{ md5 = \"{a}\", size = 0 }}"),
            format!("\"x\" = {{ md5 = \"{a}\", sha1 = \"\" }}"),
            "\"x\" = { size = 3 }".to_owned(),
            "\"x\" = { md5 = [], size = 3 }".to_owned(),
            "\"x\" = { md5 = \"zz\" }".to_owned(),
        ] {
            assert!(
                KnownFirmware::parse(&format!("[firmware]\n{bad}\n")).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn builtin_sections_from_system_dat() {
        let known = KnownFirmware::builtin();
        let md5 = |p: &str| {
            known
                .md5_for(p)
                .iter()
                .map(|h| hexs(*h))
                .collect::<Vec<_>>()
        };
        // Kickstarts (ADR 0017 §Contexte table, from System.dat).
        for (name, hash, size) in [
            (
                "kick33180.A500",
                "85ad74194e87c08904327de1a9443b7a",
                262_144,
            ),
            (
                "kick34005.A500",
                "82a21c1890cae844b3df741f2762d48d",
                262_144,
            ),
            (
                "kick37175.A500",
                "dc10d7bdd1b6f450773dfb558477c230",
                524_288,
            ),
            (
                "kick37350.A600",
                "465646c9b6729f77eea5314d1f057951",
                524_288,
            ),
            (
                "kick40063.A600",
                "e40a5dfb3d017ba8779faba30cbd1c8e",
                524_288,
            ),
            (
                "kick39106.A1200",
                "b7cc148386aa631136f510cd29e42fc3",
                524_288,
            ),
            (
                "kick40068.A1200",
                "646773759326fbac3b2311fd8c8793ee",
                524_288,
            ),
            (
                "kick40068.A4000",
                "9bdedde6a4f33555b4a270c8ca53297d",
                524_288,
            ),
        ] {
            assert_eq!(md5(name), [hash], "{name}");
            assert_eq!(known.size_for(name), Some(size), "{name}");
        }
        // Every file of the puae table is known.
        let prefs = CorePreferences::builtin();
        for core in ["puae", "puae2021"] {
            let req = prefs
                .required_firmware(&CoreId::new(core).unwrap(), "Commodore - Amiga")
                .unwrap();
            for file in req.files.values() {
                assert!(!known.md5_for(file).is_empty(), "{file}");
            }
        }
        assert_eq!(md5("tos.img"), ["c1c57ce48e8ee4135885cee9e63a68a2"]);
        assert_eq!(known.size_for("MT32_CONTROL.ROM"), Some(65_536));
        assert_eq!(known.size_for("CM32L_PCM.ROM"), Some(1_048_576));
        assert_eq!(known.size_for("vice/JiffyDOS_C64.bin"), Some(8192));
        assert_eq!(known.size_for("bios_CD_E.bin"), Some(131_072));
        assert_eq!(md5("bios_CD_U.bin"), ["2efd74e3232ff260e371b99f84024f7f"]);
        // 7.0 files (PlayStation, 3DO) are known; the libraries of the DOS section are not.
        assert_eq!(md5("scph5501.bin"), ["490f666e1afb15b7362b406ed1cea246"]);
        assert_eq!(md5("panafz10.bin"), ["51f2f43ae2f3508a14d9f56597e2d3ce"]);
        assert!(known.md5_for("bass.dll").is_empty());
    }

    fn fake_ps1(
        file: &'static str,
        region: crate::cores::Ps1Region,
        data: &[u8],
    ) -> crate::cores::Ps1Bios {
        let md5: &'static str = Box::leak(hexs(md5_of(data)).into_boxed_str());
        crate::cores::Ps1Bios {
            file,
            region,
            crc32: 0,
            md5,
            sha1: "",
        }
    }

    #[test]
    fn ps1_dump_of_the_same_region_is_accepted_under_the_canonical_name() {
        use crate::cores::Ps1Region::{Europe, Usa};
        let dir = tempfile::tempdir().unwrap();
        let table = [
            fake_ps1("scph7001.bin", Usa, b"usa-7001"),
            fake_ps1("scph7002.bin", Europe, b"eu-7002"),
        ];
        let known = KnownFirmware::default();
        let spec = |path: &str| FirmwareSpec {
            path: path.to_owned(),
            description: String::new(),
            optional: false,
            md5: None,
        };
        let state = |path: &str| check_one_with(dir.path(), &spec(path), &known, &[], &table);
        std::fs::write(dir.path().join("scph5501.bin"), b"usa-7001").unwrap();
        std::fs::write(dir.path().join("scph5502.bin"), b"usa-7001").unwrap();
        std::fs::write(dir.path().join("scph5500.bin"), b"unknown").unwrap();
        std::fs::write(dir.path().join("other.bin"), b"usa-7001").unwrap();
        assert_eq!(state("scph5501.bin"), FirmwareState::Ok);
        // Other region: not accepted (the caller refines with `ps1_wrong_region`).
        assert_eq!(state("other.bin"), FirmwareState::Unverified);
        assert!(matches!(
            state("scph5502.bin"),
            FirmwareState::Unverified | FirmwareState::WrongHash { .. }
        ));
        assert!(matches!(
            state("scph5500.bin"),
            FirmwareState::Unverified | FirmwareState::WrongHash { .. }
        ));
    }

    #[test]
    fn wrong_region_is_told_apart() {
        // Real table: the SCPH-5501 dump is not a Europe BIOS.
        let usa = crate::cores::ps1_bios_for_md5("490f666e1afb15b7362b406ed1cea246").unwrap();
        let md5 = [
            0x49, 0x0f, 0x66, 0x6e, 0x1a, 0xfb, 0x15, 0xb7, 0x36, 0x2b, 0x40, 0x6e, 0xd1, 0xce,
            0xa2, 0x46,
        ];
        assert_eq!(usa.region, crate::cores::Ps1Region::Usa);
        assert_eq!(
            crate::cores::ps1_wrong_region("scph5502.bin", &md5),
            Some(crate::cores::Ps1Region::Usa)
        );
        assert_eq!(crate::cores::ps1_wrong_region("scph5501.bin", &md5), None);
        assert_eq!(crate::cores::ps1_wrong_region("other.bin", &md5), None);
        assert_eq!(
            crate::cores::ps1_wrong_region("scph5502.bin", &[0; 16]),
            None
        );
    }

    fn table(fallback: bool) -> RequiredFirmware {
        RequiredFirmware {
            system: "S".into(),
            variant: crate::cores::FirmwareVariant::Model,
            files: BTreeMap::from([
                ("A500".to_owned(), "kick.A500".to_owned()),
                ("A500ECS".to_owned(), "kick.A500".to_owned()),
                ("A1200".to_owned(), "kick.A1200".to_owned()),
                ("A600".to_owned(), "sub/kick.A600".to_owned()),
            ]),
            default: "A500".into(),
            fallback: fallback.then(|| crate::cores::FirmwareFallback::Option {
                option: "puae_kickstart".into(),
                value: "aros".into(),
            }),
        }
    }

    #[test]
    fn required_check() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path();
        let known = KnownFirmware::parse(&format!(
            "[firmware]\n\"kick.A500\" = [\"{}\"]\n\"kick.A1200\" = [\"{}\"]\n",
            hexs(md5_of(b"ks13")),
            hexs(md5_of(b"ks31"))
        ))
        .unwrap();
        let check = |req: &RequiredFirmware, v: Option<&str>| {
            check_required(system, req, v, &known, Some("AROS")).unwrap()
        };
        // Nothing present: fallback, or missing without it.
        let c = check(&table(true), Some("A1200"));
        assert_eq!(c.wanted.as_deref(), Some("kick.A1200"));
        assert_eq!(
            c.state,
            FirmwareState::CoreFallback {
                label: "AROS".into()
            }
        );
        assert!(c.others_present.is_empty());
        assert_eq!(
            check(&table(false), Some("A1200")).state,
            FirmwareState::Missing
        );
        // Label of a hand-built table without builtin firmware.
        assert!(matches!(
            check_required(system, &table(true), None, &known, None)
                .unwrap()
                .state,
            FirmwareState::CoreFallback { .. }
        ));
        // Unknown or absent variant: default.
        assert_eq!(
            check(&table(false), Some("A3000")).wanted.as_deref(),
            Some("kick.A500")
        );
        assert_eq!(
            check(&table(false), None).wanted.as_deref(),
            Some("kick.A500")
        );

        std::fs::write(system.join("kick.A500"), b"ks13").unwrap();
        let c = check(&table(false), Some("A500ECS"));
        assert_eq!(c.state, FirmwareState::Ok);
        assert!(
            c.others_present.is_empty(),
            "the wanted file is not an « other »"
        );
        // Present with the wrong content; another file of the table is reported.
        std::fs::write(system.join("kick.A1200"), b"other").unwrap();
        let c = check(&table(true), Some("A1200"));
        assert_eq!(
            c.state,
            FirmwareState::WrongHash {
                md5: md5_of(b"other")
            }
        );
        assert_eq!(c.others_present, ["kick.A500"]);
        // Missing, with others present (« bios_CD_E.bin présent, mais ce jeu est USA »).
        let c = check(&table(false), Some("A600"));
        assert_eq!(c.state, FirmwareState::Missing);
        assert_eq!(c.others_present, ["kick.A1200", "kick.A500"]);
        // No reference hash: unverified.
        std::fs::create_dir(system.join("sub")).unwrap();
        std::fs::write(system.join("sub/kick.A600"), b"x").unwrap();
        assert_eq!(
            check(&table(true), Some("A600")).state,
            FirmwareState::Unverified
        );
        // A folder under the wanted name is not a file.
        std::fs::remove_file(system.join("kick.A500")).unwrap();
        std::fs::create_dir(system.join("kick.A500")).unwrap();
        assert!(matches!(
            check(&table(true), None).state,
            FirmwareState::Unreadable(_)
        ));
        assert!(
            !check(&table(true), Some("A600"))
                .others_present
                .contains(&"kick.A500".to_owned())
        );
    }

    #[test]
    fn required_check_data_errors() {
        let dir = tempfile::tempdir().unwrap();
        let known = KnownFirmware::default();
        let mut req = table(false);
        req.default = "nope".into();
        assert_eq!(
            check_required(dir.path(), &req, Some("A500"), &known, None),
            Err(FirmwareError::UnknownDefault("nope".into()))
        );
        let mut req = table(false);
        req.files.insert("X".into(), "../outside".into());
        assert_eq!(
            check_required(dir.path(), &req, None, &known, None),
            Err(FirmwareError::UnsafePath("../outside".into()))
        );
    }

    #[test]
    fn core_firmware_decision() {
        let dir = tempfile::tempdir().unwrap();
        let known = KnownFirmware::builtin();
        let prefs = CorePreferences::builtin();
        let puae = CoreId::new("puae").unwrap();
        let c = check_core_firmware(
            dir.path(),
            &prefs,
            &puae,
            "Commodore - Amiga",
            Some("A1200"),
            &known,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.wanted.as_deref(), Some("kick40068.A1200"));
        assert_eq!(
            c.state,
            FirmwareState::CoreFallback {
                label: "AROS (intégré au core)".into()
            }
        );
        // No table, no built-in ROM: the 1.0 `.info` check applies.
        let hatari = CoreId::new("hatari").unwrap();
        assert!(
            check_core_firmware(dir.path(), &prefs, &hatari, "Atari - ST", None, &known).is_none()
        );
        // Built-in ROMs without a table for the system: `Builtin`, nothing expected.
        let p = CorePreferences::builtin_from_text_for_tests(
            "[systems]\n[cores.cap32]\nbuiltin_firmware = { label = \"ROM Amstrad\", notice = \"N\" }\n",
        );
        let cap32 = CoreId::new("cap32").unwrap();
        assert_eq!(
            check_core_firmware(dir.path(), &p, &cap32, "Amstrad - CPC", None, &known),
            Some(Ok(FirmwareCheck::builtin("ROM Amstrad")))
        );
        assert_eq!(FirmwareCheck::builtin("x").wanted, None);
    }

    #[test]
    fn playstation_fallback_files_are_enhancements() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = CorePreferences::builtin();
        let core = CoreId::new("pcsx_rearmed").unwrap();
        let found = firmware_enhancements(
            &prefs,
            &core,
            "Sony - PlayStation",
            None,
            dir.path(),
            &KnownFirmware::builtin(),
        )
        .unwrap();
        assert!(!found.is_empty());
        assert!(found.iter().all(|e| e.kind == EnhancementKind::Fallback
            && e.state == FirmwareState::Missing
            && e.variant.is_some()));
    }

    const GBA: &str = "Nintendo - Game Boy Advance";

    fn mgba_info(with_bios: bool) -> CoreInfo {
        let firmware = if with_bios {
            "firmware_count = 1\nfirmware0_path = \"gba_bios.bin\"\nfirmware0_opt = \"true\"\n"
        } else {
            ""
        };
        parse_info(CoreId::new("mgba").unwrap(), firmware).unwrap()
    }

    fn enhancements(
        core: &str,
        system: &str,
        info: Option<&CoreInfo>,
        dir: &Path,
    ) -> Vec<FirmwareEnhancement> {
        firmware_enhancements(
            &CorePreferences::builtin(),
            &CoreId::new(core).unwrap(),
            system,
            info,
            dir,
            &KnownFirmware::builtin(),
        )
        .unwrap()
    }

    #[test]
    fn optional_requires_the_info_to_list_the_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(enhancements("mgba", GBA, None, dir.path()).is_empty());
        assert!(enhancements("mgba", GBA, Some(&mgba_info(false)), dir.path()).is_empty());
        let found = enhancements("mgba", GBA, Some(&mgba_info(true)), dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, "gba_bios.bin");
        assert_eq!(found[0].kind, EnhancementKind::Optional);
        assert_eq!(found[0].variant, None);
        assert_eq!(found[0].state, FirmwareState::Missing);
        // Another system of the same core: nothing declared.
        assert!(
            enhancements(
                "mgba",
                "Nintendo - Game Boy",
                Some(&mgba_info(true)),
                dir.path()
            )
            .is_empty()
        );
    }

    #[test]
    fn optional_state_follows_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let info = mgba_info(true);
        std::fs::write(dir.path().join("gba_bios.bin"), b"not a bios").unwrap();
        // Known md5 for gba_bios.bin in firmware.toml: other content is a wrong hash.
        let found = enhancements("mgba", GBA, Some(&info), dir.path());
        assert!(matches!(found[0].state, FirmwareState::WrongHash { .. }));
        // No reference at all: Unverified.
        let p = CorePreferences::builtin_from_text_for_tests(
            "[systems]\n[cores.x.optional_firmware]\n\"S\" = [{ file = \"zz.bin\", benefit = \"b\" }]\n",
        );
        let x = CoreId::new("x").unwrap();
        let info = parse_info(
            x.clone(),
            "firmware_count = 1\nfirmware0_path = \"zz.bin\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("zz.bin"), b"data").unwrap();
        let found = firmware_enhancements(
            &p,
            &x,
            "S",
            Some(&info),
            dir.path(),
            &KnownFirmware::default(),
        )
        .unwrap();
        assert_eq!(found[0].state, FirmwareState::Unverified);
    }

    #[test]
    fn fallback_tables_are_deduplicated_and_stable() {
        let dir = tempfile::tempdir().unwrap();
        let opera = enhancements("opera", "The 3DO Company - 3DO", None, dir.path());
        assert_eq!(opera.len(), 1);
        assert_eq!(opera[0].file, "panafz10.bin");
        assert_eq!(opera[0].variant.as_deref(), Some("europe"));
        assert_eq!(
            opera,
            enhancements("opera", "The 3DO Company - 3DO", None, dir.path())
        );
        let puae = enhancements("puae", "Commodore - Amiga", None, dir.path());
        assert!(!puae.is_empty());
        let mut files: Vec<_> = puae.iter().map(|e| &e.file).collect();
        let n = files.len();
        files.sort();
        files.dedup();
        assert_eq!(files.len(), n);
        assert!(puae.iter().all(|e| e.kind == EnhancementKind::Fallback));
    }

    #[test]
    fn required_without_fallback_is_not_an_enhancement() {
        let dir = tempfile::tempdir().unwrap();
        let mcd = "Sega - Mega-CD - Sega CD";
        assert!(enhancements("genesis_plus_gx", mcd, None, dir.path()).is_empty());
    }

    #[test]
    fn dosbox_pure_roland_roms_are_proposed_without_info_listing() {
        let dir = tempfile::tempdir().unwrap();
        let info = parse_info(CoreId::new("dosbox_pure").unwrap(), "").unwrap();
        for info in [None, Some(&info)] {
            let found = enhancements("dosbox_pure", "DOS", info, dir.path());
            let files: Vec<_> = found.iter().map(|e| e.file.as_str()).collect();
            assert_eq!(
                files,
                [
                    "MT32_CONTROL.ROM",
                    "MT32_PCM.ROM",
                    "CM32L_CONTROL.ROM",
                    "CM32L_PCM.ROM"
                ]
            );
            assert!(
                found
                    .iter()
                    .all(|e| e.kind == EnhancementKind::Optional
                        && e.state == FirmwareState::Missing)
            );
        }
        std::fs::write(dir.path().join("MT32_PCM.ROM"), b"x").unwrap();
        let found = enhancements("dosbox_pure", "DOS", None, dir.path());
        // Other revisions are valid: an unknown md5 is Unverified, not a wrong hash.
        assert_eq!(found[1].state, FirmwareState::Unverified);
    }

    #[test]
    fn shipped_optional_firmware_is_consistent() {
        let prefs = CorePreferences::builtin();
        let mut seen = 0;
        for (system, cores) in prefs.systems() {
            for core in cores {
                for o in prefs.optional_firmware(core, system) {
                    seen += 1;
                    assert!(is_safe_relative_path(&o.file));
                    assert!(!o.benefit.is_empty());
                }
            }
        }
        assert!(seen >= 7, "mgba and gambatte entries expected");
    }

    fn advice(
        core: &str,
        system: &str,
        variant: Option<&str>,
        dir: &Path,
    ) -> Option<FirmwareAdvice> {
        firmware_advice(
            &CorePreferences::builtin(),
            &CoreId::new(core).unwrap(),
            system,
            variant,
            None,
            dir,
            &KnownFirmware::builtin(),
        )
        .unwrap()
    }

    const MCD: &str = "Sega - Mega-CD - Sega CD";
    const PSX: &str = "Sony - PlayStation";

    #[test]
    fn advice_required_known_variant_lists_its_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let a = advice("picodrive", MCD, Some("usa"), dir.path()).unwrap();
        assert_eq!(a.files.len(), 1);
        assert_eq!(a.files[0].name, "bios_CD_U.bin");
        assert_eq!(a.files[0].need, AdviceNeed::Required);
        assert_eq!(a.files[0].state, FirmwareState::Missing);
        // Another region present does not satisfy a known variant.
        std::fs::write(dir.path().join("bios_CD_E.bin"), b"x").unwrap();
        let a = advice("picodrive", MCD, Some("usa"), dir.path()).unwrap();
        assert!(matches!(a.files[0].state, FirmwareState::Missing));
    }

    #[test]
    fn advice_required_unknown_variant_needs_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = advice("genesis_plus_gx", MCD, None, dir.path()).unwrap();
        assert_eq!(a.files.len(), 3);
        assert!(a.files.iter().all(|f| f.need == AdviceNeed::Required));
        // A file with a wrong md5 does not satisfy an unknown variant; an unlisted variant is unknown.
        std::fs::write(dir.path().join("bios_CD_U.bin"), b"x").unwrap();
        for v in [None, Some("brazil")] {
            let a = advice("genesis_plus_gx", MCD, v, dir.path()).unwrap();
            assert_eq!(a.files.len(), 3);
            assert!(
                a.files
                    .iter()
                    .any(|f| matches!(f.state, FirmwareState::WrongHash { .. }))
            );
        }
        // Present without reference hash (not in firmware.toml): satisfied.
        let p = CorePreferences::builtin_from_text_for_tests(
            "[systems]\n[cores.x.required_firmware.\"S\"]\nvariant = \"region\"\ndefault = \"usa\"\n\
             [cores.x.required_firmware.\"S\".files]\neurope = \"e.bin\"\nusa = \"u.bin\"\n",
        );
        let x = CoreId::new("x").unwrap();
        let run = |variant| {
            firmware_advice(
                &p,
                &x,
                "S",
                variant,
                None,
                dir.path(),
                &KnownFirmware::default(),
            )
            .unwrap()
        };
        assert!(run(None).is_some());
        std::fs::write(dir.path().join("e.bin"), b"y").unwrap();
        assert!(run(None).is_none());
        assert!(run(Some("usa")).is_some());
    }

    #[test]
    fn advice_playstation_and_3do_improve_only() {
        let dir = tempfile::tempdir().unwrap();
        for (core, system) in [
            ("pcsx_rearmed", PSX),
            ("swanstation", PSX),
            ("opera", "The 3DO Company - 3DO"),
        ] {
            let a = advice(core, system, None, dir.path()).unwrap();
            assert!(!a.files.is_empty());
            assert!(a.files.iter().all(|f| f.need == AdviceNeed::Improves));
        }
    }

    #[test]
    fn advice_unverified_improvement_alone_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        // dosbox_pure: other revisions of the Roland ROMs are valid, an unknown md5 is Unverified.
        for f in [
            "MT32_CONTROL.ROM",
            "MT32_PCM.ROM",
            "CM32L_CONTROL.ROM",
            "CM32L_PCM.ROM",
        ] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        assert!(advice("dosbox_pure", "DOS", None, dir.path()).is_none());
        // One missing: the others stay listed with their state.
        std::fs::remove_file(dir.path().join("MT32_PCM.ROM")).unwrap();
        let a = advice("dosbox_pure", "DOS", None, dir.path()).unwrap();
        assert_eq!(a.files.len(), 4);
        assert_eq!(a.files[1].state, FirmwareState::Missing);
        assert_eq!(a.files[0].state, FirmwareState::Unverified);
    }

    #[test]
    fn advice_wrong_hash_is_reported_and_unknown_core_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let info = mgba_info(true);
        std::fs::write(dir.path().join("gba_bios.bin"), b"not a bios").unwrap();
        let a = firmware_advice(
            &CorePreferences::builtin(),
            &CoreId::new("mgba").unwrap(),
            GBA,
            None,
            Some(&info),
            dir.path(),
            &KnownFirmware::builtin(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(a.core.as_str(), "mgba");
        assert!(matches!(a.files[0].state, FirmwareState::WrongHash { .. }));
        assert_eq!(a.files[0].need, AdviceNeed::Improves);
        assert!(advice("nope", GBA, None, dir.path()).is_none());
        assert!(advice("picodrive", "Sega - 32X", None, dir.path()).is_none());
    }
}
