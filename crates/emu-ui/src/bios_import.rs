//! « Importer un BIOS… » without any drawing (design v3.0 §2.2): identification of the picked file, choice of the
//! target names, import. Shared by the cores screen (`cores_ui::import`) and the CLI (`--import-bios`). Target
//! names only ever come from the catalogue (`targets` of a known file, `expected_names()` otherwise) or are the
//! fixed `rom.key` of Amiga Forever; nothing here downloads anything (ADR 0005).

use std::path::Path;

use emu_assets::bios::{
    BiosError, ExpectedFirmware, FirmwareCatalogue, Identification, ReplacePolicy, TargetState,
    builtin_free_bios, identify_firmware, import_firmware_expecting, target_state,
};
use emu_assets::cores::{CoreId, CoreInfo, CoreManager, KnownFirmware};

/// Key file of the Amiga Forever encrypted ROMs: imported unverified under this fixed name.
pub const ROM_KEY_NAME: &str = "rom.key";

/// Catalogue of the expected BIOS files: `firmware.toml`, the `.info` of the installed cores (when the manager is
/// available) and the free BIOS.
pub fn firmware_catalogue(manager: Option<&CoreManager>) -> FirmwareCatalogue {
    let infos: Vec<(CoreId, CoreInfo)> = manager
        .map(|m| m.infos().into_iter().collect())
        .unwrap_or_default();
    FirmwareCatalogue::build(&KnownFirmware::builtin(), &infos, &builtin_free_bios())
}

/// What the picked file is, and what may be done with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiosIdentified {
    /// md5 known: installed under every target name (state of each target in the system folder).
    Known {
        md5: [u8; 16],
        targets: Vec<(ExpectedFirmware, Result<TargetState, String>)>,
    },
    /// Amiga Forever encrypted ROM: only an unverified import, under a name of `names` (`suggested` first when
    /// it is one of them); `rom.key` must be imported too.
    Encrypted {
        suggested: Option<String>,
        names: Vec<String>,
    },
    /// Refused by default. « Importer quand même (non vérifié) » under a name of `names` (`rom.key` alone for the
    /// Amiga Forever key file).
    Unknown {
        md5: [u8; 16],
        size: u64,
        rom_key: bool,
        names: Vec<String>,
    },
}

impl BiosIdentified {
    /// Names an unverified import may use (empty for a known file).
    pub fn unverified_names(&self) -> &[String] {
        match self {
            BiosIdentified::Known { .. } => &[],
            BiosIdentified::Encrypted { names, .. } | BiosIdentified::Unknown { names, .. } => {
                names
            }
        }
    }
}

fn is_rom_key(src: &Path) -> bool {
    src.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(ROM_KEY_NAME))
}

fn sorted_names(expected: &[ExpectedFirmware]) -> Vec<String> {
    let mut names: Vec<String> = expected.iter().map(|e| e.name.clone()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// Hashes `src` and looks it up; for a known file, the state of each target. Blocking (hash of up to 16 MiB):
/// worker thread in the UI.
pub fn identify_bios(
    system_dir: &Path,
    src: &Path,
    catalogue: &FirmwareCatalogue,
) -> Result<BiosIdentified, String> {
    let identification = identify_firmware(src, catalogue).map_err(|e| import_error(&e))?;
    Ok(match identification {
        Identification::Known { md5, targets, .. } => BiosIdentified::Known {
            md5,
            targets: targets
                .into_iter()
                .map(|t| {
                    let state =
                        target_state(system_dir, &t.name, &md5).map_err(|e| import_error(&e));
                    (t, state)
                })
                .collect(),
        },
        Identification::AmigaForeverEncrypted { suggested, .. } => {
            let mut names = sorted_names(&catalogue.expected_names());
            if let Some(s) = suggested.as_ref().filter(|s| names.contains(s)) {
                names.retain(|n| n != s);
                names.insert(0, s.clone());
            }
            BiosIdentified::Encrypted {
                suggested: suggested.filter(|s| names.contains(s)),
                names,
            }
        }
        Identification::Unknown { md5, size, .. } => {
            let rom_key = is_rom_key(src);
            BiosIdentified::Unknown {
                md5,
                size,
                rom_key,
                names: if rom_key {
                    vec![ROM_KEY_NAME.to_owned()]
                } else {
                    sorted_names(&catalogue.expected_names())
                },
            }
        }
    })
}

/// One file to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiosImportItem {
    pub name: String,
    pub policy: ReplacePolicy,
    pub verified: bool,
}

/// Items of a known file: absent or free-BIOS targets are written; a target with the same content is skipped; a
/// user file with another content is replaced (renamed first) only when `replace` was confirmed.
pub fn known_import_items(
    targets: &[(ExpectedFirmware, Result<TargetState, String>)],
    replace: bool,
) -> Vec<BiosImportItem> {
    targets
        .iter()
        .filter_map(|(t, state)| {
            let policy = match state {
                Ok(TargetState::Absent | TargetState::FreeInstalled) => ReplacePolicy::Refuse,
                Ok(TargetState::Other { .. }) if replace => ReplacePolicy::BackupAndReplace,
                // Same content, or not replaced: nothing to write (an unreadable target is never touched).
                _ => return None,
            };
            Some(BiosImportItem {
                name: t.name.clone(),
                policy,
                verified: true,
            })
        })
        .collect()
}

/// An unverified import under `name`, if `name` is one the identification allows.
pub fn unverified_item(identified: &BiosIdentified, name: &str) -> Option<BiosImportItem> {
    identified
        .unverified_names()
        .iter()
        .any(|n| n == name)
        .then(|| BiosImportItem {
            name: name.to_owned(),
            policy: ReplacePolicy::Refuse,
            verified: false,
        })
}

/// Outcome of [`import_bios`], one French line per item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BiosImportReport {
    pub lines: Vec<String>,
    pub installed: usize,
    pub failed: usize,
}

