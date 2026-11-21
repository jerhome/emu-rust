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
use crate::computer::computed::{ComputedContext, computed_options};
use crate::game_options::GameOptions;
use crate::{Config, DataDirs, GameContent};

/// Defaults of the model options of puae / puae2021 (core option definitions).
const MODEL_DEFAULT: &str = "auto";
const MODEL_FD_DEFAULT: &str = "A500";
const MODEL_HD_DEFAULT: &str = "A1200";

/// Result of the pre-launch check of one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchFirmware {
    /// libretro database name the table was found for.
    pub system: String,
    /// Variant the table was keyed with (resolved Amiga model); `None` = the table's default.
    pub variant: Option<String>,
    pub check: FirmwareCheck,
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
/// for floppies / hard disks). `Region` (6.0): not known yet, the table's default applies.
pub(crate) fn launch_variant(
    variant: FirmwareVariant,
    first_file: Option<&Path>,
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
        _ => None,
    }
}

/// The pre-launch check of `core` for a game whose first file is `first_file` (`None`: no game). `None`: the core
/// has neither a table nor a built-in ROM (the 1.0 `.info` check applies), or the table is invalid (logged).
pub(crate) fn check_launch_firmware(
    system_dir: &Path,
    prefs: &CorePreferences,
    core: &CoreId,
    first_file: Option<&Path>,
    global: &BTreeMap<String, String>,
    game: &BTreeMap<String, String>,
) -> Option<LaunchFirmware> {
    let system = firmware_system(prefs, core)?;
    let frontend = prefs.frontend_options(core);
    let variant = prefs
        .required_firmware(core, &system)
        .and_then(|req| launch_variant(req.variant, first_file, &frontend, global, game));
    let known = KnownFirmware::builtin();
    match check_core_firmware(system_dir, prefs, core, &system, variant.as_deref(), &known)? {
        Ok(check) => Some(LaunchFirmware {
            system,
            variant,
            check,
        }),
        Err(e) => {
            tracing::warn!("contrôle du BIOS de {core} impossible : {e}");
            None
        }
    }
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
            FirmwareDecision::FallbackFirstTime { label, wanted }
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
        .map(|f| (f.option.clone(), f.value.clone()))
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
pub(crate) fn firmware_setup(
    core_id: &str,
    first_file: Option<&Path>,
    dirs: &DataDirs,
    config: &Config,
    prefs: &CorePreferences,
    game: &mut GameOptions,
) -> FirmwareSetup {
    let Some(core) = CoreId::new(core_id) else {
        return FirmwareSetup::default();
    };
    let global = config
        .core_options
        .get(core_id)
        .cloned()
        .unwrap_or_default();
    let Some(lf) = check_launch_firmware(
        &dirs.system,
        prefs,
        &core,
        first_file,
        &global,
        &game.core_options,
    ) else {
        return FirmwareSetup::default();
    };
    tracing::info!(
        "BIOS de {core_id} ({}, variante {:?}) : {:?}",
        lf.system,
        lf.variant,
        lf.check.state
    );
    let locale = sys_locale::get_locale().unwrap_or_default();
    let extension = first_file
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let computed = computed_options(&ComputedContext {
        prefs,
        core: &core,
        system: &lf.system,
        firmware: Some(&lf.check),
        locale: &locale,
        extension: &extension,
    });
    let decision = decide(&lf.check, game.input.aros_acknowledged);
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
            launch_variant(FirmwareVariant::Model, Some(path), &[], global, game)
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
                &[],
                &fd,
                &none
            )
            .as_deref(),
            Some("A600")
        );
        assert_eq!(
            launch_variant(FirmwareVariant::Model, None, &[], &none, &none).as_deref(),
            Some("A500")
        );
        assert_eq!(
            launch_variant(FirmwareVariant::Region, Some(path), &[], &none, &none),
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
        let lf = check_launch_firmware(dir.path(), &prefs, &puae(), None, &none, &none).unwrap();
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
        assert!(check_launch_firmware(dir.path(), &prefs, &gambatte, None, &none, &none).is_none());
    }
}
