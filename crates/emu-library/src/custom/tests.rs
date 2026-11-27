//! Unit tests of the pure parts of the definitions: validation, names, ids, extensions, table.

use super::*;
use crate::catalog::Catalog;

fn origin() -> DefinitionOrigin {
    DefinitionOrigin {
        core: "demo_core".into(),
        info_name: Some("Demo (Fictional Core)".into()),
        created_by: "8.0.0".into(),
        created_unix: 1_764_028_800,
    }
}

fn draft() -> CustomSystemDraft {
    CustomSystemDraft {
        id: "zzdemo".into(),
        name: "Demo System".into(),
        database: Some("Demo - Fictional - Console".into()),
        extensions: vec!["dmo".into()],
        dats: vec![DatSource::NoIntro],
        screenscraper: Some(9001),
        igdb: Some(9002),
        cores: vec!["demo_core".into()],
        ignored_firmware: Vec::new(),
        origin: origin(),
    }
}

/// The error of a new definition (`creating`) built from [`draft`] and changed by `change`.
fn error_of(change: impl FnOnce(&mut CustomSystemDraft)) -> DefinitionError {
    let mut d = draft();
    change(&mut d);
    validate_custom(&d, &Catalog::official(), true).unwrap_err()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Table

#[test]
fn the_table_is_injective_and_valid() {
    let machines = machines();
    assert!(machines.len() >= 100, "{} entries", machines.len());
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for m in machines {
        assert!(ids.insert(m.id.as_str()), "id {} twice", m.id);
        assert!(
            names.insert(m.database.as_str()),
            "name {} twice",
            m.database
        );
        assert!(is_valid_id_syntax(&m.id), "{}", m.id);
        assert!(
            !is_forbidden_id(&m.id),
            "{} is a forbidden folder name",
            m.id
        );
    }
}

#[test]
fn the_17_official_systems_are_in_the_table_with_their_own_id() {
    let official = Catalog::official();
    assert_eq!(official.systems().len(), 17);
    for s in official.systems() {
        assert_eq!(
            known_id(s.libretro_name().unwrap()),
            Some(s.id.as_str()),
            "{}",
            s.id
        );
    }
}

#[test]
fn every_table_name_passes_the_white_list_or_has_a_valid_local_file_stem() {
    for m in machines() {
        let stem = dat_file_stem(&m.database);
        assert!(
            is_valid_label(&m.database, 128) || is_valid_label(&stem, 128),
            "{} / {stem}",
            m.database
        );
        assert!(
            !stem.contains(['/', '\\', ':', '*', '?', '\"', '<', '>', '|']),
            "{stem}"
        );
        assert!(!stem.contains(".."), "{stem}");
    }
}

#[test]
fn table_lookups() {
    assert_eq!(
        known_id("NEC - PC Engine - TurboGrafx 16"),
        Some("pcengine")
    );
    assert_eq!(known_id("Sega - Mega Drive - Genesis"), Some("megadrive"));
    assert_eq!(known_id("Nobody - Nothing"), None);
    assert_eq!(
        known_tier("NEC - PC Engine - TurboGrafx 16"),
        Some(MachineTier::Cartridge)
    );
    assert_eq!(
        known_tier("NEC - PC Engine CD - TurboGrafx-CD"),
        Some(MachineTier::Disc)
    );
    assert_eq!(known_tier("Atari - ST"), Some(MachineTier::Computer));
    assert_eq!(
        known_tier("Capcom - CP System II"),
        Some(MachineTier::Arcade)
    );
    assert_eq!(known_tier("Nobody - Nothing"), None);
}

// ---------------------------------------------------------------------------------------------------------------
// Ids

#[test]
fn a_known_database_always_gets_the_id_of_the_table() {
    let taken = |_: &str| true;
    assert_eq!(
        proposed_id(Some("NEC - PC Engine - TurboGrafx 16"), "Anything", &taken),
        "pcengine"
    );
    assert_eq!(
        proposed_id(Some("Atari - Lynx"), "Whatever", &|_| false),
        "lynx"
    );
}

#[test]
fn an_added_entry_is_the_reduction_of_its_name_without_suffix() {
    let free = |_: &str| false;
    assert_eq!(
        proposed_id(Some("Acme - Foo Bar Console"), "x", &free),
        "foobarconsole"
    );
    // No manufacturer separator: the whole name.
    assert_eq!(proposed_id(Some("Homebrew Box"), "x", &free), "homebrewbox");
    // 24 characters at most.
    let long = "Acme - ".to_owned() + &"abcdefghij".repeat(5);
    assert_eq!(
        proposed_id(Some(&long), "x", &free),
        "abcdefghijabcdefghijabcd"
    );
}

#[test]
fn a_taken_or_reserved_reduction_gets_a_numeric_suffix() {
    let taken = |id: &str| id == "foobar" || id == "foobar2";
    assert_eq!(proposed_id(Some("Acme - Foo Bar"), "x", &taken), "foobar3");
    // The suffix fits in the 24 characters.
    let long = "Acme - ".to_owned() + &"a".repeat(40);
    let all_a = "a".repeat(24);
    let id = proposed_id(Some(&long), "x", &|i| i == all_a);
    assert_eq!(id.len(), 24);
    assert!(id.ends_with('2'), "{id}");
    // An id of the table belongs to its own machine: another name reducing to it takes a suffix.
    assert_eq!(proposed_id(Some("Acme - Lynx"), "x", &|_| false), "lynx2");
    // Forbidden names are never proposed.
    assert_eq!(proposed_id(None, "Con", &|_| false), "con2");
    assert_eq!(proposed_id(Some("Acme - Roms"), "x", &|_| false), "roms2");
}

#[test]
fn a_system_without_database_gets_an_id_derived_from_its_name() {
    assert_eq!(
        proposed_id(None, "My Weird Box 9000", &|_| false),
        "myweirdbox9000"
    );
    assert_eq!(proposed_id(None, "---", &|_| false), "custom");
    assert_eq!(proposed_id(None, "Nes", &|_| false), "nes2");
}

#[test]
fn table_ids_are_reserved_to_their_own_database() {
    assert!(is_reserved_id("pcengine", None));
    assert!(is_reserved_id("pcengine", Some("Acme - Other")));
    assert!(!is_reserved_id(
        "pcengine",
        Some("NEC - PC Engine - TurboGrafx 16")
    ));
    assert!(!is_reserved_id("zzdemo", None));
    assert!(!is_reserved_id("zzdemo", Some("Acme - Other")));
    for forbidden in [
        "con",
        "prn",
        "aux",
        "nul",
        "com1",
        "com9",
        "lpt1",
        "lpt9",
        "roms",
        "system",
        "saves",
        "states",
        "cores",
        "cache",
        "logs",
        "covers",
        "database",
        "rangement",
        "systems",
        "assets",
        "gameoptions",
    ] {
        assert!(is_reserved_id(forbidden, None), "{forbidden}");
        assert!(
            is_reserved_id(forbidden, Some("NEC - PC Engine - TurboGrafx 16")),
            "{forbidden}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Names and files

#[test]
fn names_are_normalised_before_validation() {
    assert_eq!(
        normalize_name("Sega 8/16 bit (various)"),
        "Sega 8-16 bit (various)"
    );
    assert_eq!(normalize_name("  Demo   System  "), "Demo System");
    assert_eq!(normalize_name("a:b*c?d\"e<f>g|h\\i"), "a-b-c-d-e-f-g-h-i");
    assert_eq!(normalize_name("Demo\u{7}\u{202e}evil"), "Demo--evil");
    assert_eq!(normalize_name("a..b"), "a.b");
    assert_eq!(normalize_name("Demo."), "Demo");
    assert_eq!(normalize_name(""), "System");
    let long = normalize_name(&"N".repeat(100));
    assert_eq!(long.chars().count(), 64);
    // Whatever the input, the output passes the validation of a name.
    for raw in [
        "Demo/Sys",
        "a..b",
        " . ",
        "\u{202e}",
        "???",
        "x".repeat(200).as_str(),
        "Ünïcode Sÿstem",
    ] {
        assert!(
            is_valid_label(&normalize_name(raw), 64),
            "{raw:?} -> {:?}",
            normalize_name(raw)
        );
    }
}

#[test]
fn local_dat_stems_replace_forbidden_characters() {
    assert_eq!(
        dat_file_stem("Sega - Mega-CD - Sega CD"),
        "Sega - Mega-CD - Sega CD"
    );
    assert_eq!(dat_file_stem("Acme - 8/16 bit"), "Acme - 8_16 bit");
    assert_eq!(dat_file_stem("a:b*c?d\"e<f>g|h\\i"), "a_b_c_d_e_f_g_h_i");
    assert!(!dat_file_stem("a/../b").contains(".."));
    assert!(!dat_file_stem("a/../b").contains('/'));
}

#[test]
fn cartridge_extensions_follow_the_8_0_lists() {
    // stella: the cartridge keeps `bin`.
    assert_eq!(
        cartridge_extensions(&strings(&["a26", "bin"])),
        ["a26", "bin"]
    );
    // mednafen_pce_fast: the disc formats go, the cartridge stays.
    assert_eq!(
        cartridge_extensions(&strings(&[
            "pce", "sgx", "cue", "ccd", "chd", "toc", "m3u", "zip"
        ])),
        ["pce", "sgx"]
    );
    // genesis_plus_gx style: `bin` goes when another disc extension is present.
    assert_eq!(
        cartridge_extensions(&strings(&[
            "md", "smd", "gen", "bin", "cue", "iso", "chd", "m3u"
        ])),
        ["md", "smd", "gen"]
    );
    // Lower case, valid syntax only, no duplicates, archives and playlists out.
    assert_eq!(
        cartridge_extensions(&strings(&[
            "DMO",
            "dmo",
            "/",
            ".",
            "V32",
            "toolongext",
            "7z",
            "m3u8",
            "a b",
            ""
        ])),
        ["dmo", "v32"]
    );
    assert!(cartridge_extensions(&strings(&["cue", "ccd", "chd", "toc", "m3u"])).is_empty());
    assert!(cartridge_extensions(&strings(&["zip"])).is_empty());
    assert!(cartridge_extensions(&[]).is_empty());
    // `bin` alone is the only extension of a cartridge core.
    assert_eq!(cartridge_extensions(&strings(&["bin"])), ["bin"]);
    // img / mdf count as disc formats too.
    assert_eq!(
        cartridge_extensions(&strings(&["rom", "bin", "img"])),
        ["rom"]
    );
}

#[test]
fn relative_paths_are_safe_or_refused() {
    for ok in [
        "a.bin",
        "firm/f1.bin",
        "Machines/MSX/cbios.rom",
        "a b/c d.bin",
        "dc/dc_boot.bin",
    ] {
        assert!(is_safe_relative_path(ok), "{ok}");
    }
    for bad in [
        "",
        "/abs.bin",
        "../x",
        "a/../b",
        "a/./b",
        "a\\b",
        "c:x",
        "C:/x",
        "a//b",
        "a/",
        "con.bin",
        "a/NUL",
        "lpt1",
        "com3.txt",
        "a ",
        "x.",
        "dir /x",
        "a\0b",
        "conin$",
        "a/aux.rom",
    ] {
        assert!(!is_safe_relative_path(bad), "{bad:?}");
    }
}

#[test]
fn retired_names() {
    assert_eq!(
        retired_file_name("zzdemo", Retirement::Removed, 1_700_000_000),
        "zzdemo-1700000000.toml"
    );
    assert_eq!(
        retired_file_name("zzdemo", Retirement::Replaced, 1_700_000_002),
        "zzdemo-1700000002.replaced.toml"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// validate_custom

#[test]
fn the_reference_draft_is_valid() {
    assert_eq!(
        validate_custom(&draft(), &Catalog::official(), true),
        Ok(())
    );
}

#[test]
fn invalid_ids() {
    for id in ["", "BAD", "a-b", "a b", "é", "a.b"] {
        assert_eq!(
            error_of(|d| d.id = id.into()),
            DefinitionError::InvalidId,
            "{id:?}"
        );
    }
    assert_eq!(
        error_of(|d| d.id = "x".repeat(25)),
        DefinitionError::InvalidId
    );
    let mut d = draft();
    d.id = "a".repeat(24);
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
}

#[test]
fn reserved_ids() {
    for id in [
        "con", "prn", "aux", "nul", "com1", "lpt9", "roms", "system", "saves", "systems", "assets",
    ] {
        assert_eq!(
            error_of(|d| d.id = id.into()),
            DefinitionError::ReservedId,
            "{id}"
        );
    }
    // An id of the table belongs to its own database (creating: official ids too).
    assert_eq!(
        error_of(|d| d.id = "pcengine".into()),
        DefinitionError::ReservedId
    );
    assert_eq!(
        error_of(|d| {
            d.id = "nes".into();
            d.database = None;
        }),
        DefinitionError::ReservedId
    );
    let mut own = draft();
    own.id = "pcengine".into();
    own.database = Some("NEC - PC Engine - TurboGrafx 16".into());
    assert_eq!(validate_custom(&own, &Catalog::official(), true), Ok(()));
}

#[test]
fn an_official_id_is_a_conflict_not_an_error_when_loading() {
    // creating = false: the file of a user already there; the conflict is the business of the catalog (ADR 0025 s10).
    let mut d = draft();
    d.id = "megadrive".into();
    d.database = Some("Demo - Another Machine".into());
    assert_eq!(validate_custom(&d, &Catalog::official(), false), Ok(()));
    d.database = None;
    d.id = "nes".into();
    assert_eq!(validate_custom(&d, &Catalog::official(), false), Ok(()));
    // But a table id of a machine that is not an official system stays reserved.
    d.id = "pcengine".into();
    d.database = Some("Demo - Another Machine".into());
    assert_eq!(
        validate_custom(&d, &Catalog::official(), false),
        Err(DefinitionError::ReservedId)
    );
}

#[test]
fn a_new_definition_cannot_take_an_active_id() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = Catalog::load(dir.path());
    assert_eq!(validate_custom(&draft(), &loaded.catalog, true), Ok(()));
    // Same id as an official of a synthetic catalog (not in the table): duplicate.
    let text = r#"[[system]]
id = "zzdemo"
name = "Z"
extensions = ["zzz"]
database = "Z - Z"
"#;
    let with = Catalog::load_with_official(text, dir.path())
        .unwrap()
        .catalog;
    assert_eq!(
        validate_custom(&draft(), &with, true),
        Err(DefinitionError::DuplicateId)
    );
    // Editing (creating = false) is fine.
    assert_eq!(validate_custom(&draft(), &with, false), Ok(()));
}

#[test]
fn names_and_databases_use_the_white_list() {
    for name in [
        "Demo/Sys",
        "a..b",
        "Demo\u{7}",
        "Demo\u{202e}evil",
        "Demo.",
        " Demo",
        "Demo ",
        "",
        "a:b",
        "a|b",
        "a\\b",
    ] {
        assert_eq!(
            error_of(|d| d.name = name.into()),
            DefinitionError::InvalidName,
            "{name:?}"
        );
    }
    assert_eq!(
        error_of(|d| d.name = "N".repeat(65)),
        DefinitionError::InvalidName
    );
    for database in [
        "Demo: Fictional",
        " Demo",
        "Demo ",
        "D".repeat(129).as_str(),
        "",
        "a..b",
        "a/b",
        "Demo.",
        "x\u{7}y",
    ] {
        assert_eq!(
            error_of(|d| d.database = Some(database.into())),
            DefinitionError::InvalidDatabase,
            "{database:?}"
        );
    }
    let mut d = draft();
    d.name = "Demo + Co., Ltd. 'X' & (Y)! v_2".into();
    d.database = Some("Demo - Fictional (A, B) & Co's - Console".into());
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
    d.name = "N".repeat(64);
    d.database = Some("D".repeat(128));
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
}

#[test]
fn a_database_already_used_by_another_custom_system_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = draft();
    first.id = "first".into();
    write_custom(dir.path(), &first, &Catalog::official()).unwrap();
    let catalog = Catalog::load(dir.path()).catalog;
    let mut second = draft();
    second.id = "second".into();
    assert_eq!(
        validate_custom(&second, &catalog, true),
        Err(DefinitionError::DuplicateDatabase)
    );
    // Editing the first one with its own database is fine; so is a system without database.
    assert_eq!(validate_custom(&first, &catalog, false), Ok(()));
    second.database = None;
    assert_eq!(validate_custom(&second, &catalog, true), Ok(()));
}

#[test]
fn extensions_are_checked() {
    assert_eq!(
        error_of(|d| d.extensions = Vec::new()),
        DefinitionError::NoExtension
    );
    for (ext, _) in [
        ("PCE", 0),
        ("abcdefghi", 0),
        (".pce", 0),
        ("p ce", 0),
        ("", 0),
        ("é", 0),
    ] {
        assert_eq!(
            error_of(|d| d.extensions = strings(&["dmo", ext])),
            DefinitionError::InvalidExtension(ext.into()),
            "{ext:?}"
        );
    }
    assert!(matches!(
        error_of(|d| d.extensions = (0..65).map(|i| format!("e{i}")).collect()),
        DefinitionError::InvalidExtension(_)
    ));
    let mut d = draft();
    d.extensions = (0..64).map(|i| format!("e{i:07}")).collect();
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
}

#[test]
fn archive_playlist_and_disc_extensions_are_forbidden_except_bin() {
    for ext in [
        "zip", "7z", "m3u", "m3u8", "cue", "chd", "iso", "ccd", "toc", "gdi", "cdi", "mds", "mdf",
        "nrg", "pbp", "cso", "img",
    ] {
        assert_eq!(
            error_of(|d| d.extensions = strings(&["dmo", ext])),
            DefinitionError::ForbiddenExtension(ext.into()),
            "{ext}"
        );
    }
    // `bin` is the extension of cartridges such as the Atari 2600: `cartridge_extensions` decides, per core.
    let mut d = draft();
    d.extensions = strings(&["a26", "bin"]);
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
}

#[test]
fn dat_sources_cores_origin_and_firmware_are_bounded() {
    assert_eq!(
        error_of(|d| d.dats = vec![DatSource::NoIntro, DatSource::Redump]),
        DefinitionError::UnsupportedDat("redump".into())
    );
    let mut d = draft();
    d.dats = vec![DatSource::NoIntro, DatSource::Tosec, DatSource::Libretro];
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
    d.dats = Vec::new();
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));

    assert_eq!(error_of(|d| d.cores = Vec::new()), DefinitionError::NoCore);
    assert_eq!(
        error_of(|d| d.cores = (0..9).map(|i| format!("c{i}")).collect()),
        DefinitionError::TooManyCores
    );
    for core in ["Demo Core", "../x", "", "a/b", &"c".repeat(65)] {
        assert_eq!(
            error_of(|d| d.cores = strings(&[core])),
            DefinitionError::InvalidCore(core.chars().take(64).collect()),
            "{core:?}"
        );
    }
    let mut d = draft();
    d.cores = (1..=8).map(|i| format!("core_{i}")).collect();
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
    d.cores = strings(&["mesen-s"]);
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));

    assert_eq!(
        error_of(|d| d.origin.info_name = Some("I".repeat(129))),
        DefinitionError::InvalidOrigin
    );
    assert_eq!(
        error_of(|d| d.origin.created_by = "B".repeat(33)),
        DefinitionError::InvalidOrigin
    );
    assert_eq!(
        error_of(|d| d.origin.core = "Bad Core".into()),
        DefinitionError::InvalidOrigin
    );
    let mut d = draft();
    d.origin.info_name = Some("Demo 8/16 bit (various) / CD".to_owned());
    d.origin.created_by = "B".repeat(32);
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
    d.origin.info_name = Some("I".repeat(128));
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));

    for path in ["../etc/passwd", "/abs.bin", "a\\b.bin", "con.bin", ""] {
        assert_eq!(
            error_of(|d| d.ignored_firmware = strings(&["ok.bin", path])),
            DefinitionError::UnsafeFirmwarePath(path.into()),
            "{path:?}"
        );
    }
    assert_eq!(
        error_of(|d| d.ignored_firmware = (0..65).map(|i| format!("f{i}.bin")).collect()),
        DefinitionError::TooManyFirmware
    );
    let mut d = draft();
    d.ignored_firmware = (0..64).map(|i| format!("firm/f{i}.bin")).collect();
    assert_eq!(validate_custom(&d, &Catalog::official(), true), Ok(()));
}

