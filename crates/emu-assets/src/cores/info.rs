use std::collections::BTreeMap;

use super::CoreId;

/// `firmwareN_*` indices above this are ignored (real files: < 40).
const MAX_FIRMWARE_INDEX: usize = 255;

/// Larger `.info` texts are refused (largest real file ≈ 10 KiB).
pub const MAX_INFO_SIZE: usize = 64 * 1024;

/// What the application uses from a libretro-core-info file (`<id>_libretro.info`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreInfo {
    pub id: CoreId,
    /// `display_name`, e.g. "Nintendo - Game Boy / Color (SameBoy)"; falls back to the id.
    pub display_name: String,
    /// `corename`.
    pub core_name: Option<String>,
    /// `display_version`.
    pub version: Option<String>,
    /// `license` as written (e.g. "GPLv2", "MIT", "Non-commercial").
    pub license: Option<String>,
    /// `supported_extensions` split on `|`, lowercased, without dots.
    pub extensions: Vec<String>,
    /// `database` split on `|`: libretro database names, the system key shared with emu-library.
    pub databases: Vec<String>,
    pub firmware: Vec<FirmwareSpec>,
    pub description: Option<String>,
    /// `systemname`, ≤ 128 chars, cleaned (design 8.0 §2.2; filled by lot PA1, `None` until then).
    pub system_name: Option<String>,
    /// `categories` split on `|`, ≤ 8 × 32 chars.
    pub categories: Vec<String>,
    /// `is_experimental`.
    pub experimental: bool,
    /// Rendering the core asks for (`hw_render`, `required_hw_api`).
    pub render: RenderNeed,
    pub supports_no_game: Option<bool>,
    pub needs_fullpath: Option<bool>,
    /// `needs_kbd_mouse_focus`.
    pub keyboard_focus: Option<bool>,
    pub savestate: Option<bool>,
}

/// `Hardware(vec![])` when `hw_render = true` without `required_hw_api`; `Unknown` when `hw_render` is absent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RenderNeed {
    Software,
    Hardware(Vec<HwApi>),
    #[default]
    Unknown,
}

/// One entry of `required_hw_api` (`OpenGL Core >= 3.3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwApi {
    pub kind: HwApiKind,
    pub min_version: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HwApiKind {
    OpenGl,
    OpenGlCore,
    OpenGlEs,
    Vulkan,
    Direct3D,
    /// Unrecognised name, ≤ 64 chars.
    Other(String),
}

/// Parses a `required_hw_api` value (`|`-separated, ≤ 8 entries). Contract of lot PA1.
// Contract stub (PX0): the body belongs to lot PA1.
#[allow(unused_variables)]
pub fn parse_required_hw_api(text: &str) -> Vec<HwApi> {
    todo!("lot PA1")
}

/// One `firmwareN_*` group, with the md5 found for the same file name in `notes` (`(!) <path> (md5): <hex>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareSpec {
    /// Relative to the system directory, `/`-separated. Specs whose path is absolute, contains `..`, `\`, a
    /// drive prefix or an empty component are dropped at parse time (the `.info` comes from the network).
    pub path: String,
    pub description: String,
    /// `firmwareN_opt` (missing ⇒ `false`, i.e. required).
    pub optional: bool,
    pub md5: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InfoError {
    #[error("fichier .info trop grand ({0} octets)")]
    TooLarge(usize),
}

