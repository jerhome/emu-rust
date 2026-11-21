//! Firmware decision before a launch, for cores with a `required_firmware` table or a built-in ROM (design v3.0
//! §2.1, §3.5.5). The check itself is `emu_assets::cores::check_core_firmware`, the single point of decision; this
//! module only finds its inputs (system, variant) and turns the result into what the library and `App::build` do.

use std::collections::BTreeMap;
use std::path::Path;

use emu_assets::cores::{
    CoreId, CorePreferences, FirmwareCheck, FirmwareState, FirmwareVariant, KnownFirmware,
    check_core_firmware,
};

use crate::computer::amiga::{
    KickstartPlan, amiga_media_kind, plan_kickstart, resolve_amiga_model,
};
use crate::computer::computed::{ComputedContext, computed_options, system_locale};
use crate::game_options::GameOptions;
use crate::{Config, DataDirs, GameContent};

/// Defaults of the model options of puae / puae2021 (core option definitions).
const MODEL_DEFAULT: &str = "auto";
const MODEL_FD_DEFAULT: &str = "A500";
const MODEL_HD_DEFAULT: &str = "A1200";

/// What the library knows of the game being launched (design v6.0 §3.5); `Default` = no game known.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GameFacts<'a> {
    /// libretro database name of the game's system (`Game::system` resolved to `database`).
    pub database: Option<&'a str>,
    /// `Game::region`.
    pub region: Option<emu_library::DiscRegion>,
}

/// Result of the pre-launch check of one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchFirmware {
    /// libretro database name the table was found for.
    pub system: String,
    /// Variant the table was keyed with (resolved Amiga model); `None` = the table's default.
    pub variant: Option<String>,
    pub check: FirmwareCheck,
    /// The `region` variant was found (always `true` for a table that is not keyed by region).
    pub variant_known: bool,
}

/// What to do with a [`LaunchFirmware`]. `Missing` and `Unreadable` are decided before `plan_kickstart` (which
/// only knows `CoreFallback` and `WrongHash`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FirmwareDecision {
    /// `Ok`, `Free`, `Unverified`, `Builtin`, or the fallback already acknowledged for this game.
    Launch,
    /// File absent, the core's built-in ROM is used; first launch of this game: warn once.
    FallbackFirstTime {
        label: String,
        wanted: String,
        /// Other files of the table present (`FirmwareCheck::others_present`: a BIOS of another region).
        others: Vec<String>,
    },
    /// Another file is under the wanted name.
    WrongHash {
        wanted: String,
        md5: [u8; 16],
    },
    /// Absent and no fallback declared.
    Missing {
        wanted: String,
    },
    Unreadable {
        wanted: String,
        error: String,
    },
}

/// The system whose `required_firmware` table applies to `core`: the first system listing the core that has
/// one. Without a table, the first system listing the core (a `builtin_firmware` core does not need it).
pub(crate) fn firmware_system(prefs: &CorePreferences, core: &CoreId) -> Option<String> {
    let mut listed = prefs.systems().filter(|(_, cores)| cores.contains(core));
    let all: Vec<&str> = listed.by_ref().map(|(s, _)| s).collect();
    all.iter()
        .find(|s| prefs.required_firmware(core, s).is_some())
        .or_else(|| all.first())
        .map(|s| (*s).to_owned())
}

/// System whose `required_firmware` table and `featured_options` apply to a game (design v6.0 §3.5, GU1):
/// `game_database` when the core lists that system (a Mega Drive game with genesis_plus_gx gets no Mega CD table
/// nor « Mega CD » section; a 32X game with picodrive gets its `Builtin`, not the Mega CD table), else
/// [`firmware_system`] (no game: computer desktop, CLI without library).
pub(crate) fn firmware_system_for(
    prefs: &CorePreferences,
    core: &CoreId,
    game_database: Option<&str>,
) -> Option<String> {
    let listed = |system: &str| {
        prefs
            .systems()
            .any(|(s, cores)| s == system && cores.contains(core))
    };
    match game_database {
        Some(db) if listed(db) => Some(db.to_owned()),
        _ => firmware_system(prefs, core),
    }
}

/// Upper bound of the zip entries examined.
const MAX_ZIP_ENTRIES: usize = 4096;