#[test]
fn an_error_never_quotes_more_than_64_characters() {
    let long = "x/../".repeat(100);
    match error_of(|d| d.ignored_firmware = vec![long.clone()]) {
        DefinitionError::UnsafeFirmwarePath(p) => assert!(p.chars().count() <= 64),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Files: write, read back, retire, restore

fn systems_dir(root: &Path) -> PathBuf {
    root.join("systems")
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_written_definition_is_read_back_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = systems_dir(tmp.path());
    let path = write_custom(&dir, &draft(), &Catalog::official()).unwrap();
    assert_eq!(path, dir.join("zzdemo.toml"));
    // Atomic: no temporary file stays next to it.
    assert_eq!(names_in(&dir), ["zzdemo.toml"]);
    let loaded = Catalog::load(&dir);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    assert_eq!(
        CustomSystemDraft::from_custom(&loaded.catalog, "zzdemo"),
        Some(draft())
    );
    let system = loaded.catalog.system("zzdemo").unwrap();
    assert!(system.is_custom());
    assert_eq!(system.name, "Demo System");
    assert_eq!(system.libretro_name(), Some("Demo - Fictional - Console"));
    assert_eq!(system.core_key(), "Demo - Fictional - Console");
    // Valid only under its own folder (ADR 0025 §12).
    assert_eq!(system.extensions, ["dmo"]);
    assert_eq!(system.folder_extensions, ["dmo"]);
    assert_eq!(system.dats, [DatSource::NoIntro]);
    assert_eq!(
        (system.screenscraper, system.igdb),
        (Some(9001), Some(9002))
    );
    let custom = loaded.catalog.custom("zzdemo").unwrap();
    assert_eq!(custom.file, path);
    assert_eq!(custom.cores, ["demo_core"]);
    assert_eq!(custom.origin, origin());
}

#[test]
fn a_system_without_database_keeps_its_id_as_core_key() {
    let tmp = tempfile::tempdir().unwrap();
    let mut d = draft();
    d.database = None;
    d.dats = Vec::new();
    d.screenscraper = None;
    d.igdb = None;
    write_custom(tmp.path(), &d, &Catalog::official()).unwrap();
    let loaded = Catalog::load(tmp.path());
    let system = loaded.catalog.system("zzdemo").unwrap();
    assert_eq!(system.libretro_name(), None);
    assert_eq!(system.core_key(), "zzdemo");
    assert!(system.dats.is_empty());
    assert_eq!(
        CustomSystemDraft::from_custom(&loaded.catalog, "zzdemo"),
        Some(d)
    );
}

#[test]
fn an_invalid_draft_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = systems_dir(tmp.path());
    let mut d = draft();
    d.extensions = strings(&["dmo", "zip"]);
    let err = write_custom(&dir, &d, &Catalog::official()).unwrap_err();
    assert!(
        matches!(
            err,
            LibraryError::Definition(DefinitionError::ForbiddenExtension(_))
        ),
        "{err:?}"
    );
    assert!(!dir.exists() || names_in(&dir).is_empty());
}

#[test]
fn a_new_definition_never_overwrites_a_file_and_an_edit_replaces_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    std::fs::create_dir_all(&dir).unwrap();
    // The user's own file, invalid so not in the catalog: kept as it is.
    std::fs::write(dir.join("zzdemo.toml"), "not toml [").unwrap();
    let err = write_custom(&dir, &draft(), &Catalog::load(&dir).catalog).unwrap_err();
    assert!(matches!(err, LibraryError::Io { .. }), "{err:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join("zzdemo.toml")).unwrap(),
        "not toml ["
    );
    assert_eq!(names_in(&dir), ["zzdemo.toml"]);

    std::fs::remove_file(dir.join("zzdemo.toml")).unwrap();
    write_custom(&dir, &draft(), &Catalog::load(&dir).catalog).unwrap();
    let catalog = Catalog::load(&dir).catalog;
    let mut edited = draft();
    edited.name = "Renamed".into();
    edited.cores = strings(&["demo_core", "other"]);
    write_custom(&dir, &edited, &catalog).unwrap();
    assert_eq!(names_in(&dir), ["zzdemo.toml"]);
    let reloaded = Catalog::load(&dir).catalog;
    assert_eq!(
        CustomSystemDraft::from_custom(&reloaded, "zzdemo"),
        Some(edited)
    );
}

