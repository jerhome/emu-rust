//! Mega CD launch check (design v2.1-megacd-express §2.3): the disc's region names one CD BIOS; its 1.0 firmware
//! state decides. Pure logic; replaced by the generic `check_required` in 6.0.

use std::path::Path;

use emu_assets::cores::{FirmwareState, FirmwareStatus};
use emu_library::{DiscRegion, disc_info, region_from_name};

pub(crate) const SYSTEM_ID: &str = "megacd";
pub(crate) const BIOS_NAMES: [&str; 3] = ["bios_CD_E.bin", "bios_CD_U.bin", "bios_CD_J.bin"];

/// The file genesis_plus_gx opens for a disc of this region (octet 0x20B of the header).
pub(crate) fn bios_name(region: DiscRegion) -> &'static str {
    match region {
        DiscRegion::Europe => BIOS_NAMES[0],
        DiscRegion::Usa => BIOS_NAMES[1],
        DiscRegion::Japan => BIOS_NAMES[2],
    }
}

pub(crate) fn region_label(region: DiscRegion) -> &'static str {
    match region {
        DiscRegion::Europe => "Mega CD, Europe",
        DiscRegion::Usa => "Sega CD, USA",
        DiscRegion::Japan => "Mega CD, Japon",
    }
}

/// Region alone, as in « un disque Europe ».
pub(crate) fn region_name(region: DiscRegion) -> &'static str {
    match region {
        DiscRegion::Europe => "Europe",
        DiscRegion::Usa => "USA",
        DiscRegion::Japan => "Japon",
    }
}