/// Writes every item. `expected_md5`: md5 found by the identification (known file), checked by the assets crate
/// before anything is installed (`BiosError::Changed`).
pub fn import_bios(
    system_dir: &Path,
    src: &Path,
    items: &[BiosImportItem],
    expected_md5: Option<[u8; 16]>,
) -> BiosImportReport {
    let mut report = BiosImportReport::default();
    for item in items {
        match import_firmware_expecting(
            system_dir,
            src,
            &item.name,
            item.policy,
            item.verified,
            expected_md5,
        ) {
            Ok(outcome) => {
                report.installed += 1;
                report.lines.push(text::installed(
                    &item.name,
                    item.verified,
                    outcome.backup.as_deref(),
                ));
                report
                    .lines
                    .extend(outcome.aliases.iter().map(crate::ps1_bios::alias_line));
            }
            Err(e) => {
                report.failed += 1;
                report
                    .lines
                    .push(format!("{} : {}", item.name, import_error(&e)));
            }
        }
    }
    if report.installed > 0 {
        // MT-32 ROMs: dosbox_pure lists them only once its MIDI cache is gone (design v4.0 §2.4).
        crate::dos_launch::invalidate_core_caches(system_dir);
    }
    report
}

/// French message of an import error (never the source path above `debug`).
pub fn import_error(e: &BiosError) -> String {
    match e {
        BiosError::TooLarge(n) => text::too_large(*n),
        BiosError::TargetExists(name) => text::target_exists(name),
        BiosError::Changed => text::changed_during_import(),
        other => other.to_string(),
    }
}

pub mod text {
    //! Strings of the import (dialog and CLI), in one place for a future translation.

    use std::path::Path;

    pub fn installed(name: &str, verified: bool, backup: Option<&Path>) -> String {
        let mut s = if verified {
            format!("{name} : installé")
        } else {
            format!("{name} : installé (non vérifié)")
        };
        if let Some(b) = backup.and_then(|b| b.file_name()) {
            s.push_str(&format!(", ancien fichier renommé {}", b.to_string_lossy()));
        }
        s
    }

    pub fn changed_during_import() -> String {
        "Le fichier a changé depuis sa vérification : rien n'a été importé.".to_owned()
    }

    pub fn too_large(size: u64) -> String {
        format!("fichier trop volumineux pour un BIOS ({} Kio)", size / 1024)
    }

    pub fn target_exists(name: &str) -> String {
        format!("un autre fichier existe déjà sous le nom {name} : conservé")
    }

