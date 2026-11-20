//! Amiga specifics (design v3.0 §4): media kind, machine model resolved from path tags, Kickstart plan.
//! Pure logic; `app.rs` / `game_options.rs` call it from U3b / U4.

use std::path::Path;

use emu_assets::cores::{FirmwareCheck, FirmwareState};

/// What a launched file is, for the default model: a floppy image or a hard-disk image / folder archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AmigaMedia {
    Floppy,
    HardDisk,
}

/// `hdf` / `hdz` / `lha` mean a hard disk; everything else (`adf`, `adz`, `dms`, `ipf`, `zip`, `m3u`…) a floppy.
pub(crate) fn amiga_media_kind(path: &Path) -> AmigaMedia {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("hdf" | "hdz" | "lha") => AmigaMedia::HardDisk,
        _ => AmigaMedia::Floppy,
    }
}

/// Tag (lower case) → value of the `puae_model` option, from the libretro-uae README. Matched on whole tokens,
/// so `A500` never matches inside `A500PLUS`.
const MODEL_TAGS: &[(&str, &str)] = &[
    ("a500og", "A500OG"),
    ("512k", "A500OG"),
    ("a500", "A500"),
    ("ocs", "A500"),
    ("a500ecs", "A500PLUS"),
    ("a500+", "A500PLUS"),
    ("a500plus", "A500PLUS"),
    ("a600og", "A600OG"),
    ("a600nf", "A600OG"),
    ("a600", "A600"),
    ("ecs", "A600"),
    ("a1200og", "A1200OG"),
    ("a1200nf", "A1200OG"),
    ("a1200", "A1200"),
    ("aga", "A1200"),
    ("cd32", "CD32"),
    ("amigacd", "CD32"),
    ("a4030", "A4030"),
    ("030", "A4030"),
    ("a4040", "A4040"),
    ("040", "A4040"),
];

/// Tags allowed outside parentheses (a numeric tag such as `040` is too ambiguous in a bare file name).
fn bare_tag_allowed(tag: &str) -> bool {
    matches!(tag, "ocs" | "ecs" | "aga" | "cd32" | "amigacd")
        || (tag.starts_with('a') && tag[1..].starts_with(|c: char| c.is_ascii_digit()))
}

/// Candidate tags of one path component, in order: any word between parentheses, and bare words of
/// restricted tags. Words are runs of ASCII alphanumerics and `+`.
fn component_tokens(component: &str) -> Vec<String> {
    fn flush(cur: &mut String, in_parens: bool, out: &mut Vec<String>) {
        if cur.is_empty() {
            return;
        }
        let token = cur.to_ascii_lowercase();
        if in_parens || bare_tag_allowed(&token) {
            out.push(token);
        }
        cur.clear();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_parens = false;
    for c in component.chars() {
        if c.is_ascii_alphanumeric() || c == '+' {
            cur.push(c);
            continue;
        }
        flush(&mut cur, in_parens, &mut out);
        match c {
            '(' => in_parens = true,
            ')' => in_parens = false,
            _ => {}
        }
    }
    flush(&mut cur, in_parens, &mut out);
    out
}

fn model_from_tags(path: &Path) -> Option<&'static str> {
    // File name first (left to right), then folders from the deepest one.
    let components: Vec<String> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(str::to_owned))
        .collect();
    for component in components.iter().rev() {
        // The extension is not a tag.
        let stem = component
            .rsplit_once('.')
            .map_or(component.as_str(), |(s, _)| s);
        for token in component_tokens(stem) {
            if let Some((_, model)) = MODEL_TAGS.iter().find(|(tag, _)| *tag == token) {
                return Some(model);
            }
        }
    }
    None
}

/// Machine model for a launch: an explicit `model` (anything but `auto`) wins; otherwise a tag of the path;
/// otherwise the default of the media kind (`model_fd` / `model_hd`).
pub(crate) fn resolve_amiga_model(
    path: &Path,
    media: AmigaMedia,
    model: &str,
    model_fd: &str,
    model_hd: &str,
) -> String {
    if !model.eq_ignore_ascii_case("auto") {
        return model.to_owned();
    }
    if let Some(tagged) = model_from_tags(path) {
        return tagged.to_owned();
    }
    match media {
        AmigaMedia::Floppy => model_fd,
        AmigaMedia::HardDisk => model_hd,
    }
    .to_owned()
}

/// What to do about the Kickstart after [`emu_assets`]'s `check_required` (design v3.0 §3.5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KickstartPlan {
    /// The wanted ROM is usable (or nothing is expected): launch with the user's setting.
    Original,
    /// The ROM is missing but puae has AROS built in: launch with AROS; `first_time` = warn (once per game).
    Aros { first_time: bool },
    /// The file is there but is not the expected ROM: the UI asks.
    WrongHash { md5: [u8; 16] },
}