pub(crate) fn is_bios_name(name: &str) -> bool {
    BIOS_NAMES.contains(&name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BiosVerdict {
    Ready,
    /// Present, md5 unlike the reference: confirm, never block.
    Different {
        name: &'static str,
    },
    /// `wanted`: one name (region known) or the three (unknown). `others_present`: BIOS of other regions found.
    Missing {
        wanted: Vec<&'static str>,
        others_present: Vec<&'static str>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Usable,
    Different,
    Absent,
}

fn presence(name: &str, statuses: &[FirmwareStatus], system_dir: &Path) -> Presence {
    match statuses
        .iter()
        .find(|s| s.spec.path == name)
        .map(|s| &s.state)
    {
        Some(FirmwareState::Ok | FirmwareState::Unverified | FirmwareState::Free { .. }) => {
            Presence::Usable
        }
        Some(FirmwareState::WrongHash { .. }) => Presence::Different,
        Some(FirmwareState::Missing | FirmwareState::Unreadable(_)) => Presence::Absent,
        // Not listed by the installed `.info`: plain presence.
        None if system_dir.join(name).is_file() => Presence::Usable,
        None => Presence::Absent,
    }
}

/// `statuses`: `CoreManager::firmware_status(core)`; `system_dir`: plain presence test for a name the `.info` lacks.
pub(crate) fn bios_verdict(
    region: Option<DiscRegion>,
    statuses: &[FirmwareStatus],
    system_dir: &Path,
) -> BiosVerdict {
    let present = |name: &str| presence(name, statuses, system_dir);
    match region {
        Some(region) => {
            let name = bios_name(region);
            match present(name) {
                Presence::Usable => BiosVerdict::Ready,
                Presence::Different => BiosVerdict::Different { name },
                Presence::Absent => BiosVerdict::Missing {
                    wanted: vec![name],
                    others_present: BIOS_NAMES
                        .into_iter()
                        .filter(|n| *n != name && present(n) != Presence::Absent)
                        .collect(),
                },
            }
        }
        // The core picks the file itself: any of the three will do.
        None if BIOS_NAMES.iter().any(|n| present(n) != Presence::Absent) => BiosVerdict::Ready,
        None => BiosVerdict::Missing {
            wanted: BIOS_NAMES.to_vec(),
            others_present: Vec::new(),
        },
    }
}

/// Header probe, then file-name tag; I/O errors are logged and give `None`.
pub(crate) fn region_of(game_path: &Path) -> Option<DiscRegion> {
    match disc_info(game_path) {
        Ok(Some(info)) if info.region.is_some() => return info.region,
        Ok(_) => {}
        Err(e) => tracing::warn!(
            "en-tête du disque illisible ({}) : {e}",
            game_path.display()
        ),
    }
    game_path
        .file_stem()
        .and_then(|s| region_from_name(&s.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_assets::cores::FirmwareSpec;

    fn status(path: &str, state: FirmwareState) -> FirmwareStatus {
        FirmwareStatus {
            spec: FirmwareSpec {
                path: path.to_owned(),
                description: String::new(),
                optional: true,
                md5: None,
            },
            state,
        }
    }

    fn all(e: FirmwareState, u: FirmwareState, j: FirmwareState) -> Vec<FirmwareStatus> {
        vec![
            status("bios_MD.bin", FirmwareState::Missing),
            status("bios_CD_E.bin", e),
            status("bios_CD_U.bin", u),
            status("bios_CD_J.bin", j),
        ]
    }

    fn empty_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    use FirmwareState::{Missing, Ok as Good, Unverified, WrongHash};

    #[test]
    fn names_follow_the_core_rule() {
        assert_eq!(bios_name(DiscRegion::Europe), "bios_CD_E.bin");
        assert_eq!(bios_name(DiscRegion::Usa), "bios_CD_U.bin");
        assert_eq!(bios_name(DiscRegion::Japan), "bios_CD_J.bin");
        assert_eq!(region_label(DiscRegion::Usa), "Sega CD, USA");
        assert!(is_bios_name("bios_CD_J.bin"));
        assert!(!is_bios_name("bios_MD.bin"));
    }

    #[test]
    fn wanted_bios_present_is_ready() {
        let dir = empty_dir();
        for state in [Good, Unverified] {
            let s = all(state, Missing, Missing);
            assert_eq!(
                bios_verdict(Some(DiscRegion::Europe), &s, dir.path()),
                BiosVerdict::Ready
            );
        }
        let s = all(Missing, Missing, FirmwareState::Free { label: "x".into() });
        assert_eq!(
            bios_verdict(Some(DiscRegion::Japan), &s, dir.path()),
            BiosVerdict::Ready
        );
    }

    #[test]
    fn wrong_hash_is_different_not_missing() {
        let dir = empty_dir();
        let s = all(Missing, WrongHash { md5: [0; 16] }, Missing);
        assert_eq!(
            bios_verdict(Some(DiscRegion::Usa), &s, dir.path()),
            BiosVerdict::Different {
                name: "bios_CD_U.bin"
            }
        );
    }

    #[test]
    fn missing_lists_the_other_regions_present() {
        let dir = empty_dir();
        let s = all(Missing, Good, FirmwareState::Unreadable("denied".into()));
        assert_eq!(
            bios_verdict(Some(DiscRegion::Europe), &s, dir.path()),
            BiosVerdict::Missing {
                wanted: vec!["bios_CD_E.bin"],
                others_present: vec!["bios_CD_U.bin"],
            }
        );
        let s = all(Missing, Missing, WrongHash { md5: [1; 16] });
        assert_eq!(
            bios_verdict(Some(DiscRegion::Europe), &s, dir.path()),
            BiosVerdict::Missing {
                wanted: vec!["bios_CD_E.bin"],
                others_present: vec!["bios_CD_J.bin"],
            }
        );
    }

    #[test]
    fn unknown_region_needs_any_bios() {
        let dir = empty_dir();
        let none = all(Missing, Missing, Missing);
        assert_eq!(
            bios_verdict(None, &none, dir.path()),
            BiosVerdict::Missing {
                wanted: BIOS_NAMES.to_vec(),
                others_present: Vec::new(),
            }
        );
        let one = all(Missing, Missing, Good);
        assert_eq!(bios_verdict(None, &one, dir.path()), BiosVerdict::Ready);
    }

    #[test]
    fn name_absent_from_info_falls_back_to_presence() {
        let dir = empty_dir();
        let statuses = vec![status("bios_MD.bin", Good)];
        assert!(matches!(
            bios_verdict(Some(DiscRegion::Europe), &statuses, dir.path()),
            BiosVerdict::Missing { .. }
        ));
        std::fs::write(dir.path().join("bios_CD_E.bin"), b"bios").unwrap();
        assert_eq!(
            bios_verdict(Some(DiscRegion::Europe), &statuses, dir.path()),
            BiosVerdict::Ready
        );
        assert_eq!(
            bios_verdict(None, &statuses, dir.path()),
            BiosVerdict::Ready
        );
    }

    fn iso(dir: &Path, name: &str, region_byte: u8) -> std::path::PathBuf {
        let mut data = vec![0u8; 0x800];
        data[..14].copy_from_slice(b"SEGADISCSYSTEM");
        data[0x20B] = region_byte;
        let path = dir.join(name);
        std::fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn region_from_header_first_then_name() {
        let dir = empty_dir();
        // The header wins over a contradicting name tag (the core reads the header).
        let p = iso(dir.path(), "Jeu (USA).iso", 0x64);
        assert_eq!(region_of(&p), Some(DiscRegion::Europe));
        let p = iso(dir.path(), "Jeu.iso", 0xA1);
        assert_eq!(region_of(&p), Some(DiscRegion::Japan));
        // `.chd`: never read, the name decides.
        let chd = dir.path().join("Dune (Europe).chd");
        std::fs::write(&chd, b"MComprHD").unwrap();
        assert_eq!(region_of(&chd), Some(DiscRegion::Europe));
        let chd = dir.path().join("Dune.chd");
        std::fs::write(&chd, b"MComprHD").unwrap();
        assert_eq!(region_of(&chd), None);
        // Missing file: no panic, name fallback.
        assert_eq!(
            region_of(&dir.path().join("Absent (Japan).cue")),
            Some(DiscRegion::Japan)
        );
    }
}
