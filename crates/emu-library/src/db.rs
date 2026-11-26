//! `library.sqlite` (ADR 0014 §3). One `Library` per thread (WAL): the UI reads while a worker scans.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{
    Connection, DropBehavior, ErrorCode, OpenFlags, OptionalExtension, TransactionBehavior, params,
};

use crate::systems::{Attribution, attribute};
use crate::{
    Dat, DatSource, DiscRegion, LibraryError, RomHashes, ScanOptions, ScanProgress, ScanReport,
    SystemInfo, normalize_title, systems_for_extension,
};

/// `PRAGMA user_version` written by this code. v4 (3.0): `dat_info` / `dat_entry` keyed by `(system, source)`.
/// v5 (4.0): `game.is_dir` (unpacked DOS game folders). v6 (6.0): `game.region` (disc systems), and the Redump
/// `serial` / `region` of each `dat_entry`. v7 (7.4): `game.needs_32x` (Sega CD 32X discs).
///
/// Design 8.0: PB0 raises it to **8** (`game.scan_pending`, one migration) together with its tests; PX0 leaves
/// it at 7 so the migration tests and the existing databases keep working until then.
pub const SCHEMA_VERSION: u32 = 7;

/// What `Library::forget_system` does with the DAT rows of the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ForgetDats {
    /// Uncertain conflict: the DAT rows are the official's (ADR 0025 §9).
    Keep,
    Remove,
}

/// Contract stubs of design 8.0 §2.1 (PX0); bodies belong to lots PB1 / PB3.
#[allow(unused_variables)]
impl Library {
    /// « Oublier ses jeux »: deletes the game rows (and with [`ForgetDats::Remove`] the DAT rows) of a system
    /// `id`; `Keep` for an uncertain conflict. Explicit action only: removing a system keeps its rows hidden.
    pub fn forget_system(&mut self, id: &str, dats: ForgetDats) -> Result<(), LibraryError> {
        todo!("lot PB1")
    }

