//! Custom system definitions (design 8.0 §2.1, ADR 0025): `<data>/systems/<id>.toml`, validation, retirement.
//! Contract stubs (PX0): the bodies belong to lots PB1 and PC1.
#![allow(unused_variables)]

use std::path::{Path, PathBuf};

use crate::catalog::Catalog;
use crate::error::LibraryError;
use crate::systems::DatSource;

pub const CUSTOM_FORMAT: u32 = 1;
pub const MAX_DEFINITION_SIZE: u64 = 64 * 1024;
pub const MAX_DEFINITION_FILES: usize = 256;

/// 8.0 lists (ADR 0025 §9): never in a custom definition. `bin` handled by [`cartridge_extensions`].
pub const DISC_EXTENSIONS_8_0: &[&str] = &[
    "cue", "chd", "iso", "ccd", "toc", "gdi", "cdi", "mds", "mdf", "nrg", "pbp", "cso", "img",
    "bin",
];
pub const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "7z"];
pub const PLAYLIST_EXTENSIONS: &[&str] = &["m3u", "m3u8"];

/// An active custom definition as the catalog holds it (the system itself is a `SystemInfo` of the catalog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomSystem {
    pub id: String,
    pub cores: Vec<String>,
    pub ignored_firmware: Vec<String>,
    pub origin: DefinitionOrigin,
    pub file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionOrigin {
    pub core: String,
    pub info_name: Option<String>,
    pub created_by: String,
    pub created_unix: u64,
}

/// What the settings and the « add » view edit; written by [`write_custom`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomSystemDraft {
    pub id: String,
    pub name: String,
    pub database: Option<String>,
    pub extensions: Vec<String>,
    pub dats: Vec<DatSource>,
    pub screenscraper: Option<u32>,
    pub igdb: Option<u32>,
    pub cores: Vec<String>,
    pub ignored_firmware: Vec<String>,
    pub origin: DefinitionOrigin,
}

impl CustomSystemDraft {
    pub fn from_custom(catalog: &Catalog, id: &str) -> Option<Self> {
        todo!("lot PC1")
    }
}

pub fn validate_custom(
    draft: &CustomSystemDraft,
    catalog: &Catalog,
    creating: bool,
) -> Result<(), DefinitionError> {
    todo!("lot PC1")
}

pub fn write_custom(
    dir: &Path,
    draft: &CustomSystemDraft,
    catalog: &Catalog,
) -> Result<PathBuf, LibraryError> {
    todo!("lot PC1")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Retirement {
    Removed,
    Replaced,
}

/// `<id>-<unix>.toml` (Removed) or `<id>-<unix>.replaced.toml` (Replaced); the only names `.retired/` accepts.
pub fn retired_file_name(id: &str, why: Retirement, unix: u64) -> String {
    todo!("lot PC1")
}

/// Never deletes; refuses an existing target name. `unix` = the switch journal's (ADR 0025 §10.4), else now.
pub fn retire_custom(
    dir: &Path,
    id: &str,
    why: Retirement,
    unix: u64,
) -> Result<PathBuf, LibraryError> {
    todo!("lot PC1")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredSystem {
    pub id: String,
    pub file: PathBuf,
    pub why: Retirement,
    pub retired_unix: u64,
}

/// Retired definitions (« Systèmes retirés » in the settings). The settings offer « Restaurer » for `Removed`
/// only (ADR 0025 §9); the switch undo restores a `Replaced` one.
pub fn retired_customs(dir: &Path) -> Vec<RetiredSystem> {
    todo!("lot PC1")
}

/// Restores a retired definition to `<dir>/<id>.toml`.
pub fn restore_custom(
    dir: &Path,
    retired: &Path,
    catalog: &Catalog,
) -> Result<PathBuf, LibraryError> {
    todo!("lot PC1")
}

/// Display name proposed at creation: forbidden characters → « - », spaces collapsed, ≤ 64.
pub fn normalize_name(raw: &str) -> String {
    todo!("lot PC1")
}

pub fn proposed_id(database: Option<&str>, name: &str, taken: &dyn Fn(&str) -> bool) -> String {
    todo!("lot PC1")
}

/// Table ids are reserved to their own database: `proposed_id` / `validate_custom` treat them as taken otherwise.
pub fn known_id(database: &str) -> Option<&'static str> {
    todo!("lot PC1")
}

pub fn is_reserved_id(id: &str, database: Option<&str>) -> bool {
    todo!("lot PC1")
}

/// Local DAT file stem: the database name, forbidden characters → « _ » (table exceptions, ADR 0025 §2).
pub fn dat_file_stem(database: &str) -> String {
    todo!("lot PC1")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MachineTier {
    Cartridge,
    Disc,
    Computer,
    Arcade,
}

pub fn known_tier(database: &str) -> Option<MachineTier> {
    todo!("lot PC1")
}

/// Lower-cased, `[a-z0-9]{1,8}`, minus archives, playlists and 8.0 disc extensions (`bin` kept when it is the
/// only disc-like extension of the core).
pub fn cartridge_extensions(core_extensions: &[String]) -> Vec<String> {
    todo!("lot PC1")
}

/// Copy of `emu_assets`' private rule (equality checked by an emu-ui test).
pub fn is_safe_relative_path(path: &str) -> bool {
    todo!("lot PC1")
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DefinitionError {
    #[error("syntaxe invalide : {0}")]
    Syntax(String),
    #[error("définition trop grande")]
    TooLarge,
    #[error("ce n'est pas un fichier ordinaire")]
    NotRegularFile,
    #[error("format {0} plus récent que cette version")]
    NewerFormat(u32),
    #[error("rendu non géré : {0}")]
    UnsupportedRender(String),
    #[error("identifiant invalide")]
    InvalidId,
    #[error("identifiant réservé")]
    ReservedId,
    #[error("l'identifiant n'est pas le nom du fichier")]
    IdNotFileName,
    #[error("identifiant déjà pris")]
    DuplicateId,
    #[error("base d'identification déjà prise")]
    DuplicateDatabase,
    #[error("nom invalide")]
    InvalidName,
    #[error("nom de base invalide")]
    InvalidDatabase,
    #[error("aucune extension")]
    NoExtension,
    #[error("extension invalide : {0}")]
    InvalidExtension(String),
    #[error("extension interdite : {0}")]
    ForbiddenExtension(String),
    #[error("source de DAT non gérée : {0}")]
    UnsupportedDat(String),
    #[error("aucun core")]
    NoCore,
    #[error("trop de cores")]
    TooManyCores,
    #[error("core invalide : {0}")]
    InvalidCore(String),
    #[error("origine invalide")]
    InvalidOrigin,
    #[error("chemin de firmware refusé : {0}")]
    UnsafeFirmwarePath(String),
    #[error("trop de fichiers de firmware")]
    TooManyFirmware,
}
