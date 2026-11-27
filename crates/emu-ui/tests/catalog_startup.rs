//! `publish_catalog` (start-up, ADR 0025 §1): the definitions of `<data>/systems/` reach `system()`. The catalog is
//! process-wide, so the one test that publishes lives alone in this binary.

use emu_ui::DataDirs;
use emu_ui::library_db::{
    Catalog, CustomSystemDraft, DatSource, DefinitionOrigin, system, systems, write_custom,
};

fn draft(id: &str) -> CustomSystemDraft {
    CustomSystemDraft {
        id: id.into(),
        name: "Demo System".into(),
        database: Some("Demo - Fictional - Console".into()),
        extensions: vec!["dmo".into()],
        dats: vec![DatSource::NoIntro],
        screenscraper: None,
        igdb: None,
        cores: vec!["demo_core".into()],
        ignored_firmware: Vec::new(),
        origin: DefinitionOrigin {
            core: "demo_core".into(),
            info_name: None,
            created_by: "8.0.0".into(),
            created_unix: 1_764_028_800,
        },
    }
}

#[test]
fn publish_catalog_reads_the_definitions_and_reports_the_broken_ones() {
    // No systems folder at all: the official catalog, no problem.
    let empty = tempfile::tempdir().unwrap();
    let none = emu_ui::publish_catalog(&DataDirs::under(empty.path()));
    assert!(none.problems.is_empty());

    let root = tempfile::tempdir().unwrap();
    let dirs = DataDirs::under(root.path());
    std::fs::create_dir_all(&dirs.custom_systems).unwrap();
    write_custom(&dirs.custom_systems, &draft("zzdemo"), &Catalog::official()).unwrap();
    std::fs::write(dirs.custom_systems.join("broken.toml"), "not = [toml").unwrap();

    assert!(system("zzdemo").is_none(), "official catalog before");
    let official = systems().len();
    let load = emu_ui::publish_catalog(&dirs);

    assert_eq!(load.problems.len(), 1);
    assert!(load.problems[0].file.ends_with("broken.toml"));
    let demo = system("zzdemo").expect("published");
    assert!(demo.is_custom());
    assert_eq!(systems().len(), official + 1);
}