    /// Switch end (ADR 0025 §10.3, §10.5): `game.system` `from` → `to` for **every** row of `from`, one
    /// transaction, same scope forward and undo. Idempotent. Returns the number of rows changed.
    pub fn reassign_system(&mut self, from: &str, to: &str) -> Result<usize, LibraryError> {
        todo!("lot PB3")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GameId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identification {
    Sha1,
    Crc32Size,
    /// File name equal to a DAT game name (normalized).
    Name,
    /// Disc header serial equal to the DAT `serial` of one game (Redump, design v6.0 §3.4).
    Serial,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: GameId,
    pub system: String,
    /// The file on disk (the archive for a zip).
    pub path: PathBuf,
    /// Zip entry hashed and launched: the first accepted entry, as `emu-runtime` loads it.
    pub entry: Option<String>,
    /// DAT game name when identified, else the file stem (ADR 0011 game name).
    pub title: String,
    pub identification: Identification,
    pub size: u64,
    /// `None` when the file was above `ScanOptions::max_file_size`.
    pub crc32: Option<u32>,
    pub sha1: Option<[u8; 20]>,
    pub favorite: bool,
    pub last_played_unix: Option<i64>,
    pub play_count: u32,
    /// Ordered disk images when the game has several (a `(Disk N of M)` group, first disk included, or the
    /// files of a `.m3u`, which is then `path`); empty for a single image. Stored in `game_media` (schema v3).
    pub media: Vec<PathBuf>,
    /// Unpacked game folder (DOS, design v4.0 §3.4): `path` is the folder. Column `game.is_dir` (schema v5).
    pub is_dir: bool,
    /// Region of a game of a `disc = true` system (design v6.0 §2.2): disc header, else Redump DAT, else name
    /// tag; `None` = unknown or not a disc system. Column `game.region` (schema v6, `DiscRegion::key`), filled by
    /// the scan (also for a row left unchanged by it, the first time).
    pub region: Option<DiscRegion>,
    /// Sega CD 32X disc (7.4, system `megacd` only): needs a core emulating the Mega CD and the 32X together
    /// (picodrive). Disc probe (`disc_needs_32x`), else Redump name tag (`name_needs_32x`). Column `game.needs_32x`
    /// (schema v7; NULL until the scan computes it, also for a row it leaves unchanged). NULL stays NULL outside
    /// `megacd` (never computed there) and reads as `false`.
    pub needs_32x: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverKind {
    Boxart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverState {
    /// `path` is relative to `<data>/covers/`; `source` is a `CoverSourceKind` name (`libretro`, `screenscraper`, `igdb`).
    Cached { path: PathBuf, source: String },
    /// Every enabled source answered "not found" at `checked_unix`.
    Missing { checked_unix: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverRecord {
    pub game: GameId,
    pub kind: CoverKind,
    pub state: CoverState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatInfo {
    pub system: String,
    /// Schema v4: a system may have one DAT per source.
    pub source: DatSource,
    pub name: String,
    pub version: Option<String>,
    pub entries: u64,
    pub imported_unix: i64,
    /// HTTP `ETag` of the downloaded file, for conditional updates.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened,
    Created,
    /// The previous file was unreadable or failed `quick_check`: renamed, then an empty base was created.
    Recovered {
        quarantined: PathBuf,
    },
}

/// Wraps the SQLite connection.
#[derive(Debug)]
pub struct Library {
    pub(crate) conn: Connection,
}

/// Schema v1 (docs/design/v1.0-bibliotheque.md §1), with two changes: `game.path` is unique on its own (one file or
/// archive = one game; `UNIQUE(path, entry)` would let NULL entries duplicate) and `game.file_size` holds the size
/// on disk used by the incremental scan, while `game.size` is the hashed content size (the zip entry for archives).
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE dat_info (
    system TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT,
    entries INTEGER NOT NULL,
    imported_unix INTEGER NOT NULL,
    etag TEXT
);
CREATE TABLE dat_entry (
    system TEXT NOT NULL REFERENCES dat_info(system) ON DELETE CASCADE,
    game TEXT NOT NULL,
    rom_name TEXT NOT NULL,
    size INTEGER NOT NULL,
    crc32 INTEGER NOT NULL,
    sha1 BLOB
);
CREATE INDEX dat_entry_sha1 ON dat_entry(sha1);
CREATE INDEX dat_entry_crc ON dat_entry(system, crc32, size);
CREATE TABLE game (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    entry TEXT,
    system TEXT NOT NULL,
    size INTEGER NOT NULL,
    file_size INTEGER NOT NULL,
    mtime_unix INTEGER NOT NULL,
    crc32 INTEGER,
    sha1 BLOB,
    crc32_nh INTEGER,
    sha1_nh BLOB,
    title TEXT NOT NULL,
    identification TEXT NOT NULL,
    favorite INTEGER NOT NULL DEFAULT 0,
    last_played_unix INTEGER,
    play_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE cover (
    game INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    path TEXT,
    source TEXT,
    checked_unix INTEGER NOT NULL,
    PRIMARY KEY (game, kind)
);
"#,
    r#"
-- 1 when a folder of the path, relative to the parent of its scan root, named `system` (folder rule of the scan):
-- the re-identification after a DAT import then keeps that system. Rows of v1 get the old approximation (any
-- path component equal to the system); the next scan rewrites the flag of any row it disagrees with.
ALTER TABLE game ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
UPDATE game SET pinned = 1
    WHERE instr(path, '/' || system || '/') > 0 OR instr(path, '\' || system || '\') > 0;
"#,
    r#"
-- Disk images of a multi-disk game (design v2.0 §3.4): `idx` is the disk order, `path` is unique over the whole
-- table (a disk belongs to one game). Additive: no existing row changes.
CREATE TABLE game_media (
    game_id INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    path TEXT NOT NULL UNIQUE,
    PRIMARY KEY (game_id, idx)
);
"#,
    // Frozen once shipped in a tag: a fix is a new migration. The source given to existing rows is the one each
    // system declared when v3 was current (2.0 catalog: `atarist` = tosec, the consoles = no-intro), written here
    // rather than read from the catalog so that the step never changes. The ETag of the ST DAT is dropped: 2.0
    // downloaded it from No-Intro (design v3.0 §10 #4), the next update fetches the TOSEC file.
    r#"
ALTER TABLE dat_entry RENAME TO dat_entry_v3;
ALTER TABLE dat_info RENAME TO dat_info_v3;
CREATE TABLE dat_info (
    system TEXT NOT NULL,
    source TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT,
    entries INTEGER NOT NULL,
    imported_unix INTEGER NOT NULL,
    etag TEXT,
    PRIMARY KEY (system, source)
);
CREATE TABLE dat_entry (
    system TEXT NOT NULL,
    source TEXT NOT NULL,
    game TEXT NOT NULL,
    rom_name TEXT NOT NULL,
    size INTEGER NOT NULL,
    crc32 INTEGER NOT NULL,
    sha1 BLOB,
    FOREIGN KEY (system, source) REFERENCES dat_info(system, source) ON DELETE CASCADE
);
INSERT INTO dat_info (system, source, name, version, entries, imported_unix, etag)
    SELECT system, CASE system WHEN 'atarist' THEN 'tosec' ELSE 'no-intro' END, name, version, entries,
           imported_unix, CASE system WHEN 'atarist' THEN NULL ELSE etag END
    FROM dat_info_v3;
INSERT INTO dat_entry (system, source, game, rom_name, size, crc32, sha1)
    SELECT system, CASE system WHEN 'atarist' THEN 'tosec' ELSE 'no-intro' END, game, rom_name, size, crc32, sha1
    FROM dat_entry_v3 ORDER BY rowid;
DROP TABLE dat_entry_v3;
DROP TABLE dat_info_v3;
CREATE INDEX dat_entry_sha1 ON dat_entry(sha1);
CREATE INDEX dat_entry_crc ON dat_entry(system, crc32, size);
"#,
    // 1 for a game that is a folder (DOS, design v4.0 §3.4): `path` is then the folder. Additive.
    r#"
ALTER TABLE game ADD COLUMN is_dir INTEGER NOT NULL DEFAULT 0;
"#,
    // Region of a game of a disc system (design v6.0 §2.2, `DiscRegion::key`), NULL until the next scan fills it;
    // serial and region of a Redump DAT line (identification by header serial, region fallback). Additive.
    r#"
ALTER TABLE game ADD COLUMN region TEXT;
ALTER TABLE dat_entry ADD COLUMN serial TEXT;
ALTER TABLE dat_entry ADD COLUMN region TEXT;
"#,
    // 1 for a Sega CD 32X disc (7.4), 0 for another `megacd` game; NULL until the next scan fills it. Additive.
    r#"
ALTER TABLE game ADD COLUMN needs_32x INTEGER;
"#,
];

const _: () = assert!(MIGRATIONS.len() == SCHEMA_VERSION as usize);

const GAME_COLUMNS: &str = "id, path, entry, system, size, crc32, sha1, title, identification, favorite, \
                            last_played_unix, play_count, is_dir, region, needs_32x";

pub(crate) fn db_error(e: rusqlite::Error) -> LibraryError {
    LibraryError::Database(e.to_string())
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Saturating: sizes come from the file system or an untrusted DAT.
pub(crate) fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

#[derive(Debug)]
enum OpenError {
    /// "not a database", "malformed" or a failed `quick_check`: the file goes to quarantine.
    Corrupt,
    Other(LibraryError),
}

impl From<rusqlite::Error> for OpenError {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => OpenError::Corrupt,
            _ => OpenError::Other(db_error(e)),
        }
    }
}

fn identification_name(i: Identification) -> &'static str {
    match i {
        Identification::Sha1 => "sha1",
        Identification::Crc32Size => "crc32_size",
        Identification::Name => "name",
        Identification::Serial => "serial",
        Identification::Unknown => "unknown",
    }
}

fn identification_from(s: &str) -> Identification {
    match s {
        "sha1" => Identification::Sha1,
        "crc32_size" => Identification::Crc32Size,
        "name" => Identification::Name,
        "serial" => Identification::Serial,
        _ => Identification::Unknown,
    }
}

fn kind_name(kind: CoverKind) -> &'static str {
    match kind {
        CoverKind::Boxart => "boxart",
    }
}

pub(crate) fn sha1_from(blob: Option<Vec<u8>>) -> Option<[u8; 20]> {
    blob.and_then(|b| b.try_into().ok())
}

fn crc_from(v: Option<i64>) -> Option<u32> {
    v.and_then(|v| u32::try_from(v).ok())
}

fn game_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Game> {
    let path: String = row.get(1)?;
    let identification: String = row.get(8)?;
    Ok(Game {
        id: GameId(row.get(0)?),
        path: PathBuf::from(path),
        entry: row.get(2)?,
        system: row.get(3)?,
        size: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
        crc32: crc_from(row.get(5)?),
        sha1: sha1_from(row.get(6)?),
        title: row.get(7)?,
        identification: identification_from(&identification),
        favorite: row.get::<_, i64>(9)? != 0,
        last_played_unix: row.get(10)?,
        play_count: u32::try_from(row.get::<_, i64>(11)?.max(0)).unwrap_or(u32::MAX),
        media: Vec::new(),
        is_dir: row.get::<_, i64>(12)? != 0,
        region: row
            .get::<_, Option<String>>(13)?
            .as_deref()
            .and_then(DiscRegion::from_key),
        needs_32x: row.get::<_, Option<i64>>(14)?.is_some_and(|v| v != 0),
    })
}

/// `library.sqlite` → `library.sqlite.corrupt-<unix>` (suffix `-<n>` if taken), with its `-wal` / `-shm`.
fn quarantine(path: &Path) -> Result<PathBuf, LibraryError> {
    let io = |source| LibraryError::Io {
        path: path.to_path_buf(),
        source,
    };
    let base = format!(
        "{}.corrupt-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        now_unix()
    );
    let mut target = path.with_file_name(&base);
    let mut n = 1;
    while target.exists() {
        target = path.with_file_name(format!("{base}-{n}"));
        n += 1;
    }
    fs::rename(path, &target).map_err(io)?;
    for suffix in ["-wal", "-shm"] {
        let mut side = path.as_os_str().to_owned();
        side.push(suffix);
        let mut dest = target.as_os_str().to_owned();
        dest.push(suffix);
        match fs::rename(&side, &dest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
    }
    Ok(target)
}

fn configure(conn: &Connection) -> Result<(), OpenError> {
    conn.busy_timeout(Duration::from_secs(5))?;
    // In-memory bases answer "memory": nothing to check.
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

fn migrate(conn: &mut Connection, from: u32) -> Result<(), OpenError> {
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = index as u32 + 1;
        if version <= from {
            continue;
        }
        // Immediate: the write lock is taken before re-reading the version, so a concurrent migrator waits
        // (busy timeout) and then finds the step already applied.
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.set_drop_behavior(DropBehavior::Rollback);
        if user_version(&tx)? >= version {
            continue;
        }
        // A step that fails on the user's file (rolled back) sends it to quarantine like a corrupt one: the
        // app keeps working on a new base and the old file is kept aside. A lock timeout is not a failure of
        // the file.
        tx.execute_batch(sql)
            .map_err(|e| match e.sqlite_error_code() {
                Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => OpenError::from(e),
                _ => {
                    tracing::warn!("library migration to v{version} failed: {e}");
                    OpenError::Corrupt
                }
            })?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

fn user_version(conn: &Connection) -> Result<u32, OpenError> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    // Negative or huge values are not ours: treated as newer, the file is left alone.
    Ok(u32::try_from(v).unwrap_or(u32::MAX))
}

/// `user_version` read through a read-only connection: closing a read-write one would checkpoint a `-wal` left by
/// a newer version into the main file. `0` when the file does not exist yet.
fn probe_version(path: &Path) -> Result<u32, OpenError> {
    if !path.exists() {
        return Ok(0);
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    user_version(&conn)
}

/// A newer schema is detected before any read-write connection: the file and its `-wal` stay byte-for-byte intact.
///
/// `create == false` never creates the file (no `SQLITE_OPEN_CREATE`): a base removed meanwhile is an error.
fn try_open(path: &Path, create: bool) -> Result<Library, OpenError> {
    let version = probe_version(path)?;
    if version > SCHEMA_VERSION {
        return Err(OpenError::Other(LibraryError::NewerSchema {
            found: version,
            supported: SCHEMA_VERSION,
        }));
    }
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let mut conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // Another process may have migrated the file in between: the read-write connection decides.
    let version = user_version(&conn)?;
    if version > SCHEMA_VERSION {
        return Err(OpenError::Other(LibraryError::NewerSchema {
            found: version,
            supported: SCHEMA_VERSION,
        }));
    }
    let check: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(OpenError::Corrupt);
    }
    configure(&conn)?;
    migrate(&mut conn, version)?;
    Ok(Library { conn })
}

/// DAT game names by normalized name, per system, loaded on first use (name fallback of the identification).
#[derive(Default)]
pub(crate) struct NameIndex(HashMap<String, HashMap<String, String>>);

impl NameIndex {
    /// The first declared DAT source of `system` wins on a name present in several.
    fn lookup(
        &mut self,
        conn: &Connection,
        system: &SystemInfo,
        normalized: &str,
    ) -> Result<Option<String>, LibraryError> {
        if !self.0.contains_key(&system.id) {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT source, game FROM dat_entry WHERE system = ?1 ORDER BY rowid",
                )
                .map_err(db_error)?;
            let mut rows: Vec<(usize, String)> = stmt
                .query_map([&system.id], |r| {
                    Ok((system.dat_rank(&r.get::<_, String>(0)?), r.get(1)?))
                })
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?;
            rows.sort_by_key(|(rank, _)| *rank);
            let mut map = HashMap::new();
            for (_, game) in rows {
                map.entry(normalize_title(&game)).or_insert(game);
            }
            self.0.insert(system.id.clone(), map);
        }
        Ok(self
            .0
            .get(&system.id)
            .and_then(|m| m.get(normalized))
            .cloned())
    }
}

pub(crate) struct Identified {
    pub system: String,
    pub title: String,
    pub how: Identification,
}

/// SHA1 (full, then headerless) > CRC32 + size > normalized file name = DAT game name > first candidate.
/// `candidates` is non-empty and in catalog order (or the single system pinned by the folder name). Within a
/// system, the DATs are tried in `SystemInfo::dats` order (first match wins).
pub(crate) fn identify(
    conn: &Connection,
    names: &mut NameIndex,
    candidates: &[&SystemInfo],
    hashes: Option<&RomHashes>,
    stem: &str,
) -> Result<Identified, LibraryError> {
    let found = |system: &SystemInfo, title: String, how| Identified {
        system: system.id.clone(),
        title,
        how,
    };
    if let Some(h) = hashes {
        let variants: Vec<_> = std::iter::once(h.full).chain(h.headerless).collect();
        let mut stmt = conn
            .prepare_cached(
                "SELECT system, source, game FROM dat_entry WHERE sha1 = ?1 ORDER BY rowid",
            )
            .map_err(db_error)?;
        for v in &variants {
            let rows: Vec<(String, String, String)> = stmt
                .query_map([&v.sha1[..]], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?;
            for c in candidates {
                let best = rows
                    .iter()
                    .filter(|(s, _, _)| *s == c.id)
                    .min_by_key(|(_, source, _)| c.dat_rank(source));
                if let Some((_, _, game)) = best {
                    return Ok(found(c, game.clone(), Identification::Sha1));
                }
            }
        }
        let mut stmt = conn
            .prepare_cached(
                "SELECT source, game FROM dat_entry WHERE system = ?1 AND crc32 = ?2 AND size = ?3
                 ORDER BY rowid",
            )
            .map_err(db_error)?;
        for c in candidates {
            for v in &variants {
                let rows: Vec<(String, String)> = stmt
                    .query_map(params![c.id, i64::from(v.crc32), to_i64(v.size)], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .map_err(db_error)?
                    .collect::<Result<_, _>>()
                    .map_err(db_error)?;
                if let Some((_, game)) = rows
                    .into_iter()
                    .min_by_key(|(source, _)| c.dat_rank(source))
                {
                    return Ok(found(c, game, Identification::Crc32Size));
                }
            }
        }
    }
    let normalized = normalize_title(stem);
    if !normalized.is_empty() {
        for c in candidates {
            if let Some(game) = names.lookup(conn, c, &normalized)? {
                return Ok(found(c, game, Identification::Name));
            }
        }
    }
    let first = candidates
        .first()
        .ok_or_else(|| LibraryError::Database("aucun système candidat".into()))?;
    Ok(found(first, stem.to_owned(), Identification::Unknown))
}

/// `(game, rom_name)` of the DAT lines of `system` with this CRC32 and size, in DAT source order.
pub(crate) fn dat_lines(
    conn: &Connection,
    system: &SystemInfo,
    crc32: u32,
    size: u64,
) -> Result<Vec<(String, String)>, LibraryError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT source, game, rom_name FROM dat_entry WHERE system = ?1 AND crc32 = ?2 AND size = ?3
             ORDER BY rowid",
        )
        .map_err(db_error)?;
    let mut rows: Vec<(usize, String, String)> = stmt
        .query_map(params![system.id, i64::from(crc32), to_i64(size)], |r| {
            Ok((
                system.dat_rank(&r.get::<_, String>(0)?),
                r.get(1)?,
                r.get(2)?,
            ))
        })
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    rows.sort_by_key(|(rank, _, _)| *rank);
    Ok(rows.into_iter().map(|(_, g, r)| (g, r)).collect())
}

/// Identification of a DOS game (design v4.0 §3.4): the DAT game with the most matching files, else the
/// normalized name against the DAT game names, else the name itself. `files` empty: name only.
pub(crate) fn identify_dos(
    conn: &Connection,
    names: &mut NameIndex,
    system: &SystemInfo,
    files: &[crate::dos::DosFile],
    stem: &str,
) -> Result<Identified, LibraryError> {
    use crate::dos::{DosMatch, dos_candidates};
    let failure = std::cell::RefCell::new(None);
    let matched = dos_candidates(files, &|crc, size| {
        dat_lines(conn, system, crc, size).unwrap_or_else(|e| {
            failure.borrow_mut().get_or_insert(e);
            Vec::new()
        })
    });
    if let Some(e) = failure.into_inner() {
        return Err(e);
    }
    match matched {
        DosMatch::Unique(game) => Ok(Identified {
            system: system.id.clone(),
            title: game,
            how: Identification::Crc32Size,
        }),
        DosMatch::Ambiguous | DosMatch::None => identify(conn, names, &[system], None, stem),
    }
}

/// Attribution of a file of a shared extension (design v3.0 §3.5.2, `Attribution::ByDat`): the hashes must be
/// found (SHA1 or CRC32 + size, not the file name) in the DATs of exactly one candidate.
pub(crate) fn dat_match(
    conn: &Connection,
    names: &mut NameIndex,
    candidates: &[&SystemInfo],
    hashes: Option<&RomHashes>,
) -> Result<Option<Identified>, LibraryError> {
    let Some(hashes) = hashes else {
        return Ok(None);
    };
    let mut hit = None;
    for c in candidates {
        let id = identify(conn, names, &[c], Some(hashes), "")?;
        if matches!(id.how, Identification::Sha1 | Identification::Crc32Size) {
            if hit.is_some() {
                return Ok(None);
            }
            hit = Some(id);
        }
    }
    Ok(hit)
}

/// Header length removed for the stored headerless hashes of `system` (to rebuild their size).
fn header_len(system: &SystemInfo) -> u64 {
    match system.header {
        Some(crate::HeaderRule::INes) => 16,
        Some(crate::HeaderRule::SnesCopier) => 512,
        None => 0,
    }
}

/// Lower-case extension of the launched file: the zip entry when present, else the path.
pub(crate) fn extension_of(path: &Path, entry: Option<&str>) -> Option<String> {
    let name = match entry {
        Some(e) => Path::new(e).extension()?.to_str()?.to_owned(),
        None => path.extension()?.to_str()?.to_owned(),
    };
    Some(name.to_ascii_lowercase())
}

/// Re-runs the identification of every game whose extension may belong to `system` (after a DAT import).
/// A game pinned by the scan's folder rule (`game.pinned`) stays on its system. Playlists are left to the next
/// scan (their system comes from their entries); a file of a shared extension no longer found in the DATs keeps
/// its system as "unknown", and the next scan re-evaluates it (design v3.0 §3.5.2).
fn reidentify(conn: &Connection, system: &str) -> Result<u64, LibraryError> {
    struct Row {
        id: i64,
        path: String,
        entry: Option<String>,
        system: String,
        size: i64,
        crc32: Option<i64>,
        sha1: Option<Vec<u8>>,
        crc32_nh: Option<i64>,
        sha1_nh: Option<Vec<u8>>,
        pinned: bool,
        identification: String,
    }
    let mut stmt = conn
        .prepare(
            "SELECT id, path, entry, system, size, crc32, sha1, crc32_nh, sha1_nh, pinned, identification
             FROM game WHERE is_dir = 0",
        )
        .map_err(db_error)?;
    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                path: r.get(1)?,
                entry: r.get(2)?,
                system: r.get(3)?,
                size: r.get(4)?,
                crc32: r.get(5)?,
                sha1: r.get(6)?,
                crc32_nh: r.get(7)?,
                sha1_nh: r.get(8)?,
                pinned: r.get::<_, i64>(9)? != 0,
                identification: r.get(10)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    let mut names = NameIndex::default();
    let mut update = conn
        .prepare("UPDATE game SET system = ?2, title = ?3, identification = ?4 WHERE id = ?1")
        .map_err(db_error)?;
    let mut changed = 0;
    for row in rows {
        let path = PathBuf::from(&row.path);
        let Some(ext) = extension_of(&path, row.entry.as_deref()) else {
            continue;
        };
        if ext == "m3u" || ext == "m3u8" {
            continue;
        }
        // Games of a `folder_games` system are identified from their file lists: the next scan redoes them.
        if crate::system(&row.system).is_some_and(|s| s.folder_games) {
            continue;
        }
        // A disc identified by its track CRC or header serial (never stored): the next scan redoes it, the name
        // alone must not undo it here.
        if crate::system(&row.system).is_some_and(|s| s.disc)
            && matches!(row.identification.as_str(), "crc32_size" | "serial")
        {
            continue;
        }
        let (candidates, by_dat): (Vec<&SystemInfo>, bool) = if ext == "bin"
            && (row.system == SEGA_32X || crate::system(&row.system).is_some_and(|s| s.disc))
        {
            // Kept as 32X, PlayStation or 3DO by its header (scanner probe): only the DAT of that system
            // re-identifies it.
            match crate::system(&row.system) {
                Some(s) if system == row.system => (vec![s], false),
                _ => continue,
            }
        } else {
            let all = systems_for_extension(&ext);
            if !all.iter().any(|s| s.id == system) {
                continue;
            }
            let pinned = all
                .iter()
                .copied()
                .find(|s| row.pinned && s.id == row.system);
            match pinned {
                Some(p) => (vec![p], false),
                None => match attribute(&ext, &[]) {
                    Attribution::Folder(s) => (vec![s], false),
                    Attribution::Candidates(c) => (c, false),
                    Attribution::ByDat(c) => (c, true),
                    Attribution::Unclassified => continue,
                },
            }
        };
        let size = u64::try_from(row.size).unwrap_or(0);
        let hashes = match (crc_from(row.crc32), sha1_from(row.sha1)) {
            (Some(crc32), Some(sha1)) => {
                let full = crate::Hashes { size, crc32, sha1 };
                let headerless = match (crc_from(row.crc32_nh), sha1_from(row.sha1_nh)) {
                    (Some(crc32), Some(sha1)) => {
                        let skip = candidates.first().map_or(0, |s| header_len(s));
                        Some(crate::Hashes {
                            size: size.saturating_sub(skip),
                            crc32,
                            sha1,
                        })
                    }
                    _ => None,
                };
                Some(RomHashes { full, headerless })
            }
            _ => None,
        };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = if by_dat {
            match dat_match(conn, &mut names, &candidates, hashes.as_ref())? {
                Some(id) => id,
                None => Identified {
                    system: row.system.clone(),
                    title: stem,
                    how: Identification::Unknown,
                },
            }
        } else {
            identify(conn, &mut names, &candidates, hashes.as_ref(), &stem)?
        };
        changed += update
            .execute(params![
                row.id,
                id.system,
                id.title,
                identification_name(id.how)
            ])
            .map_err(db_error)? as u64;
    }
    Ok(changed)
}

/// System of a `.bin` whose header says `SEGA 32X` (design v6.0 §3.4).
pub(crate) const SEGA_32X: &str = "32x";

/// One line of a Redump DAT (design v6.0 §3.4).
pub(crate) struct RedumpEntry {
    pub game: String,
    pub region: Option<DiscRegion>,
}

/// Redump lines of one disc system, indexed by track size, header serial and normalized game name.
#[derive(Default)]
pub(crate) struct RedumpSet {
    entries: Vec<RedumpEntry>,
    /// Size of a track ⇒ `(crc32, entry)`: the CRC is computed only when the size is listed.
    by_size: HashMap<u64, Vec<(u32, usize)>>,
    by_serial: HashMap<String, Vec<usize>>,
    by_name: HashMap<String, usize>,
}

impl RedumpSet {
    pub fn has_size(&self, size: u64) -> bool {
        self.by_size.contains_key(&size)
    }

    pub fn by_crc(&self, size: u64, crc32: u32) -> Option<&RedumpEntry> {
        self.by_size
            .get(&size)?
            .iter()
            .find(|(crc, _)| *crc == crc32)
            .map(|&(_, i)| &self.entries[i])
    }

    /// The game whose serial matches one of `keys` (normalized), when exactly one game does.
    pub fn by_serial(&self, keys: &[String]) -> Option<&RedumpEntry> {
        let mut hit: Option<usize> = None;
        for k in keys {
            for &i in self.by_serial.get(k).into_iter().flatten() {
                match hit {
                    Some(h) if self.entries[h].game != self.entries[i].game => return None,
                    _ => hit = Some(i),
                }
            }
            if hit.is_some() {
                break;
            }
        }
        hit.map(|i| &self.entries[i])
    }

    pub fn by_name(&self, stem: &str) -> Option<&RedumpEntry> {
        let key = normalize_title(stem);
        (!key.is_empty())
            .then(|| self.by_name.get(&key))
            .flatten()
            .map(|&i| &self.entries[i])
    }
}

/// Region of a Redump line: its `region` field (`USA`, `Japan, USA`…), else the tag of its game name.
fn dat_region(field: Option<&str>, game: &str) -> Option<DiscRegion> {
    field
        .and_then(|f| crate::region_from_name(&format!("({f})")))
        .or_else(|| crate::region_from_name(game))
}

/// Redump lines of every `disc` system declaring the `redump` source, loaded once per scan (a few thousand).
#[derive(Default)]
pub(crate) struct DiscIndex(HashMap<String, RedumpSet>);

impl DiscIndex {
    pub fn load(conn: &Connection) -> Result<Self, LibraryError> {
        let mut index = DiscIndex::default();
        let mut stmt = conn
            .prepare(
                "SELECT game, size, crc32, serial, region FROM dat_entry WHERE system = ?1 AND source = ?2
                 ORDER BY rowid",
            )
            .map_err(db_error)?;
        for system in crate::systems()
            .iter()
            .filter(|s| s.disc && s.dats.contains(&DatSource::Redump))
        {
            let mut set = RedumpSet::default();
            let rows = stmt
                .query_map([system.id.as_str(), DatSource::Redump.id()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(db_error)?;
            for row in rows {
                let (game, size, crc, serial, region) = row.map_err(db_error)?;
                let i = set.entries.len();
                if let (Ok(size), Some(crc)) = (u64::try_from(size), crc_from(Some(crc))) {
                    set.by_size.entry(size).or_default().push((crc, i));
                }
                for value in serial.iter().flat_map(|s| s.split(',')) {
                    let key = crate::disc::normalize_serial(value);
                    if !key.is_empty() {
                        set.by_serial.entry(key).or_default().push(i);
                    }
                }
                set.by_name.entry(normalize_title(&game)).or_insert(i);
                set.entries.push(RedumpEntry {
                    region: dat_region(region.as_deref(), &game),
                    game,
                });
            }
            index.0.insert(system.id.clone(), set);
        }
        Ok(index)
    }

    pub fn get(&self, system: &str) -> Option<&RedumpSet> {
        self.0.get(system)
    }
}

/// Disk images per game id, in disk order.
pub(crate) fn load_media(
    conn: &Connection,
    only: Option<i64>,
) -> Result<HashMap<i64, Vec<PathBuf>>, LibraryError> {
    let mut stmt = conn
        .prepare(
            "SELECT game_id, path FROM game_media WHERE (?1 IS NULL OR game_id = ?1)
             ORDER BY game_id, idx",
        )
        .map_err(db_error)?;
    let mut map: HashMap<i64, Vec<PathBuf>> = HashMap::new();
    for row in stmt
        .query_map([only], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(db_error)?
    {
        let (id, path) = row.map_err(db_error)?;
        map.entry(id).or_default().push(PathBuf::from(path));
    }
    Ok(map)
}

const DAT_INFO_COLUMNS: &str = "system, source, name, version, entries, imported_unix, etag";

/// `None` for a source this build does not know (written by a newer version sharing the schema).
fn dat_info_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Option<DatInfo>> {
    let Some(source) = DatSource::from_id(&r.get::<_, String>(1)?) else {
        return Ok(None);
    };
    Ok(Some(DatInfo {
        system: r.get(0)?,
        source,
        name: r.get(2)?,
        version: r.get(3)?,
        entries: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
        imported_unix: r.get(5)?,
        etag: r.get(6)?,
    }))
}

fn first_source(system: &str) -> Result<DatSource, LibraryError> {
    crate::system(system)
        .and_then(|s| s.dats.first().copied())
        .ok_or_else(|| LibraryError::UnknownSystem(system.to_owned()))
}

pub(crate) fn identification_text(i: Identification) -> &'static str {
    identification_name(i)
}

impl Library {
    /// Opens or creates `path` (WAL, foreign keys), applies migrations up to `SCHEMA_VERSION`.
    /// Newer schema ⇒ `LibraryError::NewerSchema`, file untouched.
    pub fn open(path: &Path) -> Result<(Self, OpenOutcome), LibraryError> {
        let existed = fs::metadata(path).is_ok_and(|m| m.len() > 0);
        match try_open(path, true) {
            Ok(library) => Ok((
                library,
                if existed {
                    OpenOutcome::Opened
                } else {
                    OpenOutcome::Created
                },
            )),
            Err(OpenError::Other(e)) => Err(e),
            Err(OpenError::Corrupt) => {
                let quarantined = quarantine(path)?;
                tracing::warn!(
                    "library database was corrupt: moved to {}, starting with an empty one",
                    quarantined.display()
                );
                match try_open(path, true) {
                    Ok(library) => Ok((library, OpenOutcome::Recovered { quarantined })),
                    Err(OpenError::Other(e)) => Err(e),
                    Err(OpenError::Corrupt) => Err(LibraryError::Database(
                        "base neuve illisible après mise en quarantaine".into(),
                    )),
                }
            }
        }
    }

    /// Opens a base that must already exist, for worker threads (scan, covers) that run while the UI thread
    /// keeps its own connection: never creates the file and never quarantines it. Only [`Self::open`], called
    /// once by the thread that owns recovery, may move a corrupt file aside; a second connection doing it
    /// would rename the file under the first one.
    ///
    /// Missing or empty file: `LibraryError::Io` (`NotFound`). Corrupt file: `LibraryError::Database`, file
    /// untouched. Newer schema: `NewerSchema`. Pending migrations are applied like in `open` (WAL: the
    /// other connection keeps working).
    pub fn open_existing(path: &Path) -> Result<Self, LibraryError> {
        if !fs::metadata(path).is_ok_and(|m| m.len() > 0) {
            return Err(LibraryError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        }
        match try_open(path, false) {
            Ok(library) => Ok(library),
            Err(OpenError::Other(e)) => Err(e),
            Err(OpenError::Corrupt) => Err(LibraryError::Database(
                "base illisible (non modifiée : la récupération revient au fil principal)".into(),
            )),
        }
    }

    /// Tests only need this, but it is public so `emu-ui` tests can build fixtures.
    pub fn open_in_memory() -> Result<Self, LibraryError> {
        let setup = || -> Result<Library, OpenError> {
            let mut conn = Connection::open_in_memory()?;
            configure(&conn)?;
            migrate(&mut conn, 0)?;
            Ok(Library { conn })
        };
        setup().map_err(|e| match e {
            OpenError::Other(e) => e,
            OpenError::Corrupt => LibraryError::Database("base en mémoire illisible".into()),
        })
    }

    /// Replaces the entries of the **first declared source** of `system` in one transaction, then re-identifies
    /// its games. Returns the entry count.
    pub fn import_dat(
        &mut self,
        system: &str,
        dat: &Dat,
        etag: Option<&str>,
    ) -> Result<u64, LibraryError> {
        let source = first_source(system)?;
        self.import_dat_source(system, source, dat, etag)
    }

    /// Schema v4: replaces the entries of the couple `(system, source)` only, then re-identifies the games of
    /// `system` against all its DATs in `SystemInfo::dats` order. `LibraryError::UnknownSystem` when `source`
    /// is not declared for the system. [`Library::import_dat`] is this with the first declared source.
    pub fn import_dat_source(
        &mut self,
        system: &str,
        source: DatSource,
        dat: &Dat,
        etag: Option<&str>,
    ) -> Result<u64, LibraryError> {
        let declared = crate::system(system).is_some_and(|s| s.dats.contains(&source));
        if !declared {
            return Err(LibraryError::UnknownSystem(format!(
                "{system} ({})",
                source.id()
            )));
        }
        let source = source.id();
        let tx = self.conn.transaction().map_err(db_error)?;
        tx.execute(
            "DELETE FROM dat_entry WHERE system = ?1 AND source = ?2",
            [system, source],
        )
        .map_err(db_error)?;
        tx.execute(
            "INSERT INTO dat_info (system, source, name, version, entries, imported_unix, etag)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)
             ON CONFLICT(system, source) DO UPDATE SET name = excluded.name, version = excluded.version,
                 entries = 0, imported_unix = excluded.imported_unix, etag = excluded.etag",
            params![system, source, dat.name, dat.version, now_unix(), etag],
        )
        .map_err(db_error)?;
        let mut count = 0u64;
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO dat_entry (system, source, game, rom_name, size, crc32, sha1, serial, region)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )
                .map_err(db_error)?;
            for e in &dat.entries {
                let Ok(size) = i64::try_from(e.size) else {
                    continue;
                };
                insert
                    .execute(params![
                        system,
                        source,
                        e.game,
                        e.rom_name,
                        size,
                        i64::from(e.crc32),
                        e.sha1.as_ref().map(|s| &s[..]),
                        e.serial,
                        e.region
                    ])
                    .map_err(db_error)?;
                count += 1;
            }
        }
        tx.execute(
            "UPDATE dat_info SET entries = ?3 WHERE system = ?1 AND source = ?2",
            params![system, source, to_i64(count)],
        )
        .map_err(db_error)?;
        reidentify(&tx, system)?;
        // Disc games are identified from their track size, CRC and header serial (not stored) too.
        if crate::system(system).is_some_and(|s| s.folder_games || s.disc) {
            // The identification of these games needs their file lists (not stored): an impossible mtime makes
            // the next scan analyse them again, once.
            tx.execute(
                "UPDATE game SET mtime_unix = -1 WHERE system = ?1",
                [system],
            )
            .map_err(db_error)?;
        }
        if crate::system(system).is_some_and(|s| s.disc) {
            // A `.chd` outside any folder is attributed by its name only when one Redump DAT lists it (design
            // v7.0 §3.4): a new Redump DAT may make the name of another disc system's `.chd` ambiguous.
            for other in crate::systems().iter().filter(|s| s.disc && s.id != system) {
                tx.execute(
                    "UPDATE game SET mtime_unix = -1
                     WHERE system = ?1 AND pinned = 0 AND lower(path) LIKE '%.chd'",
                    [other.id.as_str()],
                )
                .map_err(db_error)?;
            }
        }
        tx.commit().map_err(db_error)?;
        Ok(count)
    }

    /// DAT of one source of a system.
    pub fn dat_info_for(
        &self,
        system: &str,
        source: DatSource,
    ) -> Result<Option<DatInfo>, LibraryError> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {DAT_INFO_COLUMNS} FROM dat_info WHERE system = ?1 AND source = ?2"
                ),
                [system, source.id()],
                dat_info_from_row,
            )
            .optional()
            .map_err(db_error)
            .map(Option::flatten)
    }

    /// Imported DATs of a system, in `SystemInfo::dats` order (sources the catalog no longer declares last).
    pub fn dat_infos(&self, system: &str) -> Result<Vec<DatInfo>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {DAT_INFO_COLUMNS} FROM dat_info WHERE system = ?1 ORDER BY source"
            ))
            .map_err(db_error)?;
        let mut infos: Vec<DatInfo> = stmt
            .query_map([system], dat_info_from_row)
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?
            .into_iter()
            .flatten()
            .collect();
        if let Some(info) = crate::system(system) {
            infos.sort_by_key(|d| info.dat_rank(d.source.id()));
        }
        Ok(infos)
    }

    /// DAT of the **first declared source** of the system (2.0 callers; `dat_info_for` for the others).
    pub fn dat_info(&self, system: &str) -> Result<Option<DatInfo>, LibraryError> {
        match first_source(system) {
            Ok(source) => self.dat_info_for(system, source),
            Err(_) => Ok(None),
        }
    }

    /// Incremental: unchanged (path, size, mtime) is not hashed again. Games under a missing root are kept.
    /// `cancel` is polled between files; `progress` is called at most every 100 ms.
    pub fn scan(
        &mut self,
        options: &ScanOptions,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&ScanProgress),
    ) -> Result<ScanReport, LibraryError> {
        crate::scan::run(&mut self.conn, options, cancel, progress)
    }