#[test]
fn a_new_definition_leaves_no_temporary_and_clears_the_ones_a_dead_process_left() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".zzdemo.tmp-1-0"), "half written").unwrap();
    std::fs::write(dir.join(".other.tmp-1-0"), "not ours").unwrap();
    write_custom(&dir, &draft(), &Catalog::load(&dir).catalog).unwrap();
    assert_eq!(names_in(&dir), [".other.tmp-1-0", "zzdemo.toml"]);
}

#[test]
fn a_new_definition_is_refused_at_the_limit_but_an_edit_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    write_one(&dir, "zzdemo", "Demo - One");
    for i in 1..MAX_DEFINITION_FILES {
        std::fs::write(dir.join(format!("junk{i:03}.toml")), "x").unwrap();
    }
    assert_eq!(names_in(&dir).len(), MAX_DEFINITION_FILES);
    let catalog = Catalog::load(&dir).catalog;
    let mut other = draft();
    other.id = "newone".into();
    other.database = Some("Demo - New".into());
    let err = write_custom(&dir, &other, &catalog).unwrap_err();
    assert!(
        matches!(
            err,
            LibraryError::Definition(DefinitionError::TooManyDefinitions)
        ),
        "{err:?}"
    );
    assert!(!dir.join("newone.toml").exists());
    assert_eq!(names_in(&dir).len(), MAX_DEFINITION_FILES);
    let mut edited = draft();
    edited.name = "Renamed".into();
    write_custom(&dir, &edited, &catalog).unwrap();
    assert!(
        std::fs::read_to_string(dir.join("zzdemo.toml"))
            .unwrap()
            .contains("Renamed")
    );
}

