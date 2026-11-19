//! Folder scan (ADR 0014 §1): walk, hash (zip entries included), identify, store.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::{Connection, params};
use zip::ZipArchive;

use crate::db::{
    NameIndex, db_error, extension_of, identification_text, identify, sha1_from, to_i64,
};
use crate::{
    Identification, LibraryError, MAX_HASH_SIZE, RomHashes, SystemInfo, hash_reader,
    systems_for_extension,
};

/// Folders are walked without following directory symlinks. A path component equal to a system `id`
/// (`roms/gb/…`) decides between systems sharing an extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
    pub max_depth: usize,
    /// Files (or zip entries) above this size are recorded but not hashed.
    pub max_file_size: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            max_depth: 8,
            max_file_size: crate::MAX_HASH_SIZE,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub hashed: u64,
    pub identified: u64,
    pub current: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub added: u64,
    pub updated: u64,
    /// Unchanged path, size and mtime: not hashed again.
    pub unchanged: u64,
    pub removed: u64,
    /// Roots that did not exist: their games are kept (unmounted drive), not removed.
    pub missing_roots: Vec<PathBuf>,
    /// Unreadable files and archives (path + French message), the scan goes on.
    pub errors: Vec<(PathBuf, String)>,
}

/// Archives listing more entries are refused before the central directory is parsed (same bound as emu-runtime).
const MAX_ARCHIVE_ENTRIES: u64 = 4096;
/// Twice the hash limit: a stored entry right at the limit, plus headers, still fits (same rule as emu-runtime).
const MAX_ARCHIVE_SIZE: u64 = 2 * MAX_HASH_SIZE;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Files hashed before their results are written. Hashing happens outside any transaction; the write transaction
/// only stores the results, so the UI connection is never blocked for the duration of a hash.
const BATCH: usize = 128;
/// Results are also written at least this often, so a cancellation keeps the work of a slow drive.
const BATCH_TIME: Duration = Duration::from_secs(2);

struct Found {
    path: PathBuf,
    /// UTF-8 form of `path` (stored in the base).
    key: String,
    /// Path components (root folder name included) checked against system ids.
    folders: Vec<String>,
    file_size: u64,
    mtime: i64,
}

struct Existing {
    id: i64,
    file_size: i64,
    mtime: i64,
    sha1: Option<[u8; 20]>,
    entry: Option<String>,
    system: String,
    pinned: bool,
}

struct Analysis {
    entry: Option<String>,
    size: u64,
    hashes: Option<RomHashes>,
    candidates: Vec<&'static SystemInfo>,
    /// The folder rule reduced `candidates` to one system.
    pinned: bool,
    stem: String,
}

enum Failure {
    Cancelled,
    /// French message reported in `ScanReport::errors`.
    Message(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Message(message)
    }
}

fn is_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

/// Fails every read once `cancel` is set: hashing a large file (or a zip entry) stops within one buffer.
struct CancelRead<'c, R> {
    inner: R,
    cancel: &'c AtomicBool,
}

impl<R: Read> Read for CancelRead<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(self.cancel) {
            // Not `Interrupted`: `hash_reader` retries those.
            return Err(io::Error::other("scan annulé"));
        }
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for CancelRead<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Opens `path` and checks that it is still a regular file: it may have been replaced since the walk. On Unix the
/// open is non-blocking, so a FIFO swapped in does not hang the scan (no effect on regular files).
fn open_regular(path: &Path) -> Result<File, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("pas un fichier ordinaire".to_owned());
    }
    Ok(file)
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn lower_ext(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

fn is_candidate(path: &Path) -> bool {
    lower_ext(path).is_some_and(|e| e == "zip" || !systems_for_extension(&e).is_empty())
}

struct Walk<'a> {
    report: &'a mut ScanReport,
    /// Prefixes whose stored games are kept even if not seen (missing root, unreadable folder).
    keep: &'a mut Vec<PathBuf>,
    seen: &'a mut HashSet<String>,
    found: &'a mut Vec<Found>,
}

