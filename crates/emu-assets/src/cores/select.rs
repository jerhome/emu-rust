//! Core choice for a game. Pure: no file system, no network (callers pass what is installed).
//! System keys are **libretro database names** (`"Nintendo - Game Boy"`), the same as `.info` `database`
//! fields, libretro-thumbnails and the `.rdb` files, shared with emu-library.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::info::is_safe_relative_path;
use super::{CoreId, CoreInfo, Platform};

const BUILTIN: &str = include_str!("../../data/cores.toml");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreferencesFile {
    systems: BTreeMap<String, SystemEntry>,
    #[serde(default)]
    cores: BTreeMap<String, CoreEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemEntry {
    cores: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreEntry {
    #[serde(default)]
    media: Option<CoreMediaPolicy>,
    #[serde(default)]
    media_direct: Vec<String>,
    #[serde(default)]
    game_saves: Vec<String>,
    #[serde(default)]
    builtin_firmware: Option<BuiltinFirmware>,
    #[serde(default)]
    required_firmware: BTreeMap<String, RequiredFirmwareEntry>,
    #[serde(default)]
    recommended_on: Vec<String>,
    #[serde(default)]
    options: BTreeMap<String, String>,
    // 4.0 keys (design v4.0 §3.3).
    #[serde(default)]
    folders: Option<FolderPolicy>,
    #[serde(default)]
    vfs: bool,
    #[serde(default)]
    analog: bool,
    #[serde(default)]
    system_caches: Vec<String>,
    #[serde(default)]
    featured_options: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    layout_option: Option<LayoutOptionEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutOptionEntry {
    option: String,
    fallback: String,
    #[serde(default)]
    map: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequiredFirmwareEntry {
    variant: FirmwareVariant,
    default: String,
    #[serde(default)]
    fallback: Option<FirmwareFallback>,
    files: BTreeMap<String, String>,
}

/// How content files reach a core (ADR 0015 §5), `media` in `data/cores.toml`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreMediaPolicy {
    /// Paths handed over as they are (the core never writes into them).
    #[default]
    Direct,
    /// The core writes into disk images: it gets a working copy, the user's files are never modified.
    WorkingCopy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CoreProps {
    media: CoreMediaPolicy,
    media_direct: Vec<String>,
    game_saves: Vec<String>,
    builtin_firmware: Option<BuiltinFirmware>,
    required_firmware: BTreeMap<String, RequiredFirmware>,
    recommended_on: Vec<Platform>,
    options: Vec<(String, String)>,
    folders: Option<FolderPolicy>,
    vfs: bool,
    analog: bool,
    system_caches: Vec<String>,
    featured_options: BTreeMap<String, Vec<String>>,
    layout_option: Option<LayoutOption>,
}

/// Ordered core preferences per system, from the embedded `data/cores.toml`, plus user overrides
/// (`[cores.preferred]` in `config.toml`, keyed by database name), which go first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorePreferences {
    systems: BTreeMap<String, Vec<CoreId>>,
    cores: BTreeMap<CoreId, CoreProps>,
}

impl CorePreferences {
    /// Parses the embedded `data/cores.toml`; a parse error is a bug caught by a unit test.
    pub fn builtin() -> Self {
        match Self::parse(BUILTIN) {
            Ok(prefs) => prefs,
            Err(e) => {
                tracing::error!("data/cores.toml invalide : {e}");
                Self::default()
            }
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        let file: PreferencesFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut systems = BTreeMap::new();
        for (system, entry) in file.systems {
            let mut cores = Vec::new();
            for core in entry.cores {
                let id = CoreId::new(&core).ok_or_else(|| format!("id invalide : {core}"))?;
                if !cores.contains(&id) {
                    cores.push(id);
                }
            }
            systems.insert(system, cores);
        }
        let mut cores = BTreeMap::new();
        for (core, entry) in file.cores {
            let id = CoreId::new(&core).ok_or_else(|| format!("id invalide : {core}"))?;
            if let Some(key) = entry.options.keys().find(|k| !is_option_key(k)) {
                return Err(format!("option invalide pour {core} : {key}"));
            }
            let props = core_props(&core, entry)?;
            cores.insert(id, props);
        }
        Ok(Self { systems, cores })
    }

    /// Parses a `data/cores.toml`-format text (tests of sibling modules); panics on invalid data.
    #[cfg(test)]
    pub(crate) fn builtin_from_text_for_tests(text: &str) -> Self {
        Self::parse(text).unwrap()
    }

    /// `system → core_id` from the user config; invalid ids are ignored with a warning.
    pub fn with_overrides(mut self, overrides: &BTreeMap<String, String>) -> Self {
        for (system, core) in overrides {
            let Some(id) = CoreId::new(core.trim()) else {
                tracing::warn!("[cores.preferred] {system} : identifiant de core invalide, ignoré");
                continue;
            };
            let cores = self.systems.entry(system.clone()).or_default();
            cores.retain(|c| c != &id);
            cores.insert(0, id);
        }
        self
    }

    /// Preferred cores for a system, best first (empty if unknown).
    pub fn for_system(&self, system: &str) -> &[CoreId] {
        self.systems.get(system).map_or(&[], Vec::as_slice)
    }

    /// `media` of `data/cores.toml`; `Direct` for cores without an entry.
    pub fn media_policy(&self, core: &CoreId) -> CoreMediaPolicy {
        self.cores
            .get(core)
            .map_or_else(CoreMediaPolicy::default, |p| p.media)
    }

    /// Frontend defaults for the core's options (`options` of `data/cores.toml`), sorted by key. They sit
    /// above the core's own defaults and below every user setting (ADR 0015 §8).
    pub fn frontend_options(&self, core: &CoreId) -> Vec<(String, String)> {
        self.cores
            .get(core)
            .map_or_else(Vec::new, |p| p.options.clone())
    }

    /// `media_direct` of `data/cores.toml`: lower-case extensions never copied nor extracted for this core
    /// (puae: `ipf`, `lha`; design v3.0 §3.5.1). Empty for cores without the key. Same name everywhere:
    /// `cores.toml` key, this accessor, `emu_runtime::MediaRules::media_direct`.
    pub fn media_direct(&self, core: &CoreId) -> Vec<String> {
        self.cores
            .get(core)
            .map_or_else(Vec::new, |p| p.media_direct.clone())
    }

    /// `game_saves` of `data/cores.toml`: file-name patterns of the save files the core writes in its saves
    /// directory (`{game}.save*.adf`), for `emu_runtime::reset_core_game_files` (design v3.0 §3.5.3).
    /// Patterns holding a separator, `..` or more than one `*` are refused when the file is parsed.
    pub fn game_save_patterns(&self, core: &CoreId) -> Vec<String> {
        self.cores
            .get(core)
            .map_or_else(Vec::new, |p| p.game_saves.clone())
    }

    /// `[cores.<core>.required_firmware."<system>"]` of `data/cores.toml` (`system` = libretro database name),
    /// input of [`super::check_required`]. `None`: no table (use `builtin_firmware`, else the 1.0 `.info` check).
    pub fn required_firmware(&self, core: &CoreId, system: &str) -> Option<&RequiredFirmware> {
        self.cores.get(core)?.required_firmware.get(system)
    }

    /// `builtin_firmware` of `data/cores.toml`: ROMs compiled into the core (AROS in puae; cap32, VICE in 5.0).
    pub fn builtin_firmware(&self, core: &CoreId) -> Option<BuiltinFirmware> {
        self.cores.get(core)?.builtin_firmware.clone()
    }

    /// `recommended_on` of `data/cores.toml` (`["macos"]` for puae2021; values `linux`, `windows`, `macos`):
    /// the core selector of the Cores screen then shows « recommandé sur Mac » next to this alternative, on
    /// every OS. Never changes the default order (user decision of 2026-11-21: puae stays first everywhere).
    pub fn recommended_on(&self, core: &CoreId) -> Vec<Platform> {
        self.cores
            .get(core)
            .map_or_else(Vec::new, |p| p.recommended_on.clone())
    }

    /// `folders` of `data/cores.toml` (design v4.0 §3.3): how an unpacked game folder reaches the core.
    /// `None`: folders are refused for this core.
    pub fn folder_policy(&self, core: &CoreId) -> Option<FolderPolicy> {
        self.cores.get(core)?.folders
    }

    /// `vfs = true` of `data/cores.toml`: the frontend answers `GET_VFS_INTERFACE` (`CoreConfig::provide_vfs`).
    /// `false` for cores without the key (puae, hatari: unchanged).
    pub fn provides_vfs(&self, core: &CoreId) -> bool {
        self.cores.get(core).is_some_and(|p| p.vfs)
    }

    /// `[cores.<core>.featured_options] "<system>" = […]`: option keys shown first (« Réglages DOS »), in file
    /// order. Unknown core or system ⇒ empty.
    pub fn featured_options(&self, core: &CoreId, system: &str) -> Vec<String> {
        self.cores
            .get(core)
            .and_then(|p| p.featured_options.get(system))
            .cloned()
            .unwrap_or_default()
    }

    /// `layout_option` of `data/cores.toml`: core option computed from the system language (design v3.0 §3.5.4,
    /// v4.0 §3.3). `None` for cores without the key.
    pub fn layout_option(&self, core: &CoreId) -> Option<LayoutOption> {
        self.cores.get(core)?.layout_option.clone()
    }

    /// `analog = true` of `data/cores.toml`: sticks are passed as `RETRO_DEVICE_ANALOG` and no longer converted
    /// to the D-pad (ADR 0016 amended). `false` without the key.
    pub fn analog(&self, core: &CoreId) -> bool {
        self.cores.get(core).is_some_and(|p| p.analog)
    }

    /// `system_caches` of `data/cores.toml`: plain file names the core writes at the root of `system/` and that
    /// go stale after an install / import there (`DOSBoxPureMidiCache.txt`), for
    /// [`crate::bios::invalidate_system_caches`]. Empty without the key.
    pub fn system_caches(&self, core: &CoreId) -> Vec<String> {
        self.cores
            .get(core)
            .map_or_else(Vec::new, |p| p.system_caches.clone())
    }

    /// `save_scope` of `data/cores.toml` (design v5.0 §3.3): `Game` ⇒ `emu_runtime::SaveScope::Game` (cap32).
    /// `Core` for cores without the key.
    pub fn save_scope(&self, core: &CoreId) -> SaveScopePref {
        let _ = core;
        todo!("CA1: cores.toml `save_scope`")
    }

    /// `fastforward_override = true` of `data/cores.toml`: the core may request fast-forward (env 64,
    /// `CoreConfig::fastforward_override`). `false` without the key.
    pub fn fastforward_override(&self, core: &CoreId) -> bool {
        let _ = core;
        todo!("CA1: cores.toml `fastforward_override`")
    }

    /// `variable_state_size = true` of `data/cores.toml` (VICE): `CoreConfig::variable_state_size`. `false`
    /// without the key.
    pub fn variable_state_size(&self, core: &CoreId) -> bool {
        let _ = core;
        todo!("CA1: cores.toml `variable_state_size`")
    }

    /// `[cores.<core>.options_by_extension] <ext> = { key = "value" }` (cap32: `cpr` ⇒ `cap32_model = "6128+"`):
    /// third source of the computed defaults (design v3.0 §3.5.4). `extension`: case ignored, without the dot.
    /// Sorted by key; empty for an unknown core or extension.
    pub fn options_by_extension(&self, core: &CoreId, extension: &str) -> Vec<(String, String)> {
        let _ = (core, extension);
        todo!("CA1: cores.toml `options_by_extension`")
    }

    /// `m3u_launch` of `data/cores.toml` (`"command"` cap32, `"program"` VICE) ⇒ `emu_runtime::M3uLaunch`.
    /// `None` without the key (`M3uLaunch::None`).
    pub fn m3u_launch(&self, core: &CoreId) -> Option<M3uLaunchPref> {
        let _ = core;
        todo!("CA1: cores.toml `m3u_launch`")
    }

    /// `[cores.<core>.actions]` of `data/cores.toml`: keys pressed by the « Cassette » menu. Names checked when
    /// the file is parsed against the libretro key names. All `None` without the table.
    pub fn actions(&self, core: &CoreId) -> CoreActions {
        let _ = core;
        todo!("CA1: cores.toml `[actions]`")
    }

    /// Systems known to the preferences (for "install recommended cores").
    pub fn systems(&self) -> impl Iterator<Item = (&str, &[CoreId])> {
        self.systems.iter().map(|(s, c)| (s.as_str(), c.as_slice()))
    }
}

/// `save_scope` of `data/cores.toml` (design v5.0 §3.3; not `save_dir`, a path of `CoreConfig`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveScopePref {
    /// `<saves>/<core_id>/` (sub-folder only for a homonym, 2.1).
    #[default]
    Core,
    /// `<saves>/<core_id>/<save_name>/` for every game. Only such a core may list a `game_saves` pattern without
    /// `{game}` (`*.sav`): on a shared folder it would delete the files of other games.
    Game,
}

/// `m3u_launch` of `data/cores.toml` (design v5.0 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M3uLaunchPref {
    /// `#COMMAND:<launch>` first line (cap32).
    Command,
    /// `<image>:<launch>` first entry (VICE).
    Program,
}

/// `[cores.<core>.actions]` of `data/cores.toml`: libretro key names without the `RETROK_` prefix (`"HOME"`,
/// `"F13"`), converted by the UI with `RetroKey::from_name` (emu-assets does not depend on emu-libretro).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreActions {
    pub tape_play: Option<String>,
    pub tape_stop: Option<String>,
    pub tape_rewind: Option<String>,
}

impl CoreActions {
    /// No action declared: the « Cassette » menu is hidden.
    pub fn is_empty(&self) -> bool {
        self.tape_play.is_none() && self.tape_stop.is_none() && self.tape_rewind.is_none()
    }
}

/// `folders` of `data/cores.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderPolicy {
    /// The core gets a stored-zip snapshot of the folder (`emu_runtime::snapshot_folder`); the folder is never
    /// written.
    Snapshot,
}