/// `Missing` without fallback and `Unreadable` are handled by the launch dialog before this call; they map to
/// `Original` (the launch then fails or proceeds on the core's own terms).
pub(crate) fn plan_kickstart(check: &FirmwareCheck, aros_acknowledged: bool) -> KickstartPlan {
    match &check.state {
        FirmwareState::CoreFallback { .. } => KickstartPlan::Aros {
            first_time: !aros_acknowledged,
        },
        FirmwareState::WrongHash { md5 } => KickstartPlan::WrongHash { md5: *md5 },
        _ => KickstartPlan::Original,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(path: &str, media: AmigaMedia, m: &str) -> String {
        resolve_amiga_model(Path::new(path), media, m, "A500", "A1200")
    }

    #[test]
    fn media_kind_by_extension() {
        assert_eq!(
            amiga_media_kind(Path::new("a/Game.ADF")),
            AmigaMedia::Floppy
        );
        assert_eq!(
            amiga_media_kind(Path::new("a/Game.zip")),
            AmigaMedia::Floppy
        );
        assert_eq!(
            amiga_media_kind(Path::new("a/Game.hdf")),
            AmigaMedia::HardDisk
        );
        assert_eq!(
            amiga_media_kind(Path::new("a/Game.LHA")),
            AmigaMedia::HardDisk
        );
        assert_eq!(
            amiga_media_kind(Path::new("a/Game.hdz")),
            AmigaMedia::HardDisk
        );
        assert_eq!(amiga_media_kind(Path::new("a/noext")), AmigaMedia::Floppy);
    }

    #[test]
    fn explicit_model_wins_over_tags() {
        assert_eq!(model("Game (AGA).adf", AmigaMedia::Floppy, "A600"), "A600");
    }

    #[test]
    fn tags_are_case_insensitive_and_whole_tokens() {
        assert_eq!(
            model("Game (1992)(Foo)(aga).adf", AmigaMedia::Floppy, "auto"),
            "A1200"
        );
        assert_eq!(
            model("Game (A500+).adf", AmigaMedia::Floppy, "auto"),
            "A500PLUS"
        );
        assert_eq!(
            model("Game (A500PLUS).adf", AmigaMedia::Floppy, "auto"),
            "A500PLUS"
        );
        assert_eq!(
            model("Game (A500).adf", AmigaMedia::HardDisk, "auto"),
            "A500"
        );
        assert_eq!(
            model("Game (A500OG).adf", AmigaMedia::Floppy, "auto"),
            "A500OG"
        );
        assert_eq!(
            model("Game (512K).adf", AmigaMedia::Floppy, "auto"),
            "A500OG"
        );
        assert_eq!(
            model("Game (A1200NF).hdf", AmigaMedia::HardDisk, "auto"),
            "A1200OG"
        );
        assert_eq!(model("Game (CD32).iso", AmigaMedia::Floppy, "auto"), "CD32");
        assert_eq!(
            model("Game (040).hdf", AmigaMedia::HardDisk, "auto"),
            "A4040"
        );
        assert_eq!(
            model("Game (030).hdf", AmigaMedia::HardDisk, "auto"),
            "A4030"
        );
    }

    #[test]
    fn bare_words_and_numbers() {
        assert_eq!(model("Game ECS.adf", AmigaMedia::Floppy, "auto"), "A600");
        // A bare number or a word merely containing a tag is not a tag.
        assert_eq!(model("Game 040.adf", AmigaMedia::Floppy, "auto"), "A500");
        assert_eq!(
            model("Specs of Doom.adf", AmigaMedia::HardDisk, "auto"),
            "A1200"
        );
        assert_eq!(model("Hagar.adf", AmigaMedia::HardDisk, "auto"), "A1200");
    }

    #[test]
    fn folder_tags_and_extension_ignored() {
        assert_eq!(
            model("roms/amiga/AGA/Game.adf", AmigaMedia::Floppy, "auto"),
            "A1200"
        );
        assert_eq!(
            model("roms/(A600)/Game (OCS).adf", AmigaMedia::Floppy, "auto"),
            "A500"
        );
        // The extension is never a tag.
        assert_eq!(model("Game.aga", AmigaMedia::Floppy, "auto"), "A500");
    }

    #[test]
    fn defaults_by_media() {
        assert_eq!(model("Game.adf", AmigaMedia::Floppy, "auto"), "A500");
        assert_eq!(model("Game.hdf", AmigaMedia::HardDisk, "AUTO"), "A1200");
    }

    fn check(state: FirmwareState) -> FirmwareCheck {
        FirmwareCheck {
            wanted: Some("kick34005.A500".into()),
            state,
            others_present: Vec::new(),
        }
    }

    #[test]
    fn kickstart_plans() {
        assert_eq!(
            plan_kickstart(&check(FirmwareState::Ok), false),
            KickstartPlan::Original
        );
        assert_eq!(
            plan_kickstart(&check(FirmwareState::Unverified), false),
            KickstartPlan::Original
        );
        let fallback = FirmwareState::CoreFallback {
            label: "AROS".into(),
        };
        assert_eq!(
            plan_kickstart(&check(fallback.clone()), false),
            KickstartPlan::Aros { first_time: true }
        );
        assert_eq!(
            plan_kickstart(&check(fallback), true),
            KickstartPlan::Aros { first_time: false }
        );
        assert_eq!(
            plan_kickstart(&check(FirmwareState::WrongHash { md5: [7; 16] }), true),
            KickstartPlan::WrongHash { md5: [7; 16] }
        );
    }

    /// Rows go through the virtual keyboard parser (everything before `[remap]`); `[remap]` is read by U3b/U4.
    #[test]
    fn keyboard_file_is_valid() {
        use crate::computer::vkbd::KeyboardLayout;
        let text = include_str!("../../data/keyboards/amiga.toml");
        let (rows, remap) = text.split_once("[remap]").expect("[remap] table");
        let layout = KeyboardLayout::parse(rows).expect("valid layout");
        assert_eq!(layout.id, "amiga");
        assert!(layout.rows.len() >= 6);
        let table: std::collections::BTreeMap<String, String> =
            toml::from_str::<toml::Table>(remap)
                .expect("remap table")
                .into_iter()
                .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_owned()))
                .collect();
        assert_eq!(table.get("AltLeft").map(String::as_str), Some("lsuper"));
        assert_eq!(table.get("AltRight").map(String::as_str), Some("rsuper"));
        for value in table.values() {
            assert!(emu_runtime::RetroKey::from_name(value).is_some(), "{value}");
        }
        assert_eq!(table.len(), 2);
    }
}
