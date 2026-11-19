//! `config.toml` and data directories (ADR 0011). Credentials (ScreenScraper, IGDB) are `Secret`s: file kept 0600, never logged.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use emu_runtime::{FileMode, JoypadButton, MAX_PORTS, write_atomic};
use serde::de::IntoDeserializer;
use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use crate::config_library::{CoresConfig, CoversConfig, CredentialsConfig, LibraryConfig};
use crate::keyboard::{KeyMap, PadStyle};

/// Profile used when a core has no profile of its own.
pub const DEFAULT_PROFILE: &str = "default";
/// Bounds of `video.scale` (and `--scale`): beyond 16 a 320-pixel image exceeds any screen.
pub const MIN_SCALE: u32 = 1;
pub const MAX_SCALE: u32 = 16;
pub const MIN_FAST_FORWARD_RATIO: f64 = 1.0;
pub const MAX_FAST_FORWARD_RATIO: f64 = 16.0;
pub const MIN_LATENCY_MS: u32 = 8;
pub const MAX_LATENCY_MS: u32 = 500;
/// A larger `config.toml` is not read (and the config is locked).
pub const MAX_CONFIG_SIZE: u64 = 1024 * 1024;

/// Everything lives under one root (a single Docker volume).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDirs {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub system: PathBuf,
    pub saves: PathBuf,
    pub states: PathBuf,
    pub assets: PathBuf,
    /// `library.sqlite` (ADR 0006).
    pub library_db: PathBuf,
    /// `covers/<system>/boxart/` (ADR 0014).
    pub covers: PathBuf,
    /// Downloaded No-Intro DAT files: `database/no-intro/<dat>.dat` (ADR 0014).
    pub database: PathBuf,
    /// `cores/<os>-<arch>/` (ADR 0013); same folder as `emu_assets::cores::CoreDirs::under`.
    pub cores: PathBuf,
}

