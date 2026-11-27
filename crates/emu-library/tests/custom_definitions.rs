//! The synthetic definitions of `tests/tools/mkcustom.py` (lot PQ1) loaded by `Catalog::load` (lot PB2): every case
//! of its manifest gives the expected systems and errors. Public API only; no test publishes a catalog.
//!
//! Three expectations of the manifest are corrected here, each one with the reason (design 8.0, ADR 0025):
//! - `e-reserved-table-id` (id `psx`, another database): an id of an **official** system is a conflict of the catalog
//!   (`SwitchKind::Uncertain`, ADR 0025 §10), exactly like `conflict-same-id` (id `megadrive`); only the ids of the
//!   table that no official system uses stay reserved to their own database (case added below with `pcengine`);
//! - `e-forbidden-extension-bin`: `bin` is in the 8.0 disc list but is kept for cartridges (Atari 2600, ADR 0025
//!   §9): `cartridge_extensions` decides per core, the definition may hold it;
//! - the case names themselves are kept.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use emu_library::{Catalog, DefinitionError, SwitchKind};
use serde_json::Value;

/// Generated once per test binary (the only subprocess); `None` without python3.
fn data() -> Option<&'static (PathBuf, Value)> {
    static DATA: OnceLock<Option<(PathBuf, Value)>> = OnceLock::new();
    DATA.get_or_init(|| {
        let python = Command::new("python3")
            .args(["-I", "-c", "pass"])
            .output()
            .is_ok_and(|o| o.status.success());
        if !python {
            return None;
        }
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
        fs::create_dir_all(base).unwrap();
        let old = std::time::Duration::from_secs(3600);
        for e in fs::read_dir(base).unwrap().flatten() {
            let stale = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > old));
            if stale && e.file_name().to_string_lossy().starts_with("mkcustom-") {
                let _ = fs::remove_dir_all(e.path());
            }
        }
        let root = base.join(format!("mkcustom-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let out = root.join("gen");
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/tools/mkcustom.py");
        let status = Command::new("python3")
            .arg("-I")
            .arg(script)
            .arg(&out)
            .status()
            .expect("python3 runs");
        assert!(status.success(), "mkcustom.py failed");
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
        Some((out, manifest))
    })
    .as_ref()
}

