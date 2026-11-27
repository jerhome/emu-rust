//! From a game (system + extension) to a verified core path (ADR 0013 §6-8, design §2.3).
//!
//! Nothing here downloads anything: a missing core is reported as `NeedsInstall` and the screen decides. BIOS
//! files are only checked, never fetched.

use std::collections::BTreeMap;
use std::path::PathBuf;

use emu_assets::cores::{
    CoreChoice, CoreError, CoreId, CoreInfo, CoreManager, CorePreferences, FirmwareState,
    FirmwareStatus, RenderNeed, choose_core,
};

use super::text;

#[derive(Debug)]
pub(crate) enum LaunchCheck {
    /// Verified path; the core is marked in use. `warnings`: optional BIOS files that are missing or wrong
    /// (shown as an OSD message).
    Ready {
        id: CoreId,
        path: PathBuf,
        warnings: Vec<FirmwareStatus>,
    },
    /// No suitable core installed: propose installing this one (screen « Cores », job install).
    NeedsInstall(CoreId),
    /// Required BIOS files missing or wrong: ask « lancer quand même / annuler ». The core is marked in use;
    /// call [`release`] if the user cancels.
    FirmwareMissing {
        id: CoreId,
        path: PathBuf,
        missing: Vec<FirmwareStatus>,
    },
    /// No known core for this system.
    NoCore,
    /// The core cannot be loaded (altered file, missing…). Never in use.
    Failed(CoreError),
    /// Custom system whose core needs hardware rendering (planned for 9.0; design 8.0 §0.2).
    HardwareRender { id: CoreId },
}

/// Steps before the game of a custom system starts, in order (design 8.0 §2.5, ADR 0025 §6, §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomStep {
    AcceptContent,
    UntestedWarning,
    DryRun,
    Launch,
}

/// What the user and the cache already know about a (core, system) pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CustomLaunchState {
    /// The core is in `cores.toml` or the user accepted its unverified content.
    pub content_accepted: bool,
    /// « Ça marche chez moi » for the pair (core, system).
    pub works_for_me: bool,
    /// This game already passed the dry run with this core file.
    pub dry_run_passed: bool,
}

/// Steps to run for `system` played with `core`; empty for an official system. « Ça marche chez moi » skips the
/// warning and the dry run (the user vouches for the pair); a game that passed the dry run is not tried again and
/// its warning was shown at that first launch.
pub(crate) fn custom_launch_steps(
    system: &emu_library::SystemInfo,
    _core: &CoreId,
    state: &CustomLaunchState,
) -> Vec<CustomStep> {
    if !system.is_custom() {
        return Vec::new();
    }
    let mut steps = Vec::new();
    if !state.content_accepted {
        steps.push(CustomStep::AcceptContent);
    }
    if !state.works_for_me && !state.dry_run_passed {
        steps.push(CustomStep::UntestedWarning);
        steps.push(CustomStep::DryRun);
    }
    steps.push(CustomStep::Launch);
    steps
}

impl LaunchCheck {
    /// French message for the outcomes that stop the launch.
    pub fn blocking_message(&self) -> Option<String> {
        match self {
            LaunchCheck::NoCore => Some(text::no_core_for_system().to_owned()),
            LaunchCheck::Failed(e) => Some(text::error(e)),
            LaunchCheck::HardwareRender { id } => Some(text::hardware_render(id.as_str())),
            _ => None,
        }
    }
}

/// `HardwareRender` when the `.info` of `id` announces hardware rendering (a custom system: planned for 9.0).
pub(crate) fn hardware_check(info: Option<&CoreInfo>, id: &CoreId) -> Option<LaunchCheck> {
    matches!(info?.render, RenderNeed::Hardware(_))
        .then(|| LaunchCheck::HardwareRender { id: id.clone() })
}

/// The BIOS files the user told emu-rust to ignore for a custom system (`ignored_firmware` of its definition)
/// no longer stop a launch: a `FirmwareMissing` left with nothing else becomes `Ready`.
pub(crate) fn without_ignored(check: LaunchCheck, ignored: &[String]) -> LaunchCheck {
    if ignored.is_empty() {
        return check;
    }
    match check {
        LaunchCheck::FirmwareMissing { id, path, missing } => {
            let missing: Vec<FirmwareStatus> = missing
                .into_iter()
                .filter(|f| !ignored.contains(&f.spec.path))
                .collect();
            if missing.is_empty() {
                LaunchCheck::Ready {
                    id,
                    path,
                    warnings: Vec::new(),
                }
            } else {
                LaunchCheck::FirmwareMissing { id, path, missing }
            }
        }
        LaunchCheck::Ready { id, path, warnings } => LaunchCheck::Ready {
            id,
            path,
            warnings: warnings
                .into_iter()
                .filter(|f| !ignored.contains(&f.spec.path))
                .collect(),
        },
        other => other,
    }
}