impl Walk<'_> {
    fn root(
        &mut self,
        root: &Path,
        max_depth: usize,
        cancel: &AtomicBool,
    ) -> Result<(), LibraryError> {
        // The root itself may be a symlink (a mounted drive): it is the only followed directory link.
        if !fs::metadata(root).is_ok_and(|m| m.is_dir()) {
            self.report.missing_roots.push(root.to_path_buf());
            self.keep.push(root.to_path_buf());
            return Ok(());
        }
        let base = root.parent().unwrap_or(root);
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if is_cancelled(cancel) {
                return Err(LibraryError::Cancelled);
            }
            let entries = match fs::read_dir(&dir) {
                Ok(it) => it,
                Err(e) => {
                    self.report.errors.push((dir.clone(), e.to_string()));
                    self.keep.push(dir);
                    continue;
                }
            };
            let mut entries: Vec<_> = entries
                .filter_map(|e| match e {
                    Ok(e) => Some(e),
                    Err(err) => {
                        self.report.errors.push((dir.clone(), err.to_string()));
                        self.keep.push(dir.clone());
                        None
                    }
                })
                .collect();
            entries.sort_by_key(|e| e.file_name());
            let mut subdirs = Vec::new();
            for entry in entries {
                let path = entry.path();
                let Ok(kind) = entry.file_type() else {
                    self.keep.push(path);
                    continue;
                };
                let meta = if kind.is_dir() {
                    if depth < max_depth {
                        subdirs.push(path);
                    }
                    continue;
                } else if !is_candidate(&path) {
                    continue;
                } else if kind.is_symlink() {
                    // File links are followed; directory links never are (loops, escaping the root).
                    match fs::metadata(&path) {
                        Ok(m) if m.is_file() => m,
                        _ => continue,
                    }
                } else if kind.is_file() {
                    match entry.metadata() {
                        Ok(m) => m,
                        Err(e) => {
                            self.report.errors.push((path.clone(), e.to_string()));
                            self.keep.push(path);
                            continue;
                        }
                    }
                } else {
                    continue;
                };
                let Some(key) = path.to_str().map(str::to_owned) else {
                    tracing::warn!("non UTF-8 path ignored: {}", path.display());
                    self.report
                        .errors
                        .push((path, "chemin non UTF-8 ignoré".to_owned()));
                    continue;
                };
                if !self.seen.insert(key.clone()) {
                    continue;
                }
                let folders = path
                    .parent()
                    .and_then(|p| p.strip_prefix(base).ok())
                    .map(|rel| {
                        rel.components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                self.found.push(Found {
                    file_size: meta.len(),
                    mtime: mtime_of(&meta),
                    path,
                    key,
                    folders,
                });
            }
            // Reversed so the stack pops them in name order.
            stack.extend(subdirs.into_iter().rev().map(|d| (d, depth + 1)));
        }
        Ok(())
    }
}

/// Total entry count announced by the end-of-central-directory record (ZIP64 record if the 16-bit field is
/// saturated), read before `ZipArchive::new` allocates for it. `None` when no record is found.
fn declared_entries(file: &mut File, size: u64) -> io::Result<Option<u64>> {
    const EOCD_LEN: u64 = 22;
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const LOCATOR_LEN: u64 = 20;
    let window = size.min(EOCD_LEN + 65535);
    let start = size - window;
    file.seek(SeekFrom::Start(start))?;
    let mut tail = vec![0u8; window as usize];
    file.read_exact(&mut tail)?;
    let Some(pos) = tail
        .windows(EOCD_SIG.len())
        .rposition(|w| w == EOCD_SIG)
        .filter(|&p| p + EOCD_LEN as usize <= tail.len())
    else {
        return Ok(None);
    };
    let total = u64::from(u16::from_le_bytes([tail[pos + 10], tail[pos + 11]]));
    if total != 0xFFFF {
        return Ok(Some(total));
    }
    let eocd_at = start + pos as u64;
    if eocd_at < LOCATOR_LEN {
        return Ok(Some(total));
    }
    let mut locator = [0u8; LOCATOR_LEN as usize];
    file.seek(SeekFrom::Start(eocd_at - LOCATOR_LEN))?;
    file.read_exact(&mut locator)?;
    if locator[..4] != [0x50, 0x4b, 0x06, 0x07] {
        return Ok(Some(total));
    }
    let mut offset = [0u8; 8];
    offset.copy_from_slice(&locator[8..16]);
    let offset = u64::from_le_bytes(offset);
    let mut record = [0u8; 56];
    if offset
        .checked_add(record.len() as u64)
        .is_none_or(|end| end > size)
    {
        return Ok(Some(total));
    }
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut record)?;
    if record[..4] != [0x50, 0x4b, 0x06, 0x06] {
        return Ok(Some(total));
    }
    let mut n = [0u8; 8];
    n.copy_from_slice(&record[32..40]);
    Ok(Some(u64::from_le_bytes(n)))
}

