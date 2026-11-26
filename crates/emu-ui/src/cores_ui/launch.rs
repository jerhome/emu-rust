//! From a game (system + extension) to a verified core path (ADR 0013 §6-8, design §2.3).
//!
//! Nothing here downloads anything: a missing core is reported as `NeedsInstall` and the screen decides. BIOS
//! files are only checked, never fetched.

use std::collections::BTreeMap;
use std::path::PathBuf;

use emu_assets::cores::{
    CoreChoice, CoreError, CoreId, CoreInfo, CoreManager, CorePreferences, FirmwareState,
    FirmwareStatus, choose_core,
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
    /// Custom system whose core needs hardware rendering (planned for 9.0; design 8.0 §0.2). Built by PU2.
    #[allow(dead_code)]
    HardwareRender { id: CoreId },
    /// Custom system: the user has not accepted the unverified content of this core. Built by PU2.
    #[allow(dead_code)]
    ContentNotAccepted { id: CoreId },
}

/// Steps before the game of a custom system starts, in order (design 8.0 §2.5, ADR 0025 §6, §7). Built by PU2.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomStep {
    AcceptContent,
    UntestedWarning,
    DryRun,
    Launch,
}

/// What the user and the cache already know about a (core, system) pair. Built by PU2.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CustomLaunchState {
    pub content_accepted: bool,
    pub works_for_me: bool,
    pub dry_run_passed: bool,
}

/// Steps to run for `system` played with `core`; empty for an official system.
#[allow(dead_code, unused_variables)]
pub(crate) fn custom_launch_steps(
    system: &emu_library::SystemInfo,
    core: &CoreId,
    state: &CustomLaunchState,
) -> Vec<CustomStep> {
    todo!("lot PU2")
}

impl LaunchCheck {
    /// French message for the outcomes that stop the launch.
    pub fn blocking_message(&self) -> Option<String> {
        match self {
            LaunchCheck::NoCore => Some(text::no_core_for_system().to_owned()),
            LaunchCheck::Failed(e) => Some(text::error(e)),
            _ => None,
        }
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
