//! Single firmware check (design v3.0 §3.5.5) for puae / puae2021 with the embedded `cores.toml` and
//! `firmware.toml`, on fake files in a temporary system directory (never a real Kickstart).

use std::fs;

use emu_assets::cores::{
    CoreId, CorePreferences, FirmwareState, KnownFirmware, check_core_firmware,
};

const SYSTEM: &str = "Commodore - Amiga";

fn id(s: &str) -> CoreId {
    CoreId::new(s).unwrap()
}

#[test]
fn puae2021_has_the_same_tables_as_puae() {
    let prefs = CorePreferences::builtin();
    let a = prefs.required_firmware(&id("puae"), SYSTEM).unwrap();
    let b = prefs.required_firmware(&id("puae2021"), SYSTEM).unwrap();
    assert_eq!(a.files, b.files);
    assert_eq!((&a.default, &a.fallback), (&b.default, &b.fallback));
    assert_eq!(
        prefs.builtin_firmware(&id("puae")).map(|f| f.label),
        prefs.builtin_firmware(&id("puae2021")).map(|f| f.label)
    );
}

#[test]
fn every_wanted_kickstart_is_known_to_firmware_toml() {
    // A wanted file with no reference md5 could never be reported `Ok`, only `Unverified`.
    let prefs = CorePreferences::builtin();
    let known = KnownFirmware::builtin();
    let req = prefs.required_firmware(&id("puae"), SYSTEM).unwrap();
    for (model, file) in &req.files {
        assert!(!known.md5_for(file).is_empty(), "{model} -> {file}");
        assert!(known.size_for(file).is_some(), "{file}");
    }
    assert!(req.files.contains_key(&req.default));
}

#[test]
fn missing_kickstart_falls_back_to_aros_and_wrong_files_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path();
    let prefs = CorePreferences::builtin();
    let known = KnownFirmware::builtin();
    let check = |variant: Option<&str>| {
        check_core_firmware(system, &prefs, &id("puae"), SYSTEM, variant, &known)
            .expect("puae has a table")
            .unwrap()
    };

    // Empty system directory: AROS, neither missing nor blocking.
    let c = check(Some("A500"));
    assert_eq!(c.wanted.as_deref(), Some("kick34005.A500"));
    assert!(
        matches!(c.state, FirmwareState::CoreFallback { .. }),
        "{:?}",
        c.state
    );
    assert!(c.others_present.is_empty());

    // An unknown model uses the default (A500); a Kickstart for another model is only listed.
    fs::write(system.join("kick40068.A1200"), b"fake 3.1 for the A1200").unwrap();
    let c = check(Some("NOPE"));
    assert_eq!(c.wanted.as_deref(), Some("kick34005.A500"));
    assert_eq!(c.others_present, ["kick40068.A1200"]);
    let c = check(None);
    assert_eq!(c.wanted.as_deref(), Some("kick34005.A500"));

    // A file with the wanted name and a wrong content is `WrongHash` (the UI asks), never silently accepted.
    fs::write(system.join("kick34005.A500"), b"not a kickstart").unwrap();
    assert!(matches!(
        check(Some("A500")).state,
        FirmwareState::WrongHash { .. }
    ));
    // The A1200 file is also wrong for that model.
    assert!(matches!(
        check(Some("A1200")).state,
        FirmwareState::WrongHash { .. }
    ));
}

#[test]
fn a_core_with_builtin_roms_and_no_table_is_builtin_and_unknown_cores_have_no_check() {
    let tmp = tempfile::tempdir().unwrap();
    let prefs = CorePreferences::builtin();
    let known = KnownFirmware::builtin();
    // puae on a system it has no table for: its built-in ROM, nothing expected in system/.
    let c = check_core_firmware(
        tmp.path(),
        &prefs,
        &id("puae"),
        "Other - System",
        None,
        &known,
    )
    .unwrap()
    .unwrap();
    assert_eq!(c.wanted, None);
    assert!(matches!(c.state, FirmwareState::Builtin { .. }));
    // A core with neither: the caller keeps the 1.0 `.info` check.
    assert!(
        check_core_firmware(
            tmp.path(),
            &prefs,
            &id("gambatte"),
            "Nintendo - Game Boy",
            None,
            &known
        )
        .is_none()
    );
    assert!(
        fs::read_dir(tmp.path()).unwrap().next().is_none(),
        "nothing written"
    );
}