/// Last component of an entry name (`/` or `\` separated), control characters replaced; `None` when empty,
/// `.`, `..` or too long. Only used to pick and display the entry: never joined to a path.
fn entry_file_name(raw: &[u8]) -> Option<String> {
    let base = raw.rsplit(|&b| b == b'/' || b == b'\\').next()?;
    let name: String = String::from_utf8_lossy(base)
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." || trimmed.len() > 200 {
        return None;
    }
    Some(trimmed.to_owned())
}

fn hash_error_message(e: &LibraryError) -> String {
    match e {
        LibraryError::Io { source, .. } => source.to_string(),
        LibraryError::TooLarge { .. } => "taille supérieure à la limite".to_owned(),
        other => other.to_string(),
    }
}

fn header_of(candidates: &[&SystemInfo]) -> Option<crate::HeaderRule> {
    candidates.iter().find_map(|s| s.header)
}

/// The folder rule: a path component equal to a candidate id pins the system.
fn folder_pin<'s>(candidates: &[&'s SystemInfo], folders: &[String]) -> Option<&'s SystemInfo> {
    candidates.iter().copied().find(|s| folders.contains(&s.id))
}

/// `candidates` reduced by the folder rule, and whether it applied.
fn pin(
    candidates: Vec<&'static SystemInfo>,
    folders: &[String],
) -> (Vec<&'static SystemInfo>, bool) {
    match folder_pin(&candidates, folders) {
        Some(s) => (vec![s], true),
        None => (candidates, false),
    }
}

/// An unchanged file is not hashed again unless the folder rule now gives another answer (the roots changed, or
/// a v1 row whose migrated flag was approximate): the stored system and flag must match the rule.
fn pin_agrees(old: &Existing, found: &Found) -> bool {
    let Some(ext) = extension_of(&found.path, old.entry.as_deref()) else {
        return true;
    };
    match folder_pin(&systems_for_extension(&ext), &found.folders) {
        Some(s) => old.pinned && old.system == s.id,
        None => !old.pinned,
    }
}