impl DataDirs {
    pub fn under(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            config_file: root.join("config.toml"),
            system: root.join("system"),
            saves: root.join("saves"),
            states: root.join("states"),
            assets: root.join("assets"),
            library_db: root.join("library.sqlite"),
            covers: root.join("covers"),
            database: root.join("database").join("no-intro"),
            cores: root.join("cores").join(format!(
                "{}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
        }
    }

    /// Creates the directories. A root created here is private (0700 on Unix); an existing root is left as is.
    pub fn create_all(&self) -> io::Result<()> {
        if !self.root.exists() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&self.root)?;
        }
        for dir in [
            &self.system,
            &self.saves,
            &self.states,
            &self.assets,
            &self.covers,
            &self.database,
            &self.cores,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    /// Only the parser's message and position: the offending line is never copied (it may hold a secret).
    #[error("configuration invalide : {0}")]
    Parse(String),
    #[error("sérialisation impossible : {0}")]
    Serialize(String),
    #[error("{} est invalide : il n'est pas réécrit", path.display())]
    Locked { path: PathBuf },
    #[error("{} dépasse {MAX_CONFIG_SIZE} octets", path.display())]
    TooLarge { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalingChoice {
    #[default]
    Integer,
    Fit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncChoice {
    #[default]
    Audio,
    Clock,
}

// Every section keeps the keys it does not know (`extra`), so a newer or hand-edited file survives a save.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    pub vsync: bool,
    pub scaling: ScalingChoice,
    /// Initial window size = base geometry × `scale` (`MIN_SCALE..=MAX_SCALE`).
    pub scale: u32,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            vsync: true,
            scaling: ScalingChoice::Integer,
            scale: 3,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub enabled: bool,
    /// `MIN_LATENCY_MS..=MAX_LATENCY_MS`.
    pub latency_ms: u32,
    pub sync: SyncChoice,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            latency_ms: 64,
            sync: SyncChoice::Audio,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeedConfig {
    /// `MIN_FAST_FORWARD_RATIO..=MAX_FAST_FORWARD_RATIO`.
    pub fast_forward_ratio: f64,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for SpeedConfig {
    fn default() -> Self {
        Self {
            fast_forward_ratio: emu_runtime::DEFAULT_FAST_FORWARD_RATIO,
            extra: toml::Table::new(),
        }
    }
}

/// Frontend shortcuts; they take priority over game bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub menu: KeyCode,
    pub pause: KeyCode,
    pub reset: KeyCode,
    pub save_state: KeyCode,
    pub load_state: KeyCode,
    pub prev_slot: KeyCode,
    pub next_slot: KeyCode,
    /// Held, not toggled.
    pub fast_forward: KeyCode,
    pub fullscreen: KeyCode,
    pub stats: KeyCode,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            menu: KeyCode::Escape,
            pause: KeyCode::KeyP,
            reset: KeyCode::KeyH,
            save_state: KeyCode::F2,
            load_state: KeyCode::F4,
            prev_slot: KeyCode::F6,
            next_slot: KeyCode::F7,
            fast_forward: KeyCode::Space,
            fullscreen: KeyCode::F11,
            stats: KeyCode::F3,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// `"default"` or a `core_id` → bindings.
    pub keyboard: BTreeMap<String, KeyMap>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub video: VideoConfig,
    pub audio: AudioSettings,
    pub speed: SpeedConfig,
    pub hotkeys: Hotkeys,
    pub input: InputConfig,
    pub library: LibraryConfig,
    pub covers: CoversConfig,
    pub credentials: CredentialsConfig,
    pub cores: CoresConfig,
    /// `core_id` → option key → value.
    pub core_options: BTreeMap<String, BTreeMap<String, String>>,
    /// Unknown top-level keys and tables, written back unchanged.
    #[serde(flatten)]
    pub extra: toml::Table,
    /// Set when the file existed but could not be used: `save` then refuses (never overwrite the user's file)
    /// until `unlock` (explicit user action in the UI).
    #[serde(skip)]
    locked: bool,
}

/// `message (ligne L, colonne C)` from a TOML error, without the source excerpt its `Display` includes.
fn parse_error(text: &str, e: &toml::de::Error) -> ConfigError {
    let position = e.span().map(|span| {
        let before = &text.as_bytes()[..span.start.min(text.len())];
        let line = before.iter().filter(|b| **b == b'\n').count() + 1;
        let column = before.iter().rev().take_while(|b| **b != b'\n').count() + 1;
        format!(" (ligne {line}, colonne {column})")
    });
    ConfigError::Parse(format!(
        "{}{}",
        safe_message(e.message()),
        position.unwrap_or_default()
    ))
}

/// serde errors quote the offending value (future secret fields): keep only the kind of error.
fn safe_message(message: &str) -> &str {
    const SERDE_PREFIXES: [&str; 7] = [
        "invalid type",
        "invalid value",
        "invalid length",
        "unknown variant",
        "unknown field",
        "missing field",
        "duplicate field",
    ];
    if SERDE_PREFIXES.iter().any(|p| message.starts_with(p)) {
        "type invalide"
    } else {
        message
    }
}

/// ADR 0011: the file is private. A looser existing mode is tightened (only the path is logged).
#[cfg(unix)]
fn enforce_private(path: &Path, file: &std::fs::File) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = file.metadata() else {
        return;
    };
    if meta.permissions().mode() & 0o077 != 0 {
        tracing::warn!(
            "{} était lisible par d'autres : droits ramenés à 600",
            path.display()
        );
        if let Err(e) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!("{} : droits non modifiables : {e}", path.display());
        }
    }
}

impl Config {
    /// A missing file gives the defaults. Out-of-range values are brought back in range (with a warning).
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let io_err = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(io_err(e)),
        };
        #[cfg(unix)]
        enforce_private(path, &file);
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_SIZE + 1)
            .read_to_end(&mut bytes)
            .map_err(io_err)?;
        if bytes.len() as u64 > MAX_CONFIG_SIZE {
            return Err(ConfigError::TooLarge {
                path: path.to_path_buf(),
            });
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| ConfigError::Parse("le fichier n'est pas en UTF-8".into()))?;
        let mut config: Self = toml::from_str(&text).map_err(|e| parse_error(&text, &e))?;
        config.sanitize();
        Ok(config)
    }

    /// Like `load`, but a bad file is logged and yields locked defaults: `save` will not touch the file.
    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_else(|e| {
            tracing::warn!(
                "{} ignoré, valeurs par défaut (fichier conservé) : {e}",
                path.display()
            );
            Self {
                locked: true,
                ..Self::default()
            }
        })
    }

    /// Clamps numeric settings to their bounds; non-finite ratios fall back to the default.
    pub fn sanitize(&mut self) {
        let scale = self.video.scale.clamp(MIN_SCALE, MAX_SCALE);
        if scale != self.video.scale {
            tracing::warn!(
                "video.scale = {} hors de {MIN_SCALE}..={MAX_SCALE} : {scale}",
                self.video.scale
            );
            self.video.scale = scale;
        }
        let latency = self.audio.latency_ms.clamp(MIN_LATENCY_MS, MAX_LATENCY_MS);
        if latency != self.audio.latency_ms {
            tracing::warn!(
                "audio.latency_ms = {} hors de {MIN_LATENCY_MS}..={MAX_LATENCY_MS} : {latency}",
                self.audio.latency_ms
            );
            self.audio.latency_ms = latency;
        }
        let r = self.speed.fast_forward_ratio;
        let ratio = if r.is_finite() && r > 0.0 {
            r.clamp(MIN_FAST_FORWARD_RATIO, MAX_FAST_FORWARD_RATIO)
        } else {
            emu_runtime::DEFAULT_FAST_FORWARD_RATIO
        };
        if ratio.to_bits() != r.to_bits() {
            tracing::warn!("speed.fast_forward_ratio = {r} invalide : {ratio}");
            self.speed.fast_forward_ratio = ratio;
        }
    }

    /// Atomic write with mode 0600.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if self.locked {
            return Err(ConfigError::Locked {
                path: path.to_path_buf(),
            });
        }
        let text =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        write_atomic(path, text.as_bytes(), FileMode::Private).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Lets the next `save` overwrite an invalid file with the current settings. Only on an explicit,
    /// confirmed user request (menu).
    pub fn unlock(&mut self) {
        self.locked = false;
    }

    /// Core profile, else `"default"`, else the built-in layout of the core's system.
    pub fn keymap_for(&self, profile: &str, style: PadStyle) -> KeyMap {
        self.input
            .keyboard
            .get(profile)
            .or_else(|| self.input.keyboard.get(DEFAULT_PROFILE))
            .cloned()
            .unwrap_or_else(|| KeyMap::default_for(style))
    }

    pub fn set_keymap(&mut self, profile: &str, map: KeyMap) {
        self.input.keyboard.insert(profile.to_owned(), map);
    }

    /// Saved option overrides of a core, as `EmulatorOptions::core_variables`.
    pub fn core_variables(&self, core_id: &str) -> Vec<(String, String)> {
        self.core_options
            .get(core_id)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }
}

