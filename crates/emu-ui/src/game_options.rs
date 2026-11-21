//! Per-game options (ADR 0015, design §5): `<data>/game-options/<core_id>/<game>.toml`. Priority, lowest first:
//! core defaults < frontend defaults (`cores.toml`) < computed defaults < global (`config.toml`) < game.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use emu_runtime::{FileMode, write_atomic};
use serde::{Deserialize, Serialize};

use crate::config_computer::KeyboardModeChoice;

/// Larger files are not read.
const MAX_FILE_SIZE: u64 = 256 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameInput {
    /// Absent: `[computer] default_keyboard_mode`.
    pub keyboard_mode: Option<KeyboardModeChoice>,
    /// Absent: `[computer] auto_capture`.
    pub auto_capture: Option<bool>,
    /// The user accepted launching this game with the built-in AROS instead of a Kickstart (warned once per game).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub aros_acknowledged: bool,
    #[serde(flatten)]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameMedia {
    /// Index of the floppy to re-insert at launch.
    pub last_disk: Option<u32>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

/// `[dos]` (design v4.0 §4): program of a game folder launched by default (`instantané.zip#START.EXE`). Absent:
/// the core's boot menu, or the program it remembers (`AUTOBOOT.DBP`, which wins over this one).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameDos {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl GameDos {
    fn is_empty(&self) -> bool {
        self.start.is_none() && self.extra.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameOptions {
    pub core_options: BTreeMap<String, String>,
    pub input: GameInput,
    pub media: GameMedia,
    /// Written only when set: options files of the other systems keep their 3.0 shape.
    #[serde(skip_serializing_if = "GameDos::is_empty")]
    pub dos: GameDos,
    #[serde(flatten)]
    pub extra: toml::Table,
}

/// Why options were not read. The file is left untouched in every case.
#[derive(Debug, thiserror::Error)]
pub enum GameOptionsError {
    #[error("options du jeu illisibles : {0}")]
    Io(#[from] io::Error),
    #[error("options du jeu invalides (fichier conservé)")]
    Invalid,
    #[error("options du jeu trop volumineuses")]
    TooLarge,
}

/// A game's options plus whether saving is allowed: a file that exists but cannot be parsed is never overwritten.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadedGameOptions {
    pub options: GameOptions,
    pub locked: bool,
}

/// `dir` = `DataDirs::game_options`.
pub(crate) fn game_options_dir(dir: &Path, core_id: &str) -> PathBuf {
    dir.join(sanitize_component(core_id))
}

/// `<data>/game-options/<core_id>/<game>.toml`; `game` is a title or file stem, made safe as a file name.
pub(crate) fn game_options_path(dir: &Path, core_id: &str, game: &str) -> PathBuf {
    game_options_dir(dir, core_id).join(format!("{}.toml", sanitize_component(game)))
}

/// Copies `<from>.toml` to `<to>.toml` in the core's folder of `dir` (`DataDirs::game_options`) when the target
/// is absent (homonym inheriting the shared 2.0 options, `GameIdentity::inherits_legacy`). `Ok(true)` if a copy
/// was written; `Ok(false)` if the source is missing or the target exists. Atomic; never overwrites.
pub(crate) fn inherit_game_options(
    dir: &Path,
    core_id: &str,
    from: &str,
    to: &str,
) -> io::Result<bool> {
    let source = game_options_path(dir, core_id, from);
    let target = game_options_path(dir, core_id, to);
    if source == target {
        return Ok(false);
    }
    match std::fs::symlink_metadata(&target) {
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    // A link or an oversized file is not an options file we wrote: not copied.
    match std::fs::symlink_metadata(&source) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_FILE_SIZE => {}
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    }
    let mut data = Vec::new();
    io::Read::read_to_end(
        &mut io::Read::take(std::fs::File::open(&source)?, MAX_FILE_SIZE + 1),
        &mut data,
    )?;
    if data.len() as u64 > MAX_FILE_SIZE {
        return Ok(false);
    }
    write_atomic(&target, &data, FileMode::Default)?;
    Ok(true)
}

/// One path component: separators, control and reserved characters become `_`; never empty, never `.` / `..`.
fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return "_".to_owned();
    }
    let mut name: String = trimmed.chars().take(120).collect();
    if is_reserved_windows(&name) {
        name.insert(0, '_');
    }
    name
}

/// Same rule as the runtime's `fsname` (not public): DOS device names are refused by Windows with or without
/// extension, case-insensitively.
fn is_reserved_windows(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("").trim_end_matches(' ');
    let upper = base.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let mut chars = upper.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    matches!(prefix.as_str(), "COM" | "LPT")
        && matches!(chars.next(), Some('1'..='9' | '¹' | '²' | '³'))
        && chars.next().is_none()
}