    pub const ROM_KEY_NOTE: &str = "ROM chiffrée d'Amiga Forever : importez aussi le fichier rom.key (même \
                                    bouton), sinon le core ne pourra pas la lire.";
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_assets::cores::parse_info;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// md5 through the crate under test (no hashing dependency here): an unknown file reports it.
    fn md5_of(data: &[u8]) -> [u8; 16] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, data).unwrap();
        match identify_firmware(&p, &FirmwareCatalogue::default()).unwrap() {
            Identification::Unknown { md5, .. } => md5,
            other => panic!("{other:?}"),
        }
    }

    /// Catalogue of fake files only (never a real ROM): `fake-ks` expected as `kick.A500`.
    fn catalogue() -> FirmwareCatalogue {
        let md5 = md5_of(b"fake-ks");
        let text = format!(
            "database = \"Commodore - Amiga\"\nfirmware0_path = \"kick.A500\"\nfirmware0_desc = \"KS 1.3\"\n\
             firmware1_path = \"amiga-os-130.rom\"\nnotes = \"(!) kick.A500 (md5): {}\"\n",
            hex(&md5)
        );
        let info = parse_info(CoreId::new("fake").unwrap(), &text).unwrap();
        FirmwareCatalogue::build(
            &KnownFirmware::default(),
            &[(CoreId::new("fake").unwrap(), info)],
            &[],
        )
    }

    #[test]
    fn known_file_is_installed_then_same() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("my kick.rom");
        std::fs::write(&src, b"fake-ks").unwrap();
        let system = dir.path().join("system");
        let BiosIdentified::Known { md5, targets } =
            identify_bios(&system, &src, &catalogue()).unwrap()
        else {
            panic!("known");
        };
        let items = known_import_items(&targets, false);
        assert_eq!(items.len(), 1);
        let report = import_bios(&system, &src, &items, Some(md5));
        assert_eq!((report.installed, report.failed), (1, 0), "{report:?}");
        assert_eq!(std::fs::read(system.join("kick.A500")).unwrap(), b"fake-ks");
        let BiosIdentified::Known { targets, .. } =
            identify_bios(&system, &src, &catalogue()).unwrap()
        else {
            panic!("known");
        };
        assert_eq!(targets[0].1, Ok(TargetState::Same));
        assert!(known_import_items(&targets, true).is_empty());
    }

    #[test]
    fn changed_source_installs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("my kick.rom");
        std::fs::write(&src, b"fake-ks").unwrap();
        let system = dir.path().join("system");
        let BiosIdentified::Known { md5, targets } =
            identify_bios(&system, &src, &catalogue()).unwrap()
        else {
            panic!("known");
        };
        std::fs::write(&src, b"other-content").unwrap();
        let items = known_import_items(&targets, false);
        let report = import_bios(&system, &src, &items, Some(md5));
        assert_eq!((report.installed, report.failed), (0, 1), "{report:?}");
        assert!(report.lines[0].contains("a changé depuis sa vérification"));
        assert!(!system.join("kick.A500").exists());
    }

    #[test]
    fn other_user_file_is_replaced_only_after_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("ks.rom");
        std::fs::write(&src, b"fake-ks").unwrap();
        let system = dir.path().join("system");
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(system.join("kick.A500"), b"mine").unwrap();
        let BiosIdentified::Known { md5, targets } =
            identify_bios(&system, &src, &catalogue()).unwrap()
        else {
            panic!("known");
        };
        assert!(known_import_items(&targets, false).is_empty());
        let items = known_import_items(&targets, true);
        assert_eq!(items[0].policy, ReplacePolicy::BackupAndReplace);
        let report = import_bios(&system, &src, &items, Some(md5));
        assert_eq!(report.installed, 1, "{report:?}");
        let backups: Vec<_> = std::fs::read_dir(&system)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("kick.A500.user-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(backups[0].path()).unwrap(), b"mine");
    }

    #[test]
    fn unknown_file_only_under_an_expected_name_and_unverified() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("whatever.bin");
        std::fs::write(&src, b"unknown").unwrap();
        let system = dir.path().join("system");
        let identified = identify_bios(&system, &src, &catalogue()).unwrap();
        let BiosIdentified::Unknown { rom_key, names, .. } = &identified else {
            panic!("unknown");
        };
        assert!(!rom_key);
        assert!(names.contains(&"kick.A500".to_owned()));
        assert!(unverified_item(&identified, "../evil").is_none());
        assert!(unverified_item(&identified, "free-name.rom").is_none());
        let item = unverified_item(&identified, "kick.A500").unwrap();
        assert!(!item.verified);
        let report = import_bios(&system, &src, &[item], None);
        assert_eq!(report.installed, 1, "{report:?}");
        assert!(report.lines[0].contains("non vérifié"));
    }

    #[test]
    fn rom_key_goes_under_its_fixed_name() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("ROM.KEY");
        std::fs::write(&src, b"key bytes").unwrap();
        let identified = identify_bios(dir.path(), &src, &catalogue()).unwrap();
        assert_eq!(identified.unverified_names(), [ROM_KEY_NAME.to_owned()]);
        assert!(unverified_item(&identified, "kick.A500").is_none());
    }

    #[test]
    fn amiga_forever_rom_suggests_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("amiga-os-130.rom");
        std::fs::write(&src, b"AMIROMTYPE1 encrypted").unwrap();
        let identified = identify_bios(dir.path(), &src, &catalogue()).unwrap();
        let BiosIdentified::Encrypted { suggested, names } = &identified else {
            panic!("encrypted");
        };
        assert_eq!(suggested.as_deref(), Some("amiga-os-130.rom"));
        assert_eq!(names[0], "amiga-os-130.rom");
    }
}