    pub fn games(&self) -> Result<Vec<Game>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {GAME_COLUMNS} FROM game ORDER BY id"))
            .map_err(db_error)?;
        let mut games: Vec<Game> = stmt
            .query_map([], game_from_row)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        let mut media = load_media(&self.conn, None)?;
        for g in &mut games {
            g.media = media.remove(&g.id.0).unwrap_or_default();
        }
        Ok(games)
    }

    pub fn game(&self, id: GameId) -> Result<Option<Game>, LibraryError> {
        let game = self
            .conn
            .query_row(
                &format!("SELECT {GAME_COLUMNS} FROM game WHERE id = ?1"),
                [id.0],
                game_from_row,
            )
            .optional()
            .map_err(db_error)?;
        let Some(mut game) = game else {
            return Ok(None);
        };
        game.media = load_media(&self.conn, Some(id.0))?
            .remove(&id.0)
            .unwrap_or_default();
        Ok(Some(game))
    }

    /// Unknown id (removed by a concurrent scan): no-op.
    pub fn record_played(&mut self, id: GameId, unix: i64) -> Result<(), LibraryError> {
        self.conn
            .execute(
                "UPDATE game SET last_played_unix = ?2, play_count = play_count + 1 WHERE id = ?1",
                params![id.0, unix],
            )
            .map_err(db_error)?;
        Ok(())
    }

    /// Unknown id: no-op.
    pub fn set_favorite(&mut self, id: GameId, favorite: bool) -> Result<(), LibraryError> {
        self.conn
            .execute(
                "UPDATE game SET favorite = ?2 WHERE id = ?1",
                params![id.0, favorite],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn cover(&self, id: GameId, kind: CoverKind) -> Result<Option<CoverRecord>, LibraryError> {
        let row: Option<(String, Option<String>, Option<String>, i64)> = self
            .conn
            .query_row(
                "SELECT state, path, source, checked_unix FROM cover WHERE game = ?1 AND kind = ?2",
                params![id.0, kind_name(kind)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(db_error)?;
        Ok(row.and_then(|(state, path, source, checked_unix)| {
            let state = match state.as_str() {
                "cached" => CoverState::Cached {
                    path: PathBuf::from(path?),
                    source: source.unwrap_or_default(),
                },
                "missing" => CoverState::Missing { checked_unix },
                _ => return None,
            };
            Some(CoverRecord {
                game: id,
                kind,
                state,
            })
        }))
    }

    /// Unknown game ⇒ `LibraryError::Database` (foreign key).
    pub fn set_cover(
        &mut self,
        id: GameId,
        kind: CoverKind,
        state: &CoverState,
    ) -> Result<(), LibraryError> {
        let (state_name, path, source, checked) = match state {
            CoverState::Cached { path, source } => {
                let path = path
                    .to_str()
                    .ok_or_else(|| LibraryError::Database("chemin de jaquette non UTF-8".into()))?;
                ("cached", Some(path), Some(source.as_str()), now_unix())
            }
            CoverState::Missing { checked_unix } => ("missing", None, None, *checked_unix),
        };
        self.conn
            .execute(
                "INSERT INTO cover (game, kind, state, path, source, checked_unix) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(game, kind) DO UPDATE SET state = excluded.state, path = excluded.path,
                     source = excluded.source, checked_unix = excluded.checked_unix",
                params![id.0, kind_name(kind), state_name, path, source, checked],
            )
            .map_err(db_error)?;
        Ok(())
    }

    /// Games without a cached `kind`, whose last "missing" check is older than `retry_before_unix` (or never).
    pub fn games_needing_cover(
        &self,
        kind: CoverKind,
        retry_before_unix: i64,
    ) -> Result<Vec<GameId>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT g.id FROM game g LEFT JOIN cover c ON c.game = g.id AND c.kind = ?1
                 WHERE c.game IS NULL OR (c.state = 'missing' AND c.checked_unix < ?2)
                 ORDER BY g.id",
            )
            .map_err(db_error)?;
        stmt.query_map(params![kind_name(kind), retry_before_unix], |r| {
            Ok(GameId(r.get(0)?))
        })
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
    }

    /// Forgets the record of one cover (cached or missing): the game is then "never checked" for `kind`.
    /// Returns whether a record existed; unknown game or absent record is not an error. The cached file is not
    /// touched (see `CoverCache::remove` in `emu-assets`).
    pub fn clear_cover(&mut self, id: GameId, kind: CoverKind) -> Result<bool, LibraryError> {
        let n = self
            .conn
            .execute(
                "DELETE FROM cover WHERE game = ?1 AND kind = ?2",
                params![id.0, kind_name(kind)],
            )
            .map_err(db_error)?;
        Ok(n > 0)
    }

    /// Forgets every cover record (all games, all kinds, cached or missing); returns how many were deleted.
    pub fn clear_all_covers(&mut self) -> Result<usize, LibraryError> {
        self.conn.execute("DELETE FROM cover", []).map_err(db_error)
    }

    /// Forgets every `Missing` record (user action "search covers again").
    pub fn clear_missing_covers(&mut self) -> Result<(), LibraryError> {
        self.conn
            .execute("DELETE FROM cover WHERE state = 'missing'", [])
            .map_err(db_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_migration_approximates_the_folder_rule() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        for (path, system) in [
            ("/r/gb/a.gb", "gb"),
            ("/r/misc/b.gb", "gb"),
            ("/r/gbc/c.gb", "gb"),
            (r"C:\r\gbc\d.gbc", "gbc"),
        ] {
            conn.execute(
                "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification)
                 VALUES (?1, ?2, 0, 0, 0, '', 'unknown')",
                [path, system],
            )
            .unwrap();
        }
        assert!(migrate(&mut conn, 1).is_ok());
        assert_eq!(user_version(&conn).ok(), Some(SCHEMA_VERSION));
        let pinned: Vec<(String, bool)> = conn
            .prepare("SELECT path, pinned FROM game ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let flags: Vec<bool> = pinned.iter().map(|(_, p)| *p).collect();
        assert_eq!(flags, [true, false, false, true], "{pinned:?}");
    }

    #[test]
    fn v3_migration_keeps_every_row() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("v2.sqlite");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.execute_batch(MIGRATIONS[1]).unwrap();
            conn.pragma_update(None, "user_version", 2).unwrap();
            conn.execute_batch(
                "INSERT INTO dat_info VALUES ('gb', 'D', '1', 1, 5, 'etag');
                 INSERT INTO dat_entry VALUES ('gb', 'G', 'g.gb', 3, 7, NULL);
                 INSERT INTO game (id, path, entry, system, size, file_size, mtime_unix, crc32, sha1, title,
                                   identification, favorite, last_played_unix, play_count, pinned)
                 VALUES (1, '/r/gb/a.gb', NULL, 'gb', 3, 3, 9, 7, x'00', 'A', 'sha1', 1, 42, 5, 1),
                        (2, '/r/b.zip', 'b.sms', 'mastersystem', 4, 8, 10, NULL, NULL, 'B', 'unknown', 0, NULL, 0, 0);
                 INSERT INTO cover VALUES (1, 'boxart', 'missing', NULL, NULL, 77);",
            )
            .unwrap();
            drop(conn);
        }
        let (lib, outcome) = Library::open(&db).unwrap();
        assert_eq!(outcome, OpenOutcome::Opened);
        assert_eq!(user_version(&lib.conn).ok(), Some(SCHEMA_VERSION));
        let games = lib.games().unwrap();
        assert_eq!(games.len(), 2);
        let a = &games[0];
        assert_eq!(
            (
                a.title.as_str(),
                a.favorite,
                a.last_played_unix,
                a.play_count
            ),
            ("A", true, Some(42), 5)
        );
        assert!(a.media.is_empty() && games[1].media.is_empty());
        assert_eq!(games[1].entry.as_deref(), Some("b.sms"));
        assert_eq!(lib.dat_info("gb").unwrap().unwrap().entries, 1);
        assert!(lib.cover(GameId(1), CoverKind::Boxart).unwrap().is_some());
        lib.conn
            .execute("INSERT INTO game_media VALUES (1, 0, '/r/gb/a.gb')", [])
            .unwrap();
        assert_eq!(
            lib.game(GameId(1)).unwrap().unwrap().media,
            [PathBuf::from("/r/gb/a.gb")]
        );
        // The path is unique across games.
        assert!(
            lib.conn
                .execute("INSERT INTO game_media VALUES (2, 0, '/r/gb/a.gb')", [])
                .is_err()
        );
    }

    /// A 2.0 base: the frozen v1-v3 steps, then rows as the 2.0 code writes them (scan, DAT imports, favorites,
    /// history, covers in both states, a multi-disk ST game).
    fn base_v3(db: &Path) {
        let conn = Connection::open(db).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..3] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute_batch(
            "INSERT INTO dat_info VALUES ('gb', 'Nintendo - Game Boy', '20251101', 2, 100, '\"gb-etag\"');
             INSERT INTO dat_info VALUES ('atarist', 'Atari - ST', NULL, 1, 200, '\"nointro-st\"');
             INSERT INTO dat_entry VALUES ('gb', 'Tetris (World)', 'Tetris (World).gb', 3, 7, x'0102');
             INSERT INTO dat_entry VALUES ('gb', 'Other (World)', 'Other (World).gb', 4, 8, NULL);
             INSERT INTO dat_entry VALUES ('atarist', 'Saga (Disk 1 of 2)', 'Saga (Disk 1 of 2).st', 9, 11, NULL);
             INSERT INTO game (id, path, entry, system, size, file_size, mtime_unix, crc32, sha1, crc32_nh,
                               sha1_nh, title, identification, favorite, last_played_unix, play_count, pinned)
             VALUES (1, '/r/gb/t.gb', NULL, 'gb', 3, 3, 9, 7, x'0102', NULL, NULL, 'Tetris (World)', 'sha1',
                     1, 1700000000, 5, 1),
                    (2, '/r/s.zip', 's.sms', 'mastersystem', 4, 8, 10, NULL, NULL, NULL, NULL, 's', 'unknown',
                     0, NULL, 0, 0),
                    (3, '/r/atarist/Saga (Disk 1 of 2).st', NULL, 'atarist', 9, 9, 11, 11, NULL, NULL, NULL,
                     'Saga', 'crc32_size', 1, 1700000500, 2, 1);
             INSERT INTO game_media VALUES (3, 0, '/r/atarist/Saga (Disk 1 of 2).st'),
                                           (3, 1, '/r/atarist/Saga (Disk 2 of 2).st');
             INSERT INTO cover VALUES (1, 'boxart', 'cached', 'gb/boxart/Tetris (World).png', 'libretro', 50);
             INSERT INTO cover VALUES (2, 'boxart', 'missing', NULL, NULL, 77);",
        )
        .unwrap();
    }

    #[test]
    fn v4_migration_keeps_a_2_0_base() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("library.sqlite");
        base_v3(&db);
        // The v1-v3 columns as text: `games()` itself needs the v5 column.
        let snapshot = |conn: &Connection| -> Vec<String> {
            conn.prepare(
                "SELECT id, path, entry, system, size, crc32, sha1, title, identification, favorite,
                        last_played_unix, play_count, pinned FROM game ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                (0..13)
                    .map(|i| r.get_ref(i).map(|v| format!("{v:?}")))
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map(|c| c.join("|"))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
        };
        let before = snapshot(&Connection::open(&db).unwrap());
        let (lib, outcome) = Library::open(&db).unwrap();
        assert_eq!(outcome, OpenOutcome::Opened);
        assert_eq!(user_version(&lib.conn).ok(), Some(SCHEMA_VERSION));
        // Games, favorites, history, disks: identical.
        assert_eq!(snapshot(&lib.conn), before);
        assert_eq!(before.len(), 3);
        let games = lib.games().unwrap();
        assert!(games.iter().all(|g| !g.is_dir));
        assert_eq!(games[2].media.len(), 2);
        assert!(games[0].favorite && games[2].favorite);
        assert_eq!(
            lib.cover(GameId(1), CoverKind::Boxart)
                .unwrap()
                .unwrap()
                .state,
            CoverState::Cached {
                path: "gb/boxart/Tetris (World).png".into(),
                source: "libretro".into()
            }
        );
        assert_eq!(
            lib.cover(GameId(2), CoverKind::Boxart)
                .unwrap()
                .unwrap()
                .state,
            CoverState::Missing { checked_unix: 77 }
        );
        // DATs: source of the 2.0 catalog, the ST ETag dropped.
        let gb = lib.dat_info("gb").unwrap().unwrap();
        assert_eq!(
            (
                gb.source,
                gb.entries,
                gb.etag.as_deref(),
                gb.version.as_deref()
            ),
            (DatSource::NoIntro, 2, Some("\"gb-etag\""), Some("20251101"))
        );
        let st = lib
            .dat_info_for("atarist", DatSource::Tosec)
            .unwrap()
            .unwrap();
        assert_eq!(
            (st.entries, st.etag.as_deref(), st.imported_unix),
            (1, None, 200)
        );
        assert_eq!(lib.dat_infos("atarist").unwrap(), [st]);
        let entries: Vec<(String, String, String, Option<Vec<u8>>)> = lib
            .conn
            .prepare("SELECT system, source, game, sha1 FROM dat_entry ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].3.as_deref(), Some(&[1u8, 2][..]));
        assert_eq!(
            entries.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(),
            ["no-intro", "no-intro", "tosec"]
        );
        let fk: Option<String> = lib
            .conn
            .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
            .optional()
            .unwrap();
        assert_eq!(fk, None);
        let indexes: Vec<String> = lib
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'dat_entry' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(indexes, ["dat_entry_crc", "dat_entry_sha1"]);
        // The foreign key follows the new key: removing a DAT removes its entries only.
        lib.conn
            .execute("DELETE FROM dat_info WHERE system = 'gb'", [])
            .unwrap();
        let left: i64 = lib
            .conn
            .query_row("SELECT count(*) FROM dat_entry", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1);
    }

    #[test]
    fn failed_migration_goes_to_quarantine() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("library.sqlite");
        base_v3(&db);
        {
            // A name the v4 step needs is taken: the step fails on this file.
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("CREATE TABLE dat_info_v3 (x)").unwrap();
        }
        let (lib, outcome) = Library::open(&db).unwrap();
        let OpenOutcome::Recovered { quarantined } = outcome else {
            panic!("{outcome:?}");
        };
        assert!(lib.games().unwrap().is_empty());
        let old = Connection::open(&quarantined).unwrap();
        assert_eq!(user_version(&old).ok(), Some(3));
        let games: i64 = old
            .query_row("SELECT count(*) FROM game", [], |r| r.get(0))
            .unwrap();
        assert_eq!(games, 3);
    }

    #[test]
    fn try_open_without_create_leaves_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("gone.sqlite");
        assert!(try_open(&db, false).is_err());
        assert!(!db.exists());
    }

    #[test]
    fn concurrent_migrations_are_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("v1.sqlite");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let mut a = Connection::open(&db).unwrap();
        let mut b = Connection::open(&db).unwrap();
        a.busy_timeout(Duration::from_secs(5)).unwrap();
        b.busy_timeout(Duration::from_secs(5)).unwrap();
        // Both read version 1 before either migrates: the second must see the first one's work.
        migrate(&mut a, 1).unwrap();
        migrate(&mut b, 1).unwrap();
        assert_eq!(user_version(&b).ok(), Some(SCHEMA_VERSION));
    }

    #[test]
    fn v6_base_gains_needs_32x_without_loss() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("v6.sqlite");
        {
            let conn = Connection::open(&db).unwrap();
            for m in &MIGRATIONS[..6] {
                conn.execute_batch(m).unwrap();
            }
            conn.pragma_update(None, "user_version", 6).unwrap();
            conn.execute(
                "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification, favorite,
                                   play_count, region)
                 VALUES ('/r/megacd/F.cue', 'megacd', 1, 1, 7, 'F', 'name', 1, 3, 'usa')",
                [],
            )
            .unwrap();
        }
        let (lib, outcome) = Library::open(&db).unwrap();
        assert_eq!(outcome, OpenOutcome::Opened);
        assert_eq!(user_version(&lib.conn).ok(), Some(SCHEMA_VERSION));
        let games = lib.games().unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert!(g.favorite && g.play_count == 3 && g.region == Some(DiscRegion::Usa));
        assert!(!g.needs_32x);
        let stored: Option<i64> = lib
            .conn
            .query_row("SELECT needs_32x FROM game", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, None, "NULL until a scan computes it");
        lib.conn
            .execute("UPDATE game SET needs_32x = 1", [])
            .unwrap();
        assert!(lib.games().unwrap()[0].needs_32x);
    }
}
