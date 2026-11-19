//! Firmware (BIOS) checks. Nothing is ever downloaded here (ADR 0005): commercial BIOS are provided by the
//! user in the system directory; the free ones (EmuTOS, AROS) get their own installer in 2.0 / 3.0.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::Deserialize;

use super::info::{is_safe_relative_path, parse_md5};
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
    Unreadable(String),
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
}

/// One status per `info.firmware` entry, same order. A spec's own md5 (from `notes`) and `known` are both
/// accepted. Symlinks are followed (users link BIOS collections); the target is hashed.
pub fn check_firmware(
    system_dir: &Path,
    info: &CoreInfo,
    known: &KnownFirmware,
) -> Vec<FirmwareStatus> {
    info.firmware
        .iter()
        .map(|spec| FirmwareStatus {
            state: check_one(system_dir, spec, known),
            spec: spec.clone(),
        })
        .collect()
}

fn check_one(system_dir: &Path, spec: &FirmwareSpec, known: &KnownFirmware) -> FirmwareState {
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
    if accepted.peek().is_none() {
        FirmwareState::Unverified
    } else if accepted.any(|h| *h == md5) {
        FirmwareState::Ok
    } else {
        FirmwareState::WrongHash { md5 }
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
        let states: Vec<_> = check_firmware(system, &info, &known)
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
    fn unsafe_spec_is_not_read() {
        let spec = FirmwareSpec {
            path: "../etc/passwd".into(),
            description: String::new(),
            optional: true,
            md5: None,
        };
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            check_one(dir.path(), &spec, &KnownFirmware::default()),
            FirmwareState::Unreadable(_)
        ));
    }
}
