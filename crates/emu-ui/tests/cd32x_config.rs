//! `[library] cd32x` of `config.toml` (7.2, ADR 0024 §2, design v7.2 §4 point 5 and the 7.1 compatibility
//! criterion): a 7.1 file is read unchanged, a hand-written list is read back as written, a file written by 7.2
//! stays readable by 7.1 (the key falls into the flattened `extra` table) and is kept `0600`.

use std::fs;

use emu_ui::Config;

/// What the 7.1 `LibraryConfig` looked like: no `cd32x` field, unknown keys kept by `#[serde(flatten)]`.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct Library71 {
    roots: Vec<std::path::PathBuf>,
    scan_on_start: bool,
    covers_missing_reset: u32,
    #[serde(flatten)]
    extra: toml::Table,
}

impl Default for Library71 {
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
struct Config71 {
    library: Library71,
    #[serde(flatten)]
    rest: toml::Table,
}

const FILE_71: &str = "[library]\nroots = [\"/roms\"]\nscan_on_start = false\ncovers_missing_reset = 2\n\n\
[covers]\ndownload = false\n";

#[test]
fn a_7_1_file_is_read_and_rewritten_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, FILE_71).unwrap();
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
fn a_7_2_file_stays_readable_and_intact_for_7_1() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, FILE_71).unwrap();
    let mut cfg = Config::load(&path).unwrap();
    cfg.library.cd32x = vec!["night trap (disc 1).chd".into()];
    cfg.save(&path).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    let old: Config71 = toml::from_str(&text).expect("7.1 reads a 7.2 file");
    assert!(!old.library.scan_on_start);
    assert_eq!(old.library.covers_missing_reset, 2);
    assert!(old.library.extra.contains_key("cd32x"), "kept, not dropped");
    // 7.1 writing it back keeps the list for the next 7.2 session.
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
