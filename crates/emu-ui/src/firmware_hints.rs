//! Cores improved by an original file (ADR 0023 §5, design v7.1 §2.3): what the cores screen lists and what the
//! library announces once per (core, file). Nothing here downloads anything; files only come from « Importer un
//! BIOS… ». The pure parts are unit-tested; [`installed_enhancements`] reads the system folder (md5 of the few
//! files present).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use emu_assets::cores::{
    CoreId, CoreInfo, CorePreferences, EnhancementKind, FirmwareEnhancement, FirmwareState,
    KnownFirmware, firmware_enhancements,
};

/// Key of `[interface] firmware_hints_seen`: `"<core_id>/<file>"`.
pub(crate) fn hint_key(core: &str, file: &str) -> String {
    format!("{core}/{file}")
}

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

/// An `Optional` file absent for an installed core, not announced yet: one line of the library banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptionalHint {
    pub core: CoreId,
    /// What the banner shows: [`core_display_name`].
    pub core_name: String,
    /// Library system name (« Game Boy Advance »).
    pub system: String,
    pub file: String,
    pub benefit: String,
}

impl OptionalHint {
    pub(crate) fn key(&self) -> String {
        hint_key(self.core.as_str(), &self.file)
    }
}

/// `display_name` of the core's `.info` (sanitized: untrusted text), its id without one.
pub(crate) fn core_display_name(core: &CoreId, info: Option<&CoreInfo>) -> String {
    info.map(|i| crate::ui::text::sanitize_core_line(&i.display_name))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| core.as_str().to_owned())
}

/// `Optional` files absent and whose key is not in `seen` (design v7.1 §2.3: « bandeau d'information
/// refermable »). `Fallback` files are announced by the launch dialog of the 7.0 instead. `infos`: `.info` of
/// the installed cores, for their display name.
pub(crate) fn pending_hints(
    enhancements: &BTreeMap<CoreId, Vec<CoreEnhancement>>,
    seen: &BTreeSet<String>,
    infos: &BTreeMap<CoreId, CoreInfo>,
) -> Vec<OptionalHint> {
    let mut out = Vec::new();
    for (core, list) in enhancements {
        for e in list {
            let f = &e.enhancement;
            if f.kind != EnhancementKind::Optional
                || hint_state(&f.state) != HintState::Missing
                || seen.contains(&hint_key(core.as_str(), &f.file))
            {
                continue;
            }
            out.push(OptionalHint {
                core: core.clone(),
                core_name: core_display_name(core, infos.get(core)),
                system: system_name(&e.database),
                file: f.file.clone(),
                benefit: f.benefit.clone(),
            });
        }
    }
    out
}

/// The fallback warning of a launch (`CoreFallback`) was already given: for this (core, file) — 7.1, confirmed by
/// the user — or for this game (`GameInput::aros_acknowledged`, 7.0 and earlier).
pub(crate) fn fallback_seen(
    seen: &BTreeSet<String>,
    core: &str,
    wanted: Option<&str>,
    game_acknowledged: bool,
) -> bool {
    game_acknowledged || wanted.is_some_and(|w| seen.contains(&hint_key(core, w)))
}

/// Keys to add after a launch decision: the 7.0 per-game acknowledgement is carried over to its (core, file).
/// `None` when nothing changes.
pub(crate) fn key_to_record(
    seen: &BTreeSet<String>,
    core: &str,
    wanted: Option<&str>,
    acknowledged: bool,
) -> Option<String> {
    let key = hint_key(core, wanted?);
    (acknowledged && !seen.contains(&key)).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enh(file: &str, kind: EnhancementKind, state: FirmwareState) -> CoreEnhancement {
        CoreEnhancement {
            database: "Nintendo - Game Boy Advance".into(),
            enhancement: FirmwareEnhancement {
                file: file.into(),
                variant: None,
                kind,
                benefit: "écran de démarrage".into(),
                state,
            },
        }
    }

    fn id(s: &str) -> CoreId {
        CoreId::new(s).unwrap()
    }

    #[test]
    fn key_is_core_slash_file() {
        assert_eq!(hint_key("mgba", "gba_bios.bin"), "mgba/gba_bios.bin");
    }

    #[test]
    fn only_missing_optional_files_not_seen_are_announced() {
        let mut map = BTreeMap::new();
        map.insert(
            id("mgba"),
            vec![
                enh(
                    "gba_bios.bin",
                    EnhancementKind::Optional,
                    FirmwareState::Missing,
                ),
                enh("ok.bin", EnhancementKind::Optional, FirmwareState::Ok),
                enh(
                    "bad.bin",
                    EnhancementKind::Optional,
                    FirmwareState::WrongHash { md5: [0; 16] },
                ),
                enh(
                    "scph5502.bin",
                    EnhancementKind::Fallback,
                    FirmwareState::Missing,
                ),
            ],
        );
        let hints = pending_hints(&map, &BTreeSet::new(), &BTreeMap::new());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].file, "gba_bios.bin");
        assert_eq!(hints[0].system, "Game Boy Advance");
        assert_eq!(hints[0].key(), "mgba/gba_bios.bin");
        // Once per (core, file): the same file of another core is still announced.
        let seen: BTreeSet<String> = ["mgba/gba_bios.bin".to_owned()].into();
        assert!(pending_hints(&map, &seen, &BTreeMap::new()).is_empty());
        map.insert(
            id("vba_next"),
            vec![enh(
                "gba_bios.bin",
                EnhancementKind::Optional,
                FirmwareState::Missing,
            )],
        );
        let info = emu_assets::cores::parse_info(
            id("vba_next"),
            "display_name = \"Nintendo - Game Boy Advance (VBA Next)\"\n",
        )
        .unwrap();
        let infos: BTreeMap<CoreId, CoreInfo> = [(id("vba_next"), info)].into();
        let hints = pending_hints(&map, &seen, &infos);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].core, id("vba_next"));
        assert_eq!(hints[0].core_name, "Nintendo - Game Boy Advance (VBA Next)");
        assert_eq!(hints[0].key(), "vba_next/gba_bios.bin");
        // Without a `.info`: the id.
        assert_eq!(core_display_name(&id("mgba"), None), "mgba");
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

    /// Once per (core, file); a game already warned (7.0 `aros_acknowledged`) is silent and gives its key.
    #[test]
    fn fallback_warning_once_per_core_and_file() {
        let mut seen = BTreeSet::new();
        assert!(!fallback_seen(
            &seen,
            "swanstation",
            Some("scph5502.bin"),
            false
        ));
        assert!(fallback_seen(
            &seen,
            "swanstation",
            Some("scph5502.bin"),
            true
        ));
        assert_eq!(
            key_to_record(&seen, "swanstation", Some("scph5502.bin"), true).as_deref(),
            Some("swanstation/scph5502.bin")
        );
        assert_eq!(key_to_record(&seen, "swanstation", None, true), None);
        assert_eq!(
            key_to_record(&seen, "swanstation", Some("scph5502.bin"), false),
            None
        );
        seen.insert("swanstation/scph5502.bin".to_owned());
        assert!(fallback_seen(
            &seen,
            "swanstation",
            Some("scph5502.bin"),
            false
        ));
        assert_eq!(
            key_to_record(&seen, "swanstation", Some("scph5502.bin"), true),
            None
        );
        // Another core, or another file of the same core: warned again.
        assert!(!fallback_seen(
            &seen,
            "mednafen_psx",
            Some("scph5502.bin"),
            false
        ));
        assert!(!fallback_seen(
            &seen,
            "swanstation",
            Some("scph5501.bin"),
            false
        ));
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