#[test]
fn displayed_text_is_cleaned_when_written() {
    let tmp = tempfile::tempdir().unwrap();
    let mut d = draft();
    d.origin.info_name = Some(format!("Demo{}{}evil / x", char::from(7), '\u{202e}'));
    d.origin.created_by = format!("8.0{}.0", char::from(0x1b));
    write_custom(tmp.path(), &d, &Catalog::official()).unwrap();
    let loaded = Catalog::load(tmp.path());
    let back = CustomSystemDraft::from_custom(&loaded.catalog, "zzdemo").unwrap();
    assert_eq!(back.origin.info_name.as_deref(), Some("Demoevil / x"));
    assert_eq!(back.origin.created_by, "8.0.0");
}

fn write_one(dir: &Path, id: &str, database: &str) {
    let mut d = draft();
    d.id = id.into();
    d.database = Some(database.into());
    let catalog = Catalog::load(dir).catalog;
    write_custom(dir, &d, &catalog).unwrap();
}

#[test]
fn retiring_moves_the_file_and_never_deletes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    write_one(&dir, "zzdemo", "Demo - One");
    let original = std::fs::read(dir.join("zzdemo.toml")).unwrap();

    let retired = retire_custom(&dir, "zzdemo", Retirement::Removed, 1_700_000_000).unwrap();
    assert_eq!(retired, dir.join(".retired").join("zzdemo-1700000000.toml"));
    assert_eq!(std::fs::read(&retired).unwrap(), original);
    assert!(!dir.join("zzdemo.toml").exists());
    // The catalog no longer has it, its folder is reserved.
    let loaded = Catalog::load(&dir);
    assert!(loaded.catalog.system("zzdemo").is_none());
    assert_eq!(loaded.catalog.reserved_folders(), ["zzdemo"]);
}

