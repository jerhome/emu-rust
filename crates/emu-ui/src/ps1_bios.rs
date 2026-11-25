//! Display of the PlayStation BIOS files of the system folder (ADR 0021 amendment 2026-11-23, point 8): region
//! recognised by hash (by name when the hash is unknown), and region aliases (`scph5501.bin` = copy of
//! `SCPH-7001`). Pure texts are unit-tested; [`scan`] hashes the few files present.

use std::collections::BTreeMap;
use std::path::Path;

use emu_assets::bios::{FirmwareAlias, FirmwareCatalogue, Identification, identify_firmware};
use emu_assets::cores::{PS1_BIOS, Ps1Region, ps1_region_for_name, ps1_wrong_region};

/// Cores that look the canonical `scph550x.bin` names up (the targets of an alias).
pub const PS1_CORES: &str = "swanstation, beetle_psx et pcsx_rearmed";

/// What is known of one PS1 BIOS file present in the system folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ps1Seen {
    pub region: Option<Ps1Region>,
    /// Console model this canonical file is a copy of (`SCPH-7001`): the dump is none of the canonical ones.
    pub copy_of: Option<String>,
}

pub fn region_label(region: Ps1Region) -> &'static str {
    match region {
        Ps1Region::Japan => "Japon",
        Ps1Region::Usa => "USA",
        Ps1Region::Europe => "Europe",
    }
}

fn hex(md5: &[u8; 16]) -> String {
    md5.iter().map(|b| format!("{b:02x}")).collect()
}

/// Recognition of `name` from its content (`md5`), by name when the hash is unknown.
pub fn recognise(name: &str, md5: &[u8; 16]) -> Ps1Seen {
    let hex = hex(md5);
    let entries: Vec<_> = PS1_BIOS.iter().filter(|b| b.md5 == hex).collect();
    let region = entries
        .first()
        .map(|b| b.region)
        .or_else(|| ps1_region_for_name(name));
    let canonical = [Ps1Region::Japan, Ps1Region::Usa, Ps1Region::Europe]
        .iter()
        .any(|r| r.canonical_file() == name);
    let copy_of = (canonical && !entries.is_empty() && entries.iter().all(|b| b.file != name))
        .then(|| entries[0].model());
    Ps1Seen { region, copy_of }
}

/// Every known PS1 file present in `system_dir`, keyed by file name. Unreadable files are left out.
pub fn scan(system_dir: &Path) -> BTreeMap<String, Ps1Seen> {
    let mut names: Vec<&str> = PS1_BIOS.iter().map(|b| b.file).collect();
    names.extend(
        [Ps1Region::Japan, Ps1Region::Usa, Ps1Region::Europe].map(Ps1Region::canonical_file),
    );
    let mut out = BTreeMap::new();
    for name in names {
        let path = system_dir.join(name);
        if !path.is_file() {
            continue;
        }
        // An empty catalogue reports every file as unknown, with its md5.
        if let Ok(Identification::Unknown { md5, .. }) =
            identify_firmware(&path, &FirmwareCatalogue::default())
        {
            out.insert(name.to_owned(), recognise(name, &md5));
        }
    }
    out
}

/// Suffix of a present file: « , USA, copie de SCPH-7001 ». Empty when nothing is known.
pub fn present_suffix(seen: &Ps1Seen) -> String {
    let mut s = String::new();
    if let Some(r) = seen.region {
        s.push_str(&format!(", {}", region_label(r)));
    }
    if let Some(model) = &seen.copy_of {
        s.push_str(&format!(", copie de {model}"));
    }
    s
}

/// « SCPH-7001 (USA) — copie sous scph5501.bin, utilisée par swanstation, beetle_psx et pcsx_rearmed ».
pub fn alias_line(alias: &FirmwareAlias) -> String {
    format!(
        "{} ({}) — copie sous {}, utilisée par {PS1_CORES}",
        alias.revision,
        region_label(alias.region),
        alias.name
    )
}

/// « BIOS d'une autre région » refinement of a wrong hash on a canonical name, `None` otherwise.
pub fn wrong_region_text(wanted: &str, md5: &[u8; 16]) -> Option<String> {
    let found = ps1_wrong_region(wanted, md5)?;
    let expected = Ps1Region::from_canonical_file(wanted)?;
    Some(format!(
        "BIOS d'une autre région ({}) : {} attendu ici ({})",
        region_label(found),
        region_label(expected),
        wanted
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md5_of_hex(s: &str) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    fn md5_of(file: &str) -> [u8; 16] {
        md5_of_hex(PS1_BIOS.iter().find(|b| b.file == file).unwrap().md5)
    }

    #[test]
    fn recognises_region_by_hash_and_flags_copies() {
        let own = recognise("scph5501.bin", &md5_of("scph5501.bin"));
        assert_eq!(own.region, Some(Ps1Region::Usa));
        assert_eq!(own.copy_of, None);
        assert_eq!(present_suffix(&own), ", USA");

        let copy = recognise("scph5501.bin", &md5_of("scph7001.bin"));
        assert_eq!(copy.region, Some(Ps1Region::Usa));
        assert_eq!(copy.copy_of.as_deref(), Some("SCPH-7001"));
        assert_eq!(present_suffix(&copy), ", USA, copie de SCPH-7001");

        // A non canonical name is never « a copy ».
        assert_eq!(
            recognise("scph7001.bin", &md5_of("scph7001.bin")).copy_of,
            None
        );
    }

    #[test]
    fn unknown_hash_falls_back_to_the_name() {
        let seen = recognise("scph7000.bin", &[0; 16]);
        assert_eq!(seen.region, Some(Ps1Region::Japan));
        assert_eq!(present_suffix(&seen), ", Japon");
        let none = recognise("autre.bin", &[0; 16]);
        assert_eq!(present_suffix(&none), "");
    }

    #[test]
    fn alias_line_names_model_region_target_and_cores() {
        let alias = FirmwareAlias {
            name: "scph5501.bin".into(),
            source_name: "scph7001.bin".into(),
            revision: "SCPH-7001".into(),
            region: Ps1Region::Usa,
        };
        assert_eq!(
            alias_line(&alias),
            "SCPH-7001 (USA) — copie sous scph5501.bin, utilisée par swanstation, beetle_psx et pcsx_rearmed"
        );
    }

    #[test]
    fn wrong_region_text_only_for_a_known_other_region() {
        let europe = md5_of("scph1002.bin");
        let t = wrong_region_text("scph5501.bin", &europe).unwrap();
        assert!(
            t.contains("autre région (Europe)") && t.contains("USA"),
            "{t}"
        );
        assert_eq!(wrong_region_text("scph5502.bin", &europe), None);
        assert_eq!(wrong_region_text("scph5501.bin", &[0; 16]), None);
        assert_eq!(wrong_region_text("autre.bin", &europe), None);
    }

    #[test]
    fn scan_reads_the_system_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("scph5501.bin"), b"fake").unwrap();
        std::fs::write(dir.path().join("autre.bin"), b"x").unwrap();
        let seen = scan(dir.path());
        assert_eq!(seen.len(), 1);
        // Unknown content under a canonical name: region from the name, not a copy.
        assert_eq!(seen["scph5501.bin"].region, Some(Ps1Region::Usa));
        assert_eq!(seen["scph5501.bin"].copy_of, None);
    }
}
