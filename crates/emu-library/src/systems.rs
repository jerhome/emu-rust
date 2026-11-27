//! System catalog, parsed once from the embedded `data/systems.toml` (ADR 0014 §1).

use std::collections::HashSet;

use serde::Deserialize;

use crate::catalog::Catalog;

pub(crate) const CATALOG: &str = include_str!("../data/systems.toml");

/// Copier / dump header removed before the "headerless" hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderRule {
    /// 16-byte iNES header, only when the file starts with `NES\x1A`.
    INes,
    /// 512-byte copier header, only when `size % 1024 == 512`.
    SnesCopier,
}

/// Console (cartridge, one file) or computer (disks, keyboard and mouse, several images per game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemKind {
    #[default]
    Console,
    Computer,
}

/// Where an identification DAT of a system lives in libretro-database (`<repo_dir>/<database>.dat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum DatSource {
    /// `metadat/no-intro/`.
    #[default]
    NoIntro,
    /// `metadat/tosec/`.
    Tosec,
    /// Top-level `dat/` (WHDLoad `.lha` sets of `Commodore - Amiga.dat`: crc + sha1, no md5).
    Libretro,
    /// `metadat/redump/` (Mega CD, design v6.0 §3.4: track 01 only, `size` / `crc` / `serial`).
    Redump,
}

impl DatSource {
    /// Stable name: value of `dat` in `systems.toml`, of the `source` column (schema v4), and local folder
    /// of the downloaded file (`<database dir>/<id>/<database>.dat`; two sources of one system share the
    /// file name).
    pub fn id(self) -> &'static str {
        match self {
            DatSource::NoIntro => "no-intro",
            DatSource::Tosec => "tosec",
            DatSource::Libretro => "libretro",
            DatSource::Redump => "redump",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [
            DatSource::NoIntro,
            DatSource::Tosec,
            DatSource::Libretro,
            DatSource::Redump,
        ]
        .into_iter()
        .find(|s| s.id() == id)
    }

    /// Folder of the DAT in the libretro-database repository.
    pub fn repo_dir(self) -> &'static str {
        match self {
            DatSource::NoIntro => "metadat/no-intro",
            DatSource::Tosec => "metadat/tosec",
            DatSource::Libretro => "dat",
            DatSource::Redump => "metadat/redump",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    /// Stable, persisted (`megadrive`, `snes`, `nes`, `gb`, `gbc`, `gamegear`, `mastersystem`).
    pub id: String,
    pub name: String,
    /// Lower case, without the dot.
    pub extensions: Vec<String>,
    pub header: Option<HeaderRule>,
    /// libretro database name (`Nintendo - Game Boy`): DAT file name, libretro-thumbnails folder, and the system key
    /// of `emu_assets::cores::choose_core` (ADR 0013 owns the cores of each system).
    pub database: String,
    pub screenscraper: Option<u32>,
    pub igdb: Option<u32>,
    pub kind: SystemKind,
    /// DAT sources in identification order (first match wins); `systems.toml` `dat`: one name or a list,
    /// default `["no-intro"]`. Never empty.
    pub dats: Vec<DatSource>,
    /// Subset of `extensions` kept for this system only under a folder named after its `id` (design v3.0
    /// §3.5.2; `systems.toml` `folder_extensions`, default empty).
    pub folder_extensions: Vec<String>,
    /// A sub-folder of `roms/<id>/` holding an `exe` / `com` / `bat` at its root is one game (DOS, design v4.0
    /// §3.4; `systems.toml` `folder_games`, default `false`).
    pub folder_games: bool,
    /// Keyboard mode of a game of this system when neither the game nor `[computer] keyboard_mode` chooses
    /// (design v5.0 §3.4; `systems.toml` `default_input`, default `"keyboard"`; `cpc`, `c64`: `"joystick"`).
    pub default_input: InputDefault,
    /// CD system (design v6.0 §3.4; `systems.toml` `disc`, default `false`; `megacd`): game named after its
    /// `.cue` / `.chd` / `.iso` / `.m3u`, Redump identification, region of the game, « CD » label in the UI.
    pub disc: bool,
    /// `(Disc N)` files of one title are grouped into one game with a generated `.m3u` (`systems.toml`
    /// `group_discs`, default `true`; `3do`: `false`, opera has neither disk interface nor `.m3u`; design v7.0
    /// §3.4). Only meaningful when `disc` is set.
    pub group_discs: bool,
    /// Where the system comes from (design 8.0 §2.1): the embedded catalog or a user definition.
    pub origin: SystemOrigin,
}

/// Official (embedded `systems.toml`) or custom (`<data>/systems/<id>.toml`, ADR 0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SystemOrigin {
    #[default]
    Official,
    Custom,
}

