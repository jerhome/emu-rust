//! Import of a BIOS provided by the user (ADR 0017 §3, design v3.0 §2.2) against fake files: **never** a real ROM.
//! The test catalogue maps the md5 of made-up bytes to Kickstart-like names; nothing touches the network.

use std::fs;
use std::path::{Path, PathBuf};

use emu_assets::bios::{
    BiosError, FirmwareCatalogue, Identification, MANIFEST_FILE_NAME, MAX_FIRMWARE_BYTES,
    ReplacePolicy, TargetState, identify_firmware, import_firmware, target_state,
};
use emu_assets::cores::{CoreId, KnownFirmware, parse_info};
use md5::Md5;
use sha2::{Digest, Sha256};

const KICK: &[u8] = b"fake kickstart 1.3 image";
const KICK_USER: &[u8] = b"a kickstart the user already had";

fn md5_of(data: &[u8]) -> [u8; 16] {
    Md5::digest(data).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `KICK` is expected as `kick34005.A500` and `kick33180.A500` (two names for one file, as in the real data);
/// `tos.img` is a free BIOS name (EmuTOS) with its own made-up hash.
fn catalogue() -> FirmwareCatalogue {
    let text = format!(
        "database = \"Commodore - Amiga\"\n\
         firmware0_path = \"kick34005.A500\"\nfirmware0_desc = \"Kickstart 1.3 (A500)\"\n\
         firmware1_path = \"kick33180.A500\"\nfirmware2_path = \"kick40063.A600\"\n\
         firmware3_path = \"tos.img\"\n\
         notes = \"(!) kick34005.A500 (md5): {0}|(!) kick33180.A500 (md5): {0}|(!) tos.img (md5): {1}\"\n",
        hex(&md5_of(KICK)),
        hex(&md5_of(b"emutos"))
    );
    let info = parse_info(CoreId::new("puae").unwrap(), &text).unwrap();
    FirmwareCatalogue::build(&KnownFirmware::default(), &[(info.id.clone(), info)], &[])
}

fn write(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, data).unwrap();
    path
}

/// Names in a folder, without the manifest.
fn names(dir: &Path) -> Vec<String> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<String> = read
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != MANIFEST_FILE_NAME)
        .collect();
    v.sort();
    v
}

fn manifest(system: &Path) -> String {
    fs::read_to_string(system.join(MANIFEST_FILE_NAME)).unwrap_or_default()
}

fn known(id: Identification) -> (Vec<String>, [u8; 16]) {
    match id {
        Identification::Known { md5, targets, .. } => {
            (targets.into_iter().map(|t| t.name).collect(), md5)
        }
        other => panic!("expected Known, got {other:?}"),
    }
}

#[test]
fn known_file_is_installed_under_every_target_name() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let src = write(tmp.path(), "downloads/Kickstart v1.3.rom", KICK);
    let (targets, md5) = known(identify_firmware(&src, &catalogue()).unwrap());
    assert_eq!(targets, ["kick33180.A500", "kick34005.A500"]);

    for name in &targets {
        assert_eq!(
            target_state(&system, name, &md5).unwrap(),
            TargetState::Absent
        );
        let out = import_firmware(&system, &src, name, ReplacePolicy::Refuse, true).unwrap();
        assert_eq!(out.installed, system.join(name));
        assert_eq!(out.backup, None);
        assert_eq!(out.md5, md5);
        assert_eq!(fs::read(&out.installed).unwrap(), KICK);
        // Second import of the same file: nothing to do.
        assert_eq!(
            target_state(&system, name, &md5).unwrap(),
            TargetState::Same
        );
        let again = import_firmware(&system, &src, name, ReplacePolicy::Refuse, true).unwrap();
        assert_eq!(again.backup, None);
    }
    assert_eq!(names(&system), ["kick33180.A500", "kick34005.A500"]);
    let record = manifest(&system);
    assert!(record.contains("kick34005.A500") && record.contains("verified = true"));
    // Privacy: the record keeps the file name, never its folder.
    assert!(record.contains("Kickstart v1.3.rom") && !record.contains("downloads"));
}

