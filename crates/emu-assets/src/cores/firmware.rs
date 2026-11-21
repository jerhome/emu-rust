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
        // Not authorised yet (7.0), nor libraries of the DOS section.
        assert!(known.md5_for("scph5501.bin").is_empty());
        assert!(known.md5_for("panafz10.bin").is_empty());
        assert!(known.md5_for("bass.dll").is_empty());
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
}