/// Core option whose default is computed from the system language (`layout_option` of `data/cores.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutOption {
    /// Option key (`dosbox_pure_keyboard_layout`).
    pub option: String,
    /// BCP 47 language (`fr`, `fr-BE`) → option value; exact language first, then without region.
    pub map: BTreeMap<String, String>,
    /// Value when no entry matches (`us`).
    pub fallback: String,
}

impl LayoutOption {
    /// Value for a system locale (`fr-BE`, `fr_FR.UTF-8`, `de-CH`): exact language (case-insensitive), then the
    /// language without its region, else `fallback`. `None` (locale unknown) ⇒ `fallback`.
    pub fn value_for(&self, locale: Option<&str>) -> &str {
        let tag = locale
            .unwrap_or("")
            .split(['.', '@'])
            .next()
            .unwrap_or("")
            .replace('_', "-");
        let primary = tag.split('-').next().unwrap_or("");
        let find = |wanted: &str| {
            (!wanted.is_empty())
                .then(|| {
                    self.map
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(wanted))
                        .map(|(_, v)| v.as_str())
                })
                .flatten()
        };
        find(&tag)
            .or_else(|| find(primary))
            .unwrap_or(&self.fallback)
    }
}

/// Where the UI takes the variant passed to [`super::check_required`] from (`variant` of a
/// `required_firmware` table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareVariant {
    /// Resolved machine model (Amiga: option `puae_model` after `auto` resolution).
    Model,
    /// Region of the game / disc (Mega CD in 6.0, PlayStation in 7.0).
    Region,
}