#[test]
fn a_user_file_is_refused_then_backed_up_never_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let existing = write(&system, "kick34005.A500", KICK_USER);
    let src = write(tmp.path(), "k.rom", KICK);
    let md5 = md5_of(KICK);

    assert_eq!(
        target_state(&system, "kick34005.A500", &md5).unwrap(),
        TargetState::Other {
            md5: md5_of(KICK_USER)
        }
    );
    let err =
        import_firmware(&system, &src, "kick34005.A500", ReplacePolicy::Refuse, true).unwrap_err();
    assert!(matches!(err, BiosError::TargetExists(_)), "{err}");
    assert_eq!(fs::read(&existing).unwrap(), KICK_USER);
    assert_eq!(names(&system), ["kick34005.A500"], "nothing written");

    let out = import_firmware(
        &system,
        &src,
        "kick34005.A500",
        ReplacePolicy::BackupAndReplace,
        true,
    )
    .unwrap();
    let backup = out.backup.expect("backup path");
    let backup_name = backup.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        backup_name.starts_with("kick34005.A500.user-"),
        "{backup_name}"
    );
    assert_eq!(fs::read(&backup).unwrap(), KICK_USER);
    assert_eq!(fs::read(&existing).unwrap(), KICK);
    assert_eq!(names(&system).len(), 2);
}

#[test]
fn two_backups_in_the_same_second_do_not_overwrite_each_other() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let src = write(tmp.path(), "k.rom", KICK);
    write(&system, "kick34005.A500", b"first");
    import_firmware(
        &system,
        &src,
        "kick34005.A500",
        ReplacePolicy::BackupAndReplace,
        true,
    )
    .unwrap();
    // The newly installed file is again "other" once the user swaps it for another one.
    fs::write(system.join("kick34005.A500"), b"second").unwrap();
    import_firmware(
        &system,
        &src,
        "kick34005.A500",
        ReplacePolicy::BackupAndReplace,
        true,
    )
    .unwrap();
    let mut saved: Vec<Vec<u8>> = names(&system)
        .iter()
        .filter(|n| n.contains(".user-"))
        .map(|n| fs::read(system.join(n)).unwrap())
        .collect();
    saved.sort();
    assert_eq!(saved, [b"first".to_vec(), b"second".to_vec()]);
}

#[test]
fn a_free_bios_installed_by_emu_rust_is_replaced_without_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let emutos = b"emutos";
    write(&system, "tos.img", emutos);
    // Manifest as written by `FreeBiosManager` (format documented on `ManifestEntry`).
    fs::write(
        system.join(MANIFEST_FILE_NAME),
        format!(
            "[[installed]]\nfile = \"tos.img\"\nbios = \"emutos\"\nvariant = \"192fr\"\nversion = \"1.4\"\n\
             label = \"EmuTOS\"\nsha256 = \"{}\"\nmd5 = \"{}\"\ninstalled_unix = 1\n",
            hex(&Sha256::digest(emutos)),
            hex(&md5_of(emutos))
        ),
    )
    .unwrap();
    let src = write(tmp.path(), "other-tos.img", b"a TOS the user prefers");
    let md5 = md5_of(b"a TOS the user prefers");
    assert_eq!(
        target_state(&system, "tos.img", &md5).unwrap(),
        TargetState::FreeInstalled
    );
    let out = import_firmware(&system, &src, "tos.img", ReplacePolicy::Refuse, false).unwrap();
    assert_eq!(out.backup, None);
    assert_eq!(
        fs::read(system.join("tos.img")).unwrap(),
        b"a TOS the user prefers"
    );
    assert_eq!(names(&system), ["tos.img"]);
    let record = manifest(&system);
    assert!(
        !record.contains("file = \"tos.img\"") && record.contains("installed = []"),
        "{record}"
    );
    // The file is now the user's: the next import needs a backup.
    assert!(matches!(
        target_state(&system, "tos.img", &md5_of(b"x")).unwrap(),
        TargetState::Other { .. }
    ));
}

