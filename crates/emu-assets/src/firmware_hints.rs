//! Advice on user-provided firmware that improves (or is required by) a core. Pure: no network, no download
//! (ADR 0005); the UI only reports it and lets the user import the files.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

const BUILTIN: &str = include_str!("../data/firmware_hints.toml");

#[derive(Deserialize)]
struct Table {
    cores: BTreeMap<String, Hint>,
}

#[derive(Deserialize)]
struct Hint {
    system: String,
    note: String,
    files: Vec<HintFile>,
}

#[derive(Deserialize)]
struct HintFile {
    name: String,
    required: bool,
}

/// One expected file and whether it is already in the system directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdviceFile {
    pub name: String,
    pub required: bool,
    pub present: bool,
}

/// Firmware advice for a core: `files` lists everything the core can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareAdvice {
    pub core_id: String,
    pub files: Vec<AdviceFile>,
    pub note: String,
}

/// Advice for `core_id` running a game of emu-library system id `system`, or `None` if there is no entry for
/// that pair or at least one file is present for every
/// requirement (nothing to report). Files are looked up in `system_dir` only.
pub fn firmware_advice(core_id: &str, system: &str, system_dir: &Path) -> Option<FirmwareAdvice> {
    let table: Table = toml::from_str(BUILTIN).ok()?;
    advice_from(&table, core_id, system, system_dir)
}

fn advice_from(
    table: &Table,
    core_id: &str,
    system: &str,
    system_dir: &Path,
) -> Option<FirmwareAdvice> {
    let hint = table.cores.get(core_id).filter(|h| h.system == system)?;
    let files: Vec<AdviceFile> = hint
        .files
        .iter()
        .map(|f| AdviceFile {
            name: f.name.clone(),
            required: f.required,
            // Names come from the embedded table, never from user input.
            present: system_dir.join(&f.name).is_file(),
        })
        .collect();
    // Variants (regional BIOS) are alternatives: one present file is enough.
    if files.iter().any(|f| f.present) {
        return None;
    }
    Some(FirmwareAdvice {
        core_id: core_id.to_owned(),
        files,
        note: hint.note.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_table_parses_and_has_no_path_escape() {
        let t: Table = toml::from_str(BUILTIN).unwrap();
        assert!(t.cores.contains_key("mgba"));
        for h in t.cores.values() {
            assert!(!h.files.is_empty());
            for f in &h.files {
                let p = std::path::Path::new(&f.name);
                assert!(
                    !f.name.contains('\\')
                        && !f.name.contains(':')
                        && p.components()
                            .all(|c| matches!(c, std::path::Component::Normal(_))),
                    "unsafe firmware name {:?}",
                    f.name
                );
            }
        }
    }

    #[test]
    fn missing_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let a = firmware_advice("mgba", "gba", dir.path()).unwrap();
        assert_eq!(a.core_id, "mgba");
        assert_eq!(a.files.len(), 1);
        assert!(!a.files[0].present && !a.files[0].required);
    }

    #[test]
    fn present_file_means_no_advice() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gba_bios.bin"), b"x").unwrap();
        assert!(firmware_advice("mgba", "gba", dir.path()).is_none());
    }

    #[test]
    fn one_regional_bios_is_enough_and_required_flag_kept() {
        let dir = tempfile::tempdir().unwrap();
        let a = firmware_advice("picodrive", "megacd", dir.path()).unwrap();
        assert!(a.files.iter().all(|f| f.required && !f.present));
        std::fs::write(dir.path().join("bios_CD_U.bin"), b"x").unwrap();
        assert!(firmware_advice("picodrive", "megacd", dir.path()).is_none());
    }

    #[test]
    fn unknown_core_has_no_advice() {
        let dir = tempfile::tempdir().unwrap();
        assert!(firmware_advice("nope", "gba", dir.path()).is_none());
    }

    #[test]
    fn entry_applies_only_to_its_system() {
        let dir = tempfile::tempdir().unwrap();
        assert!(firmware_advice("genesis_plus_gx", "megadrive", dir.path()).is_none());
        assert!(firmware_advice("picodrive", "32x", dir.path()).is_none());
        assert!(firmware_advice("genesis_plus_gx", "megacd", dir.path()).is_some());
    }
}