#[test]
fn retiring_refuses_a_taken_name_a_missing_file_and_a_bad_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    write_one(&dir, "zzdemo", "Demo - One");
    let kept = dir.join(".retired");
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::write(kept.join("zzdemo-1700000000.replaced.toml"), "older").unwrap();
    let err = retire_custom(&dir, "zzdemo", Retirement::Replaced, 1_700_000_000).unwrap_err();
    assert!(matches!(err, LibraryError::Io { .. }), "{err:?}");
    // Nothing moved, nothing overwritten.
    assert!(dir.join("zzdemo.toml").exists());
    assert_eq!(
        std::fs::read_to_string(kept.join("zzdemo-1700000000.replaced.toml")).unwrap(),
        "older"
    );
    assert!(retire_custom(&dir, "nothere", Retirement::Removed, 1).is_err());
    // An id is a bare name: nothing outside `systems/` is ever moved.
    std::fs::write(tmp.path().join("decoy.toml"), "x").unwrap();
    for bad in ["..", "../decoy", "a/b", "A", ""] {
        assert!(
            retire_custom(&dir, bad, Retirement::Removed, 1).is_err(),
            "{bad:?}"
        );
    }
    assert!(tmp.path().join("decoy.toml").exists());
}

#[test]
fn retired_definitions_are_listed_by_exact_names_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    let kept = dir.join(".retired");
    std::fs::create_dir_all(&kept).unwrap();
    for name in [
        "zzdemo-1700000000.toml",
        "zzdemo-1700000002.replaced.toml",
        "alpha-5.toml",
        // Refused: other suffix, upper case, not a number, number not canonical, no id, too long id.
        "zzdemo-1700000003.bak",
        "ZZ-1.toml",
        "zzdemo-notanumber.toml",
        "zzdemo-01.toml",
        "-5.toml",
        "zzdemo.toml",
        "zzdemo-1.replaced.bak",
        "aaaaaaaaaaaaaaaaaaaaaaaaa-1.toml",
    ] {
        std::fs::write(kept.join(name), "x").unwrap();
    }
    // A folder with a good name is not a definition.
    std::fs::create_dir(kept.join("folder-9.toml")).unwrap();
    let listed = retired_customs(&dir);
    let summary: Vec<(String, Retirement, u64)> = listed
        .iter()
        .map(|r| (r.id.clone(), r.why, r.retired_unix))
        .collect();
    assert_eq!(
        summary,
        [
            ("alpha".to_owned(), Retirement::Removed, 5),
            ("zzdemo".to_owned(), Retirement::Removed, 1_700_000_000),
            ("zzdemo".to_owned(), Retirement::Replaced, 1_700_000_002),
        ]
    );
    assert_eq!(listed[0].file, kept.join("alpha-5.toml"));
    // No `.retired` folder: nothing.
    assert!(retired_customs(&tmp.path().join("nowhere")).is_empty());
}