/// `key = "value"` lines, `#` comments, unknown keys ignored, malformed lines skipped. Only size can fail.
pub fn parse_info(id: CoreId, text: &str) -> Result<CoreInfo, InfoError> {
    if text.len() > MAX_INFO_SIZE {
        return Err(InfoError::TooLarge(text.len()));
    }
    let mut keys: BTreeMap<&str, String> = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = parse_line(line) {
            keys.insert(key, value);
        }
    }
    let get = |key: &str| keys.get(key).filter(|v| !v.is_empty()).cloned();

    let md5s = notes_md5(keys.get("notes").map_or("", String::as_str));
    let mut groups: BTreeMap<usize, (Option<String>, Option<String>, bool)> = BTreeMap::new();
    for (key, value) in &keys {
        let Some(rest) = key.strip_prefix("firmware") else {
            continue;
        };
        let Some((index, field)) = rest.split_once('_') else {
            continue;
        };
        let Some(index) = index
            .parse::<usize>()
            .ok()
            .filter(|i| *i <= MAX_FIRMWARE_INDEX && index.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        let group = groups.entry(index).or_default();
        match field {
            "path" => group.0 = Some(value.clone()),
            "desc" => group.1 = Some(value.clone()),
            "opt" => group.2 = value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    let firmware = groups
        .into_values()
        .filter_map(|(path, desc, optional)| {
            let path = path?;
            if !is_safe_relative_path(&path) {
                tracing::debug!("{id}.info : chemin de BIOS refusé");
                return None;
            }
            Some(FirmwareSpec {
                md5: md5s.get(path.as_str()).copied(),
                description: desc.unwrap_or_else(|| path.clone()),
                optional,
                path,
            })
        })
        .collect();

    Ok(CoreInfo {
        display_name: get("display_name").unwrap_or_else(|| id.to_string()),
        core_name: get("corename"),
        version: get("display_version"),
        license: get("license"),
        extensions: split_list(keys.get("supported_extensions"))
            .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect(),
        databases: split_list(keys.get("database"))
            .map(str::to_owned)
            .collect(),
        firmware,
        description: get("description"),
        system_name: None,
        categories: Vec::new(),
        experimental: false,
        render: RenderNeed::Unknown,
        supports_no_game: None,
        needs_fullpath: None,
        keyboard_focus: None,
        savestate: None,
        id,
    })
}

/// `key = value`, `key = "value"`; `None` for comments, blank and malformed lines (no `=`, bad key,
/// unterminated quote). Unquoted values end at `#`.
fn parse_line(line: &str) -> Option<(&str, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    let value = value.trim();
    let value = if let Some(quoted) = value.strip_prefix('"') {
        // libretro's config reader has no escapes: the value ends at the next quote.
        let (inner, rest) = quoted.split_once('"')?;
        let rest = rest.trim_start();
        if !rest.is_empty() && !rest.starts_with('#') {
            return None;
        }
        inner
    } else {
        value.split('#').next().unwrap_or("").trim()
    };
    Some((key, value.to_owned()))
}

fn split_list(value: Option<&String>) -> impl Iterator<Item = &str> {
    value
        .map_or("", String::as_str)
        .split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// `notes` lines such as `(!) bios_MD.bin (md5): d3293ebaaa7f4eb2a6766b68a0fb4609`, `|`-separated.
fn notes_md5(notes: &str) -> BTreeMap<&str, [u8; 16]> {
    notes
        .split('|')
        .filter_map(|note| {
            let note = note.trim();
            let note = note.strip_prefix("(!)").unwrap_or(note).trim_start();
            let (path, hash) = note.split_once(" (md5):")?;
            Some((path.trim(), parse_md5(hash.trim())?))
        })
        .collect()
}

pub(crate) fn parse_md5(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// Relative, `/`-separated, no empty / `.` / `..` component, no `\`, `:` (drive prefixes, NTFS streams) or NUL,
/// no component ending in `.` or a space, no Windows device name: joined to the system directory, it always stays
/// inside and names a regular file on every target (checked everywhere: `.info` files are shared by all targets).
pub(crate) fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|c| {
            !c.is_empty() && c.trim() == c && !c.ends_with('.') && !is_windows_device_name(c)
        })
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM0-9`, `LPT0-9` (also with `¹²³`), `CONIN$`, `CONOUT$`, in any case, alone or
/// followed by an extension (Windows ignores everything from the first `.` and trailing spaces before it).
fn is_windows_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|n| {
            let mut chars = n.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> CoreId {
        CoreId::new(s).unwrap()
    }

    // Synthetic, in the format of libretro-core-info files.
    const GPGX: &str = r#"# Software Information
display_name = "Sega - MS/GG/MD/CD (Genesis Plus GX)"
authors = "Charles MacDonald|Eke-Eke"
supported_extensions = "mdx|md|SMD|.gen|bin|cue|iso|sms|bms|gg|sg|68k|sgd|chd|m3u"
corename = "Genesis Plus GX"
license = "Non-commercial"
display_version = "v1.7.4"   # trailing comment
database = "Sega - Mega Drive - Genesis|Sega - Master System - Mark III| Sega - Game Gear |"
firmware_count = 4
firmware0_desc = "bios_CD_E.bin (MegaCD EU BIOS)"
firmware0_path = "bios_CD_E.bin"
firmware0_opt = "true"
firmware1_path = "bios_MD.bin"
firmware1_opt = "false"
firmware2_desc = "no path"
firmware3_path = "../escape.bin"
firmware4_path = "/etc/passwd"
firmware5_path = "C:\evil.bin"
firmware6_path = "a\\b.bin"
firmware7_path = "sub//x.bin"
firmware8_path = "sub/ok.bin"
firmware8_opt = TRUE
firmwareX_path = "ignored.bin"
notes = "(!) bios_CD_E.bin (md5): e66fa1dc5820d254611fdcdba0662372|(!) bios_MD.bin (md5): nothex|(!) sub/ok.bin (md5): D3293EBAAA7F4EB2A6766B68A0FB4609"
description = "A # inside quotes is kept" # comment with "quotes"
broken line without equals
unterminated = "oops
license = "a" trailing junk
"#;

    #[test]
    fn parses_fields() {
        let info = parse_info(id("genesis_plus_gx"), GPGX).unwrap();
        assert_eq!(info.display_name, "Sega - MS/GG/MD/CD (Genesis Plus GX)");
        assert_eq!(info.core_name.as_deref(), Some("Genesis Plus GX"));
        assert_eq!(info.version.as_deref(), Some("v1.7.4"));
        assert_eq!(info.license.as_deref(), Some("Non-commercial"));
        assert_eq!(&info.extensions[..4], ["mdx", "md", "smd", "gen"]);
        assert_eq!(
            info.databases,
            [
                "Sega - Mega Drive - Genesis",
                "Sega - Master System - Mark III",
                "Sega - Game Gear"
            ]
        );
        assert_eq!(
            info.description.as_deref(),
            Some("A # inside quotes is kept")
        );
    }

    #[test]
    fn firmware_paths_and_hashes() {
        let info = parse_info(id("genesis_plus_gx"), GPGX).unwrap();
        let paths: Vec<_> = info.firmware.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["bios_CD_E.bin", "bios_MD.bin", "sub/ok.bin"]);
        let cd = &info.firmware[0];
        assert!(cd.optional);
        assert_eq!(cd.description, "bios_CD_E.bin (MegaCD EU BIOS)");
        assert_eq!(cd.md5, parse_md5("e66fa1dc5820d254611fdcdba0662372"));
        let md = &info.firmware[1];
        assert!(!md.optional);
        assert_eq!(md.description, "bios_MD.bin");
        assert_eq!(md.md5, None);
        assert!(info.firmware[2].optional);
        assert_eq!(info.firmware[2].md5.unwrap()[0], 0xd3);
    }

    #[test]
    fn minimal_and_garbage() {
        let info = parse_info(id("x"), "\u{0}\u{ff}= =\n=\n\"\"\n").unwrap();
        assert_eq!(info.display_name, "x");
        assert!(info.extensions.is_empty() && info.firmware.is_empty());
        assert_eq!(info.core_name, None);
    }

    #[test]
    fn too_large() {
        let text = "#".repeat(MAX_INFO_SIZE + 1);
        assert_eq!(
            parse_info(id("x"), &text),
            Err(InfoError::TooLarge(MAX_INFO_SIZE + 1))
        );
    }

    #[test]
    fn safe_paths() {
        for ok in [
            "a.bin",
            "dir/a b (En).bin",
            "SGB1.sfc/program.rom",
            "console.bin",
            "COM10",
            "lpt.bin",
            "nul_x.bin",
            "CONIN.bin",
            "auxiliary/x",
            ".hidden",
        ] {
            assert!(is_safe_relative_path(ok), "{ok}");
        }
        for bad in [
            "",
            "/a",
            "a/",
            "../a",
            "a/../b",
            "./a",
            "C:\\a",
            "C:a",
            "a\\b",
            "a\0b",
            " a",
            "a//b",
            "a.",
            "a ",
            "dir./a",
            "dir /a",
            "CON",
            "con",
            "nul.txt",
            "Aux.bin",
            "PRN.tar.gz",
            "NUL .bin",
            "dir/com1",
            "COM9.bin",
            "lpt1",
            "LPT0",
            "COM\u{b9}",
            "lpt\u{b3}.rom",
            "CONIN$",
            "conout$.x",
            "a/LPT5.sav",
        ] {
            assert!(!is_safe_relative_path(bad), "{bad:?}");
        }
    }
}