#[test]
fn unknown_file_is_only_installed_when_forced_and_marked_unverified() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let src = write(tmp.path(), "mystery.bin", b"who knows");
    let cat = catalogue();
    match identify_firmware(&src, &cat).unwrap() {
        Identification::Unknown { md5, size, .. } => {
            assert_eq!(md5, md5_of(b"who knows"));
            assert_eq!(size, 9);
        }
        other => panic!("{other:?}"),
    }
    // The UI offers these names for a forced import.
    let offered: Vec<String> = cat.expected_names().into_iter().map(|e| e.name).collect();
    assert!(offered.contains(&"kick40063.A600".to_owned()));
    assert!(!system.exists(), "identification writes nothing");

    import_firmware(
        &system,
        &src,
        "kick40063.A600",
        ReplacePolicy::Refuse,
        false,
    )
    .unwrap();
    assert_eq!(
        fs::read(system.join("kick40063.A600")).unwrap(),
        b"who knows"
    );
    let record = manifest(&system);
    assert!(record.contains("verified = false"), "{record}");
}

#[test]
fn amiga_forever_header_is_detected_before_the_hash_lookup() {
    let tmp = tempfile::tempdir().unwrap();
    let mut data = b"AMIROMTYPE1".to_vec();
    data.extend_from_slice(&[7; 64]);
    let src = write(tmp.path(), "amiga-os-130.rom", &data);
    match identify_firmware(&src, &catalogue()).unwrap() {
        Identification::AmigaForeverEncrypted { size, suggested } => {
            assert_eq!(size, data.len() as u64);
            assert_eq!(suggested.as_deref(), Some("amiga-os-130.rom"));
        }
        other => panic!("{other:?}"),
    }
    let other = write(tmp.path(), "kickstart.rom", &data);
    match identify_firmware(&other, &catalogue()).unwrap() {
        Identification::AmigaForeverEncrypted { suggested, .. } => assert_eq!(suggested, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn hostile_target_names_are_refused_and_nothing_escapes() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let src = write(tmp.path(), "k.rom", KICK);
    let hostile = [
        "../x",
        "a/../../x",
        "/abs",
        "C:\\x",
        "C:x",
        "a\\b",
        "",
        ".hidden",
        MANIFEST_FILE_NAME,
        "sub/.tmp",
        "a//b",
        "a/b/c/d/e/f",
    ];
    for name in hostile {
        let r = import_firmware(&system, &src, name, ReplacePolicy::BackupAndReplace, false);
        assert!(
            matches!(r, Err(BiosError::InvalidTargetName(_))),
            "{name:?}: {r:?}"
        );
        assert!(
            matches!(
                target_state(&system, name, &md5_of(KICK)),
                Err(BiosError::InvalidTargetName(_))
            ),
            "{name:?}"
        );
    }
    assert!(!tmp.path().join("x").exists());
    assert!(names(&system).is_empty());
    // A sub-folder declared by a `.info` is fine.
    import_firmware(&system, &src, "vice/kick.rom", ReplacePolicy::Refuse, false).unwrap();
    assert!(system.join("vice/kick.rom").is_file());
}

#[test]
fn a_file_over_the_limit_is_refused_without_reading_it() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let src = tmp.path().join("huge.bin");
    let f = fs::File::create(&src).unwrap();
    f.set_len(MAX_FIRMWARE_BYTES + 1).unwrap();
    drop(f);
    assert!(matches!(
        identify_firmware(&src, &catalogue()),
        Err(BiosError::TooLarge(n)) if n == MAX_FIRMWARE_BYTES + 1
    ));
    assert!(matches!(
        import_firmware(&system, &src, "kick.rom", ReplacePolicy::Refuse, false),
        Err(BiosError::TooLarge(_))
    ));
    assert!(!system.join("kick.rom").exists());
}

#[cfg(unix)]
#[test]
fn a_symlink_as_source_is_followed_but_a_folder_is_not_a_source() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let real = write(tmp.path(), "collection/kick.rom", KICK);
    let link = tmp.path().join("link.rom");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let (targets, _) = known(identify_firmware(&link, &catalogue()).unwrap());
    assert!(!targets.is_empty());
    import_firmware(
        &system,
        &link,
        "kick34005.A500",
        ReplacePolicy::Refuse,
        true,
    )
    .unwrap();
    // The installed file is a regular copy, not a link to the user's collection.
    let meta = fs::symlink_metadata(system.join("kick34005.A500")).unwrap();
    assert!(meta.is_file() && !meta.file_type().is_symlink());
    assert_eq!(fs::read(&real).unwrap(), KICK);

    let dir_link = tmp.path().join("dirlink");
    std::os::unix::fs::symlink(tmp.path().join("collection"), &dir_link).unwrap();
    assert!(identify_firmware(&dir_link, &catalogue()).is_err());
    assert!(import_firmware(&system, &dir_link, "x.rom", ReplacePolicy::Refuse, false).is_err());
    assert!(!system.join("x.rom").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_subfolder_in_the_system_directory_never_leads_outside() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&system).unwrap();
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, system.join("vice")).unwrap();
    let src = write(tmp.path(), "k.rom", KICK);
    let r = import_firmware(&system, &src, "vice/kick.rom", ReplacePolicy::Refuse, false);
    assert!(r.is_err(), "{r:?}");
    assert!(names(&outside).is_empty());
}

