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
use super::{CoreInfo, FirmwareSpec};

const BUILTIN: &str = include_str!("../../data/firmware.toml");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FirmwareFile {
    firmware: BTreeMap<String, Vec<String>>,
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
        for (path, hashes) in file.firmware {
            if !is_safe_relative_path(&path) {
                return Err(format!("chemin refusé : {path}"));
            }
            let hashes = hashes
                .iter()
                .map(|h| parse_md5(h).ok_or_else(|| format!("md5 invalide pour {path}")))
                .collect::<Result<Vec<_>, _>>()?;
            by_path.insert(path, hashes);
        }
        Ok(Self { by_path })
    }

    /// Accepted md5 for a relative path (case-sensitive, `/`-separated).
    pub fn md5_for(&self, path: &str) -> &[[u8; 16]] {
        self.by_path.get(path).map_or(&[], Vec::as_slice)
    }

    /// Expected size of `path`, when `data/firmware.toml` gives one (table form
    /// `"name" = { md5 = "…" | ["…", …], size = 262144 }`, design v3.0 §3.5.6). Only a quick reject at import and
    /// a display hint: the md5 stays the sole verification.
    pub fn size_for(&self, path: &str) -> Option<u64> {
        let _ = path;
        todo!("A3: design v3.0 §3.5.6")
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
    let _ = (system_dir, req, variant, known, fallback_label);
    todo!("A3: design v3.0 §3.5.5")
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
    // `FirmwareSpec` fields are public: re-checked here, not only at parse time.
    if !is_safe_relative_path(&spec.path) {
        return FirmwareState::Unreadable("chemin refusé".into());
    }
    let path: PathBuf = spec
        .path
        .split('/')
        .fold(system_dir.to_path_buf(), |p, c| p.join(c));
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
}