/// Opens a regular file; on Unix the open is non-blocking so a FIFO never hangs the caller, and is then refused.
fn open_regular(path: &Path) -> Option<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// Extensions of the entries of a zip (central directory only, nothing is decompressed); empty if unreadable.
fn zip_entry_extensions(path: &Path) -> Vec<String> {
    let Some(file) = open_regular(path) else {
        return Vec::new();
    };
    let Ok(mut archive) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        return Vec::new();
    };
    (0..archive.len().min(MAX_ZIP_ENTRIES))
        .filter_map(|i| {
            let entry = archive.by_index_raw(i).ok()?;
            let name = entry.name().ok()?.into_owned();
            let ext = Path::new(&name).extension()?.to_str()?.to_ascii_lowercase();
            Some(ext)
        })
        .collect()
}

/// libretro database name of the system of `first_file` for `core` when no library entry says it (CLI
/// `--core/--rom`): the systems accepting its extension (for a `.zip`, the extension of its first recognised
/// entry) that list the core; a disc header (`disc_info`) picks a `disc = true` system, otherwise the first
/// cartridge system. A file whose extension is unknown (`.7z`, odd name) never selects a system with a
/// `required_firmware` table by default: the first system of the core without table applies (a Mega Drive `.zip`
/// is not a Mega CD game). `None`: no file, or the core only lists systems with a table ([`firmware_system`]).
pub(crate) fn content_database(
    prefs: &CorePreferences,
    core: &CoreId,
    first_file: Option<&Path>,
) -> Option<String> {
    let path = first_file?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let lists_core = |s: &emu_library::SystemInfo| {
        prefs
            .systems()
            .any(|(db, cores)| db == s.database && cores.contains(core))
    };
    let candidates_of = |ext: &str| -> Vec<&emu_library::SystemInfo> {
        emu_library::systems_for_extension(ext)
            .into_iter()
            .filter(|s| lists_core(s))
            .collect()
    };
    let mut candidates = candidates_of(&ext);
    if candidates.is_empty() && ext == "zip" {
        candidates = zip_entry_extensions(path)
            .iter()
            .map(|e| candidates_of(e))
            .find(|c| !c.is_empty())
            .unwrap_or_default();
    }
    let chosen = match candidates.as_slice() {
        [] => None,
        [only] => Some(*only),
        several => {
            let disc = matches!(emu_library::disc_info(path), Ok(Some(_)));
            several
                .iter()
                .find(|s| s.disc == disc)
                .or_else(|| several.first())
                .copied()
        }
    };
    chosen.map(|s| s.database.clone()).or_else(|| {
        let plain = |db: &str| {
            prefs
                .systems()
                .any(|(s, cores)| s == db && cores.contains(core))
                && prefs.required_firmware(core, db).is_none()
        };
        // Mega Drive / 32X first (the cores that also play Mega CD), then the first plain system of the core.
        ["Sega - Mega Drive - Genesis", "Sega - 32X"]
            .into_iter()
            .find(|db| plain(db))
            .map(str::to_owned)
            .or_else(|| {
                prefs
                    .systems()
                    .find(|(db, cores)| cores.contains(core) && plain(db))
                    .map(|(db, _)| db.to_owned())
            })
    })
}

/// Variant key of a `variant = "region"` table (design v6.0 §2.2, GU1): `region` (`Game::region`) when known,
/// else the header probe of `first_file` (`emu_library::disc_info`: base not rescanned since v5), else its name tag
/// (`region_from_name`); `None` = unknown (the table's default applies and `variant_known` is `false`).
pub(crate) fn region_variant(
    region: Option<emu_library::DiscRegion>,
    first_file: Option<&Path>,
) -> Option<&'static str> {
    if let Some(region) = region {
        return Some(region.key());
    }
    let first_file = first_file?;
    let probed = emu_library::disc_info(first_file)
        .ok()
        .flatten()
        .and_then(|info| info.region);
    let named = || {
        first_file
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(emu_library::region_from_name)
    };
    probed.or_else(named).map(emu_library::DiscRegion::key)
}

/// Effective value of a core option before the computed layer: game > config > `cores.toml` > `default`.
fn option_value<'a>(
    key: &str,
    frontend: &'a [(String, String)],
    global: &'a BTreeMap<String, String>,
    game: &'a BTreeMap<String, String>,
    default: &'a str,
) -> &'a str {
    game.get(key)
        .or_else(|| global.get(key))
        .map(String::as_str)
        .or_else(|| {
            frontend
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        })
        .unwrap_or(default)
}

