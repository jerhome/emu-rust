//! `[library] cd32x` of `config.toml` (ADR 0024 §2, design v7.5 §4, merged lines 7.4 + PC 7.2): a 7.4 file is read
//! unchanged, a hand-written list is read back as written, a file written by 7.5 stays readable by 7.4 (the key
//! falls into the flattened `extra` table), is written atomically, kept `0600`, and a failed write leaves the
//! previous file intact.

use std::fs;

use emu_ui::Config;

/// What the 7.4 `LibraryConfig` looked like: no `cd32x` field, unknown keys kept by `#[serde(flatten)]`.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct Library74 {
    roots: Vec<std::path::PathBuf>,
    scan_on_start: bool,
    covers_missing_reset: u32,
    #[serde(flatten)]
    extra: toml::Table,
}

impl Default for Library74 {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            scan_on_start: true,
            covers_missing_reset: 0,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct Config74 {
    library: Library74,
    #[serde(flatten)]
    rest: toml::Table,
}

const FILE_74: &str = "[app]\nshow_terminal = true\n\n[ui]\nwizard_done = true\n\n[library]\nroots = [\"/roms\"]\nscan_on_start = false\ncovers_missing_reset = 2\n\n\
[covers]\ndownload = false\n";

#[test]
fn a_7_4_file_is_read_and_rewritten_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, FILE_74).unwrap();
    let cfg = Config::load(&path).unwrap();
    assert!(cfg.library.cd32x.is_empty());
    assert!(!cfg.library.scan_on_start);
    cfg.save(&path).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("cd32x"), "{text}");
    let after = Config::load(&path).unwrap();
    assert_eq!(after.library.roots, cfg.library.roots);
    assert_eq!(after.library.covers_missing_reset, 2);
}

#[test]
fn a_hand_written_list_is_read_back_as_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[library]\ncd32x = [\"NIGHT TRAP (DISC 1).CHD\", \"Épée Trap.chd\"]\n",
    )
    .unwrap();
    let cfg = Config::load(&path).unwrap();
    assert_eq!(
        cfg.library.cd32x,
        ["NIGHT TRAP (DISC 1).CHD", "Épée Trap.chd"],
        "entries are kept as typed; the comparison is done in lower case at use"
    );
    cfg.save(&path).unwrap();
    assert_eq!(
        Config::load(&path).unwrap().library.cd32x,
        cfg.library.cd32x
    );
}

#[test]
fn a_7_5_file_stays_readable_and_intact_for_7_4() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, FILE_74).unwrap();
    let mut cfg = Config::load(&path).unwrap();
    cfg.library.cd32x = vec!["night trap (disc 1).chd".into()];
    cfg.save(&path).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    let old: Config74 = toml::from_str(&text).expect("7.4 reads a 7.5 file");
    assert!(!old.library.scan_on_start);
    assert_eq!(old.library.covers_missing_reset, 2);
    assert!(
        old.rest.contains_key("app") && old.rest.contains_key("ui"),
        "7.4 tables kept by the 7.5 file: {text}"
    );
    assert!(old.library.extra.contains_key("cd32x"), "kept, not dropped");
    // 7.4 writing it back keeps the list for the next 7.5 session.
    let rewritten = toml::to_string(&old).unwrap();
    let again: toml::Table = toml::from_str(&rewritten).unwrap();
    assert_eq!(
        again["library"]["cd32x"].as_array().map(Vec::len),
        Some(1),
        "{rewritten}"
    );
}

#[cfg(unix)]
#[test]
fn the_file_written_with_a_list_is_0600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut cfg = Config::default();
    cfg.library.cd32x = vec!["a.chd".into()];
    cfg.save(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        !dir.path().read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("tmp")),
        "no temporary file left"
    );
}
/// A write that fails (read-only folder) reports the error and leaves the previous file byte for byte.
#[cfg(unix)]
#[test]
fn a_failed_write_keeps_the_previous_file_and_reports_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, FILE_74).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let mut cfg = Config::load(&path).unwrap();
    cfg.library.cd32x = vec!["a.chd".into()];
    let result = cfg.save(&path);
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    if result.is_ok() {
        // Running as root: permissions do not apply, nothing to check.
        return;
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), FILE_74);
    assert!(
        !dir.path().read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("tmp")),
        "no temporary file left"
    );
}

/// Rolling back to the 7.4: the 7.5 file (list + `show_terminal`) is read, then written back by the 7.4 model
/// without losing the list; the 7.5 reads the result and finds the marks again.
#[test]
fn rolling_back_to_7_4_and_forward_again_keeps_the_marks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut cfg = Config::default();
    cfg.app.show_terminal = true;
    cfg.library.cd32x = vec![
        "night trap (disc 1).chd".into(),
        "night trap (disc 2).chd".into(),
    ];
    cfg.save(&path).unwrap();
    let old: Config74 = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    fs::write(&path, toml::to_string(&old).unwrap()).unwrap();
    let back = Config::load(&path).unwrap();
    assert_eq!(back.library.cd32x, cfg.library.cd32x);
    assert!(back.app.show_terminal);
}