#[derive(Serialize, Deserialize)]
struct BindingRepr {
    key: String,
    port: usize,
    button: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct KeyMapRepr {
    bindings: Vec<BindingRepr>,
}

fn key_name(key: KeyCode) -> String {
    // Unit variants print as their name, which is also what winit's serde accepts.
    format!("{key:?}")
}

fn parse_key(name: &str) -> Option<KeyCode> {
    KeyCode::deserialize(IntoDeserializer::<serde::de::value::Error>::into_deserializer(name)).ok()
}

impl From<KeyMap> for KeyMapRepr {
    fn from(map: KeyMap) -> Self {
        let bindings = map
            .bindings()
            .iter()
            .filter_map(|(key, port, button)| {
                let name = key_name(*key);
                if parse_key(&name) != Some(*key) {
                    tracing::warn!("touche {name} non sauvegardable, liaison ignorée");
                    return None;
                }
                Some(BindingRepr {
                    key: name,
                    port: *port,
                    button: button.name().to_owned(),
                })
            })
            .collect();
        Self { bindings }
    }
}

impl From<KeyMapRepr> for KeyMap {
    fn from(repr: KeyMapRepr) -> Self {
        let mut map = KeyMap::default();
        for b in repr.bindings {
            let (Some(key), Some(button)) = (parse_key(&b.key), JoypadButton::from_name(&b.button))
            else {
                tracing::warn!(
                    "liaison ignorée : touche « {} », bouton « {} »",
                    b.key,
                    b.button
                );
                continue;
            };
            if b.port >= MAX_PORTS {
                tracing::warn!("liaison ignorée : port {} hors limites", b.port);
                continue;
            }
            map.bind(key, b.port, button);
        }
        map
    }
}

impl Serialize for KeyMap {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        KeyMapRepr::from(self.clone()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for KeyMap {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        KeyMapRepr::deserialize(d).map(KeyMap::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut cfg = Config::default();
        cfg.video.vsync = false;
        cfg.video.scaling = ScalingChoice::Fit;
        cfg.hotkeys.pause = KeyCode::F9;
        cfg.set_keymap(DEFAULT_PROFILE, KeyMap::genesis_default());
        let mut other = KeyMap::default();
        other.bind(KeyCode::KeyQ, 1, JoypadButton::R3);
        cfg.set_keymap("mock", other);
        cfg.core_options
            .entry("mock".into())
            .or_default()
            .insert("mock_color".into(), "red".into());
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn every_key_name_round_trips() {
        for key in [
            KeyCode::KeyA,
            KeyCode::Escape,
            KeyCode::F11,
            KeyCode::Numpad3,
            KeyCode::ShiftRight,
        ] {
            assert_eq!(parse_key(&key_name(key)), Some(key));
        }
    }

    #[test]
    fn partial_file_gives_defaults() {
        let cfg: Config = toml::from_str("[video]\nvsync = false\n").unwrap();
        assert!(!cfg.video.vsync);
        assert_eq!(cfg.video.scale, 3);
        assert_eq!(cfg.hotkeys, Hotkeys::default());
        assert_eq!(cfg.audio, AudioSettings::default());
    }

    #[test]
    fn unknown_key_or_button_is_skipped() {
        let text = r#"
[[input.keyboard.default.bindings]]
key = "KeyZ"
port = 0
button = "y"
[[input.keyboard.default.bindings]]
key = "NoSuchKey"
port = 0
button = "a"
[[input.keyboard.default.bindings]]
key = "KeyX"
port = 0
button = "nope"
[[input.keyboard.default.bindings]]
key = "KeyC"
port = 99
button = "a"
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        let map = cfg.keymap_for("anything", PadStyle::Generic);
        assert_eq!(map.bindings(), &[(KeyCode::KeyZ, 0, JoypadButton::Y)]);
    }

    #[test]
    fn keymap_fallbacks() {
        let mut cfg = Config::default();
        assert_eq!(
            cfg.keymap_for("x", PadStyle::Generic),
            KeyMap::generic_default()
        );
        assert_eq!(
            cfg.keymap_for("genesis_plus_gx", PadStyle::MegaDrive),
            KeyMap::genesis_default()
        );
        assert_eq!(
            cfg.keymap_for("snes9x", PadStyle::Snes),
            KeyMap::snes_default()
        );
        let mut d = KeyMap::default();
        d.bind(KeyCode::KeyQ, 0, JoypadButton::A);
        cfg.set_keymap(DEFAULT_PROFILE, d.clone());
        assert_eq!(cfg.keymap_for("x", PadStyle::Generic), d);
        let mut c = KeyMap::default();
        c.bind(KeyCode::KeyW, 0, JoypadButton::B);
        cfg.set_keymap("x", c.clone());
        assert_eq!(cfg.keymap_for("x", PadStyle::Generic), c);
        assert_eq!(cfg.keymap_for("y", PadStyle::Generic), d);
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::load(&dir.path().join("nope.toml")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn invalid_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "video = [[[ not toml").unwrap();
        assert!(matches!(Config::load(&path), Err(ConfigError::Parse(_))));
        let cfg = Config::load_or_default(&path);
        assert!(cfg.is_locked());
        assert_eq!(cfg.video, VideoConfig::default());
        assert!(matches!(cfg.save(&path), Err(ConfigError::Locked { .. })));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "video = [[[ not toml"
        );
    }

    #[test]
    fn wrong_types_are_invalid_not_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[hotkeys]\npause = \"Bogus\"\n").unwrap();
        assert!(Config::load_or_default(&path).is_locked());
    }

