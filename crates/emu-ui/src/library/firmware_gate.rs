//! Pre-launch firmware gate of a library game (design v6.0 §2.2, §3.5): turns the single check of the 3.0
//! (`emu_assets::cores::check_core_firmware`) into what the library does. Pure, tested. Replaces the Mega CD
//! check of the 2.1 (`library/megacd.rs`): no Mega CD specific code remains.

use emu_assets::cores::{FirmwareCheck, FirmwareState};

/// What the library does with a [`FirmwareCheck`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirmwareGate {
    /// `Ok` / `Free` / `Unverified` / `Builtin`, or the wanted file absent while the variant is unknown and
    /// another file of the table is present (2.1 rule: the core picks).
    Launch,
    /// `CoreFallback`: launch, the UI warns once per game.
    WarnOnce,
    /// `Missing` (no other file usable), `WrongHash`, `Unreadable`: the generic firmware dialog (« Importer un
    /// BIOS… », « Réessayer », « Copier le chemin », « Annuler », then « Lancer quand même », never focused first).
    /// A wrong hash is a warning, never a block.
    Ask,
}

/// `variant_known`: the variant comes from the game (`Game::region`, else the header probe, else the name tag);
/// `false` when the table's default was used. Unknown variant + wanted file absent + `others_present` not empty
/// ⇒ `Launch` (design v6.0 §2.2: a `.chd` without tag, playable in 2.1 with the one BIOS present).
pub(crate) fn firmware_gate(check: &FirmwareCheck, variant_known: bool) -> FirmwareGate {
    match &check.state {
        FirmwareState::Ok
        | FirmwareState::Free { .. }
        | FirmwareState::Unverified
        | FirmwareState::Builtin { .. } => FirmwareGate::Launch,
        FirmwareState::CoreFallback { .. } => FirmwareGate::WarnOnce,
        FirmwareState::Missing if !variant_known && !check.others_present.is_empty() => {
            FirmwareGate::Launch
        }
        FirmwareState::Missing | FirmwareState::WrongHash { .. } | FirmwareState::Unreadable(_) => {
            FirmwareGate::Ask
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ported one for one from the 2.1 `library/megacd.rs` (`bios_verdict`): present, absent, other region only,
    //! different md5, unknown region.
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;

    use emu_assets::cores::{CoreId, CorePreferences};
    use emu_library::DiscRegion;

    use crate::launch_firmware::{GameFacts, check_launch_firmware};

    const MEGA_CD: &str = "Sega - Mega-CD - Sega CD";

    fn check(state: FirmwareState, others: &[&str]) -> FirmwareCheck {
        FirmwareCheck {
            wanted: Some("bios_CD_E.bin".into()),
            state,
            others_present: others.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// The single check of genesis_plus_gx for a Mega CD disc of `region` (`None`: unknown), on `system_dir`.
    fn mega_cd(system_dir: &Path, region: Option<DiscRegion>) -> (FirmwareCheck, bool) {
        let none = BTreeMap::new();
        let lf = check_launch_firmware(
            system_dir,
            &CorePreferences::builtin(),
            &CoreId::new("genesis_plus_gx").unwrap(),
            Some(Path::new("/nonexistent/Dune.chd")),
            GameFacts {
                database: Some(MEGA_CD),
                region,
            },
            &none,
            &none,
        )
        .expect("genesis_plus_gx has a Mega CD table");
        (lf.check, lf.variant_known)
    }

    fn gate(system_dir: &Path, region: Option<DiscRegion>) -> FirmwareGate {
        let (check, known) = mega_cd(system_dir, region);
        firmware_gate(&check, known)
    }

    #[test]
    fn names_follow_the_core_rule() {
        let dir = tempfile::tempdir().unwrap();
        for (region, name) in [
            (DiscRegion::Europe, "bios_CD_E.bin"),
            (DiscRegion::Usa, "bios_CD_U.bin"),
            (DiscRegion::Japan, "bios_CD_J.bin"),
        ] {
            let (check, known) = mega_cd(dir.path(), Some(region));
            assert_eq!(check.wanted.as_deref(), Some(name));
            assert!(known);
        }
    }

    #[test]
    fn wanted_bios_present_is_ready() {
        for state in [
            FirmwareState::Ok,
            FirmwareState::Unverified,
            FirmwareState::Free { label: "x".into() },
            FirmwareState::Builtin { label: "x".into() },
        ] {
            assert_eq!(
                firmware_gate(&check(state, &[]), true),
                FirmwareGate::Launch
            );
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bios_CD_J.bin"), b"bios").unwrap();
        // Present under the exact name: launched, or at most a non-blocking wrong-hash warning.
        assert_ne!(
            mega_cd(dir.path(), Some(DiscRegion::Japan)).0.state,
            FirmwareState::Missing
        );
    }

    #[test]
    fn wrong_hash_asks_and_is_not_missing() {
        assert_eq!(
            firmware_gate(&check(FirmwareState::WrongHash { md5: [0; 16] }, &[]), true),
            FirmwareGate::Ask
        );
        // A fake file under the exact name is a different revision (or unverifiable), never « absent ».
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bios_CD_U.bin"), b"not a bios").unwrap();
        let (c, _) = mega_cd(dir.path(), Some(DiscRegion::Usa));
        assert!(
            matches!(
                c.state,
                FirmwareState::WrongHash { .. } | FirmwareState::Unverified
            ),
            "{c:?}"
        );
    }

    #[test]
    fn missing_lists_the_other_regions_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bios_CD_U.bin"), b"bios").unwrap();
        let (c, known) = mega_cd(dir.path(), Some(DiscRegion::Europe));
        assert_eq!(c.state, FirmwareState::Missing);
        assert_eq!(c.others_present, vec!["bios_CD_U.bin".to_owned()]);
        // Region known: another region's BIOS does not launch the game (the core would refuse it).
        assert_eq!(firmware_gate(&c, known), FirmwareGate::Ask);
        assert_eq!(
            firmware_gate(
                &check(FirmwareState::Unreadable("denied".into()), &[]),
                true
            ),
            FirmwareGate::Ask
        );
    }

    #[test]
    fn unknown_region_needs_any_bios() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(gate(dir.path(), None), FirmwareGate::Ask);
        std::fs::write(dir.path().join("bios_CD_J.bin"), b"bios").unwrap();
        assert_eq!(gate(dir.path(), None), FirmwareGate::Launch);
        assert_eq!(
            firmware_gate(&check(FirmwareState::Missing, &["bios_CD_J.bin"]), false),
            FirmwareGate::Launch
        );
        assert_eq!(
            firmware_gate(&check(FirmwareState::Missing, &[]), false),
            FirmwareGate::Ask
        );
    }

    #[test]
    fn fallback_warns_once() {
        assert_eq!(
            firmware_gate(
                &check(
                    FirmwareState::CoreFallback {
                        label: "AROS".into()
                    },
                    &[]
                ),
                true
            ),
            FirmwareGate::WarnOnce
        );
    }
}