/// Core option that switches to the ROM built into the core when the wanted file is absent
/// (`("puae_kickstart", "aros")`): applied by the UI as a computed default, never written (design v3.0 §3.5.4).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmwareFallback {
    pub option: String,
    pub value: String,
}

/// One `[cores.<core>.required_firmware."<system>"]` table (design v3.0 §3.5.5). Data only: later versions
/// add tables, not code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredFirmware {
    /// libretro database name (`"Commodore - Amiga"`).
    pub system: String,
    pub variant: FirmwareVariant,
    /// Variant → file name relative to the system directory (`A500` → `kick34005.A500`).
    pub files: BTreeMap<String, String>,
    /// Key of `files` used when the variant is unknown.
    pub default: String,
    /// Requires `builtin_firmware` on the same core (checked when the file is parsed).
    pub fallback: Option<FirmwareFallback>,
}

/// ROMs compiled into a core: nothing to provide, nothing downloaded. `notice` is the attribution shown in
/// the Cores screen and the release README (design v3.0 §3.5.7); never part of `THIRD-PARTY-DATA.txt`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinFirmware {
    pub label: String,
    pub notice: String,
}

/// Checks and converts one `[cores.<core>]` table (the file is embedded, but its rules are those later
/// versions rely on: refused here rather than half-applied at run time).
fn core_props(core: &str, entry: CoreEntry) -> Result<CoreProps, String> {
    let mut media_direct = Vec::new();
    for ext in &entry.media_direct {
        let ext = ext.to_ascii_lowercase();
        if !is_extension(&ext) {
            return Err(format!("media_direct invalide pour {core} : {ext}"));
        }
        if !media_direct.contains(&ext) {
            media_direct.push(ext);
        }
    }
    if let Some(pattern) = entry.game_saves.iter().find(|p| !is_save_pattern(p)) {
        return Err(format!("game_saves invalide pour {core} : {pattern}"));
    }
    if let Some(b) = &entry.builtin_firmware
        && (b.label.trim().is_empty() || b.notice.trim().is_empty())
    {
        return Err(format!("builtin_firmware incomplet pour {core}"));
    }
    let mut required_firmware = BTreeMap::new();
    for (system, req) in entry.required_firmware {
        let req = RequiredFirmware {
            system: system.clone(),
            variant: req.variant,
            files: req.files,
            default: req.default,
            fallback: req.fallback,
        };
        check_required_table(core, &req, entry.builtin_firmware.is_some())?;
        required_firmware.insert(system, req);
    }
    let mut recommended_on = Vec::new();
    for os in &entry.recommended_on {
        let platform = match os.as_str() {
            "linux" => Platform::LinuxX86_64,
            "windows" => Platform::WindowsX86_64,
            "macos" => Platform::MacosX86_64,
            other => return Err(format!("recommended_on invalide pour {core} : {other}")),
        };
        if !recommended_on.contains(&platform) {
            recommended_on.push(platform);
        }
    }
    let mut system_caches: Vec<String> = Vec::new();
    for name in &entry.system_caches {
        if !is_cache_name(name) {
            return Err(format!("system_caches invalide pour {core} : {name}"));
        }
        if !system_caches.contains(name) {
            system_caches.push(name.clone());
        }
    }
    let mut featured_options = BTreeMap::new();
    for (system, keys) in entry.featured_options {
        let mut list: Vec<String> = Vec::new();
        for key in keys {
            if !is_option_key(&key) {
                return Err(format!(
                    "featured_options invalide pour {core} ({system}) : {key}"
                ));
            }
            if !list.contains(&key) {
                list.push(key);
            }
        }
        featured_options.insert(system, list);
    }
    let layout_option = entry
        .layout_option
        .map(|l| layout_option(core, l))
        .transpose()?;
    Ok(CoreProps {
        media: entry.media.unwrap_or_default(),
        media_direct,
        game_saves: entry.game_saves,
        builtin_firmware: entry.builtin_firmware,
        required_firmware,
        recommended_on,
        options: entry.options.into_iter().collect(),
        folders: entry.folders,
        vfs: entry.vfs,
        analog: entry.analog,
        system_caches,
        featured_options,
        layout_option,
    })
}