fn analyze_zip(found: &Found, max: u64, cancel: &AtomicBool) -> Result<Analysis, String> {
    if found.file_size > MAX_ARCHIVE_SIZE {
        return Err("archive trop grosse".to_owned());
    }
    let mut file = open_regular(&found.path)?;
    match declared_entries(&mut file, found.file_size) {
        Ok(Some(n)) if n > MAX_ARCHIVE_ENTRIES => {
            return Err(format!("archive illisible : trop d'entrées ({n})"));
        }
        Ok(_) => {}
        Err(e) => return Err(e.to_string()),
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(BufReader::new(CancelRead {
        inner: file,
        cancel,
    }))
    .map_err(|e| format!("archive illisible : {e}"))?;
    if archive.len() as u64 > MAX_ARCHIVE_ENTRIES {
        return Err("archive illisible : trop d'entrées".to_owned());
    }
    // First file entry with a catalog extension, as emu-runtime picks the first accepted entry.
    let mut chosen = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_data(index)
            .map_err(|e| format!("archive illisible : {e}"))?;
        if !entry.is_file() {
            continue;
        }
        let Some(name) = entry_file_name(entry.name_raw()) else {
            continue;
        };
        let Some(ext) = lower_ext(Path::new(&name)) else {
            continue;
        };
        let candidates = systems_for_extension(&ext);
        if !candidates.is_empty() {
            chosen = Some((index, name, entry.size(), candidates));
            break;
        }
    }
    let Some((index, name, declared, candidates)) = chosen else {
        return Err("aucun jeu reconnu dans l'archive".to_owned());
    };
    let (candidates, pinned) = pin(candidates, &found.folders);
    let hashes = if declared > max {
        None
    } else {
        let entry = archive
            .by_index(index)
            .map_err(|e| format!("archive illisible : {e}"))?;
        // `hash_reader` fails past `declared` bytes: a lying entry (zip bomb) is never read further.
        let reader = entry.take(declared.saturating_add(1));
        Some(
            hash_reader(reader, declared, header_of(&candidates), max)
                .map_err(|e| format!("archive illisible : {}", hash_error_message(&e)))?,
        )
    };
    Ok(Analysis {
        entry: Some(name),
        size: declared,
        hashes,
        candidates,
        pinned,
        stem: stem_of(&found.path),
    })
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn analyze(found: &Found, max: u64, cancel: &AtomicBool) -> Result<Analysis, Failure> {
    let result = analyze_file(found, max, cancel);
    // A read failed by `CancelRead` surfaces as an I/O or archive error: the flag tells them apart.
    if result.is_err() && is_cancelled(cancel) {
        return Err(Failure::Cancelled);
    }
    result.map_err(Failure::from)
}

fn analyze_file(found: &Found, max: u64, cancel: &AtomicBool) -> Result<Analysis, String> {
    let Some(ext) = lower_ext(&found.path) else {
        return Err("extension inconnue".to_owned());
    };
    if ext == "zip" {
        return analyze_zip(found, max, cancel);
    }
    let (candidates, pinned) = pin(systems_for_extension(&ext), &found.folders);
    let hashes = if found.file_size > max {
        None
    } else {
        let file = open_regular(&found.path)?;
        Some(
            hash_reader(
                CancelRead {
                    inner: file,
                    cancel,
                },
                found.file_size,
                header_of(&candidates),
                max,
            )
            .map_err(|e| hash_error_message(&e))?,
        )
    };
    Ok(Analysis {
        entry: None,
        size: found.file_size,
        hashes,
        candidates,
        pinned,
        stem: stem_of(&found.path),
    })
}

fn load_existing(conn: &Connection) -> Result<HashMap<String, Existing>, LibraryError> {
    let mut stmt = conn
        .prepare("SELECT id, path, file_size, mtime_unix, sha1, entry, system, pinned FROM game")
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(1)?,
            Existing {
                id: r.get(0)?,
                file_size: r.get(2)?,
                mtime: r.get(3)?,
                sha1: sha1_from(r.get(4)?),
                entry: r.get(5)?,
                system: r.get(6)?,
                pinned: r.get::<_, i64>(7)? != 0,
            },
        ))
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Inserts or updates the row of `found`; returns whether it was identified.
fn store(
    conn: &Connection,
    names: &mut NameIndex,
    found: &Found,
    analysis: &Analysis,
    existing: Option<&Existing>,
) -> Result<bool, LibraryError> {
    let id = identify(
        conn,
        names,
        &analysis.candidates,
        analysis.hashes.as_ref(),
        &analysis.stem,
    )?;
    let full = analysis.hashes.map(|h| h.full);
    let headerless = analysis.hashes.and_then(|h| h.headerless);
    conn.execute(
        "INSERT INTO game (path, entry, system, size, file_size, mtime_unix, crc32, sha1, crc32_nh, sha1_nh,
                           title, identification, pinned)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(path) DO UPDATE SET entry = excluded.entry, system = excluded.system, size = excluded.size,
             file_size = excluded.file_size, mtime_unix = excluded.mtime_unix, crc32 = excluded.crc32,
             sha1 = excluded.sha1, crc32_nh = excluded.crc32_nh, sha1_nh = excluded.sha1_nh,
             title = excluded.title, identification = excluded.identification, pinned = excluded.pinned",
        params![
            found.key,
            analysis.entry,
            id.system,
            to_i64(analysis.size),
            to_i64(found.file_size),
            found.mtime,
            full.map(|h| i64::from(h.crc32)),
            full.as_ref().map(|h| &h.sha1[..]),
            headerless.map(|h| i64::from(h.crc32)),
            headerless.as_ref().map(|h| &h.sha1[..]),
            id.title,
            identification_text(id.how),
            analysis.pinned,
        ],
    )
    .map_err(db_error)?;
    // Covers are cached by content hash: a different content needs a new lookup.
    if let Some(old) = existing
        && old.sha1 != full.map(|h| h.sha1)
    {
        conn.execute("DELETE FROM cover WHERE game = ?1", [old.id])
            .map_err(db_error)?;
    }
    Ok(id.how != Identification::Unknown)
}