#[cfg(unix)]
#[test]
fn a_symlinked_target_is_never_written_through() {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let kept = write(tmp.path(), "collection/mine.rom", KICK_USER);
    fs::create_dir_all(&system).unwrap();
    std::os::unix::fs::symlink(&kept, system.join("kick34005.A500")).unwrap();
    let src = write(tmp.path(), "k.rom", KICK);
    let err =
        import_firmware(&system, &src, "kick34005.A500", ReplacePolicy::Refuse, true).unwrap_err();
    assert!(matches!(err, BiosError::TargetExists(_)), "{err}");
    assert_eq!(fs::read(&kept).unwrap(), KICK_USER);
    // With a backup the link itself moves aside; the user's file behind it is untouched.
    import_firmware(
        &system,
        &src,
        "kick34005.A500",
        ReplacePolicy::BackupAndReplace,
        true,
    )
    .unwrap();
    assert_eq!(fs::read(&kept).unwrap(), KICK_USER);
    assert_eq!(fs::read(system.join("kick34005.A500")).unwrap(), KICK);
}

#[test]
fn the_real_firmware_toml_gives_the_kickstart_names_and_sizes() {
    let known = KnownFirmware::builtin();
    let cat = FirmwareCatalogue::build(&known, &[], &[]);
    let expected = cat.expected_names();
    let size_of = |name: &str| {
        expected
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .size
    };
    assert_eq!(size_of("kick34005.A500"), Some(262_144));
    assert_eq!(size_of("kick33180.A500"), Some(262_144));
    assert_eq!(size_of("kick40063.A600"), Some(524_288));
    assert_eq!(size_of("kick40068.A1200"), Some(524_288));
    for name in ["kick34005.A500", "kick40063.A600", "kick39106.A1200"] {
        let md5s = known.md5_for(name);
        assert!(!md5s.is_empty(), "{name}");
        let targets = cat.targets_for(&md5s[0]);
        assert!(targets.iter().any(|t| t.name == name), "{name}");
    }
    assert_eq!(
        known.md5_for("kick34005.A500"),
        [parse_hex("82a21c1890cae844b3df741f2762d48d")]
    );
}

fn parse_hex(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}