fn layout_option(core: &str, entry: LayoutOptionEntry) -> Result<LayoutOption, String> {
    let context = format!("layout_option de {core}");
    if !is_option_key(&entry.option) {
        return Err(format!("{context} : option invalide {}", entry.option));
    }
    if !is_option_value(&entry.fallback) {
        return Err(format!("{context} : fallback invalide"));
    }
    if let Some((lang, value)) = entry
        .map
        .iter()
        .find(|(lang, value)| !is_language_tag(lang) || !is_option_value(value))
    {
        return Err(format!("{context} : entrée invalide {lang} = {value}"));
    }
    // Case-insensitive lookup (`value_for`): two keys differing only by case would be ambiguous.
    let mut seen = std::collections::BTreeSet::new();
    if let Some(dup) = entry
        .map
        .keys()
        .find(|k| !seen.insert(k.to_ascii_lowercase()))
    {
        return Err(format!("{context} : langue en double {dup}"));
    }
    Ok(LayoutOption {
        option: entry.option,
        map: entry.map,
        fallback: entry.fallback,
    })
}

/// BCP 47 language tag as written in `layout_option` maps (`fr`, `fr-BE`, `de-CH`).
fn is_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 16
        && tag
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// A core option value: non-empty, printable, no surrounding space.
fn is_option_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// A plain file name at the root of `system/` (same rule as `crate::bios::invalidate_system_caches`).
fn is_cache_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn check_required_table(
    core: &str,
    req: &RequiredFirmware,
    has_builtin: bool,
) -> Result<(), String> {
    let context = format!("required_firmware de {core} ({})", req.system);
    if req.files.is_empty() {
        return Err(format!("{context} : table files vide"));
    }
    if !req.files.contains_key(&req.default) {
        return Err(format!("{context} : défaut inconnu {}", req.default));
    }
    if let Some(path) = req.files.values().find(|p| !is_safe_relative_path(p)) {
        return Err(format!("{context} : chemin refusé {path}"));
    }
    if let Some(fallback) = &req.fallback {
        if !has_builtin {
            return Err(format!("{context} : fallback sans builtin_firmware"));
        }
        if !is_option_key(&fallback.option) || fallback.value.trim().is_empty() {
            return Err(format!("{context} : fallback invalide"));
        }
    }
    Ok(())
}