/// Writes the analysed files of `pending` in one short transaction, then empties it.
fn store_batch(
    conn: &mut Connection,
    names: &mut NameIndex,
    existing: &HashMap<String, Existing>,
    pending: &mut Vec<(&Found, Analysis)>,
    state: &mut ScanProgress,
    report: &mut ScanReport,
) -> Result<(), LibraryError> {
    if pending.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction().map_err(db_error)?;
    let mut added = 0;
    let mut updated = 0;
    for (f, analysis) in pending.iter() {
        let old = existing.get(&f.key);
        if store(&tx, names, f, analysis, old)? {
            state.identified += 1;
        }
        if old.is_some() {
            updated += 1;
        } else {
            added += 1;
        }
    }
    tx.commit().map_err(db_error)?;
    // Counted after the commit: a failed batch reports nothing as stored.
    report.added += added;
    report.updated += updated;
    pending.clear();
    Ok(())
}

pub(crate) fn run(
    conn: &mut Connection,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport, LibraryError> {
    let mut report = ScanReport::default();
    let mut keep = Vec::new();
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    {
        let mut walk = Walk {
            report: &mut report,
            keep: &mut keep,
            seen: &mut seen,
            found: &mut found,
        };
        for root in &options.roots {
            walk.root(root, options.max_depth, cancel)?;
        }
    }

    let existing = load_existing(conn)?;
    let mut names = NameIndex::default();
    let mut state = ScanProgress::default();
    let mut last_emit: Option<Instant> = None;
    let mut pending: Vec<(&Found, Analysis)> = Vec::new();
    let mut batch_start = Instant::now();
    let mut cancelled = false;
    for f in &found {
        if is_cancelled(cancel) {
            cancelled = true;
            break;
        }
        state.files_seen += 1;
        state.current = Some(f.path.clone());
        let old = existing.get(&f.key);
        if old.is_some_and(|o| {
            o.file_size == to_i64(f.file_size) && o.mtime == f.mtime && pin_agrees(o, f)
        }) {
            report.unchanged += 1;
        } else {
            // No transaction is open here: a slow read never holds the write lock.
            match analyze(f, options.max_file_size, cancel) {
                Ok(analysis) => {
                    if analysis.hashes.is_some() {
                        state.hashed += 1;
                    }
                    pending.push((f, analysis));
                }
                Err(Failure::Cancelled) => {
                    cancelled = true;
                    break;
                }
                Err(Failure::Message(message)) => {
                    tracing::warn!("cannot scan {}: {message}", f.path.display());
                    report.errors.push((f.path.clone(), message));
                }
            }
        }
        if pending.len() >= BATCH || (!pending.is_empty() && batch_start.elapsed() >= BATCH_TIME) {
            store_batch(
                conn,
                &mut names,
                &existing,
                &mut pending,
                &mut state,
                &mut report,
            )?;
            batch_start = Instant::now();
        }
        if last_emit.is_none_or(|t| t.elapsed() >= PROGRESS_INTERVAL) {
            last_emit = Some(Instant::now());
            progress(&state);
        }
    }
    // Work done before a cancellation is kept: the next scan skips it as unchanged.
    store_batch(
        conn,
        &mut names,
        &existing,
        &mut pending,
        &mut state,
        &mut report,
    )?;
    if cancelled || is_cancelled(cancel) {
        return Err(LibraryError::Cancelled);
    }

    let tx = conn.transaction().map_err(db_error)?;
    for (path, old) in &existing {
        if seen.contains(path) || keep.iter().any(|k| Path::new(path).starts_with(k)) {
            continue;
        }
        report.removed += tx
            .execute("DELETE FROM game WHERE id = ?1", [old.id])
            .map_err(db_error)? as u64;
    }
    tx.commit().map_err(db_error)?;
    state.current = None;
    progress(&state);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names() {
        assert_eq!(
            entry_file_name(b"dir/sub/Game.gb").as_deref(),
            Some("Game.gb")
        );
        assert_eq!(
            entry_file_name(b"..\\..\\evil.gb").as_deref(),
            Some("evil.gb")
        );
        assert_eq!(entry_file_name(b"/abs/x.nes").as_deref(), Some("x.nes"));
        assert_eq!(entry_file_name(b"a\x01b.gb").as_deref(), Some("a_b.gb"));
        assert_eq!(entry_file_name(b"dir/.."), None);
        assert_eq!(entry_file_name(b"dir/"), None);
        assert_eq!(entry_file_name(&[b'a'; 300]), None);
    }

    #[cfg(unix)]
    #[test]
    fn fifo_is_not_opened_as_a_rom() {
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("swapped.gb");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            eprintln!("mkfifo unavailable: skipped");
            return;
        }
        // Without O_NONBLOCK this open would wait for a writer forever.
        assert_eq!(open_regular(&fifo).unwrap_err(), "pas un fichier ordinaire");
        let file = tmp.path().join("ok.gb");
        fs::write(&file, b"x").unwrap();
        assert!(open_regular(&file).is_ok());
    }

    #[test]
    fn cancellation_stops_a_hash_in_progress() {
        /// An endless file whose first read is the moment the user presses "cancel".
        struct Endless<'c> {
            cancel: &'c AtomicBool,
            reads: usize,
        }
        impl Read for Endless<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.reads += 1;
                self.cancel.store(true, Ordering::Relaxed);
                buf.fill(0);
                Ok(buf.len())
            }
        }
        let cancel = AtomicBool::new(false);
        let mut inner = Endless {
            cancel: &cancel,
            reads: 0,
        };
        let reader = CancelRead {
            inner: &mut inner,
            cancel: &cancel,
        };
        assert!(hash_reader(reader, MAX_HASH_SIZE, None, MAX_HASH_SIZE).is_err());
        assert_eq!(inner.reads, 1);

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.gb");
        fs::write(&path, vec![0u8; 1 << 20]).unwrap();
        let found = Found {
            key: path.to_str().unwrap().to_owned(),
            path,
            folders: Vec::new(),
            file_size: 1 << 20,
            mtime: 0,
        };
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            analyze(&found, MAX_HASH_SIZE, &cancel),
            Err(Failure::Cancelled)
        ));
        assert!(analyze(&found, MAX_HASH_SIZE, &AtomicBool::new(false)).is_ok());
    }

    #[test]
    fn folder_rule() {
        let gb = systems_for_extension("gb");
        let (pinned, applied) = pin(gb.clone(), &["roms".into(), "gb".into()]);
        assert_eq!((pinned[0].id.as_str(), applied), ("gb", true));
        assert!(!pin(gb, &["roms".into()]).1);
        // A folder naming a system that does not accept the extension does not pin.
        let sfc = systems_for_extension("sfc");
        let (pinned, applied) = pin(sfc, &["gb".into()]);
        assert_eq!((pinned[0].id.as_str(), applied), ("snes", false));
    }
}