/// Variant of the table for this launch. `Model`: the resolved Amiga model (option > tags of the path > model
/// for floppies / hard disks). `Region` (6.0): [`region_variant`], `None` = the table's default applies.
pub(crate) fn launch_variant(
    variant: FirmwareVariant,
    first_file: Option<&Path>,
    region: Option<emu_library::DiscRegion>,
    frontend: &[(String, String)],
    global: &BTreeMap<String, String>,
    game: &BTreeMap<String, String>,
) -> Option<String> {
    match variant {
        FirmwareVariant::Model => {
            let path = first_file.unwrap_or(Path::new(""));
            let get = |key, default| option_value(key, frontend, global, game, default);
            Some(resolve_amiga_model(
                path,
                amiga_media_kind(path),
                get("puae_model", MODEL_DEFAULT),
                get("puae_model_fd", MODEL_FD_DEFAULT),
                get("puae_model_hd", MODEL_HD_DEFAULT),
            ))
        }
        FirmwareVariant::Region => region_variant(region, first_file).map(str::to_owned),
    }
}

/// The pre-launch check of `core` for a game whose first file is `first_file` (`None`: no game). `None`: the core
/// has neither a table nor a built-in ROM (the 1.0 `.info` check applies), or the table is invalid (logged).
pub(crate) fn check_launch_firmware(
    system_dir: &Path,
    prefs: &CorePreferences,
    core: &CoreId,
    first_file: Option<&Path>,
    facts: GameFacts<'_>,
    global: &BTreeMap<String, String>,
    game: &BTreeMap<String, String>,
) -> Option<LaunchFirmware> {
    let system = firmware_system_for(prefs, core, facts.database)?;
    let frontend = prefs.frontend_options(core);
    let table_variant = prefs.required_firmware(core, &system).map(|r| r.variant);
    let variant = table_variant
        .and_then(|v| launch_variant(v, first_file, facts.region, &frontend, global, game));
    let variant_known = table_variant != Some(FirmwareVariant::Region) || variant.is_some();
    let known = KnownFirmware::builtin();
    match check_core_firmware(system_dir, prefs, core, &system, variant.as_deref(), &known)? {
        Ok(check) => Some(LaunchFirmware {
            system,
            variant,
            check,
            variant_known,
        }),
        Err(e) => {
            tracing::warn!("contrôle du BIOS de {core} impossible : {e}");
            None
        }
    }
}

/// [`decide`] after the gate of the library (design v6.0 §2.2): an unknown region with another file of the table
/// present launches without notice, as in 2.1. Used by the library dialog and `App::build` (OSD notice).
pub(crate) fn decide_launch(
    lf: &LaunchFirmware,
    core: &str,
    aros_acknowledged: bool,
) -> FirmwareDecision {
    use crate::library::firmware_gate::{FirmwareGate, firmware_gate};
    if firmware_gate(&lf.check, lf.variant_known, core) == FirmwareGate::Launch {
        return FirmwareDecision::Launch;
    }
    decide(&lf.check, aros_acknowledged)
}

/// `aros_acknowledged`: the fallback warning was already shown for this game (`GameInput::aros_acknowledged`).
pub(crate) fn decide(check: &FirmwareCheck, aros_acknowledged: bool) -> FirmwareDecision {
    let wanted = check.wanted.clone().unwrap_or_default();
    match &check.state {
        FirmwareState::Missing => return FirmwareDecision::Missing { wanted },
        FirmwareState::Unreadable(e) => {
            return FirmwareDecision::Unreadable {
                wanted,
                error: e.clone(),
            };
        }
        _ => {}
    }
    match plan_kickstart(check, aros_acknowledged) {
        KickstartPlan::Original | KickstartPlan::Aros { first_time: false } => {
            FirmwareDecision::Launch
        }
        KickstartPlan::Aros { first_time: true } => {
            let label = match &check.state {
                FirmwareState::CoreFallback { label } => label.clone(),
                _ => String::new(),
            };
            FirmwareDecision::FallbackFirstTime {
                label,
                wanted,
                others: check.others_present.clone(),
            }
        }
        KickstartPlan::WrongHash { md5 } => FirmwareDecision::WrongHash { wanted, md5 },
    }
}