/// Lower-case file extension without the dot (`ipf`).
fn is_extension(ext: &str) -> bool {
    !ext.is_empty()
        && ext.len() <= 16
        && ext
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// A file-name pattern (design v3.0 §3.5.3): no separator, no `..`, at most one `*`, `{game}` the only
/// placeholder; it must name something else than "every file" (`*` alone is refused).
fn is_save_pattern(pattern: &str) -> bool {
    let without_game = pattern.replace("{game}", "");
    !pattern.is_empty()
        && pattern != "*"
        && pattern.len() <= 128
        && !pattern.contains(['/', '\\', ':', '\0'])
        && !pattern.contains("..")
        && pattern.matches('*').count() <= 1
        && !without_game.contains(['{', '}'])
        && !pattern.starts_with(' ')
        && !pattern.ends_with([' ', '.'])
}

/// libretro option keys are plain identifiers (`hatari_vkbd`).
fn is_option_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreChoice {
    Installed(CoreId),
    /// Nothing suitable installed; this one is recommended (UI offers to install it).
    Install(CoreId),
    /// No known core for this system / extension.
    Unknown,
}

/// 1. `system` (database name) known to `prefs` ⇒ first preferred core that is installed,
///    else `Install(first preferred)`.
/// 2. Otherwise, the first installed core (by id) whose `.info` lists `extension`.
/// 3. Otherwise `Unknown`.
pub fn choose_core(
    system: Option<&str>,
    extension: Option<&str>,
    installed: &[CoreId],
    infos: &BTreeMap<CoreId, CoreInfo>,
    prefs: &CorePreferences,
) -> CoreChoice {
    if let Some(preferred) = system
        .map(|s| prefs.for_system(s))
        .filter(|p| !p.is_empty())
    {
        return preferred
            .iter()
            .find(|id| installed.contains(id))
            .map_or_else(
                || CoreChoice::Install(preferred[0].clone()),
                |id| CoreChoice::Installed(id.clone()),
            );
    }
    if let Some(ext) = extension
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
    {
        let mut sorted: Vec<&CoreId> = installed.iter().collect();
        sorted.sort();
        if let Some(id) = sorted
            .into_iter()
            .find(|id| infos.get(*id).is_some_and(|i| i.extensions.contains(&ext)))
        {
            return CoreChoice::Installed(id.clone());
        }
    }
    CoreChoice::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: &str = "Nintendo - Game Boy";

    fn id(s: &str) -> CoreId {
        CoreId::new(s).unwrap()
    }

    fn info(core: &str, exts: &[&str]) -> (CoreId, CoreInfo) {
        let text = format!("supported_extensions = \"{}\"", exts.join("|"));
        (id(core), super::super::parse_info(id(core), &text).unwrap())
    }

    #[test]
    fn builtin_parses_with_expected_recommendations() {
        assert!(CorePreferences::parse(BUILTIN).is_ok());
        let prefs = CorePreferences::builtin();
        let first = |s: &str| prefs.for_system(s)[0].to_string();
        assert_eq!(first("Sega - Mega Drive - Genesis"), "genesis_plus_gx");
        assert_eq!(first("Sega - Master System - Mark III"), "genesis_plus_gx");
        assert_eq!(first("Sega - Game Gear"), "genesis_plus_gx");
        assert_eq!(
            first("Nintendo - Super Nintendo Entertainment System"),
            "snes9x"
        );
        assert_eq!(first(GB), "gambatte");
        assert_eq!(first("Nintendo - Game Boy Color"), "gambatte");
        assert_eq!(first("Nintendo - Nintendo Entertainment System"), "mesen");
        assert_eq!(first("Atari - ST"), "hatari");
        assert_eq!(prefs.for_system("Atari - ST")[1].to_string(), "hatarib");
        // 8 systems of 1.0 / 2.0, Mega CD (2.1), Amiga (3.0), DOS (4.0).
        assert_eq!(prefs.systems().count(), 11);
        let dos: Vec<String> = prefs
            .for_system("DOS")
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            dos,
            ["dosbox_pure"],
            "ADR 0018 §1: dosbox_core / dosbox_svn by hand only"
        );
        assert_eq!(first("Commodore - Amiga"), "puae");
        assert_eq!(
            prefs.for_system("Commodore - Amiga")[1].to_string(),
            "puae2021"
        );
        let mcd: Vec<String> = prefs
            .for_system("Sega - Mega-CD - Sega CD")
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(mcd, ["genesis_plus_gx", "picodrive"]);
        assert!(prefs.for_system("Unknown").is_empty());
    }

    #[test]
    fn core_properties() {
        let prefs = CorePreferences::builtin();
        assert_eq!(
            prefs.media_policy(&id("hatari")),
            CoreMediaPolicy::WorkingCopy
        );
        assert_eq!(prefs.media_policy(&id("hatarib")), CoreMediaPolicy::Direct);
        assert_eq!(prefs.media_policy(&id("gambatte")), CoreMediaPolicy::Direct);
        assert_eq!(
            prefs.frontend_options(&id("hatari")),
            [("hatari_vkbd".to_owned(), "disabled".to_owned())]
        );
        assert_eq!(
            prefs.frontend_options(&id("genesis_plus_gx")),
            [(
                "genesis_plus_gx_system_bram".to_owned(),
                "per game".to_owned()
            )]
        );
        assert_eq!(
            prefs.media_policy(&id("genesis_plus_gx")),
            CoreMediaPolicy::Direct
        );
        assert!(prefs.frontend_options(&id("hatarib")).is_empty());
        assert!(prefs.frontend_options(&id("snes9x")).is_empty());
        // Overrides keep the per-core properties.
        let overrides = BTreeMap::from([("Atari - ST".to_owned(), "hatarib".to_owned())]);
        let prefs = prefs.with_overrides(&overrides);
        assert_eq!(
            prefs.media_policy(&id("hatari")),
            CoreMediaPolicy::WorkingCopy
        );

        let parse = |t: &str| CorePreferences::parse(t);
        assert!(parse("[systems]\n[cores.x]\nmedia = \"copy\"\n").is_err());
        assert!(parse("[systems]\n[cores.x]\nother = 1\n").is_err());
        assert!(parse("[systems]\n[cores.\"../x\"]\n").is_err());
        assert!(parse("[systems]\n[cores.x]\noptions = { \"a b\" = \"1\" }\n").is_err());
        assert!(parse("[systems]\n[cores.x]\noptions = { a = 1 }\n").is_err());
        let p = parse("[systems]\n[cores.x]\noptions = { b = \"2\", a = \"1\" }\n").unwrap();
        assert_eq!(p.media_policy(&id("x")), CoreMediaPolicy::Direct);
        assert_eq!(
            p.frontend_options(&id("x")),
            [("a".into(), "1".into()), ("b".into(), "2".into())]
        );
    }

    #[test]
    fn choice() {
        let prefs = CorePreferences::builtin();
        let infos = BTreeMap::from([info("sameboy", &["gb", "gbc"]), info("zz", &["gb"])]);
        let none = BTreeMap::new();
        assert_eq!(
            choose_core(Some(GB), Some("gb"), &[], &none, &prefs),
            CoreChoice::Install(id("gambatte"))
        );
        assert_eq!(
            choose_core(Some(GB), None, &[id("mgba"), id("sameboy")], &none, &prefs),
            CoreChoice::Installed(id("sameboy"))
        );
        // Unknown system: falls back to extensions, first installed id in order.
        assert_eq!(
            choose_core(
                Some("Other"),
                Some(".GB"),
                &[id("zz"), id("sameboy")],
                &infos,
                &prefs
            ),
            CoreChoice::Installed(id("sameboy"))
        );
        assert_eq!(
            choose_core(None, Some("nes"), &[id("sameboy")], &infos, &prefs),
            CoreChoice::Unknown
        );
        assert_eq!(
            choose_core(None, None, &[], &none, &prefs),
            CoreChoice::Unknown
        );
    }

    #[test]
    fn overrides_go_first() {
        let overrides = BTreeMap::from([
            (GB.to_owned(), "sameboy".to_owned()),
            ("New - System".to_owned(), "newcore".to_owned()),
            ("Nintendo - Game Boy Color".to_owned(), "../bad".to_owned()),
        ]);
        let prefs = CorePreferences::builtin().with_overrides(&overrides);
        let names = |s: &str| -> Vec<String> {
            prefs
                .for_system(s)
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(names(GB), ["sameboy", "gambatte", "mgba"]);
        assert_eq!(names("New - System"), ["newcore"]);
        assert_eq!(names("Nintendo - Game Boy Color")[0], "gambatte");
        assert_eq!(
            choose_core(Some(GB), None, &[id("gambatte")], &BTreeMap::new(), &prefs),
            CoreChoice::Installed(id("gambatte"))
        );
        assert_eq!(
            choose_core(Some(GB), None, &[], &BTreeMap::new(), &prefs),
            CoreChoice::Install(id("sameboy"))
        );
    }

    #[test]
    fn amiga_cores() {
        let prefs = CorePreferences::builtin();
        for core in ["puae", "puae2021"] {
            let c = id(core);
            assert_eq!(prefs.media_policy(&c), CoreMediaPolicy::WorkingCopy);
            assert_eq!(prefs.media_direct(&c), ["ipf", "lha"]);
            assert_eq!(prefs.game_save_patterns(&c), ["{game}.save*.adf"]);
            let builtin = prefs.builtin_firmware(&c).unwrap();
            assert_eq!(builtin.label, "AROS (intégré au core)");
            assert!(builtin.notice.contains("AROS Public License 1.1"));
            let req = prefs.required_firmware(&c, "Commodore - Amiga").unwrap();
            assert_eq!(req.system, "Commodore - Amiga");
            assert_eq!(req.variant, FirmwareVariant::Model);
            assert_eq!(req.default, "A500");
            assert_eq!(
                req.fallback,
                Some(FirmwareFallback {
                    option: "puae_kickstart".into(),
                    value: "aros".into()
                })
            );
            // ADR 0017 §Contexte: every model preset of puae.
            let expected = [
                ("A500OG", "kick34005.A500"),
                ("A500", "kick34005.A500"),
                ("A500ECS", "kick34005.A500"),
                ("A2000OG", "kick34005.A500"),
                ("A500PLUS", "kick37175.A500"),
                ("A600OG", "kick40063.A600"),
                ("A600", "kick40063.A600"),
                ("A2000", "kick40063.A600"),
                ("A1200OG", "kick40068.A1200"),
                ("A1200", "kick40068.A1200"),
                ("A4030", "kick40068.A4000"),
                ("A4040", "kick40068.A4000"),
            ];
            assert_eq!(req.files.len(), expected.len());
            for (model, file) in expected {
                assert_eq!(req.files[model], file, "{core} {model}");
            }
            assert!(prefs.required_firmware(&c, "Atari - ST").is_none());
            assert_eq!(
                prefs.frontend_options(&c),
                [
                    (
                        "puae_floppy_write_redirect".to_owned(),
                        "disabled".to_owned()
                    ),
                    ("puae_mapper_mouse_toggle".to_owned(), "---".to_owned()),
                    ("puae_mapper_select".to_owned(), "---".to_owned()),
                    ("puae_physicalmouse".to_owned(), "enabled".to_owned()),
                ]
            );
        }
        assert!(prefs.recommended_on(&id("puae")).is_empty());
        assert_eq!(
            prefs.recommended_on(&id("puae2021")),
            [Platform::MacosX86_64]
        );
        // Cores without the 3.0 keys.
        let hatari = id("hatari");
        assert!(prefs.media_direct(&hatari).is_empty());
        assert!(prefs.game_save_patterns(&hatari).is_empty());
        assert!(prefs.builtin_firmware(&hatari).is_none());
        assert!(prefs.recommended_on(&id("unknown")).is_empty());
    }

    #[test]
    fn v3_keys_are_checked() {
        let parse = |t: &str| CorePreferences::parse(&format!("[systems]\n[cores.x]\n{t}"));
        let builtin = "builtin_firmware = { label = \"L\", notice = \"N\" }\n";
        let table = |extra: &str| {
            format!(
                "{builtin}[cores.x.required_firmware.S]\nvariant = \"region\"\ndefault = \"E\"\n{extra}\
                 [cores.x.required_firmware.S.files]\nE = \"bios_E.bin\"\nU = \"sub/bios_U.bin\"\n"
            )
        };
        let ok = parse(&table("fallback = { option = \"o\", value = \"v\" }\n")).unwrap();
        let req = ok.required_firmware(&id("x"), "S").unwrap();
        assert_eq!(req.variant, FirmwareVariant::Region);
        assert_eq!(req.files.len(), 2);
        assert!(parse(&table("")).is_ok());
        // Unknown default, unsafe path, fallback without builtin firmware, bad fallback option.
        assert!(parse(&table("").replace("default = \"E\"", "default = \"J\"")).is_err());
        assert!(parse(&table("").replace("sub/bios_U.bin", "../x")).is_err());
        assert!(parse(&table("").replace("sub/bios_U.bin", "/abs")).is_err());
        assert!(
            parse(&table("fallback = { option = \"o\", value = \"v\" }\n").replace(builtin, ""))
                .is_err()
        );
        assert!(parse(&table("fallback = { option = \"a b\", value = \"v\" }\n")).is_err());
        assert!(parse(&table("").replace("variant = \"region\"", "variant = \"os\"")).is_err());
        assert!(parse("builtin_firmware = { label = \"\", notice = \"N\" }\n").is_err());
        // media_direct: lower-cased, deduplicated, plain extensions only.
        let p = parse("media_direct = [\"IPF\", \"ipf\", \"lha\"]\n").unwrap();
        assert_eq!(p.media_direct(&id("x")), ["ipf", "lha"]);
        assert!(parse("media_direct = [\".ipf\"]\n").is_err());
        assert!(parse("media_direct = [\"\"]\n").is_err());
        // game_saves: file-name patterns only.
        assert!(parse("game_saves = [\"{game}.brm\", \"*.sav\"]\n").is_ok());
        for bad in [
            "*",
            "a/b",
            "a\\\\b",
            "../x",
            "{game}*.*",
            "{other}.sav",
            "",
            "x.",
            "c:x",
        ] {
            assert!(
                parse(&format!("game_saves = [\"{bad}\"]\n")).is_err(),
                "{bad}"
            );
        }
        // recommended_on: known OS names only.
        let p = parse("recommended_on = [\"macos\", \"linux\", \"macos\"]\n").unwrap();
        assert_eq!(
            p.recommended_on(&id("x")),
            [Platform::MacosX86_64, Platform::LinuxX86_64]
        );
        assert!(parse("recommended_on = [\"mac\"]\n").is_err());
    }

    #[test]
    fn dos_core() {
        let prefs = CorePreferences::builtin();
        let c = id("dosbox_pure");
        assert_eq!(prefs.media_policy(&c), CoreMediaPolicy::WorkingCopy);
        assert_eq!(
            prefs.media_direct(&c),
            ["zip", "dosz", "iso", "cue", "chd", "ins"]
        );
        assert_eq!(prefs.folder_policy(&c), Some(FolderPolicy::Snapshot));
        assert!(prefs.provides_vfs(&c));
        assert!(prefs.analog(&c));
        assert_eq!(
            prefs.game_save_patterns(&c),
            ["{game}.pure.zip", "{game}-*.sav"]
        );
        assert_eq!(prefs.system_caches(&c), ["DOSBoxPureMidiCache.txt"]);
        assert_eq!(
            prefs.frontend_options(&c),
            [("dosbox_pure_midi".to_owned(), "disabled".to_owned())]
        );
        assert!(prefs.builtin_firmware(&c).is_none());
        assert!(prefs.required_firmware(&c, "DOS").is_none());
        let featured = prefs.featured_options(&c, "DOS");
        assert_eq!(
            featured,
            [
                "dosbox_pure_cycles",
                "dosbox_pure_memory_size",
                "dosbox_pure_cpu_type",
                "dosbox_pure_machine",
                "dosbox_pure_sblaster_type",
                "dosbox_pure_midi",
                "dosbox_pure_keyboard_layout",
                "dosbox_pure_mouse_speed_factor",
            ]
        );
        assert!(prefs.featured_options(&c, "Commodore - Amiga").is_empty());
        assert!(prefs.featured_options(&id("unknown"), "DOS").is_empty());
        let layout = prefs.layout_option(&c).unwrap();
        assert_eq!(layout.option, "dosbox_pure_keyboard_layout");
        assert_eq!(layout.fallback, "us");
        assert_eq!(layout.map.len(), 8);
        for (locale, value) in [
            (Some("fr-FR"), "fr"),
            (Some("fr_FR.UTF-8"), "fr"),
            (Some("fr-BE"), "be"),
            (Some("fr_be"), "be"),
            (Some("fr-CH"), "sf"),
            (Some("de-CH"), "sg"),
            (Some("de-AT"), "gr"),
            (Some("en-GB"), "uk"),
            (Some("en-US"), "us"),
            (Some("es-ES"), "sp"),
            (Some("it"), "it"),
            (Some("ja-JP"), "us"),
            (Some("C"), "us"),
            (Some(""), "us"),
            (None, "us"),
        ] {
            assert_eq!(layout.value_for(locale), value, "{locale:?}");
        }
        // Cores without the 4.0 keys keep the 3.0 behaviour.
        for core in ["puae", "hatari", "genesis_plus_gx", "unknown"] {
            let c = id(core);
            assert_eq!(prefs.folder_policy(&c), None, "{core}");
            assert!(!prefs.provides_vfs(&c), "{core}");
            assert!(!prefs.analog(&c), "{core}");
            assert!(prefs.system_caches(&c).is_empty(), "{core}");
            assert!(prefs.layout_option(&c).is_none(), "{core}");
        }
    }

    #[test]
    fn v4_keys_are_checked() {
        let parse = |t: &str| CorePreferences::parse(&format!("[systems]\n[cores.x]\n{t}"));
        let p = parse(
            "folders = \"snapshot\"\nvfs = true\nanalog = true\n\
             system_caches = [\"a.txt\", \"a.txt\", \"B-c_d\"]\n\
             featured_options = { S = [\"o2\", \"o1\", \"o2\"], T = [] }\n",
        )
        .unwrap();
        let x = id("x");
        assert_eq!(p.folder_policy(&x), Some(FolderPolicy::Snapshot));
        assert!(p.provides_vfs(&x) && p.analog(&x));
        assert_eq!(p.system_caches(&x), ["a.txt", "B-c_d"]);
        assert_eq!(p.featured_options(&x, "S"), ["o2", "o1"]);
        assert!(p.featured_options(&x, "T").is_empty());
        assert!(p.featured_options(&x, "U").is_empty());
        assert!(parse("folders = \"copy\"\n").is_err());
        assert!(parse("vfs = \"yes\"\n").is_err());
        assert!(parse("analog = 1\n").is_err());
        for bad in ["../x", "a/b", "a\\\\b", ".hidden", "", "c:x", "a b"] {
            assert!(
                parse(&format!("system_caches = [\"{bad}\"]\n")).is_err(),
                "{bad}"
            );
        }
        assert!(parse("featured_options = { S = [\"a b\"] }\n").is_err());
        assert!(parse("featured_options = { S = \"o\" }\n").is_err());
        // layout_option.
        let layout = |t: &str| parse(&format!("layout_option = {{ {t} }}\n"));
        let ok =
            layout("option = \"k\", fallback = \"us\", map = { fr = \"fr\", \"pt-BR\" = \"br\" }")
                .unwrap();
        let l = ok.layout_option(&x).unwrap();
        assert_eq!(l.value_for(Some("pt_BR")), "br");
        assert_eq!(l.value_for(Some("pt-PT")), "us");
        assert!(layout("option = \"k\", fallback = \"us\"").is_ok());
        assert!(layout("option = \"a b\", fallback = \"us\"").is_err());
        assert!(layout("option = \"k\", fallback = \"\"").is_err());
        assert!(layout("option = \"k\", fallback = \" us\"").is_err());
        assert!(layout("option = \"k\"").is_err());
        assert!(layout("option = \"k\", fallback = \"us\", map = { \"fr_FR\" = \"fr\" }").is_err());
        assert!(layout("option = \"k\", fallback = \"us\", map = { \"fr-\" = \"fr\" }").is_err());
        assert!(layout("option = \"k\", fallback = \"us\", map = { fr = \"\" }").is_err());
        assert!(
            layout("option = \"k\", fallback = \"us\", map = { fr = \"fr\", FR = \"x\" }").is_err()
        );
        assert!(layout("option = \"k\", fallback = \"us\", other = 1").is_err());
    }

    /// `cores.toml` and emu-library's `systems.toml` share their system keys (libretro database names): every
    /// catalogued system has cores and every system of `cores.toml` is catalogued. Read from disk (no crate
    /// dependency between emu-assets and emu-library).
    #[test]
    fn systems_match_the_library_catalogue() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../emu-library/data/systems.toml"
        );
        let text = std::fs::read_to_string(path).unwrap();
        let catalogue: toml::Table = toml::from_str(&text).unwrap();
        let databases: std::collections::BTreeSet<&str> = catalogue["system"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["database"].as_str().unwrap())
            .collect();
        let prefs = CorePreferences::builtin();
        let ours: std::collections::BTreeSet<&str> = prefs.systems().map(|(s, _)| s).collect();
        assert_eq!(ours, databases);
        assert!(ours.contains("DOS"));
        assert!(prefs.systems().all(|(_, cores)| !cores.is_empty()));
    }
}
