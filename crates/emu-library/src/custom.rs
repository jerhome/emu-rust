//! Custom system definitions (design 8.0 §2.1, ADR 0025): `<data>/systems/<id>.toml`, validation, retirement.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::catalog::Catalog;
use crate::error::LibraryError;
use crate::systems::{DatSource, InputDefault, SystemInfo, SystemKind, SystemOrigin};

#[cfg(test)]
mod tests;

pub(crate) struct Machine {
    pub(crate) database: String,
    pub(crate) id: String,
    pub(crate) tier: MachineTier,
}

/// Folder names of `<data>` and Windows device names: never the id of a system (ADR 0025 §2).
const DATA_FOLDERS: &[&str] = &[
    "roms",
    "system",
    "saves",
    "states",
    "cores",
    "cache",
    "logs",
    "covers",
    "database",
    "rangement",
    "systems",
    "assets",
    "gameoptions",
];

/// Longest id, name, database, extension, core id, quoted text (error messages).
const MAX_ID: usize = 24;
const MAX_NAME: usize = 64;
const MAX_DATABASE: usize = 128;
const MAX_EXTENSION: usize = 8;
const MAX_EXTENSIONS: usize = 64;
const MAX_CORES: usize = 8;
const MAX_CORE_ID: usize = 64;
const MAX_INFO_NAME: usize = 128;
const MAX_CREATED_BY: usize = 32;
const MAX_IGNORED_FIRMWARE: usize = 64;
const MAX_QUOTED: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    machine: Vec<RawMachine>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMachine {
    database: String,
    id: String,
    tier: RawTier,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawTier {
    Cartridge,
    Disc,
    Computer,
    Arcade,
}

struct Table {
    machines: Vec<Machine>,
    by_database: HashMap<String, usize>,
    ids: HashSet<String>,
}

const TABLE: &str = include_str!("../data/libretro_ids.toml");

fn table() -> &'static Table {
    static TABLE_ONCE: OnceLock<Table> = OnceLock::new();
    TABLE_ONCE.get_or_init(|| {
        let raw: RawTable = toml::from_str(TABLE).unwrap_or_else(|e| {
            // Unreachable in a tested build; no table keeps every id free rather than aborting.
            tracing::error!("embedded libretro_ids.toml is invalid: {e}");
            RawTable {
                machine: Vec::new(),
            }
        });
        let machines: Vec<Machine> = raw
            .machine
            .into_iter()
            .map(|m| Machine {
                database: m.database,
                id: m.id,
                tier: match m.tier {
                    RawTier::Cartridge => MachineTier::Cartridge,
                    RawTier::Disc => MachineTier::Disc,
                    RawTier::Computer => MachineTier::Computer,
                    RawTier::Arcade => MachineTier::Arcade,
                },
            })
            .collect();
        Table {
            by_database: machines
                .iter()
                .enumerate()
                .map(|(i, m)| (m.database.clone(), i))
                .collect(),
            ids: machines.iter().map(|m| m.id.clone()).collect(),
            machines,
        }
    })
}

#[cfg(test)]
pub(crate) fn machines() -> &'static [Machine] {
    &table().machines
}

/// `len` bytes of lower-case ASCII letters and digits, plus `extra`.
fn is_lower_token(s: &str, len: std::ops::RangeInclusive<usize>, extra: &[u8]) -> bool {
    len.contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra.contains(&b))
}

/// `[a-z0-9]{1,24}`.
pub(crate) fn is_valid_id_syntax(id: &str) -> bool {
    is_lower_token(id, 1..=MAX_ID, b"")
}

fn is_windows_device_id(id: &str) -> bool {
    matches!(id, "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|prefix| {
            id.strip_prefix(prefix)
                .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
        })
}

/// Names a folder of `<data>` or a Windows device: never an id, whatever the database.
pub(crate) fn is_forbidden_id(id: &str) -> bool {
    is_windows_device_id(id) || DATA_FOLDERS.contains(&id)
}

fn is_label_char(c: char) -> bool {
    c.is_alphanumeric() || " -_+.,'&!()".contains(c)
}

/// White list of `name` and `database` (ADR 0025 §2): 1 to `max` characters, letters, digits, space and `-_+.,'&!()`
/// only, no `..`, no space or dot at either end.
pub(crate) fn is_valid_label(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.chars().count())
        && s.chars().all(is_label_char)
        && !s.contains("..")
        && !s.starts_with([' ', '.'])
        && !s.ends_with([' ', '.'])
}