#[test]
fn restoring_puts_the_file_back_and_refuses_what_would_clash() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    write_one(&dir, "zzdemo", "Demo - One");
    let original = std::fs::read(dir.join("zzdemo.toml")).unwrap();
    let removed = retire_custom(&dir, "zzdemo", Retirement::Removed, 1_700_000_000).unwrap();

    let restored = restore_custom(&dir, &removed, &Catalog::load(&dir).catalog).unwrap();
    assert_eq!(restored, dir.join("zzdemo.toml"));
    assert_eq!(std::fs::read(&restored).unwrap(), original);
    assert!(!removed.exists());
    assert_eq!(
        Catalog::load(&dir)
            .catalog
            .custom("zzdemo")
            .map(|c| c.id.as_str()),
        Some("zzdemo")
    );

    // A replaced one comes back too (the undo of a switch).
    let replaced = retire_custom(&dir, "zzdemo", Retirement::Replaced, 1_700_000_001).unwrap();
    restore_custom(&dir, &replaced, &Catalog::load(&dir).catalog).unwrap();
    assert!(dir.join("zzdemo.toml").exists());

    // The place is taken: refused, the retired file stays.
    let again = retire_custom(&dir, "zzdemo", Retirement::Removed, 1_700_000_005).unwrap();
    write_one(&dir, "zzdemo", "Demo - Two");
    let err = restore_custom(&dir, &again, &Catalog::load(&dir).catalog).unwrap_err();
    assert!(matches!(err, LibraryError::Io { .. }), "{err:?}");
    assert!(again.exists());
    assert!(
        std::fs::read_to_string(dir.join("zzdemo.toml"))
            .unwrap()
            .contains("Demo - Two")
    );
}