fn variant(error: &DefinitionError) -> String {
    let text = format!("{error:?}");
    text.split(['(', ' ']).next().unwrap().to_owned()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn every_definition_case_of_the_manifest_loads_as_expected() {
    let Some((root, manifest)) = data() else {
        eprintln!("python3 unavailable: skipped");
        return;
    };
    let cases = manifest["definitions"].as_array().unwrap();
    assert!(cases.len() >= 70, "{} cases", cases.len());
    let mut seen = 0;
    for case in cases {
        let name = case["case"].as_str().unwrap();
        if case["api_only"].as_bool() == Some(true) {
            continue;
        }
        let dir = root.join(case["dir"].as_str().unwrap());
        let loaded = Catalog::load(&dir);
        let mut ok = strings(&case["expect"]["ok"]);
        let mut errors: BTreeMap<String, String> = case["expect"]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["id"].as_str().unwrap().to_owned(),
                    e["error"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        // Corrections explained at the top of this file.
        if name == "e-reserved-table-id" {
            ok = vec!["psx".into()];
            errors.clear();
        }
        if name == "e-forbidden-extension-bin" {
            ok = vec!["fe".into()];
            errors.clear();
        }
        let mut got_ok: Vec<String> = loaded.catalog.customs().map(|c| c.id.clone()).collect();
        got_ok.sort();
        ok.sort();
        assert_eq!(got_ok, ok, "{name}: loaded systems");
        let got_errors: BTreeMap<String, String> = loaded
            .problems
            .iter()
            .map(|p| (p.id.clone(), variant(&p.error)))
            .collect();
        assert_eq!(got_errors, errors, "{name}: errors");
        assert_eq!(
            loaded.problems.len(),
            errors.len(),
            "{name}: one problem per file"
        );
        for p in &loaded.problems {
            assert!(p.file.starts_with(&dir), "{name}: {:?}", p.file);
        }
        seen += 1;
    }
    assert!(seen >= 70, "{seen} cases checked");
}

#[test]
fn a_newer_format_keeps_the_file_intact_and_a_pcengine_with_another_database_is_reserved() {
    let Some((root, _)) = data() else { return };
    let dir = root.join("definitions").join("e-newer-format");
    let before = fs::read(dir.join("fmt2.toml")).unwrap();
    let loaded = Catalog::load(&dir);
    assert_eq!(loaded.problems[0].error, DefinitionError::NewerFormat(2));
    assert_eq!(fs::read(dir.join("fmt2.toml")).unwrap(), before);

    // A table id that no official system uses is reserved to its own database.
    let tmp = tempfile::tempdir().unwrap();
    let text = fs::read_to_string(
        root.join("definitions")
            .join("valid-pcengine")
            .join("pcengine.toml"),
    )
    .unwrap();
    fs::write(
        tmp.path().join("pcengine.toml"),
        text.replace("NEC - PC Engine - TurboGrafx 16", "Demo - Not A PC Engine"),
    )
    .unwrap();
    let loaded = Catalog::load(tmp.path());
    assert_eq!(loaded.problems.len(), 1);
    assert_eq!(loaded.problems[0].error, DefinitionError::ReservedId);
    // With its own database it loads.
    fs::write(tmp.path().join("pcengine.toml"), text).unwrap();
    assert!(
        Catalog::load(tmp.path())
            .catalog
            .custom("pcengine")
            .is_some()
    );
}

#[test]
fn conflict_cases_give_the_documented_switch_kinds() {
    let Some((root, _)) = data() else { return };
    let kind = |case: &str, custom: &str| -> (SwitchKind, Vec<String>, Vec<String>) {
        let loaded = Catalog::load(&root.join("definitions").join(case));
        let p = loaded
            .pending_switches
            .iter()
            .find(|p| p.custom_id == custom)
            .unwrap_or_else(|| panic!("{case}: no proposal for {custom}"));
        (
            p.kind,
            loaded
                .catalog
                .masked_official()
                .iter()
                .map(|s| s.id.clone())
                .collect(),
            loaded.catalog.reserved_folders().to_vec(),
        )
    };
    assert_eq!(
        kind("conflict-same-id", "megadrive"),
        (
            SwitchKind::Uncertain,
            vec!["megadrive".to_owned()],
            Vec::new()
        )
    );
    assert_eq!(
        kind("conflict-same-database", "mymd"),
        (
            SwitchKind::NeedsMove,
            vec!["megadrive".to_owned()],
            vec!["megadrive".to_owned()]
        )
    );
    assert_eq!(
        kind("conflict-both", "megadrive"),
        (
            SwitchKind::InPlace,
            vec!["megadrive".to_owned()],
            Vec::new()
        )
    );
    // No database but the id of an official system: an uncertain conflict, as the manifest suggests.
    assert_eq!(
        kind("conflict-no-database-official-id", "nes"),
        (SwitchKind::Uncertain, vec!["nes".to_owned()], Vec::new())
    );
    // The same cases without a conflict give no proposal.
    let loaded = Catalog::load(&root.join("definitions").join("valid-minimal"));
    assert!(loaded.pending_switches.is_empty() && loaded.catalog.masked_official().is_empty());
}

#[test]
fn the_trees_of_the_manifest_give_the_documented_reserved_folders() {
    let Some((root, manifest)) = data() else {
        return;
    };
    let trees = manifest["trees"].as_array().unwrap();
    let load = |case: &str| {
        let t = trees
            .iter()
            .find(|t| t["case"] == case)
            .unwrap_or_else(|| panic!("{case}"));
        Catalog::load(&root.join(t["dir"].as_str().unwrap()).join("systems"))
    };
    // gone (retired), broken (in error); roms/orphan is the tidy's business (an id of the database).
    assert_eq!(
        load("reserved-folders").catalog.reserved_folders(),
        ["broken", "gone"]
    );
    // megadrive is an active official id: an invalid file or a replaced one never reserves it.
    let active = load("active-id-never-reserved");
    assert_eq!(active.problems.len(), 1);
    assert!(active.catalog.reserved_folders().is_empty());
    assert!(
        active
            .catalog
            .system("megadrive")
            .is_some_and(|s| !s.is_custom())
    );
    // An uncertain conflict keeps the id of the custom system active.
    let uncertain = load("conflict-uncertain-rows");
    assert!(uncertain.catalog.reserved_folders().is_empty());
    assert_eq!(uncertain.pending_switches[0].kind, SwitchKind::Uncertain);
    // The retired names: only the exact ones are accepted.
    let listed = emu_library::retired_customs(
        &root
            .join(
                trees.iter().find(|t| t["case"] == "retired-names").unwrap()["dir"]
                    .as_str()
                    .unwrap(),
            )
            .join("systems"),
    );
    let names: Vec<String> = listed
        .iter()
        .map(|r| r.file.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["zzdemo-1700000000.toml", "zzdemo-1700000002.replaced.toml"]
    );
}