fn is_problem(f: &FirmwareStatus) -> bool {
    matches!(
        f.state,
        FirmwareState::Missing | FirmwareState::WrongHash { .. }
    )
}

/// Marks `id` in use, verifies its hash and checks its BIOS files. An altered core is never returned.
pub(crate) fn prepare_launch(manager: &CoreManager, id: &CoreId) -> LaunchCheck {
    // Marked before verifying so the file cannot be replaced between the check and the load.
    manager.set_in_use(Some(id.clone()));
    let path = match manager.verified_path(id) {
        Ok(p) => p,
        Err(e) => {
            manager.set_in_use(None);
            return LaunchCheck::Failed(e);
        }
    };
    let (missing, warnings): (Vec<_>, Vec<_>) = manager
        .firmware_status(id)
        .into_iter()
        .filter(is_problem)
        .partition(|f| !f.spec.optional);
    if missing.is_empty() {
        LaunchCheck::Ready {
            id: id.clone(),
            path,
            warnings,
        }
    } else {
        LaunchCheck::FirmwareMissing {
            id: id.clone(),
            path,
            missing,
        }
    }
}

/// Libretro database whose core list (`cores.toml`) names the cores able to emulate the 32X.
pub(crate) const SYSTEM_32X: &str = "Sega - 32X";
/// Fallback when `cores.toml` lists no 32X core.
const CORE_32X: &str = "picodrive";

/// What the core choice needs to know about a game beyond its system and extension.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GameNeeds<'a> {
    /// Mega CD + 32X disc (`Game::needs_32x`): only a core of [`SYSTEM_32X`] runs it (genesis_plus_gx does not
    /// emulate the 32X).
    pub needs_32x: bool,
    /// `[cores.preferred]` of the game's system as written by the user (not the built-in order).
    pub explicit: Option<&'a CoreId>,
}

/// `choose_core` plus the per-game constraints: a 32X disc gets a 32X-capable core, the user's explicit choice
/// first when it is capable; an incapable explicit choice is overridden with a warning in the log. Other games
/// (Mega CD included) are chosen exactly as before.
pub(crate) fn choose_game_core(
    system: Option<&str>,
    extension: Option<&str>,
    installed: &[CoreId],
    infos: &BTreeMap<CoreId, CoreInfo>,
    prefs: &CorePreferences,
    needs: GameNeeds<'_>,
) -> CoreChoice {
    if !needs.needs_32x {
        return choose_core(system, extension, installed, infos, prefs);
    }
    // The user's `[cores.preferred] 32x` may name a core without 32X: only the built-in list (or a core unknown
    // to the whole built-in table, i.e. user-declared) counts as capable. User order is kept among those.
    let builtin = CorePreferences::builtin();
    let builtin_32x = builtin.for_system(SYSTEM_32X);
    let known = |id: &CoreId| builtin.systems().any(|(_, cores)| cores.contains(id));
    let capable: Vec<&CoreId> = prefs
        .for_system(SYSTEM_32X)
        .iter()
        .filter(|id| builtin_32x.contains(id) || !known(id))
        .collect();
    let mut order: Vec<CoreId> = Vec::new();
    match needs.explicit {
        Some(id) if capable.contains(&id) => order.push(id.clone()),
        Some(id) => tracing::warn!("{}", text::not_32x_capable(id.as_str())),
        None => {}
    }
    for id in capable {
        if !order.contains(id) {
            order.push(id.clone());
        }
    }
    if order.is_empty() {
        order.extend(CoreId::new(CORE_32X));
    }
    match order.iter().find(|id| installed.contains(id)) {
        Some(id) => CoreChoice::Installed(id.clone()),
        None => CoreChoice::Install(order[0].clone()),
    }
}

/// Picks the core for a game (`system` = libretro database name, `extension` without dot) then prepares it.
pub(crate) fn plan_launch(
    manager: &CoreManager,
    prefs: &CorePreferences,
    system: Option<&str>,
    extension: Option<&str>,
    needs: GameNeeds<'_>,
) -> LaunchCheck {
    let installed: Vec<CoreId> = manager.installed().into_iter().map(|c| c.id).collect();
    match choose_game_core(
        system,
        extension,
        &installed,
        &manager.infos(),
        prefs,
        needs,
    ) {
        CoreChoice::Installed(id) => prepare_launch(manager, &id),
        CoreChoice::Install(id) => LaunchCheck::NeedsInstall(id),
        CoreChoice::Unknown => LaunchCheck::NoCore,
    }
}

/// The game is over (or the user cancelled): the core may be updated or removed again.
pub(crate) fn release(manager: &CoreManager) {
    manager.set_in_use(None);
}

#[cfg(test)]
mod steps_tests {
    use emu_library::{SystemInfo, SystemOrigin};