impl GameOptions {
    /// Missing file: defaults, saving allowed. Unreadable or invalid file: defaults, `locked` (kept as is).
    pub fn load(path: &Path) -> LoadedGameOptions {
        match Self::try_load(path) {
            Ok(Some(options)) => LoadedGameOptions {
                options,
                locked: false,
            },
            Ok(None) => LoadedGameOptions::default(),
            Err(e) => {
                tracing::warn!("{} ignoré : {e}", path.display());
                LoadedGameOptions {
                    options: Self::default(),
                    locked: true,
                }
            }
        }
    }

    fn try_load(path: &Path) -> Result<Option<Self>, GameOptionsError> {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut text = String::new();
        io::Read::read_to_string(&mut io::Read::take(file, MAX_FILE_SIZE + 1), &mut text)
            .map_err(|_| GameOptionsError::Invalid)?;
        if text.len() as u64 > MAX_FILE_SIZE {
            return Err(GameOptionsError::TooLarge);
        }
        toml::from_str(&text)
            .map(Some)
            .map_err(|_| GameOptionsError::Invalid)
    }

    /// Atomic write (parent folders created). Refuses when the loaded file was invalid.
    pub fn save(&self, path: &Path, locked: bool) -> io::Result<()> {
        if locked {
            return Err(io::Error::other(
                "options du jeu invalides : fichier non réécrit",
            ));
        }
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        write_atomic(path, text.as_bytes(), FileMode::Default)
    }
}

/// Core option values to apply: later layers override earlier ones. Sorted by key. No computed layer.
#[cfg(test)]
pub(crate) fn effective_core_options(
    core_defaults: &[(String, String)],
    frontend: &[(String, String)],
    global: &BTreeMap<String, String>,
    game: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    effective_core_options_with(core_defaults, frontend, &[], global, game)
}