/// Option and value that switch the core to its built-in ROM (« Utiliser AROS »), when declared.
pub(crate) fn fallback_option(
    prefs: &CorePreferences,
    core: &CoreId,
    system: &str,
) -> Option<(String, String)> {
    prefs
        .required_firmware(core, system)
        .and_then(|r| r.fallback.as_ref())
        .and_then(|f| f.option_value())
        .map(|(option, value)| (option.to_owned(), value.to_owned()))
}

/// What a launch decides about firmware before loading the core (design v3.0 §2.1, §3.5.4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FirmwareSetup {
    /// Computed defaults (`puae_kickstart = "aros"` when the Kickstart is missing): between `cores.toml` and the
    /// configuration, never written.
    pub computed: Vec<(String, String)>,
    /// OSD message for the start of the game (fallback first time, wrong or missing file).
    pub notice: Option<String>,
    /// The fallback warning was given now: `aros_acknowledged` was set in the game's options.
    pub acknowledged_now: bool,
}

/// [`FirmwareSetup`] of a launch, `game` being this game's options (updated, not saved). Used by `App::build`.
/// Without facts about the game: see [`firmware_setup_for`].
pub(crate) fn firmware_setup(
    core_id: &str,
    first_file: Option<&Path>,
    dirs: &DataDirs,
    config: &Config,
    prefs: &CorePreferences,
    game: &mut GameOptions,
) -> FirmwareSetup {
    firmware_setup_for(
        core_id,
        first_file,
        GameFacts::default(),
        dirs,
        config,
        prefs,
        game,
    )
}

/// [`firmware_setup`] for a game whose system and region are known (`GameFacts`): a Mega Drive game under
/// genesis_plus_gx gets no Mega CD table. Used by `App::build`.
pub(crate) fn firmware_setup_for(
    core_id: &str,
    first_file: Option<&Path>,
    facts: GameFacts<'_>,
    dirs: &DataDirs,
    config: &Config,
    prefs: &CorePreferences,
    game: &mut GameOptions,
) -> FirmwareSetup {
    let Some(core) = CoreId::new(core_id) else {
        return FirmwareSetup::default();
    };
    // No library entry (CLI, tools): the system comes from the file, never from the core's first table.
    let from_file = if facts.database.is_none() {
        content_database(prefs, &core, first_file)
    } else {
        None
    };
    let facts = GameFacts {
        database: facts.database.or(from_file.as_deref()),
        ..facts
    };
    let global = config
        .core_options
        .get(core_id)
        .cloned()
        .unwrap_or_default();
    let locale = system_locale();
    let extension = first_file
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let Some(lf) = check_launch_firmware(
        &dirs.system,
        prefs,
        &core,
        first_file,
        facts,
        &global,
        &game.core_options,
    ) else {
        // No firmware decision, but the other computed defaults (DOS keyboard layout) still apply.
        let system = firmware_system_for(prefs, &core, facts.database).unwrap_or_default();
        return FirmwareSetup {
            computed: computed_options(&ComputedContext {
                prefs,
                core: &core,
                system: &system,
                firmware: None,
                locale: &locale,
                extension: &extension,
            }),
            ..FirmwareSetup::default()
        };
    };
    tracing::info!(
        "BIOS de {core_id} ({}, variante {:?}) : {:?}",
        lf.system,
        lf.variant,
        lf.check.state
    );
    let computed = computed_options(&ComputedContext {
        prefs,
        core: &core,
        system: &lf.system,
        firmware: Some(&lf.check),
        locale: &locale,
        extension: &extension,
    });
    // Read only here (CLI, `App::build`): the library records the (core, file) key when its dialog is answered.
    let acknowledged = crate::firmware_hints::fallback_seen(
        &config.interface.firmware_hints_seen,
        core_id,
        lf.check.wanted.as_deref(),
        game.input.aros_acknowledged,
    );
    let decision = decide_launch(&lf, core_id, acknowledged);
    let acknowledged_now = matches!(decision, FirmwareDecision::FallbackFirstTime { .. });
    if acknowledged_now {
        // Warned once per game (design v3.0 §9 q.3): the library dialog already did it, or this OSD (CLI).
        game.input.aros_acknowledged = true;
    }
    FirmwareSetup {
        computed,
        notice: crate::ui::text::firmware_launch_notice(&decision),
        acknowledged_now,
    }
}