/// At most [`MAX_QUOTED`] characters of untrusted text, for an error message.
fn clip(s: &str) -> String {
    s.chars().take(MAX_QUOTED).collect()
}

fn is_unsafe_text_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Displayed text (`info_name`, `created_by`): control and bidirectional characters removed, nothing else changed.
fn clean_text(s: &str) -> String {
    s.chars().filter(|c| !is_unsafe_text_char(*c)).collect()
}

/// `[a-z0-9]{1,8}`.
fn is_valid_extension(ext: &str) -> bool {
    is_lower_token(ext, 1..=MAX_EXTENSION, b"")
}

/// `[a-z0-9_-]{1,64}`: the syntax of a core id (`emu_assets::CoreId`).
fn is_valid_core_id(id: &str) -> bool {
    is_lower_token(id, 1..=MAX_CORE_ID, b"_-")
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

/// Lower-case ASCII letters and digits, 24 at most: of the part after the manufacturer (`Acme - Foo` -> `foo`)
/// when `manufacturer` is set and the name has one.
fn reduce(name: &str, manufacturer: bool) -> String {
    let part = if manufacturer {
        name.split_once(" - ").map_or(name, |(_, rest)| rest)
    } else {
        name
    };
    let reduced = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .take(MAX_ID)
            .collect()
    };
    let r = reduced(part);
    if r.is_empty() && manufacturer {
        reduced(name)
    } else {
        r
    }
}