/// [`effective_core_options`] with the computed defaults layer (design v3.0 §3.5.4) between `cores.toml` and the
/// config: `computed` is never persisted, so any explicit `global` / `game` value overrides it.
pub(crate) fn effective_core_options_with(
    core_defaults: &[(String, String)],
    frontend: &[(String, String)],
    computed: &[(String, String)],
    global: &BTreeMap<String, String>,
    game: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut map: BTreeMap<&str, &str> = BTreeMap::new();
    for (k, v) in core_defaults.iter().chain(frontend).chain(computed) {
        map.insert(k, v);
    }
    for (k, v) in global.iter().chain(game) {
        map.insert(k, v);
    }
    map.into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_windows_names_are_prefixed() {
        for (raw, want) in [
            ("CON", "_CON"),
            ("nul", "_nul"),
            ("COM1", "_COM1"),
            ("lpt9.x", "_lpt9.x"),
            ("COM10", "COM10"),
            ("Console", "Console"),
        ] {
            assert_eq!(sanitize_component(raw), want);
        }
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect()
    }

    fn map(v: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs(v).into_iter().collect()
    }

    #[test]
    fn priority_order() {
        let out = effective_core_options(
            &pairs(&[("a", "core"), ("b", "core"), ("c", "core"), ("d", "core")]),
            &pairs(&[("b", "front"), ("c", "front"), ("d", "front")]),
            &map(&[("c", "global"), ("d", "global")]),
            &map(&[("d", "game"), ("e", "game")]),
        );
        assert_eq!(
            out,
            pairs(&[
                ("a", "core"),
                ("b", "front"),
                ("c", "global"),
                ("d", "game"),
                ("e", "game")
            ])
        );
    }

    #[test]
    fn computed_sits_between_frontend_and_config() {
        let out = effective_core_options_with(
            &pairs(&[("k", "core"), ("m", "core")]),
            &pairs(&[("k", "front"), ("m", "front")]),
            &pairs(&[("k", "aros"), ("m", "aros"), ("n", "aros")]),
            &map(&[("m", "global")]),
            &map(&[]),
        );
        assert_eq!(out, pairs(&[("k", "aros"), ("m", "global"), ("n", "aros")]));
        let game = effective_core_options_with(
            &[],
            &[],
            &pairs(&[("k", "aros")]),
            &map(&[]),
            &map(&[("k", "kick")]),
        );
        assert_eq!(game, pairs(&[("k", "kick")]));
    }

    #[test]
    fn aros_flag_round_trips_and_is_omitted_when_false() {
        let mut o = GameOptions::default();
        assert!(!toml::to_string(&o).unwrap().contains("aros_acknowledged"));
        o.input.aros_acknowledged = true;
        let back: GameOptions = toml::from_str(&toml::to_string(&o).unwrap()).unwrap();
        assert!(back.input.aros_acknowledged);
    }

    /// Design v4.0 §5: `[dos] start` read and written back; absent from the files of the other systems.
    #[test]
    fn dos_start_round_trips_and_is_omitted_when_unset() {
        let o = GameOptions::default();
        assert!(!toml::to_string(&o).unwrap().contains("[dos]"));
        let text = "[core_options]\ndosbox_pure_cycles = \"7800\"\n[input]\nkeyboard_mode = \"keyboard\"\n[dos]\nstart = \"MONKEY.EXE\"\n";
        let o: GameOptions = toml::from_str(text).unwrap();
        assert_eq!(o.dos.start.as_deref(), Some("MONKEY.EXE"));
        assert_eq!(o.core_options["dosbox_pure_cycles"], "7800");
        let back: GameOptions = toml::from_str(&toml::to_string(&o).unwrap()).unwrap();
        assert_eq!(back, o);
        // A 3.0 file (no `[dos]`) is read unchanged and written without one.
        let old = "[core_options]\npuae_model = \"A500\"\n[input]\naros_acknowledged = true\n";
        let o: GameOptions = toml::from_str(old).unwrap();
        assert_eq!(o.dos, GameDos::default());
        assert!(!toml::to_string(&o).unwrap().contains("dos"));
    }

    #[test]
    fn path_is_confined() {
        let root = Path::new("/data/game-options");
        assert_eq!(
            game_options_path(root, "hatari", "Nova Defense"),
            Path::new("/data/game-options/hatari/Nova Defense.toml")
        );
        let evil = game_options_path(root, "../x", "../../etc/passwd");
        assert!(evil.starts_with("/data/game-options"));
        assert!(!evil.components().any(|c| c.as_os_str() == ".."));
        assert_eq!(sanitize_component(".."), "_");
        assert_eq!(sanitize_component(""), "_");
        assert_eq!(sanitize_component("a:b?"), "a_b_");
    }

    #[test]
    fn round_trip_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = game_options_path(&dir.path().join("game-options"), "hatari", "Jeu");
        assert_eq!(GameOptions::load(&path), LoadedGameOptions::default());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "future = 1\n[core_options]\nhatari_machinetype = \"ste\"\n[input]\nkeyboard_mode = \"joystick\"\n[media]\nlast_disk = 1\n",
        )
        .unwrap();
        let loaded = GameOptions::load(&path);
        assert!(!loaded.locked);
        let o = &loaded.options;
        assert_eq!(o.core_options["hatari_machinetype"], "ste");
        assert_eq!(o.input.keyboard_mode, Some(KeyboardModeChoice::Joystick));
        assert_eq!(o.media.last_disk, Some(1));
        o.save(&path, loaded.locked).unwrap();
        let again = GameOptions::load(&path);
        assert_eq!(again.options, *o);
        assert!(std::fs::read_to_string(&path).unwrap().contains("future"));
    }

    #[test]
    fn invalid_file_is_locked_and_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.toml");
        std::fs::write(&path, "core_options = [oops").unwrap();
        let loaded = GameOptions::load(&path);
        assert!(loaded.locked);
        assert_eq!(loaded.options, GameOptions::default());
        assert!(loaded.options.save(&path, loaded.locked).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "core_options = [oops"
        );
        // Wrong types are invalid too.
        std::fs::write(&path, "[core_options]\nx = 3\n").unwrap();
        assert!(GameOptions::load(&path).locked);
    }

    #[test]
    fn oversized_file_is_locked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.toml");
        std::fs::write(&path, vec![b'#'; (MAX_FILE_SIZE + 10) as usize]).unwrap();
        assert!(GameOptions::load(&path).locked);
    }

    #[test]
    fn inherit_copies_only_when_the_target_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("game-options");
        // No source: nothing.
        assert!(!inherit_game_options(&root, "hatari", "G", "G (2)").unwrap());
        let source = game_options_path(&root, "hatari", "G");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "[core_options]\nk = \"v\"\n").unwrap();
        assert!(inherit_game_options(&root, "hatari", "G", "G (2)").unwrap());
        let target = game_options_path(&root, "hatari", "G (2)");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            std::fs::read(&source).unwrap()
        );
        // Never overwritten.
        std::fs::write(&target, "mine").unwrap();
        assert!(!inherit_game_options(&root, "hatari", "G", "G (2)").unwrap());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "mine");
        assert!(!inherit_game_options(&root, "hatari", "G", "G").unwrap());
        // Oversized source: not copied.
        std::fs::write(&source, vec![b'#'; 300 * 1024]).unwrap();
        assert!(!inherit_game_options(&root, "hatari", "G", "G (3)").unwrap());
        assert!(!game_options_path(&root, "hatari", "G (3)").exists());
    }
}