/// [`firmware_setup`] for a game whose options live in `options_path` (`ResolvedGame::options_path`): loads them,
/// and writes `aros_acknowledged` back when the warning is given now. The same decision as a launch, without a
/// window (tests, tools).
pub fn prepare_launch_firmware(
    core_id: &str,
    content: &GameContent,
    dirs: &DataDirs,
    config: &Config,
    options_path: &Path,
) -> FirmwareSetup {
    let prefs = crate::cores_ui::preferences(&config.cores);
    let mut loaded = GameOptions::load(options_path);
    let setup = firmware_setup(
        core_id,
        content.first(),
        dirs,
        config,
        &prefs,
        &mut loaded.options,
    );
    if setup.acknowledged_now
        && let Err(e) = loaded.options.save(options_path, loaded.locked)
    {
        tracing::warn!("options du jeu non enregistrées : {e}");
    }
    setup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_extensions_refuse_fifo_and_bound_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("pipe.zip");
        if std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success())
        {
            assert!(zip_entry_extensions(&fifo).is_empty());
        }
        let path = tmp.path().join("many.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for i in 0..MAX_ZIP_ENTRIES + 10 {
            writer.start_file(format!("f{i}.md"), options).unwrap();
        }
        writer.finish().unwrap();
        assert_eq!(zip_entry_extensions(&path).len(), MAX_ZIP_ENTRIES);
    }

    fn puae() -> CoreId {
        CoreId::new("puae").unwrap()
    }

    fn opts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn amiga_cores_use_the_amiga_table() {
        let prefs = CorePreferences::builtin();
        assert_eq!(
            firmware_system(&prefs, &puae()).as_deref(),
            Some("Commodore - Amiga")
        );
        let p2021 = CoreId::new("puae2021").unwrap();
        assert_eq!(
            firmware_system(&prefs, &p2021).as_deref(),
            Some("Commodore - Amiga")
        );
        assert_eq!(firmware_system(&prefs, &CoreId::new("nope").unwrap()), None);
    }

    #[test]
    fn model_layers_game_over_config_over_tags() {
        let path = Path::new("roms/amiga/Game (AGA).adf");
        let none = BTreeMap::new();
        let v = |global: &BTreeMap<String, String>, game: &BTreeMap<String, String>| {
            launch_variant(FirmwareVariant::Model, Some(path), None, &[], global, game)
        };
        assert_eq!(v(&none, &none).as_deref(), Some("A1200"));
        let global = opts(&[("puae_model", "A600")]);
        assert_eq!(v(&global, &none).as_deref(), Some("A600"));
        let game = opts(&[("puae_model", "A500PLUS")]);
        assert_eq!(v(&global, &game).as_deref(), Some("A500PLUS"));
        let fd = opts(&[("puae_model_fd", "A600")]);
        assert_eq!(
            launch_variant(
                FirmwareVariant::Model,
                Some(Path::new("Game.adf")),
                None,
                &[],
                &fd,
                &none
            )
            .as_deref(),
            Some("A600")
        );
        assert_eq!(
            launch_variant(FirmwareVariant::Model, None, None, &[], &none, &none).as_deref(),
            Some("A500")
        );
        assert_eq!(
            launch_variant(FirmwareVariant::Region, Some(path), None, &[], &none, &none),
            None
        );
    }

    #[test]
    fn empty_system_dir_falls_back_to_aros_and_warns_once() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = CorePreferences::builtin();
        let none = BTreeMap::new();
        let lf = check_launch_firmware(
            dir.path(),
            &prefs,
            &puae(),
            Some(Path::new("Game.adf")),
            GameFacts::default(),
            &none,
            &none,
        )
        .expect("puae has a table");
        assert_eq!(lf.variant.as_deref(), Some("A500"));
        assert_eq!(lf.check.wanted.as_deref(), Some("kick34005.A500"));
        assert!(matches!(
            decide(&lf.check, false),
            FirmwareDecision::FallbackFirstTime { .. }
        ));
        assert_eq!(decide(&lf.check, true), FirmwareDecision::Launch);
        assert_eq!(
            fallback_option(&prefs, &puae(), &lf.system),
            Some(("puae_kickstart".to_owned(), "aros".to_owned()))
        );
    }

    #[test]
    fn present_file_launches_and_wrong_one_asks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("kick34005.A500"), b"not a kickstart").unwrap();
        let prefs = CorePreferences::builtin();
        let none = BTreeMap::new();
        let lf = check_launch_firmware(
            dir.path(),
            &prefs,
            &puae(),
            None,
            GameFacts::default(),
            &none,
            &none,
        )
        .unwrap();
        // The md5 is known from System.dat: a fake file is a wrong hash.
        assert!(matches!(
            decide(&lf.check, true),
            FirmwareDecision::WrongHash { .. }
        ));
    }

    #[test]
    fn missing_and_unreadable_are_decided_first() {
        let check = |state| FirmwareCheck {
            wanted: Some("x.rom".into()),
            state,
            others_present: Vec::new(),
        };
        assert_eq!(
            decide(&check(FirmwareState::Missing), true),
            FirmwareDecision::Missing {
                wanted: "x.rom".into()
            }
        );
        assert!(matches!(
            decide(&check(FirmwareState::Unreadable("e".into())), true),
            FirmwareDecision::Unreadable { .. }
        ));
        assert_eq!(
            decide(&check(FirmwareState::Ok), false),
            FirmwareDecision::Launch
        );
        assert_eq!(
            decide(
                &check(FirmwareState::Builtin {
                    label: "ROM".into()
                }),
                false
            ),
            FirmwareDecision::Launch
        );
    }

    #[test]
    fn cores_without_table_keep_the_info_check() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = CorePreferences::builtin();
        let none = BTreeMap::new();
        let gambatte = CoreId::new("gambatte").unwrap();
        assert!(
            check_launch_firmware(
                dir.path(),
                &prefs,
                &gambatte,
                None,
                GameFacts::default(),
                &none,
                &none
            )
            .is_none()
        );
    }

    const MEGA_DRIVE: &str = "Sega - Mega Drive - Genesis";
    const MEGA_CD: &str = "Sega - Mega-CD - Sega CD";

    fn facts(database: &str) -> GameFacts<'_> {
        GameFacts {
            database: Some(database),
            region: None,
        }
    }

    #[test]
    fn firmware_system_follows_the_game() {
        let prefs = CorePreferences::builtin();
        let gpgx = CoreId::new("genesis_plus_gx").unwrap();
        let pico = CoreId::new("picodrive").unwrap();
        for core in [&gpgx, &pico] {
            assert_eq!(
                firmware_system_for(&prefs, core, Some(MEGA_DRIVE)).as_deref(),
                Some(MEGA_DRIVE)
            );
            assert_eq!(
                firmware_system_for(&prefs, core, Some(MEGA_CD)).as_deref(),
                Some(MEGA_CD)
            );
        }
        assert_eq!(
            firmware_system_for(&prefs, &pico, Some("Sega - 32X")).as_deref(),
            Some("Sega - 32X")
        );
        // A system the core does not list, or no game: the table system.
        assert_eq!(
            firmware_system_for(&prefs, &puae(), Some(MEGA_DRIVE)).as_deref(),
            Some("Commodore - Amiga")
        );
        assert_eq!(
            firmware_system_for(&prefs, &puae(), None).as_deref(),
            Some("Commodore - Amiga")
        );
    }

    #[test]
    fn mega_drive_and_32x_games_ask_for_no_cd_bios() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = CorePreferences::builtin();
        let none = BTreeMap::new();
        let gpgx = CoreId::new("genesis_plus_gx").unwrap();
        let pico = CoreId::new("picodrive").unwrap();
        let rom = Some(Path::new("Sonic.md"));
        assert!(
            check_launch_firmware(
                dir.path(),
                &prefs,
                &gpgx,
                rom,
                facts(MEGA_DRIVE),
                &none,
                &none
            )
            .is_none()
        );
        let lf = check_launch_firmware(
            dir.path(),
            &prefs,
            &pico,
            Some(Path::new("Doom.32x")),
            facts("Sega - 32X"),
            &none,
            &none,
        )
        .expect("picodrive has built-in BIOS");
        assert_eq!(lf.check.wanted, None);
        assert_eq!(decide(&lf.check, false), FirmwareDecision::Launch);
        let lf = check_launch_firmware(
            dir.path(),
            &prefs,
            &pico,
            rom,
            facts(MEGA_DRIVE),
            &none,
            &none,
        )
        .expect("builtin");
        assert_eq!(decide(&lf.check, false), FirmwareDecision::Launch);
    }

    #[test]
    fn mega_cd_game_wants_the_bios_of_its_region() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = CorePreferences::builtin();
        let none = BTreeMap::new();
        let gpgx = CoreId::new("genesis_plus_gx").unwrap();
        let ask = |region| {
            let facts = GameFacts {
                database: Some(MEGA_CD),
                region,
            };
            check_launch_firmware(
                dir.path(),
                &prefs,
                &gpgx,
                Some(Path::new("/nonexistent/Dune.cue")),
                facts,
                &none,
                &none,
            )
            .expect("table")
        };
        let eu = ask(Some(emu_library::DiscRegion::Europe));
        assert_eq!(eu.variant.as_deref(), Some("europe"));
        assert_eq!(eu.check.wanted.as_deref(), Some("bios_CD_E.bin"));
        assert!(eu.variant_known);
        let unknown = ask(None);
        assert_eq!(unknown.variant, None);
        assert!(!unknown.variant_known);
        assert_eq!(unknown.check.wanted.as_deref(), Some("bios_CD_U.bin"));
    }

    #[test]
    fn region_variant_order_is_base_then_probe_then_name() {
        use emu_library::DiscRegion as R;
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("Dune (Japan).iso");
        let mut head = vec![0u8; 0x220];
        head[..14].copy_from_slice(b"SEGADISCSYSTEM");
        head[0x20B] = 0x64; // Europe
        std::fs::write(&iso, &head).unwrap();
        assert_eq!(region_variant(Some(R::Usa), Some(&iso)), Some("usa"));
        let probed = region_variant(None, Some(&iso));
        assert!(matches!(probed, Some("europe" | "usa" | "japan")));
        let named = dir.path().join("Dune (Japan).chd");
        std::fs::write(&named, b"x").unwrap();
        assert_eq!(region_variant(None, Some(&named)), Some("japan"));
        assert_eq!(
            region_variant(None, Some(Path::new("/nonexistent/x.cue"))),
            None
        );
        assert_eq!(region_variant(None, None), None);
    }

    #[test]
    fn header_wins_over_a_contradicting_name_tag() {
        // Ported from the 2.1 `megacd::region_of`: the core reads the header, so does the check.
        use emu_library::DiscRegion as R;
        let dir = tempfile::tempdir().unwrap();
        let iso = |name: &str, byte: u8| {
            let mut data = vec![0u8; 0x800];
            data[..14].copy_from_slice(b"SEGADISCSYSTEM");
            data[0x20B] = byte;
            let path = dir.path().join(name);
            std::fs::write(&path, data).unwrap();
            path
        };
        assert_eq!(
            region_variant(None, Some(&iso("Jeu (USA).iso", 0x64))),
            Some(R::Europe.key())
        );
        assert_eq!(
            region_variant(None, Some(&iso("Jeu.iso", 0xA1))),
            Some(R::Japan.key())
        );
        let chd = dir.path().join("Dune.chd");
        std::fs::write(&chd, b"MComprHD").unwrap();
        assert_eq!(region_variant(None, Some(&chd)), None);
    }

    #[test]
    fn unknown_region_with_another_bios_launches_without_notice() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::under(tmp.path());
        std::fs::create_dir_all(&dirs.system).unwrap();
        std::fs::write(dirs.system.join("bios_CD_E.bin"), b"bios").unwrap();
        let prefs = CorePreferences::builtin();
        let none = BTreeMap::new();
        let gpgx = CoreId::new("genesis_plus_gx").unwrap();
        let disc = Some(Path::new("/nonexistent/Dune.chd"));
        let facts = |region| GameFacts {
            database: Some(MEGA_CD),
            region,
        };
        let lf = |region| {
            check_launch_firmware(
                &dirs.system,
                &prefs,
                &gpgx,
                disc,
                facts(region),
                &none,
                &none,
            )
            .unwrap()
        };
        assert_eq!(
            decide_launch(&lf(None), "genesis_plus_gx", false),
            FirmwareDecision::Launch
        );
        assert_eq!(
            decide_launch(
                &lf(Some(emu_library::DiscRegion::Usa)),
                "genesis_plus_gx",
                false
            ),
            FirmwareDecision::Missing {
                wanted: "bios_CD_U.bin".into()
            }
        );
        // `App::build` gives no « BIOS manquant » notice for the unknown region either.
        let mut options = GameOptions::default();
        let setup = firmware_setup_for(
            "genesis_plus_gx",
            disc,
            facts(None),
            &dirs,
            &Config::default(),
            &prefs,
            &mut options,
        );
        assert_eq!(setup.notice, None);
        let setup = firmware_setup_for(
            "genesis_plus_gx",
            disc,
            facts(Some(emu_library::DiscRegion::Japan)),
            &dirs,
            &Config::default(),
            &prefs,
            &mut options,
        );
        assert!(setup.notice.is_some_and(|n| n.contains("bios_CD_J.bin")));
    }

    #[test]
    fn cli_zip_never_asks_for_the_cd_bios() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::under(tmp.path());
        std::fs::create_dir_all(&dirs.system).unwrap();
        let prefs = CorePreferences::builtin();
        let zip_of = |name: &str, entry: &str| {
            let path = tmp.path().join(name);
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            w.start_file(entry, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"rom").unwrap();
            w.finish().unwrap();
            path
        };
        let gpgx = CoreId::new("genesis_plus_gx").unwrap();
        let sonic = zip_of("Sonic.zip", "Sonic.md");
        assert_eq!(
            content_database(&prefs, &gpgx, Some(&sonic)).as_deref(),
            Some(MEGA_DRIVE)
        );
        let doom = zip_of("Doom.zip", "Doom.32x");
        let pico = CoreId::new("picodrive").unwrap();
        assert_eq!(
            content_database(&prefs, &pico, Some(&doom)).as_deref(),
            Some("Sega - 32X")
        );
        let setup = |core: &str, path: &Path| {
            firmware_setup(
                core,
                Some(path),
                &dirs,
                &Config::default(),
                &prefs,
                &mut GameOptions::default(),
            )
        };
        assert_eq!(setup("genesis_plus_gx", &sonic).notice, None);
        assert_eq!(setup("picodrive", &doom).notice, None);
        let missing = Path::new("/nonexistent/Sonic.7z");
        assert_eq!(setup("genesis_plus_gx", missing).notice, None);
        let cue = setup("genesis_plus_gx", Path::new("/nonexistent/Dune.cue"));
        assert!(cue.notice.is_some_and(|n| n.contains("bios_CD_")));
    }

    #[test]
    fn cli_content_finds_the_system_of_the_file() {
        let prefs = CorePreferences::builtin();
        let gpgx = CoreId::new("genesis_plus_gx").unwrap();
        let pico = CoreId::new("picodrive").unwrap();
        let db = |core, path: &str| content_database(&prefs, core, Some(Path::new(path)));
        assert_eq!(db(&gpgx, "/r/Sonic.md").as_deref(), Some(MEGA_DRIVE));
        assert_eq!(db(&gpgx, "/r/Dune.cue").as_deref(), Some(MEGA_CD));
        assert_eq!(db(&gpgx, "/r/Dune.CHD").as_deref(), Some(MEGA_CD));
        assert_eq!(db(&pico, "/r/Doom.32x").as_deref(), Some("Sega - 32X"));
        // Unknown extension: the first system of the core without BIOS table, never the Mega CD.
        assert_eq!(db(&gpgx, "/r/x.zzz").as_deref(), Some(MEGA_DRIVE));
        assert_eq!(db(&gpgx, "/r/Sonic.zip").as_deref(), Some(MEGA_DRIVE));
        assert_eq!(db(&gpgx, "/r/Sonic.7z").as_deref(), Some(MEGA_DRIVE));
        let pdb = db(&pico, "/r/Doom.zip");
        assert!(pdb.is_some() && pdb.as_deref() != Some(MEGA_CD));
        assert_eq!(content_database(&prefs, &gpgx, None), None);
        // A `.bin` cartridge stays a cartridge (never the Mega CD table).
        let bin = db(&gpgx, "/nonexistent/Sonic.bin");
        assert_ne!(bin.as_deref(), Some(MEGA_CD));
    }
}
