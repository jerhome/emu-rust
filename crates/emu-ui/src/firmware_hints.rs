//! Cores improved by an original file (ADR 0023 §5, design v7.5 §8): what the cores screen lists under each
//! installed core. The library announces nothing by itself: the banner of the selected game is the only notice.
//! Nothing here downloads anything; files only come from « Importer un BIOS… ». The pure parts are unit-tested;
//! [`installed_enhancements`] reads the system folder (md5 of the few files present).

use std::collections::BTreeMap;
use std::path::Path;

use emu_assets::cores::{
    CoreId, CoreInfo, CorePreferences, FirmwareEnhancement, FirmwareState, KnownFirmware,
    firmware_enhancements,
};

/// One original file of a core, with the library system it improves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoreEnhancement {
    /// libretro database name (`"Nintendo - Game Boy Advance"`).
    pub database: String,
    pub enhancement: FirmwareEnhancement,
}

/// Library name of a database (« Game Boy Advance »), the database itself when unknown.
pub(crate) fn system_name(database: &str) -> String {
    emu_library::systems()
        .iter()
        .find(|s| s.database == database)
        .map_or_else(|| database.to_owned(), |s| s.name.clone())
}

/// Every original file that improves `core`, for each system of `cores.toml` that lists it (a file shared by two
/// systems is kept once, for the first one).
pub(crate) fn core_enhancements(
    prefs: &CorePreferences,
    core: &CoreId,
    info: Option<&CoreInfo>,
    system_dir: &Path,
    known: &KnownFirmware,
) -> Vec<CoreEnhancement> {
    let mut out: Vec<CoreEnhancement> = Vec::new();
    for (database, cores) in prefs.systems() {
        if !cores.contains(core) {
            continue;
        }
        match firmware_enhancements(prefs, core, database, info, system_dir, known) {
            Ok(list) => {
                for enhancement in list {
                    if out.iter().any(|e| e.enhancement.file == enhancement.file) {
                        continue;
                    }
                    out.push(CoreEnhancement {
                        database: database.to_owned(),
                        enhancement,
                    });
                }
            }
            Err(e) => tracing::warn!("fichiers d'origine de {core} ({database}) : {e}"),
        }
    }
    out
}

/// [`core_enhancements`] of every installed core (`infos`: their `.info`).
pub(crate) fn installed_enhancements(
    installed: &[CoreId],
    infos: &BTreeMap<CoreId, CoreInfo>,
    prefs: &CorePreferences,
    system_dir: &Path,
) -> BTreeMap<CoreId, Vec<CoreEnhancement>> {
    let known = KnownFirmware::builtin();
    installed
        .iter()
        .map(|id| {
            (
                id.clone(),
                core_enhancements(prefs, id, infos.get(id), system_dir, &known),
            )
        })
        .filter(|(_, list)| !list.is_empty())
        .collect()
}

/// What the cores screen says about the file: present and verified / absent / unexpected md5 / unverifiable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HintState {
    Present,
    Missing,
    WrongHash,
    Unverified,
}

pub(crate) fn hint_state(state: &FirmwareState) -> HintState {
    match state {
        FirmwareState::Ok | FirmwareState::Free { .. } | FirmwareState::Builtin { .. } => {
            HintState::Present
        }
        FirmwareState::Missing | FirmwareState::CoreFallback { .. } => HintState::Missing,
        FirmwareState::WrongHash { .. } => HintState::WrongHash,
        FirmwareState::Unverified | FirmwareState::Unreadable(_) => HintState::Unverified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_assets::cores::EnhancementKind;

    fn id(s: &str) -> CoreId {
        CoreId::new(s).unwrap()
    }

    #[test]
    fn states_shown_on_the_cores_screen() {
        assert_eq!(hint_state(&FirmwareState::Ok), HintState::Present);
        assert_eq!(hint_state(&FirmwareState::Missing), HintState::Missing);
        assert_eq!(
            hint_state(&FirmwareState::Unverified),
            HintState::Unverified
        );
        assert_eq!(
            hint_state(&FirmwareState::WrongHash { md5: [1; 16] }),
            HintState::WrongHash
        );
    }

    /// The real `cores.toml`: mgba lists `gba_bios.bin` only when its `.info` does.
    #[test]
    fn real_preferences_need_the_info() {
        let prefs = CorePreferences::builtin();
        let dir = tempfile::tempdir().unwrap();
        let known = KnownFirmware::builtin();
        let none = core_enhancements(&prefs, &id("mgba"), None, dir.path(), &known);
        assert!(
            none.iter()
                .all(|e| e.enhancement.kind != EnhancementKind::Optional)
        );
        let info = emu_assets::cores::parse_info(
            id("mgba"),
            "display_name = \"mGBA\"\nfirmware_count = 1\nfirmware0_path = \"gba_bios.bin\"\n\
             firmware0_opt = \"true\"\ndatabase = \"Nintendo - Game Boy Advance\"\n",
        )
        .unwrap();
        let list = core_enhancements(&prefs, &id("mgba"), Some(&info), dir.path(), &known);
        assert!(
            list.iter().any(|e| e.enhancement.file == "gba_bios.bin"
                && e.enhancement.kind == EnhancementKind::Optional
                && e.enhancement.state == FirmwareState::Missing),
            "{list:?}"
        );
    }
}