    use super::*;

    fn system(origin: SystemOrigin) -> SystemInfo {
        SystemInfo {
            id: "acme".to_owned(),
            name: "Acme".to_owned(),
            extensions: vec!["acm".to_owned()],
            header: None,
            database: String::new(),
            screenscraper: None,
            igdb: None,
            kind: emu_library::SystemKind::Console,
            dats: Vec::new(),
            folder_extensions: Vec::new(),
            folder_games: false,
            default_input: emu_library::InputDefault::Keyboard,
            disc: false,
            group_discs: true,
            origin,
        }
    }

    fn core() -> CoreId {
        CoreId::new("acme_core").unwrap()
    }

    #[test]
    fn an_official_system_has_no_step() {
        let steps = custom_launch_steps(
            &system(SystemOrigin::Official),
            &core(),
            &CustomLaunchState::default(),
        );
        assert!(steps.is_empty());
    }

    #[test]
    fn a_first_launch_accepts_warns_tries_then_launches() {
        let steps = custom_launch_steps(
            &system(SystemOrigin::Custom),
            &core(),
            &CustomLaunchState::default(),
        );
        assert_eq!(
            steps,
            [
                CustomStep::AcceptContent,
                CustomStep::UntestedWarning,
                CustomStep::DryRun,
                CustomStep::Launch
            ]
        );
    }

    #[test]
    fn an_accepted_core_skips_the_acceptance_only() {
        let state = CustomLaunchState {
            content_accepted: true,
            ..CustomLaunchState::default()
        };
        let steps = custom_launch_steps(&system(SystemOrigin::Custom), &core(), &state);
        assert_eq!(steps[0], CustomStep::UntestedWarning);
        assert_eq!(steps.len(), 3);
    }

    #[test]
    fn a_game_that_passed_the_dry_run_launches_at_once() {
        let state = CustomLaunchState {
            content_accepted: true,
            dry_run_passed: true,
            ..CustomLaunchState::default()
        };
        assert_eq!(
            custom_launch_steps(&system(SystemOrigin::Custom), &core(), &state),
            [CustomStep::Launch]
        );
    }

    #[test]
    fn works_for_me_skips_the_warning_and_the_dry_run_but_not_the_acceptance() {
        let state = CustomLaunchState {
            works_for_me: true,
            ..CustomLaunchState::default()
        };
        assert_eq!(
            custom_launch_steps(&system(SystemOrigin::Custom), &core(), &state),
            [CustomStep::AcceptContent, CustomStep::Launch]
        );
    }

    fn missing(name: &str) -> FirmwareStatus {
        FirmwareStatus {
            spec: emu_assets::cores::FirmwareSpec {
                path: name.to_owned(),
                description: String::new(),
                optional: false,
                md5: None,
            },
            state: FirmwareState::Missing,
        }
    }

    fn firmware_missing(names: &[&str]) -> LaunchCheck {
        LaunchCheck::FirmwareMissing {
            id: core(),
            path: PathBuf::from("core.so"),
            missing: names.iter().map(|n| missing(n)).collect(),
        }
    }

    #[test]
    fn ignored_bios_files_no_longer_stop_the_launch() {
        let ignored = vec!["a.bin".to_owned()];
        assert!(matches!(
            without_ignored(firmware_missing(&["a.bin"]), &ignored),
            LaunchCheck::Ready { .. }
        ));
        let LaunchCheck::FirmwareMissing { missing, .. } =
            without_ignored(firmware_missing(&["a.bin", "b.bin"]), &ignored)
        else {
            panic!("b.bin is still missing");
        };
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].spec.path, "b.bin");
        assert!(matches!(
            without_ignored(firmware_missing(&["a.bin"]), &[]),
            LaunchCheck::FirmwareMissing { .. }
        ));
    }

    #[test]
    fn hardware_rendering_is_read_from_the_info() {
        use emu_assets::cores::{HwApi, HwApiKind};
        let mut info = CoreInfo {
            id: core(),
            display_name: "Acme".to_owned(),
            core_name: None,
            version: None,
            license: None,
            extensions: Vec::new(),
            databases: Vec::new(),
            firmware: Vec::new(),
            description: None,
            system_name: None,
            categories: Vec::new(),
            experimental: false,
            render: RenderNeed::Software,
            supports_no_game: None,
            needs_fullpath: None,
            keyboard_focus: None,
            savestate: None,
        };
        assert!(hardware_check(Some(&info), &core()).is_none());
        assert!(hardware_check(None, &core()).is_none());
        info.render = RenderNeed::Hardware(vec![HwApi {
            kind: HwApiKind::Vulkan,
            min_version: None,
        }]);
        let check = hardware_check(Some(&info), &core()).unwrap();
        assert!(check.blocking_message().unwrap().contains("9.0"));
    }
}