#[test]
fn restoring_refuses_an_active_id_and_files_outside_the_retired_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    write_one(&dir, "zzdemo", "Demo - One");
    let retired = retire_custom(&dir, "zzdemo", Retirement::Removed, 1_700_000_000).unwrap();
    // A synthetic official system now holds the id: the catalog says it is active.
    let text = r#"[[system]]
id = "zzdemo"
name = "Z"
extensions = ["zzz"]
database = "Z - Z"
"#;
    let taken = Catalog::load_with_official(text, &dir).unwrap().catalog;
    assert!(restore_custom(&dir, &retired, &taken).is_err());
    assert!(retired.exists());
    // Outside `.retired/`, or a name `.retired/` does not accept.
    std::fs::write(dir.join("other-1.toml"), "x").unwrap();
    assert!(restore_custom(&dir, &dir.join("other-1.toml"), &Catalog::official()).is_err());
    let odd = dir.join(".retired").join("zzdemo-1.bak");
    std::fs::write(&odd, "x").unwrap();
    assert!(restore_custom(&dir, &odd, &Catalog::official()).is_err());
    let up = dir.join(".retired").join("..").join("other-1.toml");
    assert!(restore_custom(&dir, &up, &Catalog::official()).is_err());
    assert!(!dir.join("other.toml").exists());
}

