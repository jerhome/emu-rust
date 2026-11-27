//! The only filter of the « add a system » view (ADR 0025 §8, §9): what a `.info` allows.

use emu_assets::cores::{CoreInfo, CorePreferences, RenderNeed};
use emu_library::{MachineTier, cartridge_extensions, known_tier};

/// Verdict for one core of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Addability {
    /// Can be added; `warnings` are shown on the sheet.
    Ok { warnings: Vec<AddWarning> },
    /// Needs hardware rendering: greyed « prévu en 9.0 ».
    HardwareRender,
    /// Disc machines only: greyed « prévu en 8.4 ».
    DiscOnly,
    /// Arcade: greyed « prévu en 8.6 ».
    Arcade,
    /// Not proposed (no usable extension, not an emulator…).
    NotProposed,
}

/// Warning of the sheet of a core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddWarning {
    Experimental,
    /// The core wants the keyboard (`needs_kbd_mouse_focus`): keyboard systems come in 8.5.
    KeyboardNeeded,
    /// `savestate = false` in the `.info`.
    NoSavestates,
    /// Core absent from `cores.toml`: « contenu non vérifié ».
    UnverifiedContent,
}

/// Extensions that name a container, not a format: they never make a system on their own.
const CONTAINER_EXTENSIONS: [&str; 4] = ["zip", "7z", "m3u", "m3u8"];

/// `CoreInfo` + the rules of `emu-library` (extensions, tiers). Never reports [`AddWarning::UnverifiedContent`]:
/// that one depends on `cores.toml`, see [`addability_with`].
pub fn addability(info: &CoreInfo) -> Addability {
    if matches!(info.render, RenderNeed::Hardware(_)) {
        return Addability::HardwareRender;
    }
    let tiers: Vec<MachineTier> = info
        .databases
        .iter()
        .filter_map(|d| known_tier(d))
        .collect();
    let extensions: Vec<String> = info
        .extensions
        .iter()
        .map(|e| e.to_ascii_lowercase())
        .collect();
    if cartridge_extensions(&extensions).is_empty() {
        return refusal(&extensions, &tiers);
    }
    let mut warnings = Vec::new();
    if info.experimental {
        warnings.push(AddWarning::Experimental);
    }
    if info.keyboard_focus == Some(true) || tiers.contains(&MachineTier::Computer) {
        warnings.push(AddWarning::KeyboardNeeded);
    }
    if info.savestate == Some(false) {
        warnings.push(AddWarning::NoSavestates);
    }
    Addability::Ok { warnings }
}

/// No cartridge-like extension left: which greyed line to show.
fn refusal(extensions: &[String], tiers: &[MachineTier]) -> Addability {
    let only_archives =
        !extensions.is_empty() && extensions.iter().all(|e| e == "zip" || e == "7z");
    if tiers.contains(&MachineTier::Arcade) || only_archives {
        return Addability::Arcade;
    }
    let disc_like = extensions.iter().any(|e| {
        !CONTAINER_EXTENSIONS.contains(&e.as_str())
            && (1..=8).contains(&e.len())
            && e.bytes().all(|b| b.is_ascii_alphanumeric())
    });
    if disc_like {
        Addability::DiscOnly
    } else {
        Addability::NotProposed
    }
}

/// [`addability`] plus the « unverified content » warning of a core `prefs` has not examined.
pub fn addability_with(info: &CoreInfo, prefs: &CorePreferences) -> Addability {
    match addability(info) {
        Addability::Ok { mut warnings } => {
            if !prefs.is_examined(&info.id) {
                warnings.push(AddWarning::UnverifiedContent);
            }
            Addability::Ok { warnings }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use emu_assets::cores::{CoreId, HwApi, HwApiKind};

    use super::*;

    fn info(extensions: &[&str], databases: &[&str]) -> CoreInfo {
        CoreInfo {
            id: CoreId::new("some_core").unwrap(),
            display_name: "Some core".to_owned(),
            core_name: None,
            version: None,
            license: None,
            extensions: extensions.iter().map(|s| (*s).to_owned()).collect(),
            databases: databases.iter().map(|s| (*s).to_owned()).collect(),
            firmware: Vec::new(),
            description: None,
            system_name: None,
            categories: vec!["Emulator".to_owned()],
            experimental: false,
            render: RenderNeed::Software,
            supports_no_game: None,
            needs_fullpath: None,
            keyboard_focus: None,
            savestate: None,
        }
    }

    #[test]
    fn a_cartridge_core_is_addable_without_warning() {
        assert_eq!(
            addability(&info(&["a26", "bin"], &["Atari - 2600"])),
            Addability::Ok {
                warnings: Vec::new()
            }
        );
    }

    #[test]
    fn hardware_rendering_is_refused_for_the_ninth_version() {
        let mut core = info(&["a26"], &["Atari - 2600"]);
        core.render = RenderNeed::Hardware(vec![HwApi {
            kind: HwApiKind::OpenGlCore,
            min_version: Some((3, 3)),
        }]);
        assert_eq!(addability(&core), Addability::HardwareRender);
    }

    #[test]
    fn an_unknown_render_need_is_not_a_refusal() {
        let mut core = info(&["a26"], &[]);
        core.render = RenderNeed::Unknown;
        assert!(matches!(addability(&core), Addability::Ok { .. }));
    }

    #[test]
    fn disc_only_cores_wait_for_the_disc_version() {
        assert_eq!(
            addability(&info(&["cue", "chd", "m3u"], &["Sony - PlayStation"])),
            Addability::DiscOnly
        );
    }

    #[test]
    fn an_archive_only_core_is_arcade() {
        assert_eq!(
            addability(&info(&["zip", "7z"], &["Some - Unknown Games"])),
            Addability::Arcade
        );
    }

    #[test]
    fn no_extension_is_not_proposed() {
        assert_eq!(addability(&info(&[], &[])), Addability::NotProposed);
        assert_eq!(addability(&info(&["m3u"], &[])), Addability::NotProposed);
    }

    #[test]
    fn warnings_follow_the_info_flags() {
        let mut core = info(&["xyz"], &[]);
        core.experimental = true;
        core.keyboard_focus = Some(true);
        core.savestate = Some(false);
        assert_eq!(
            addability(&core),
            Addability::Ok {
                warnings: vec![
                    AddWarning::Experimental,
                    AddWarning::KeyboardNeeded,
                    AddWarning::NoSavestates
                ]
            }
        );
    }

    #[test]
    fn a_core_outside_cores_toml_carries_the_unverified_warning() {
        let core = info(&["xyz"], &[]);
        let verdict = addability_with(&core, &CorePreferences::builtin());
        assert_eq!(
            verdict,
            Addability::Ok {
                warnings: vec![AddWarning::UnverifiedContent]
            }
        );
        let known = CorePreferences::builtin();
        let examined = known.systems().flat_map(|(_, c)| c.iter()).next().unwrap();
        let mut core = info(&["xyz"], &[]);
        core.id = examined.clone();
        assert_eq!(
            addability_with(&core, &known),
            Addability::Ok {
                warnings: Vec::new()
            }
        );
    }
}
