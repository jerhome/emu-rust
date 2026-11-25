//! From a game (system + extension) to a verified core path (ADR 0013 §6-8, design §2.3).
//!
//! Nothing here downloads anything: a missing core is reported as `NeedsInstall` and the screen decides. BIOS
//! files are only checked, never fetched.

use std::path::PathBuf;

use emu_assets::cores::{
    CoreChoice, CoreError, CoreId, CoreManager, CorePreferences, FirmwareState, FirmwareStatus,
    choose_cd32x_core, choose_core,
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

/// Picks the core for a game (`system` = libretro database name, `extension` without dot) then prepares it.
/// `cd32x`: the game is a Sega CD 32X (ADR 0024 §3): the system's `cd32x_cores` decide, the « Core utilisé »
/// preference and the extension do not; a system without such a list falls back to [`choose_core`].
pub(crate) fn plan_launch(
    manager: &CoreManager,
    prefs: &CorePreferences,
    system: Option<&str>,
    cd32x: bool,
    extension: Option<&str>,
) -> LaunchCheck {
    let installed: Vec<CoreId> = manager.installed().into_iter().map(|c| c.id).collect();
    let imposed = system
        .filter(|_| cd32x)
        .and_then(|s| choose_cd32x_core(s, &installed, prefs));
    let choice = imposed
        .unwrap_or_else(|| choose_core(system, extension, &installed, &manager.infos(), prefs));
    match choice {
        CoreChoice::Installed(id) => prepare_launch(manager, &id),
        CoreChoice::Install(id) => LaunchCheck::NeedsInstall(id),
        CoreChoice::Unknown => LaunchCheck::NoCore,
    }
}

/// The game is over (or the user cancelled): the core may be updated or removed again.
pub(crate) fn release(manager: &CoreManager) {
    manager.set_in_use(None);
}