#[test]
fn only_a_custom_system_has_a_draft() {
    let tmp = tempfile::tempdir().unwrap();
    let loaded = Catalog::load(tmp.path());
    assert_eq!(
        CustomSystemDraft::from_custom(&loaded.catalog, "snes"),
        None
    );
    assert_eq!(
        CustomSystemDraft::from_custom(&loaded.catalog, "nothere"),
        None
    );
}

#[test]
fn restoring_is_refused_at_the_limit_like_a_creation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systems");
    write_one(&dir, "aaa", "Demo - One");
    let removed = retire_custom(&dir, "aaa", Retirement::Removed, 1_700_000_000).unwrap();
    for i in 0..MAX_DEFINITION_FILES {
        std::fs::write(dir.join(format!("junk{i:03}.toml")), "x").unwrap();
    }
    let catalog = Catalog::load(&dir).catalog;
    let err = restore_custom(&dir, &removed, &catalog).unwrap_err();
    assert!(
        matches!(
            err,
            LibraryError::Definition(DefinitionError::TooManyDefinitions)
        ),
        "{err:?}"
    );
    assert!(removed.exists());
    assert!(!dir.join("aaa.toml").exists());
    // One place freed: the restore goes through.
    std::fs::remove_file(dir.join("junk000.toml")).unwrap();
    restore_custom(&dir, &removed, &Catalog::load(&dir).catalog).unwrap();
    assert!(dir.join("aaa.toml").exists());
}