/// Files of a definition, as TOML (format 1, ADR 0025 §2). The writer and the reader share these shapes.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawDefinition {
    format: u32,
    render: String,
    system: RawSystem,
    cores: RawCores,
    origin: RawOrigin,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawSystem {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    database: Option<String>,
    extensions: Vec<String>,
    #[serde(default)]
    dat: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    screenscraper: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    igdb: Option<u32>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawCores {
    list: Vec<String>,
    #[serde(default)]
    ignored_firmware: Vec<String>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawOrigin {
    core: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    info_name: Option<String>,
    created_by: String,
    created_unix: u64,
}

/// A parse message without the document: bounded, since a definition is a file the user may have edited by hand.
fn syntax(message: impl std::fmt::Display) -> DefinitionError {
    DefinitionError::Syntax(message.to_string().chars().take(200).collect())
}

/// Text of a definition to a draft (no validation but the format, the render and the DAT names).
fn parse_definition(bytes: &[u8]) -> Result<CustomSystemDraft, DefinitionError> {
    let text = std::str::from_utf8(bytes).map_err(|_| syntax("texte non UTF-8"))?;
    let table: toml::Table = toml::from_str(text).map_err(|e| syntax(e.message()))?;
    match table.get("format").and_then(toml::Value::as_integer) {
        Some(1) => {}
        Some(n) if n > i64::from(CUSTOM_FORMAT) => {
            return Err(DefinitionError::NewerFormat(
                u32::try_from(n).unwrap_or(u32::MAX),
            ));
        }
        _ => return Err(syntax("format absent ou invalide")),
    }
    if let Some(render) = table.get("render").and_then(toml::Value::as_str)
        && render != "software"
    {
        return Err(DefinitionError::UnsupportedRender(clip(render)));
    }
    let raw: RawDefinition = table.try_into().map_err(|e| syntax(e.message()))?;
    let mut dats = Vec::with_capacity(raw.system.dat.len());
    for name in &raw.system.dat {
        dats.push(
            DatSource::from_id(name).ok_or_else(|| DefinitionError::UnsupportedDat(clip(name)))?,
        );
    }
    Ok(CustomSystemDraft {
        id: raw.system.id,
        name: raw.system.name,
        database: raw.system.database,
        extensions: raw.system.extensions,
        dats,
        screenscraper: raw.system.screenscraper,
        igdb: raw.system.igdb,
        cores: raw.cores.list,
        ignored_firmware: raw.cores.ignored_firmware,
        origin: DefinitionOrigin {
            core: raw.origin.core,
            info_name: raw.origin.info_name.map(|n| clean_text(&n)),
            created_by: clean_text(&raw.origin.created_by),
            created_unix: raw.origin.created_unix,
        },
    })
}

/// Opens a regular file without ever blocking (a FIFO swapped in after the check must not hang the start).
fn open_regular(path: &Path) -> Result<fs::File, DefinitionError> {
    let meta = fs::symlink_metadata(path).map_err(syntax)?;
    if !meta.file_type().is_file() {
        return Err(DefinitionError::NotRegularFile);
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(syntax)?;
    if !file.metadata().map_err(syntax)?.is_file() {
        return Err(DefinitionError::NotRegularFile);
    }
    Ok(file)
}

/// Reads one definition file: regular, at most [`MAX_DEFINITION_SIZE`] bytes.
pub(crate) fn read_definition(path: &Path) -> Result<CustomSystemDraft, DefinitionError> {
    let file = open_regular(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_DEFINITION_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(syntax)?;
    if bytes.len() as u64 > MAX_DEFINITION_SIZE {
        return Err(DefinitionError::TooLarge);
    }
    parse_definition(&bytes)
}

/// A definition file of the folder `stem.toml`, read and validated against the systems already loaded.
pub(crate) fn load_definition(
    path: &Path,
    stem: &str,
    ctx: &Context,
) -> Result<CustomSystemDraft, DefinitionError> {
    let draft = read_definition(path)?;
    validate_id(&draft, ctx, false)?;
    if draft.id != stem {
        return Err(DefinitionError::IdNotFileName);
    }
    validate_fields(&draft, ctx)?;
    Ok(draft)
}

/// The system of the catalog: every extension is valid under `roms/<id>/` only (ADR 0025 §12), cartridges only.
pub(crate) fn build_system(draft: &CustomSystemDraft) -> SystemInfo {
    let mut extensions: Vec<String> = Vec::new();
    for ext in &draft.extensions {
        if !extensions.contains(ext) {
            extensions.push(ext.clone());
        }
    }
    let mut dats: Vec<DatSource> = Vec::new();
    for dat in &draft.dats {
        if !dats.contains(dat) {
            dats.push(*dat);
        }
    }
    SystemInfo {
        id: draft.id.clone(),
        name: draft.name.clone(),
        folder_extensions: extensions.clone(),
        extensions,
        header: None,
        // Empty while the field is public (design 8.0 A10): `libretro_name()` is then `None`.
        database: draft.database.clone().unwrap_or_default(),
        screenscraper: draft.screenscraper,
        igdb: draft.igdb,
        kind: SystemKind::Console,
        dats,
        folder_games: false,
        default_input: InputDefault::Keyboard,
        disc: false,
        group_discs: true,
        origin: SystemOrigin::Custom,
    }
}

pub(crate) fn build_custom(draft: &CustomSystemDraft, file: &Path) -> CustomSystem {
    CustomSystem {
        id: draft.id.clone(),
        cores: draft.cores.clone(),
        ignored_firmware: draft.ignored_firmware.clone(),
        origin: draft.origin.clone(),
        file: file.to_path_buf(),
    }
}

fn io_error(path: &Path, source: std::io::Error) -> LibraryError {
    LibraryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn invalid(path: &Path, message: &str) -> LibraryError {
    io_error(
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidInput, message.to_owned()),
    )
}

/// Moves `from` to `to` without ever overwriting: a hard link then the removal of the first name; an exclusive
/// copy where the file system has no links. Refuses an existing `to` (nothing changes then). Never deletes data:
/// the source goes away only once the target holds it.
pub(crate) fn move_no_overwrite(from: &Path, to: &Path) -> Result<(), LibraryError> {
    match fs::hard_link(from, to) {
        Ok(()) => fs::remove_file(from).map_err(|e| io_error(from, e)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(io_error(to, e)),
        Err(_) => {
            let mut source = fs::File::open(from).map_err(|e| io_error(from, e))?;
            let mut target = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(to)
                .map_err(|e| io_error(to, e))?;
            let copied = std::io::copy(&mut source, &mut target)
                .and_then(|_| target.sync_all())
                .map_err(|e| io_error(to, e));
            if let Err(e) = copied {
                // Our own partial copy: the source is untouched.
                let _ = fs::remove_file(to);
                return Err(e);
            }
            fs::remove_file(from).map_err(|e| io_error(from, e))
        }
    }
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

/// `.retired/` of a definitions folder.
pub(crate) fn retired_dir(dir: &Path) -> PathBuf {
    dir.join(".retired")
}

/// `<id>-<unix>.toml` or `<id>-<unix>.replaced.toml`, id `[a-z0-9]{1,24}`, `unix` in canonical decimal: the only
/// names `.retired/` accepts.
fn parse_retired_name(name: &str) -> Option<(String, Retirement, u64)> {
    let (stem, why) = if let Some(stem) = name.strip_suffix(".replaced.toml") {
        (stem, Retirement::Replaced)
    } else {
        (name.strip_suffix(".toml")?, Retirement::Removed)
    };
    let (id, unix) = stem.rsplit_once('-')?;
    let number: u64 = unix.parse().ok()?;
    (is_valid_id_syntax(id) && number.to_string() == unix).then(|| (id.to_owned(), why, number))
}

/// Names read at most from `.retired/` (a folder the user may fill).
pub(crate) const MAX_RETIRED_READ: usize = 1024;

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
        let custom = catalog.custom(id)?;
        let system = catalog.system(id).filter(|s| s.is_custom())?;
        Some(CustomSystemDraft {
            id: system.id.clone(),
            name: system.name.clone(),
            database: system.libretro_name().map(str::to_owned),
            extensions: system.extensions.clone(),
            dats: system.dats.clone(),
            screenscraper: system.screenscraper,
            igdb: system.igdb,
            cores: custom.cores.clone(),
            ignored_firmware: custom.ignored_firmware.clone(),
            origin: custom.origin.clone(),
        })
    }
}

/// What the validation needs to know of the other systems of a catalog (built once per load, or per call).
pub(crate) struct Context {
    /// Ids of the official systems, active or masked.
    officials: HashSet<String>,
    /// Every active id.
    active: HashSet<String>,
    /// `database` -> id of the custom system that owns it.
    database_owner: HashMap<String, String>,
}

impl Context {
    pub(crate) fn of(catalog: &Catalog) -> Self {
        let mut ctx = Context::officials(catalog.systems().iter().filter(|s| !s.is_custom()));
        ctx.officials
            .extend(catalog.masked_official().iter().map(|s| s.id.clone()));
        for s in catalog.systems().iter().filter(|s| s.is_custom()) {
            ctx.add_custom(s);
        }
        ctx
    }

    /// The official systems only.
    pub(crate) fn officials<'a>(officials: impl Iterator<Item = &'a SystemInfo>) -> Self {
        let ids: HashSet<String> = officials.map(|s| s.id.clone()).collect();
        Context {
            active: ids.clone(),
            officials: ids,
            database_owner: HashMap::new(),
        }
    }

    pub(crate) fn add_custom(&mut self, system: &SystemInfo) {
        self.active.insert(system.id.clone());
        if let Some(database) = system.libretro_name() {
            self.database_owner
                .entry(database.to_owned())
                .or_insert_with(|| system.id.clone());
        }
    }
}

/// Validation of the id, in the order of the errors (ADR 0025 §2, §3). `creating`: a new definition; otherwise the id
/// of a system that already exists (a file being read, an edit), where an id of the table taken by an official
/// system is a conflict of the catalog, not an error (§10).
pub(crate) fn validate_id(
    draft: &CustomSystemDraft,
    ctx: &Context,
    creating: bool,
) -> Result<(), DefinitionError> {
    if !is_valid_id_syntax(&draft.id) {
        return Err(DefinitionError::InvalidId);
    }
    if is_forbidden_id(&draft.id) {
        return Err(DefinitionError::ReservedId);
    }
    if is_reserved_id(&draft.id, draft.database.as_deref())
        && (creating || !ctx.officials.contains(&draft.id))
    {
        return Err(DefinitionError::ReservedId);
    }
    if creating && ctx.active.contains(&draft.id) {
        return Err(DefinitionError::DuplicateId);
    }
    Ok(())
}

/// Everything but the id.
pub(crate) fn validate_fields(
    draft: &CustomSystemDraft,
    ctx: &Context,
) -> Result<(), DefinitionError> {
    if !is_valid_label(&draft.name, MAX_NAME) {
        return Err(DefinitionError::InvalidName);
    }
    if let Some(database) = &draft.database {
        // A name of the table is a trusted value, accepted even outside the white list.
        if !is_valid_label(database, MAX_DATABASE) && known_id(database).is_none() {
            return Err(DefinitionError::InvalidDatabase);
        }
        if ctx
            .database_owner
            .get(database)
            .is_some_and(|owner| *owner != draft.id)
        {
            return Err(DefinitionError::DuplicateDatabase);
        }
    }
    if draft.extensions.is_empty() {
        return Err(DefinitionError::NoExtension);
    }
    if draft.extensions.len() > MAX_EXTENSIONS {
        return Err(DefinitionError::InvalidExtension(format!(
            "plus de {MAX_EXTENSIONS} extensions"
        )));
    }
    for ext in &draft.extensions {
        if !is_valid_extension(ext) {
            return Err(DefinitionError::InvalidExtension(clip(ext)));
        }
        let listed = ARCHIVE_EXTENSIONS.contains(&ext.as_str())
            || PLAYLIST_EXTENSIONS.contains(&ext.as_str())
            // `bin` is the extension of cartridges too (Atari 2600): `cartridge_extensions` decides per core.
            || (ext != "bin" && DISC_EXTENSIONS_8_0.contains(&ext.as_str()));
        if listed {
            return Err(DefinitionError::ForbiddenExtension(ext.clone()));
        }
    }
    if let Some(dat) = draft.dats.iter().find(|d| **d == DatSource::Redump) {
        return Err(DefinitionError::UnsupportedDat(dat.id().to_owned()));
    }
    if draft.cores.is_empty() {
        return Err(DefinitionError::NoCore);
    }
    if draft.cores.len() > MAX_CORES {
        return Err(DefinitionError::TooManyCores);
    }
    if let Some(core) = draft.cores.iter().find(|c| !is_valid_core_id(c)) {
        return Err(DefinitionError::InvalidCore(clip(core)));
    }
    let origin = &draft.origin;
    let info_ok = origin
        .info_name
        .as_deref()
        .is_none_or(|n| clean_text(n).chars().count() <= MAX_INFO_NAME);
    if !is_valid_core_id(&origin.core)
        || !info_ok
        || clean_text(&origin.created_by).chars().count() > MAX_CREATED_BY
    {
        return Err(DefinitionError::InvalidOrigin);
    }
    if draft.ignored_firmware.len() > MAX_IGNORED_FIRMWARE {
        return Err(DefinitionError::TooManyFirmware);
    }
    if let Some(path) = draft
        .ignored_firmware
        .iter()
        .find(|p| !is_safe_relative_path(p))
    {
        return Err(DefinitionError::UnsafeFirmwarePath(clip(path)));
    }
    Ok(())
}

pub fn validate_custom(
    draft: &CustomSystemDraft,
    catalog: &Catalog,
    creating: bool,
) -> Result<(), DefinitionError> {
    let ctx = Context::of(catalog);
    validate_id(draft, &ctx, creating)?;
    validate_fields(draft, &ctx)
}

/// Validates, then writes `<dir>/<id>.toml` atomically (a temporary file of the same folder, synced, renamed). A
/// definition the catalog already has is replaced (an edit); a new one never overwrites an existing file, even one
/// the catalog could not read. Text shown to the user (`info_name`, `created_by`) is cleaned of control and
/// bidirectional characters.
pub fn write_custom(
    dir: &Path,
    draft: &CustomSystemDraft,
    catalog: &Catalog,
) -> Result<PathBuf, LibraryError> {
    let creating = catalog.custom(&draft.id).is_none();
    validate_custom(draft, catalog, creating).map_err(LibraryError::Definition)?;
    let raw = RawDefinition {
        format: CUSTOM_FORMAT,
        render: "software".to_owned(),
        system: RawSystem {
            id: draft.id.clone(),
            name: draft.name.clone(),
            database: draft.database.clone(),
            extensions: draft.extensions.clone(),
            dat: draft.dats.iter().map(|d| d.id().to_owned()).collect(),
            screenscraper: draft.screenscraper,
            igdb: draft.igdb,
        },
        cores: RawCores {
            list: draft.cores.clone(),
            ignored_firmware: draft.ignored_firmware.clone(),
        },
        origin: RawOrigin {
            core: draft.origin.core.clone(),
            info_name: draft.origin.info_name.as_deref().map(clean_text),
            created_by: clean_text(&draft.origin.created_by),
            created_unix: draft.origin.created_unix,
        },
    };
    let text = toml::to_string(&raw).map_err(|e| invalid(dir, &e.to_string()))?;
    if creating && crate::catalog::definition_count(dir) >= MAX_DEFINITION_FILES {
        return Err(LibraryError::Definition(
            DefinitionError::TooManyDefinitions,
        ));
    }
    fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
    remove_stale_temporaries(dir, &draft.id);
    let target = dir.join(format!("{}.toml", draft.id));
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let temporary = dir.join(format!(
        "{TEMPORARY_PREFIX}{}{TEMPORARY_INFIX}{}-{}",
        draft.id,
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        let placed = if creating {
            // Never overwrites a file that appeared since the catalog was read (the user's own definition).
            match move_no_overwrite(&temporary, &target) {
                Ok(()) => Ok(()),
                Err(LibraryError::Io { source, .. }) => Err(source),
                Err(other) => Err(std::io::Error::other(other.to_string())),
            }
        } else {
            fs::rename(&temporary, &target)
        };
        placed?;
        sync_dir(dir);
        Ok::<(), std::io::Error>(())
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(&target, e));
    }
    Ok(target)
}

const TEMPORARY_PREFIX: &str = ".";
const TEMPORARY_INFIX: &str = ".tmp-";

/// Temporaries `.<id>.tmp-*` left by a process that died between the write and the move: never definitions.
fn remove_stale_temporaries(dir: &Path, id: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let prefix = format!("{TEMPORARY_PREFIX}{id}{TEMPORARY_INFIX}");
    for e in entries.flatten().take(MAX_RETIRED_READ * 4) {
        if e.file_name().to_string_lossy().starts_with(&prefix)
            && e.file_type().is_ok_and(|t| t.is_file())
        {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// Makes the new name durable (Unix: a directory can be synced; elsewhere nothing to do).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = fs::File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Retirement {
    Removed,
    Replaced,
}

/// `<id>-<unix>.toml` (Removed) or `<id>-<unix>.replaced.toml` (Replaced); the only names `.retired/` accepts.
pub fn retired_file_name(id: &str, why: Retirement, unix: u64) -> String {
    match why {
        Retirement::Removed => format!("{id}-{unix}.toml"),
        Retirement::Replaced => format!("{id}-{unix}.replaced.toml"),
    }
}

/// Never deletes; refuses an existing target name. `unix` = the switch journal's (ADR 0025 §10.4), else now.
pub fn retire_custom(
    dir: &Path,
    id: &str,
    why: Retirement,
    unix: u64,
) -> Result<PathBuf, LibraryError> {
    if !is_valid_id_syntax(id) {
        return Err(invalid(dir, "identifiant de système invalide"));
    }
    let from = dir.join(format!("{id}.toml"));
    if !is_regular_file(&from) {
        return Err(invalid(&from, "définition introuvable"));
    }
    let kept = retired_dir(dir);
    fs::create_dir_all(&kept).map_err(|e| io_error(&kept, e))?;
    let to = kept.join(retired_file_name(id, why, unix));
    move_no_overwrite(&from, &to)?;
    Ok(to)
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
    let Ok(entries) = fs::read_dir(retired_dir(dir)) else {
        return Vec::new();
    };
    let mut names: Vec<(String, PathBuf)> = entries
        .flatten()
        .take(MAX_RETIRED_READ)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .collect();
    names.sort();
    let mut out: Vec<RetiredSystem> = names
        .into_iter()
        .filter_map(|(name, file)| {
            let (id, why, retired_unix) = parse_retired_name(&name)?;
            Some(RetiredSystem {
                id,
                file,
                why,
                retired_unix,
            })
        })
        .collect();
    out.sort_by(|a, b| (&a.id, a.retired_unix).cmp(&(&b.id, b.retired_unix)));
    out
}

/// Restores a retired definition to `<dir>/<id>.toml`. Like a creation, refused at the limit of definition files.
pub fn restore_custom(
    dir: &Path,
    retired: &Path,
    catalog: &Catalog,
) -> Result<PathBuf, LibraryError> {
    let kept = retired_dir(dir);
    let name = retired.file_name().and_then(|n| n.to_str());
    let Some((id, _, _)) = name.and_then(parse_retired_name) else {
        return Err(invalid(retired, "nom de fichier retiré inconnu"));
    };
    if retired.parent() != Some(kept.as_path()) {
        return Err(invalid(
            retired,
            "fichier hors du dossier des systèmes retirés",
        ));
    }
    if !is_regular_file(retired) {
        return Err(invalid(retired, "fichier retiré introuvable"));
    }
    if catalog.system(&id).is_some() {
        return Err(invalid(
            retired,
            "un système actif porte déjà cet identifiant",
        ));
    }
    if crate::catalog::definition_count(dir) >= MAX_DEFINITION_FILES {
        return Err(LibraryError::Definition(
            DefinitionError::TooManyDefinitions,
        ));
    }
    let to = dir.join(format!("{id}.toml"));
    move_no_overwrite(retired, &to)?;
    Ok(to)
}

/// Display name proposed at creation: forbidden characters → « - », spaces collapsed, ≤ 64.
pub fn normalize_name(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .map(|c| if is_label_char(c) { c } else { '-' })
        .collect();
    let mut collapsed = String::with_capacity(mapped.len());
    for c in mapped.chars() {
        if c == ' ' && collapsed.ends_with(' ') {
            continue;
        }
        collapsed.push(c);
    }
    while collapsed.contains("..") {
        collapsed = collapsed.replace("..", ".");
    }
    let mut name: String = collapsed
        .trim_matches([' ', '.'])
        .chars()
        .take(MAX_NAME)
        .collect();
    // Cutting at 64 can leave a space or a dot at the end.
    name.truncate(name.trim_end_matches([' ', '.']).len());
    if name.is_empty() {
        "System".to_owned()
    } else {
        name
    }
}

pub fn proposed_id(database: Option<&str>, name: &str, taken: &dyn Fn(&str) -> bool) -> String {
    if let Some(id) = database.and_then(known_id) {
        return id.to_owned();
    }
    let base = database
        .map(|d| reduce(d, true))
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| reduce(name, false));
    let base = if base.is_empty() {
        "custom".to_owned()
    } else {
        base
    };
    let mut candidate = base.clone();
    let mut n = 1u32;
    while taken(&candidate) || is_reserved_id(&candidate, database) {
        n += 1;
        let suffix = n.to_string();
        let keep = MAX_ID - suffix.len();
        candidate = format!("{}{suffix}", &base[..base.len().min(keep)]);
    }
    candidate
}

/// Table ids are reserved to their own database: `proposed_id` / `validate_custom` treat them as taken otherwise.
pub fn known_id(database: &str) -> Option<&'static str> {
    let table = table();
    table
        .by_database
        .get(database)
        .map(|&i| table.machines[i].id.as_str())
}

pub fn is_reserved_id(id: &str, database: Option<&str>) -> bool {
    is_forbidden_id(id) || (table().ids.contains(id) && database.and_then(known_id) != Some(id))
}

/// Local DAT file stem: the database name, forbidden characters → « _ » (table exceptions, ADR 0025 §2).
pub fn dat_file_stem(database: &str) -> String {
    let mut stem: String = database
        .chars()
        .map(|c| if is_label_char(c) { c } else { '_' })
        .collect();
    while stem.contains("..") {
        stem = stem.replace("..", "._");
    }
    // A space or a dot at either end is not kept by Windows.
    let mut chars: Vec<char> = stem.chars().collect();
    if let Some(first) = chars.first_mut()
        && (*first == ' ' || *first == '.')
    {
        *first = '_';
    }
    if let Some(last) = chars.last_mut()
        && (*last == ' ' || *last == '.')
    {
        *last = '_';
    }
    chars.into_iter().collect()
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
    let table = table();
    table
        .by_database
        .get(database)
        .map(|&i| table.machines[i].tier)
}

/// Lower-cased, `[a-z0-9]{1,8}`, minus archives, playlists and 8.0 disc extensions (`bin` kept when it is the
/// only disc-like extension of the core).
pub fn cartridge_extensions(core_extensions: &[String]) -> Vec<String> {
    let mut lower: Vec<String> = Vec::new();
    for ext in core_extensions {
        let ext = ext.to_ascii_lowercase();
        if is_valid_extension(&ext) && !lower.contains(&ext) {
            lower.push(ext);
        }
    }
    let other_disc = lower
        .iter()
        .any(|e| e != "bin" && DISC_EXTENSIONS_8_0.contains(&e.as_str()));
    lower
        .into_iter()
        .filter(|e| {
            !ARCHIVE_EXTENSIONS.contains(&e.as_str())
                && !PLAYLIST_EXTENSIONS.contains(&e.as_str())
                && (!DISC_EXTENSIONS_8_0.contains(&e.as_str()) || (e == "bin" && !other_disc))
        })
        .collect()
}

/// Copy of `emu_assets`' private rule (equality checked by an emu-ui test).
pub fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|c| {
            !c.is_empty() && c.trim() == c && !c.ends_with('.') && !is_windows_device_name(c)
        })
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
    #[error("trop de définitions : fichier ignoré")]
    TooManyDefinitions,
}