    #[test]
    fn save_then_load_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.video.scale = 5;
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), cfg);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn data_dirs_layout_and_creation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let dirs = DataDirs::under(&root);
        assert_eq!(dirs.config_file, root.join("config.toml"));
        dirs.create_all().unwrap();
        for d in [&dirs.system, &dirs.saves, &dirs.states, &dirs.assets] {
            assert!(d.is_dir());
        }
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[video]\nscale = 4000000000\n[audio]\nlatency_ms = 1\n[speed]\nfast_forward_ratio = nan\n",
        )
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.video.scale, MAX_SCALE);
        assert_eq!(cfg.audio.latency_ms, MIN_LATENCY_MS);
        assert_eq!(
            cfg.speed.fast_forward_ratio,
            emu_runtime::DEFAULT_FAST_FORWARD_RATIO
        );
        for (text, want) in [
            ("-3.0", 4.0),
            ("inf", 4.0),
            ("0.5", 1.0),
            ("99.0", 16.0),
            ("2.5", 2.5),
        ] {
            let mut c: Config =
                toml::from_str(&format!("[speed]\nfast_forward_ratio = {text}\n")).unwrap();
            c.sanitize();
            assert_eq!(c.speed.fast_forward_ratio, want, "{text}");
        }
        let mut c: Config =
            toml::from_str("[video]\nscale = 0\n[audio]\nlatency_ms = 9999\n").unwrap();
        c.sanitize();
        assert_eq!(
            (c.video.scale, c.audio.latency_ms),
            (MIN_SCALE, MAX_LATENCY_MS)
        );
    }

    #[test]
    fn parse_error_never_echoes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[video]\nvsync = true\ntoken = \"hunter2\n").unwrap();
        let Err(ConfigError::Parse(msg)) = Config::load(&path) else {
            panic!("parse error expected");
        };
        assert!(!msg.contains("hunter2"), "{msg}");
        assert!(msg.contains("ligne 3"), "{msg}");
    }

    #[test]
    fn serde_error_never_echoes_the_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[video]\nvsync = \"hunter2\"\n").unwrap();
        let Err(ConfigError::Parse(msg)) = Config::load(&path) else {
            panic!("parse error expected");
        };
        assert!(!msg.contains("hunter2"), "{msg}");
        assert!(
            msg.contains("type invalide") && msg.contains("ligne 2"),
            "{msg}"
        );
    }

    #[test]
    fn oversized_file_is_locked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let big = format!(
            "# {}\n",
            "x".repeat(usize::try_from(MAX_CONFIG_SIZE).unwrap())
        );
        std::fs::write(&path, &big).unwrap();
        assert!(matches!(
            Config::load(&path),
            Err(ConfigError::TooLarge { .. })
        ));
        assert!(Config::load_or_default(&path).is_locked());
    }

    #[test]
    fn unknown_keys_survive_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "future_top = 7\n[video]\nscale = 2\nshader = \"crt\"\n[library]\nroms = \"/roms\"\n",
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.video.vsync = false;
        cfg.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let back = Config::load(&path).unwrap();
        assert_eq!(back, cfg);
        assert!(
            text.contains("shader") && text.contains("/roms") && text.contains("future_top"),
            "{text}"
        );
    }

    #[test]
    fn unlock_allows_overwriting_an_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "video = [[[").unwrap();
        let mut cfg = Config::load_or_default(&path);
        assert!(cfg.save(&path).is_err());
        cfg.unlock();
        cfg.save(&path).unwrap();
        assert!(!Config::load_or_default(&path).is_locked());
    }

    #[cfg(unix)]
    #[test]
    fn loose_mode_is_tightened_on_load() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[video]\nvsync = false\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        Config::load(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn new_root_is_private_existing_root_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("new/data");
        DataDirs::under(&root).create_all().unwrap();
        let mode = std::fs::metadata(&root).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let existing = dir.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o755)).unwrap();
        DataDirs::under(&existing).create_all().unwrap();
        let mode = std::fs::metadata(&existing).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}
