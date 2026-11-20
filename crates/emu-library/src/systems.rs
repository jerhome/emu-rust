//! System catalog, parsed once from the embedded `data/systems.toml` (ADR 0014 §1).

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Deserialize;

const CATALOG: &str = include_str!("../data/systems.toml");

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
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [DatSource::NoIntro, DatSource::Tosec, DatSource::Libretro]
            .into_iter()
            .find(|s| s.id() == id)
    }

    /// Folder of the DAT in the libretro-database repository.
    pub fn repo_dir(self) -> &'static str {
        match self {
            DatSource::NoIntro => "metadat/no-intro",
            DatSource::Tosec => "metadat/tosec",
            DatSource::Libretro => "dat",
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

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum RawDat {
    #[default]
    NoIntro,
    Tosec,
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
    dat: RawDat,
}

/// Parses and validates a catalog: unique non-empty ids, lower-case dot-less extensions, non-empty database.
fn parse_catalog(text: &str) -> Result<Vec<SystemInfo>, String> {
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
            // TODO(B3): `dat` as a string or a list (`libretro` included), `folder_extensions` parsed and
            // checked against `extensions`.
            dats: vec![match s.dat {
                RawDat::NoIntro => DatSource::NoIntro,
                RawDat::Tosec => DatSource::Tosec,
            }],
            folder_extensions: Vec::new(),
        });
    }
    Ok(out)
}

/// The whole catalog, in file order. The embedded file is checked by a unit test, so parsing cannot fail at run time.
pub fn systems() -> &'static [SystemInfo] {
    static SYSTEMS: OnceLock<Vec<SystemInfo>> = OnceLock::new();
    SYSTEMS.get_or_init(|| {
        parse_catalog(CATALOG).unwrap_or_else(|e| {
            // Unreachable in a tested build; an empty catalog keeps the app usable rather than aborting.
            tracing::error!("embedded systems.toml is invalid: {e}");
            Vec::new()
        })
    })
}

pub fn system(id: &str) -> Option<&'static SystemInfo> {
    systems().iter().find(|s| s.id == id)
}

/// Systems accepting `extension` (case-insensitive, without the dot), in catalog order.
pub fn systems_for_extension(extension: &str) -> Vec<&'static SystemInfo> {
    systems()
        .iter()
        .filter(|s| {
            s.extensions
                .iter()
                .any(|e| e.eq_ignore_ascii_case(extension))
        })
        .collect()
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
                "atarist"
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
        for s in systems().iter().filter(|s| s.id != "atarist") {
            assert_eq!(
                (s.kind, s.dats.as_slice()),
                (SystemKind::Console, &[DatSource::NoIntro][..])
            );
        }
        assert!(
            systems_for_extension("m3u")
                .iter()
                .all(|s| s.id == "atarist")
        );
    }

    #[test]
    fn lookups() {
        assert_eq!(system("gb").unwrap().database, "Nintendo - Game Boy");
        assert_eq!(system("nes").unwrap().header, Some(HeaderRule::INes));
        assert_eq!(system("snes").unwrap().header, Some(HeaderRule::SnesCopier));
        assert_eq!(system("megadrive").unwrap().header, None);
        assert!(system("psx").is_none());
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
        assert!(parse_catalog(&one(&format!("id = \"a\"\n{base}\ndat = \"redump\""))).is_err());
        assert!(
            parse_catalog(&one(&format!(
                "id = \"a\"\n{base}\nkind = \"computer\"\ndat = \"tosec\""
            )))
            .is_ok()
        );
    }
}