/// `systems.toml` `default_input` (user decision of 2026-11-21: Joystick for CPC and C64).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputDefault {
    #[default]
    Keyboard,
    Joystick,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    system: Vec<RawSystem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawHeader {
    Ines,
    SnesCopier,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum RawKind {
    #[default]
    Console,
    Computer,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
enum RawDat {
    NoIntro,
    Tosec,
    Libretro,
    Redump,
}

impl RawDat {
    fn source(self) -> DatSource {
        match self {
            RawDat::NoIntro => DatSource::NoIntro,
            RawDat::Tosec => DatSource::Tosec,
            RawDat::Libretro => DatSource::Libretro,
            RawDat::Redump => DatSource::Redump,
        }
    }
}

/// `dat = "tosec"` or `dat = ["tosec", "libretro"]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawDats {
    One(RawDat),
    Many(Vec<RawDat>),
}

impl Default for RawDats {
    fn default() -> Self {
        RawDats::One(RawDat::NoIntro)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSystem {
    id: String,
    name: String,
    extensions: Vec<String>,
    header: Option<RawHeader>,
    database: String,
    screenscraper: Option<u32>,
    igdb: Option<u32>,
    #[serde(default)]
    kind: RawKind,
    #[serde(default)]
    dat: RawDats,
    #[serde(default)]
    folder_extensions: Vec<String>,
    #[serde(default)]
    folder_games: bool,
    #[serde(default)]
    default_input: RawInput,
    #[serde(default)]
    disc: bool,
    #[serde(default = "default_true")]
    group_discs: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum RawInput {
    #[default]
    Keyboard,
    Joystick,
}

/// Parses and validates a catalog: unique non-empty ids, lower-case dot-less extensions, non-empty database,
/// a non-empty list of distinct DAT sources, `folder_extensions` a subset of `extensions` (`m3u` / `m3u8` excluded: the
/// system of a playlist comes from its entries).
pub(crate) fn parse_catalog(text: &str) -> Result<Vec<SystemInfo>, String> {
    let raw: RawCatalog = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut ids = HashSet::new();
    let mut out = Vec::with_capacity(raw.system.len());
    for s in raw.system {
        let id_ok = !s.id.is_empty()
            && s.id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if !id_ok {
            return Err(format!("invalid id `{}`", s.id));
        }
        if !ids.insert(s.id.clone()) {
            return Err(format!("duplicate id `{}`", s.id));
        }
        if s.database.trim().is_empty() || s.name.trim().is_empty() {
            return Err(format!("`{}`: empty name or database", s.id));
        }
        if s.extensions.is_empty() {
            return Err(format!("`{}`: no extension", s.id));
        }
        for ext in &s.extensions {
            let ext_ok = !ext.is_empty()
                && ext
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
            // `zip` is handled by the scanner itself (one archive = one game).
            if !ext_ok || ext == "zip" {
                return Err(format!("`{}`: invalid extension `{ext}`", s.id));
            }
        }
        let dats: Vec<DatSource> = match s.dat {
            RawDats::One(d) => vec![d.source()],
            RawDats::Many(list) => list.into_iter().map(RawDat::source).collect(),
        };
        if dats.is_empty() || dats.iter().collect::<HashSet<_>>().len() != dats.len() {
            return Err(format!("`{}`: empty or repeated `dat`", s.id));
        }
        if s.disc
            && !s
                .extensions
                .iter()
                .any(|e| crate::DISC_EXTENSIONS.contains(&e.as_str()))
        {
            return Err(format!("`{}`: `disc` without a disc extension", s.id));
        }
        let mut restricted = HashSet::new();
        for ext in &s.folder_extensions {
            if !s.extensions.contains(ext)
                || ext == "m3u"
                || ext == "m3u8"
                || !restricted.insert(ext)
            {
                return Err(format!("`{}`: invalid folder extension `{ext}`", s.id));
            }
        }
        out.push(SystemInfo {
            id: s.id,
            name: s.name,
            extensions: s.extensions,
            header: s.header.map(|h| match h {
                RawHeader::Ines => HeaderRule::INes,
                RawHeader::SnesCopier => HeaderRule::SnesCopier,
            }),
            database: s.database,
            screenscraper: s.screenscraper,
            igdb: s.igdb,
            kind: match s.kind {
                RawKind::Console => SystemKind::Console,
                RawKind::Computer => SystemKind::Computer,
            },
            dats,
            folder_extensions: s.folder_extensions,
            folder_games: s.folder_games,
            default_input: match s.default_input {
                RawInput::Keyboard => InputDefault::Keyboard,
                RawInput::Joystick => InputDefault::Joystick,
            },
            disc: s.disc,
            group_discs: s.group_discs,
            origin: SystemOrigin::Official,
        });
    }
    Ok(out)
}

/// The whole published catalog, in catalog order (officials in file order, then custom systems).
pub fn systems() -> &'static [SystemInfo] {
    crate::catalog::catalog().systems()
}

pub fn system(id: &str) -> Option<&'static SystemInfo> {
    crate::catalog::catalog().system(id)
}

impl SystemInfo {
    pub fn is_custom(&self) -> bool {
        self.origin == SystemOrigin::Custom
    }

    /// libretro name: DAT file, thumbnails folder. `None` for a custom system without one (empty `database`
    /// while the field is public, design 8.0 A10).
    pub fn libretro_name(&self) -> Option<&str> {
        Some(self.database.as_str()).filter(|d| !d.is_empty())
    }

    /// Key of `CorePreferences`: the libretro name, else the id (ids never contain « - »).
    pub fn core_key(&self) -> &str {
        self.libretro_name().unwrap_or(&self.id)
    }

    /// `extension` (lower case) is accepted by this system only under a folder named after its id.
    pub(crate) fn is_folder_only(&self, extension: &str) -> bool {
        self.folder_extensions.iter().any(|e| e == extension)
    }

    /// Rank of `source` in [`SystemInfo::dats`] (identification order); undeclared sources come last.
    pub(crate) fn dat_rank(&self, source: &str) -> usize {
        self.dats
            .iter()
            .position(|d| d.id() == source)
            .unwrap_or(self.dats.len())
    }
}

/// How a file of a given extension is attributed to a system (design v3.0 §3.5.2), before any DAT lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Attribution {
    /// Folder rule: a folder named after a system accepting the extension (restricted or not).
    Folder(&'static SystemInfo),
    /// Systems accepting the extension outside their folder, in catalog order. The identification picks among
    /// them and falls back to the first (1.0 rule: one system, or consoles sharing `.gb`).
    Candidates(Vec<&'static SystemInfo>),
    /// Shared by several systems, a computer among them (`.ipf`): only a DAT hash match in exactly one of them
    /// attributes the file, otherwise it is unclassified.
    ByDat(Vec<&'static SystemInfo>),
    /// No system takes the extension here (a folder-only extension outside its folder).
    Unclassified,
}

/// `extension` is lower case without the dot; `folders` are the path components checked against system ids
/// (exact match, as the 1.0 folder rule).
#[cfg(test)]
pub(crate) fn attribute(extension: &str, folders: &[String]) -> Attribution {
    attribute_in(crate::catalog::catalog(), extension, folders)
}

/// [`attribute`] on a given catalog snapshot.
pub(crate) fn attribute_in(
    cat: &'static Catalog,
    extension: &str,
    folders: &[String],
) -> Attribution {
    let all = cat.for_extension(extension);
    if let Some(s) = all.iter().copied().find(|s| folders.contains(&s.id)) {
        return Attribution::Folder(s);
    }
    let open: Vec<&'static SystemInfo> = all
        .into_iter()
        .filter(|s| !s.is_folder_only(extension))
        .collect();
    match open.len() {
        0 => Attribution::Unclassified,
        1 => Attribution::Candidates(open),
        _ if open.iter().any(|s| s.kind == SystemKind::Computer) => Attribution::ByDat(open),
        _ => Attribution::Candidates(open),
    }
}

/// Systems accepting `extension` (case-insensitive, without the dot), in catalog order. Includes the systems that
/// restrict it to their folder (`SystemInfo::folder_extensions`).
pub fn systems_for_extension(extension: &str) -> Vec<&'static SystemInfo> {
    crate::catalog::catalog().for_extension(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_valid() {
        let parsed = parse_catalog(CATALOG).expect("embedded systems.toml");
        let ids: Vec<&str> = parsed.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "megadrive",
                "snes",
                "nes",
                "gb",
                "gbc",
                "gamegear",
                "mastersystem",
                "atarist",
                "megacd",
                "amiga",
                "dos",
                "cpc",
                "c64",
                "gba",
                "32x",
                "psx",
                "3do"
            ]
        );
        assert_eq!(systems().len(), parsed.len());
        for s in systems() {
            assert!(s.extensions.iter().all(|e| *e == e.to_lowercase()));
            assert!(s.screenscraper.is_some() && s.igdb.is_some(), "{}", s.id);
        }
    }

    #[test]
    fn atari_st_is_a_tosec_computer_and_the_others_are_unchanged() {
        let st = system("atarist").unwrap();
        assert_eq!(
            (st.kind, st.dats.as_slice()),
            (SystemKind::Computer, &[DatSource::Tosec][..])
        );
        assert_eq!(st.database, "Atari - ST");
        assert_eq!(st.extensions, ["st", "msa", "dim", "stx", "ipf", "m3u"]);
        assert_eq!(
            (st.dats[0].id(), st.dats[0].repo_dir()),
            ("tosec", "metadat/tosec")
        );
        assert_eq!(DatSource::from_id("libretro"), Some(DatSource::Libretro));
        assert!(st.folder_extensions.is_empty());
        for s in systems().iter().filter(|s| s.kind == SystemKind::Console) {
            let cd = ["megacd", "psx", "3do"].contains(&s.id.as_str());
            let dat = if cd {
                DatSource::Redump
            } else {
                DatSource::NoIntro
            };
            assert_eq!(s.dats.as_slice(), &[dat][..], "{}", s.id);
            assert_eq!(s.disc, cd, "{}", s.id);
            // Only the disc systems share their disc extensions with `dos` (design v4.0 §10 #7, v7.0 §3.4).
            assert_eq!(s.folder_extensions.is_empty(), !cd, "{}", s.id);
            assert!(!s.folder_games, "{}", s.id);
        }
        let m3u: Vec<_> = systems_for_extension("m3u").iter().map(|s| &s.id).collect();
        assert_eq!(
            m3u,
            ["atarist", "megacd", "amiga", "dos", "cpc", "c64", "psx"]
        );
        let m3u8: Vec<_> = systems_for_extension("m3u8")
            .iter()
            .map(|s| &s.id)
            .collect();
        assert_eq!(m3u8, ["dos"]);
    }

    #[test]
    fn amiga_has_two_dats_and_folder_only_extensions() {
        let amiga = system("amiga").unwrap();
        assert_eq!(amiga.kind, SystemKind::Computer);
        assert_eq!(amiga.database, "Commodore - Amiga");
        assert_eq!(amiga.dats, [DatSource::Tosec, DatSource::Libretro]);
        assert_eq!(amiga.folder_extensions, ["hdf", "hdz", "lha"]);
        assert!(amiga.is_folder_only("lha") && !amiga.is_folder_only("adf"));
        assert_eq!(
            (
                amiga.dat_rank("tosec"),
                amiga.dat_rank("libretro"),
                amiga.dat_rank("no-intro")
            ),
            (0, 1, 2)
        );
        let ipf: Vec<_> = systems_for_extension("ipf").iter().map(|s| &s.id).collect();
        assert_eq!(ipf, ["atarist", "amiga"]);
        assert_eq!(DatSource::Libretro.repo_dir(), "dat");
        for d in [
            DatSource::NoIntro,
            DatSource::Tosec,
            DatSource::Libretro,
            DatSource::Redump,
        ] {
            assert_eq!(DatSource::from_id(d.id()), Some(d));
        }
        assert_eq!(DatSource::Redump.repo_dir(), "metadat/redump");
        assert_eq!(DatSource::from_id("trurip"), None);
    }

    #[test]
    fn dos_has_folder_games_and_shares_discs_with_the_mega_cd() {
        let dos = system("dos").unwrap();
        assert_eq!((dos.kind, dos.folder_games), (SystemKind::Computer, true));
        assert_eq!(
            (dos.database.as_str(), dos.dats.as_slice()),
            ("DOS", &[DatSource::Libretro][..])
        );
        assert!(
            dos.is_folder_only("exe") && dos.is_folder_only("cue") && !dos.is_folder_only("ima")
        );
        assert!(
            !dos.is_folder_only("dosz")
                && !dos.is_folder_only("m3u8")
                && !dos.is_folder_only("jrc")
        );
        assert_eq!(
            system("megacd").unwrap().folder_extensions,
            ["cue", "chd", "iso"]
        );
        assert_eq!(systems().iter().filter(|s| s.folder_games).count(), 1);
        let cue: Vec<_> = systems_for_extension("cue").iter().map(|s| &s.id).collect();
        assert_eq!(cue, ["megacd", "dos", "psx", "3do"]);
    }

    #[test]
    fn attribution_rules() {
        let f = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let ids = |a: Attribution| match a {
            Attribution::Folder(s) => format!("folder:{}", s.id),
            Attribution::Candidates(c) => format!(
                "candidates:{}",
                c.iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Attribution::ByDat(c) => format!(
                "dat:{}",
                c.iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Attribution::Unclassified => "unclassified".into(),
        };
        assert_eq!(ids(attribute("st", &[])), "candidates:atarist");
        assert_eq!(ids(attribute("adf", &[])), "candidates:amiga");
        assert_eq!(ids(attribute("gb", &[])), "candidates:gb,gbc");
        assert_eq!(ids(attribute("gb", &f(&["roms", "gbc"]))), "folder:gbc");
        assert_eq!(ids(attribute("ipf", &[])), "dat:atarist,amiga");
        assert_eq!(ids(attribute("ipf", &f(&["amiga"]))), "folder:amiga");
        assert_eq!(ids(attribute("ipf", &f(&["atarist"]))), "folder:atarist");
        assert_eq!(ids(attribute("lha", &[])), "unclassified");
        // Discs: only under a system folder (outside, the scanner probes the Sega CD header).
        assert_eq!(ids(attribute("cue", &[])), "unclassified");
        assert_eq!(ids(attribute("chd", &f(&["megacd"]))), "folder:megacd");
        assert_eq!(ids(attribute("cue", &f(&["roms", "dos"]))), "folder:dos");
        assert_eq!(ids(attribute("exe", &[])), "unclassified");
        assert_eq!(ids(attribute("ima", &[])), "candidates:dos");
        assert_eq!(
            ids(attribute("m3u", &[])),
            "dat:atarist,megacd,amiga,dos,cpc,c64,psx"
        );
        // 7.0: discs shared by four systems, `.pbp` PlayStation only (design v7.0 §3.4).
        assert_eq!(ids(attribute("chd", &f(&["roms", "psx"]))), "folder:psx");
        assert_eq!(ids(attribute("iso", &f(&["3do"]))), "folder:3do");
        assert_eq!(ids(attribute("chd", &[])), "unclassified");
        assert_eq!(ids(attribute("pbp", &[])), "candidates:psx");
        assert_eq!(ids(attribute("m3u", &f(&["psx"]))), "folder:psx");
        assert_eq!(ids(attribute("m3u", &f(&["megacd"]))), "folder:megacd");
        assert_eq!(ids(attribute("32x", &[])), "candidates:32x");
        assert_eq!(ids(attribute("gba", &[])), "candidates:gba");
        // `.bin` stays Mega Drive here: the 32X header is a probe of the scanner.
        assert_eq!(ids(attribute("bin", &f(&["32x"]))), "candidates:megadrive");
        assert_eq!(ids(attribute("m3u8", &[])), "candidates:dos");
        assert_eq!(ids(attribute("lha", &f(&["atarist"]))), "unclassified");
        assert_eq!(
            ids(attribute("hdf", &f(&["roms", "amiga", "whd"]))),
            "folder:amiga"
        );
        // CPC / C64 (design v5.0 §3.4): `.tap` is a C64 tape unless under `cpc/`.
        assert_eq!(ids(attribute("tap", &[])), "candidates:c64");
        assert_eq!(ids(attribute("tap", &f(&["roms", "cpc"]))), "folder:cpc");
        assert_eq!(ids(attribute("sna", &[])), "unclassified");
        assert_eq!(ids(attribute("sna", &f(&["cpc"]))), "folder:cpc");
        assert_eq!(ids(attribute("voc", &[])), "unclassified");
        assert_eq!(ids(attribute("dsk", &[])), "candidates:cpc");
        assert_eq!(ids(attribute("cdt", &[])), "candidates:cpc");
        assert_eq!(ids(attribute("d64", &[])), "candidates:c64");
        assert_eq!(ids(attribute("prg", &[])), "candidates:c64");
        // Exact folder names, as in 1.0.
        assert_eq!(ids(attribute("hdf", &f(&["Amiga"]))), "unclassified");
    }

    #[test]
    fn cpc_and_c64_are_joystick_computers_and_the_others_keyboard() {
        let cpc = system("cpc").unwrap();
        assert_eq!(cpc.kind, SystemKind::Computer);
        assert_eq!(cpc.database, "Amstrad - CPC");
        assert_eq!(cpc.dats, [DatSource::Libretro, DatSource::Tosec]);
        assert_eq!(cpc.folder_extensions, ["sna", "voc", "tap"]);
        let c64 = system("c64").unwrap();
        assert_eq!(c64.database, "Commodore - 64");
        assert_eq!(c64.dats, [DatSource::NoIntro]);
        assert!(c64.folder_extensions.is_empty());
        for s in systems() {
            let want = matches!(s.id.as_str(), "cpc" | "c64");
            assert_eq!(s.default_input == InputDefault::Joystick, want, "{}", s.id);
        }
        let tap: Vec<_> = systems_for_extension("tap").iter().map(|s| &s.id).collect();
        assert_eq!(tap, ["cpc", "c64"]);
        let one = |extra: &str| {
            parse_catalog(&format!(
                "[[system]]\nid = \"a\"\nname = \"N\"\ndatabase = \"D\"\nextensions = [\"x\"]\n{extra}\n"
            ))
        };
        assert_eq!(one("").unwrap()[0].default_input, InputDefault::Keyboard);
        assert_eq!(
            one("default_input = \"joystick\"").unwrap()[0].default_input,
            InputDefault::Joystick
        );
        assert!(one("default_input = \"mouse\"").is_err());
    }

    #[test]
    fn lookups() {
        assert_eq!(system("gb").unwrap().database, "Nintendo - Game Boy");
        assert_eq!(system("nes").unwrap().header, Some(HeaderRule::INes));
        assert_eq!(system("snes").unwrap().header, Some(HeaderRule::SnesCopier));
        assert_eq!(system("megadrive").unwrap().header, None);
        assert!(system("ps2").is_none());
        let gbc: Vec<_> = systems_for_extension("GBC").iter().map(|s| &s.id).collect();
        assert_eq!(gbc, ["gbc"]);
        let gb: Vec<_> = systems_for_extension("gb").iter().map(|s| &s.id).collect();
        assert_eq!(gb, ["gb", "gbc"]);
        assert!(systems_for_extension("zip").is_empty());
        assert!(systems_for_extension("").is_empty());
    }

    #[test]
    fn invalid_catalogs_are_rejected() {
        let one = |body: &str| format!("[[system]]\n{body}\n");
        let base = "name = \"N\"\ndatabase = \"D\"\nextensions = [\"x\"]";
        assert!(parse_catalog(&one(&format!("id = \"a\"\n{base}"))).is_ok());
        let dup = format!(
            "{}{}",
            one(&format!("id = \"a\"\n{base}")),
            one(&format!("id = \"a\"\n{base}"))
        );
        assert!(parse_catalog(&dup).is_err());
        assert!(
            parse_catalog(&one(
                "id = \"a\"\nname = \"N\"\ndatabase = \"D\"\nextensions = [\"GB\"]"
            ))
            .is_err()
        );
        assert!(
            parse_catalog(&one(
                "id = \"a\"\nname = \"N\"\ndatabase = \"D\"\nextensions = [\".gb\"]"
            ))
            .is_err()
        );
        assert!(parse_catalog(&one(&format!("id = \"a\"\n{base}\nheader = \"x\""))).is_err());
        assert!(parse_catalog(&one(&format!("id = \"a\"\n{base}\ncores = []"))).is_err());
        assert!(parse_catalog(&one(&format!("id = \"a\"\n{base}\nkind = \"tablet\""))).is_err());
        assert!(parse_catalog(&one(&format!("id = \"a\"\n{base}\ndat = \"trurip\""))).is_err());
        assert!(
            parse_catalog(&one(&format!(
                "id = \"a\"\n{base}\nkind = \"computer\"\ndat = \"tosec\""
            )))
            .is_ok()
        );
        let parsed = parse_catalog(&one(&format!(
            "id = \"a\"\n{base}\ndat = [\"libretro\", \"no-intro\"]"
        )))
        .unwrap();
        assert_eq!(parsed[0].dats, [DatSource::Libretro, DatSource::NoIntro]);
        for bad in [
            "dat = []",
            "dat = [\"tosec\", \"tosec\"]",
            "dat = [\"tosec\", \"trurip\"]",
            "disc = true",
            "disc = \"yes\"",
            "folder_extensions = [\"y\"]",
            "folder_extensions = [\"x\", \"x\"]",
        ] {
            assert!(
                parse_catalog(&one(&format!("id = \"a\"\n{base}\n{bad}"))).is_err(),
                "{bad}"
            );
        }
        let m3u = "id = \"a\"\nname = \"N\"\ndatabase = \"D\"\nextensions = [\"x\", \"m3u\"]";
        assert!(parse_catalog(&one(&format!("{m3u}\nfolder_extensions = [\"x\"]"))).is_ok());
        assert!(parse_catalog(&one(&format!("{m3u}\nfolder_extensions = [\"m3u\"]"))).is_err());
    }

    #[test]
    fn v6_systems_gba_32x_and_the_mega_cd_disc_system() {
        let gba = system("gba").unwrap();
        assert_eq!(
            (gba.database.as_str(), gba.extensions.as_slice(), gba.kind),
            (
                "Nintendo - Game Boy Advance",
                &["gba".to_owned()][..],
                SystemKind::Console
            )
        );
        assert_eq!((gba.screenscraper, gba.igdb), (Some(12), Some(24)));
        let s32x = system("32x").unwrap();
        assert_eq!(s32x.database, "Sega - 32X");
        assert_eq!(s32x.extensions, ["32x"]);
        assert_eq!((s32x.screenscraper, s32x.igdb), (Some(19), Some(30)));
        let cd = system("megacd").unwrap();
        assert_eq!(cd.database, "Sega - Mega-CD - Sega CD");
        assert_eq!(cd.extensions, ["cue", "chd", "iso", "m3u"]);
        assert_eq!(cd.dats, [DatSource::Redump]);
        assert!(cd.disc && !gba.disc && !s32x.disc);
        assert_eq!(systems().iter().filter(|s| s.disc).count(), 3);
        assert!(cd.group_discs);
        let disc =
            "id = \"a\"\nname = \"N\"\ndatabase = \"D\"\nextensions = [\"iso\"]\ndisc = true";
        let parsed = parse_catalog(&format!("[[system]]\n{disc}\ndat = \"redump\"\n")).unwrap();
        assert!(parsed[0].disc && parsed[0].group_discs);
        assert_eq!(parsed[0].dats, [DatSource::Redump]);
        let single = parse_catalog(&format!(
            "[[system]]\n{disc}\ndat = \"redump\"\ngroup_discs = false\n"
        ))
        .unwrap();
        assert!(!single[0].group_discs);
    }

    #[test]
    fn v7_playstation_and_3do() {
        let psx = system("psx").unwrap();
        assert_eq!(
            (psx.name.as_str(), psx.database.as_str(), psx.kind),
            ("PlayStation", "Sony - PlayStation", SystemKind::Console)
        );
        assert_eq!(psx.extensions, ["cue", "chd", "iso", "pbp", "m3u"]);
        assert_eq!(psx.folder_extensions, ["cue", "chd", "iso"]);
        assert_eq!(psx.dats, [DatSource::Redump]);
        assert!(psx.disc && psx.group_discs);
        assert_eq!((psx.screenscraper, psx.igdb), (Some(57), Some(7)));
        let tdo = system("3do").unwrap();
        assert_eq!(
            (tdo.name.as_str(), tdo.database.as_str(), tdo.kind),
            ("3DO", "The 3DO Company - 3DO", SystemKind::Console)
        );
        assert_eq!(tdo.extensions, ["cue", "chd", "iso"]);
        assert_eq!(tdo.folder_extensions, ["cue", "chd", "iso"]);
        assert_eq!(tdo.dats, [DatSource::Redump]);
        assert!(tdo.disc && !tdo.group_discs);
        assert_eq!((tdo.screenscraper, tdo.igdb), (Some(29), Some(50)));
        assert_eq!(crate::DiscFormat::Psx.system_id(), psx.id);
        assert_eq!(crate::DiscFormat::Opera.system_id(), tdo.id);
    }
}
